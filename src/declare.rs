//! `ascot declare`: everything the other commands would ask a live instance for, written to a
//! declaration file. Part of the `declare` feature.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

use crate::DocKind;
use crate::atelier::{AtelierContext, class_infos, routine_names};
use crate::bindings::{RoutineInfo, Source};
use crate::iris::{DECLARATIONS_VERSION, Declarations, DeclaredClass};
use crate::iris_connection::RawAtelier;

const BATCH: usize = 100;
const WORKERS: usize = 8;

impl RawAtelier {
    /// Every class, include file and routine in the namespace, for `ascot declare`;
    /// `progress(kind, done, total)` once each total is known and after each batch.
    pub fn declarations(
        self,
        mut progress: impl FnMut(DocKind, usize, usize),
    ) -> Result<Declarations, String> {
        let namespace = self.namespace().to_string();
        let iris_version = self.server_version()?;
        let ctx = AtelierContext::from(self);
        // `%sqlcq.*` and `%SYS.sqlcq.*` are cached SQL, which `declare`'s own queries add to, so
        // without this each run would add classes to the next run's file.
        let mut class_names: Vec<String> = ctx
            .rows("SELECT Name FROM %Dictionary.CompiledClass", &[])?
            .into_iter()
            .filter_map(|row| Some(row.get("Name")?.as_str()?.to_string()))
            .filter(|name| !name.starts_with("%sqlcq.") && !name.starts_with("%SYS.sqlcq."))
            .collect();
        class_names.sort();
        let inc_names: Vec<String> = routine_names(&ctx, "*.inc")?.into_iter().collect();
        // Every routine, system ones included, so one without source is still declared.
        let all_rtn_names = routine_names(&ctx, "*.mac,*.int,*.obj")?;
        // Hand-written routines only (a class's `.int`s are generated), read as the `.int`
        // where there is one (compiled, macro-expanded; what `get_rtn` prefers), else the `.mac`.
        let rtn_names: Vec<String> = routine_names(&ctx, "*.mac,*.int")?.into_iter().collect();

        let totals = [class_names.len(), inc_names.len(), rtn_names.len()];
        for kind in [DocKind::Cls, DocKind::Inc, DocKind::Rtn] {
            progress(kind, 0, totals[kind as usize]);
        }
        let jobs: Vec<(DocKind, &[String])> = (class_names.chunks(BATCH))
            .map(|batch| (DocKind::Cls, batch))
            .chain(inc_names.chunks(BATCH).map(|batch| (DocKind::Inc, batch)))
            .chain(rtn_names.chunks(BATCH).map(|batch| (DocKind::Rtn, batch)))
            .collect();
        let next = AtomicUsize::new(0);
        let classes = Mutex::new(BTreeMap::new());
        let includes = Mutex::new(BTreeMap::new());
        let routines = Mutex::new(
            all_rtn_names
                .into_iter()
                .map(|name| {
                    let info = RoutineInfo {
                        labels: None,
                        uri: None,
                    };
                    (name, info)
                })
                .collect::<BTreeMap<_, _>>(),
        );
        let failed = Mutex::new(None);
        let (tx, rx) = mpsc::channel();
        let mut done = [0; 3];
        std::thread::scope(|scope| {
            let (jobs, next, ctx, failed) = (&jobs, &next, &ctx, &failed);
            let (classes, includes, routines) = (&classes, &includes, &routines);
            for _ in 0..WORKERS.min(jobs.len()) {
                let tx = tx.clone();
                scope.spawn(move || {
                    while failed.lock().unwrap().is_none() {
                        let Some(&(kind, batch)) = jobs.get(next.fetch_add(1, Ordering::Relaxed))
                        else {
                            break;
                        };
                        let fetched = match kind {
                            DocKind::Cls => {
                                let params: Vec<&str> = batch.iter().map(String::as_str).collect();
                                let filter = format!("IN ({})", vec!["?"; params.len()].join(","));
                                class_infos(ctx, &filter, &params).map(|found| {
                                    let found = found.into_iter().map(|(name, info)| {
                                        let (declared, dropped) = DeclaredClass::validated(&info);
                                        for header in dropped {
                                            eprintln!("declare: {name}: left out `{header}`, which the class parser rejects");
                                        }
                                        (name, declared)
                                    });
                                    classes.lock().unwrap().extend(found)
                                })
                            }
                            DocKind::Inc => sources(ctx, batch, &["inc"])
                                .map(|found| includes.lock().unwrap().extend(found)),
                            DocKind::Rtn => sources(ctx, batch, &["int", "mac"]).map(|found| {
                                let found = found.into_iter().map(|(name, source)| {
                                    (name, crate::routine_info(&source.text, None))
                                });
                                routines.lock().unwrap().extend(found)
                            }),
                        };
                        if let Err(err) = fetched {
                            failed.lock().unwrap().get_or_insert(err);
                            break;
                        }
                        let _ = tx.send((kind, batch.len()));
                    }
                });
            }
            drop(tx);
            for (kind, count) in rx {
                done[kind as usize] += count;
                progress(kind, done[kind as usize], totals[kind as usize]);
            }
        });
        if let Some(err) = failed.into_inner().unwrap() {
            return Err(err);
        }
        Ok(Declarations {
            version: DECLARATIONS_VERSION.to_string(),
            namespace,
            iris_version,
            classes: classes.into_inner().unwrap(),
            includes: includes.into_inner().unwrap(),
            routines: routines.into_inner().unwrap(),
            ..Default::default()
        })
    }
}

fn sources(
    ctx: &AtelierContext,
    names: &[String],
    exts: &[&str],
) -> Result<Vec<(String, Source)>, String> {
    let mut found = vec![];
    for name in names {
        if let Some(source) = ctx.first_doc(exts.iter().map(|ext| format!("{name}.{ext}")))? {
            found.push((name.clone(), source));
        }
    }
    Ok(found)
}
