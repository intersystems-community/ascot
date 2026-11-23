//! The statement rules of the desugaring (see `grammar`), as `query`'s.

use tygr::{StringEqCI, Wrap};

use super::Desugar;
use crate::convert::VecSep;
use crate::sql::grammar::{core, full};
use crate::sql::leaf::{ArgSep, Space};
use crate::sql::src::Src;

/// `INSERT ... SET a = x, b = y` is `INSERT ... (a, b) VALUES (x, y)`.
pub fn set_as_values(
    _: &StringEqCI!("SET"),
    assignments: &(Space, VecSep<Src<full::SQLSetAssignment>, ArgSep>),
) -> core::InsertValues {
    let (after_set, list) = assignments;
    let mut open_sp = Space::default();
    let mut exprs = VecSep {
        items: Vec::new(),
        seps: list.seps.desugar(),
    };
    for (i, assignment) in list.items.iter().enumerate() {
        let eq = assignment.eq.0.join(&assignment.eq.2);
        match i.checked_sub(1) {
            None => open_sp = open_sp.join(&eq),
            Some(before) => exprs.seps[before].2 = exprs.seps[before].2.join(&eq),
        }
        exprs.items.push(assignment.value.desugar());
    }
    let columns = VecSep {
        items: list
            .items
            .iter()
            .map(|assignment| assignment.column.desugar())
            .collect(),
        seps: vec![(Space::default(), Default::default(), Space::blank()); list.seps.len()],
    };
    core::InsertValues::ColumnList {
        columns: Some(Wrap::new(
            (
                Space::default(),
                Default::default(),
                Space::default().join(after_set),
            ),
            columns,
            (Space::default(), Default::default()),
        )),
        values: (
            Space::blank(),
            core::ValueSource::ValSpec {
                values_kw: Default::default(),
                sp: Space::blank(),
                spec: core::ValSpec::Scalx {
                    open: (Default::default(), open_sp),
                    exprs,
                    close: (Space::default(), Default::default()),
                },
            },
        ),
    }
}

/// `UPDATE t (a, b) VALUES (x, y)` is `UPDATE t SET a = x, b = y`; a column list with a query or a
/// host array for its values has no SET form and stays.
pub fn asign(asign: &full::Asign) -> core::Asign {
    match asign {
        full::Asign::SetList {
            set_kw,
            assignments,
        } => core::Asign::SetList {
            set_kw: *set_kw,
            assignments: assignments.desugar(),
        },
        full::Asign::ColumnList {
            columns: Some(columns),
            values:
                (
                    before_values,
                    full::ValueSource::ValSpec {
                        sp,
                        spec: full::ValSpec::Scalx { open, exprs, close },
                        ..
                    },
                ),
        } if columns.wrapped.items.len() == exprs.items.len() => {
            let items = columns
                .wrapped
                .items
                .iter()
                .zip(&exprs.items)
                .map(|(column, value)| {
                    let full::Scalx::Legacy(terms) = value;
                    let last = terms.items.len() - 1;
                    let range = terms.items[0].range().start..terms.items[last].range().end;
                    Src::new(
                        range,
                        core::SQLSetAssignment {
                            column: column.desugar(),
                            eq: (Space::blank(), Default::default(), Space::blank()),
                            value: value.desugar(),
                        },
                    )
                })
                .collect();
            let seps = exprs
                .seps
                .iter()
                .zip(&columns.wrapped.seps)
                .map(|((before, comma, after), (col_before, _, col_after))| {
                    (before.join(col_before), *comma, after.join(col_after))
                })
                .collect();
            let removed = [
                &columns.prefix.0,
                &columns.prefix.2,
                &columns.suffix.0,
                before_values,
                sp,
                &open.1,
                &close.0,
            ];
            let sp = removed
                .into_iter()
                .fold(Space::blank(), |sp, removed| sp.join(removed));
            core::Asign::SetList {
                set_kw: Default::default(),
                assignments: (sp, VecSep { items, seps }),
            }
        }
        full::Asign::ColumnList { columns, values } => core::Asign::ColumnList {
            columns: columns.desugar(),
            values: values.desugar(),
        },
    }
}

/// `%INTRANS` is `%INTRANSACTION`, `COMMIT WORK` is `COMMIT`.
pub fn tp_option(option: &full::TPOption) -> core::TPOption {
    use full::TPOption::*;
    match option {
        InTransaction(kw) => core::TPOption::InTransaction(*kw),
        BegTrans(kw) => core::TPOption::BegTrans(*kw),
        InTrans(_) => core::TPOption::InTransaction(Default::default()),
        Commit { commit_kw, .. } => core::TPOption::Commit {
            commit_kw: *commit_kw,
        },
    }
}

/// `ROLLBACK WORK` is `ROLLBACK`.
pub fn rollback(rollback: &full::RollB) -> core::RollB {
    core::RollB {
        rollback_kw: rollback.rollback_kw,
    }
}
