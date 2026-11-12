//! `Atelier`, the two Atelier REST API requests, and `AtelierContext`, the cached
//! `IrisContext` ascot makes out of them: the `%Dictionary` queries behind `get_cls`, and the
//! parsing behind `get_rtn`. ascot's own `Atelier` is `RawAtelier` (native only).

use std::collections::{BTreeSet, HashMap};
use std::ops::Deref;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::Value;
use tygr::{Grammar, Parse, Prefix, Print, Raw, StringEq, StringOf1};
use tygr_objectscript::cos::VecSep;

use crate::IrisContext;
use crate::bindings::{
    Cardinality, ClassInfo, CollectionKind, IndexInfo, IndexKey, IndexSubscript, MemberInfo,
    MemberKind, NormalArg, ParameterInfo, PropertyInfo, RefMode, RelationshipInfo, RoutineInfo,
    Signature, Source, VariadicArg,
};
use crate::cos::{self, LocalVariableName, OSString, Space, Type};

const CACHE_TTL: Duration = Duration::from_secs(60 * 60);

struct Cache<V: Clone>(Mutex<HashMap<String, (Option<Instant>, V)>>);

impl<V: Clone> Cache<V> {
    fn new() -> Self {
        Self(Mutex::new(HashMap::new()))
    }

    fn get_or_try_insert(
        &self,
        key: &str,
        compute: impl FnOnce() -> Result<V, String>,
    ) -> Result<V, String> {
        if let Some((at, value)) = self.0.lock().unwrap().get(key)
            && at.is_none_or(|at| at.elapsed() < CACHE_TTL)
        {
            return Ok(value.clone());
        }
        let value = compute()?;
        // wasm32-unknown-unknown has no clock, so entries there never expire.
        let at = (!cfg!(target_arch = "wasm32")).then(Instant::now);
        self.0
            .lock()
            .unwrap()
            .insert(key.to_string(), (at, value.clone()));
        Ok(value)
    }
}

fn doc_basename(doc: &str) -> String {
    doc.rsplit_once('.')
        .map_or(doc, |(name, _)| name)
        .to_string()
}

/// The two Atelier REST API requests the lookups are made of, for a host with its own connection.
pub trait Atelier: Send + Sync {
    /// The rows of `/action/query` as a JSON array, each an object keyed by column name.
    fn query(&self, sql: &str, params: &[&str]) -> Result<String, String>;
    /// The text of `/doc/{name}`; `None` if there's no such document.
    fn get_doc(&self, name: &str) -> Result<Option<String>, String>;
}

#[cfg(target_arch = "wasm32")]
impl Atelier for crate::bindings::Atelier {
    fn query(&self, sql: &str, params: &[&str]) -> Result<String, String> {
        let params: Vec<String> = params.iter().map(|param| param.to_string()).collect();
        crate::bindings::Atelier::query(self, sql, &params)
    }

    fn get_doc(&self, name: &str) -> Result<Option<String>, String> {
        crate::bindings::Atelier::get_doc(self, name)
    }
}

/// The `IrisContext` that looks code up through an `Atelier`, caching what it finds.
pub struct AtelierContext {
    atelier: Box<dyn Atelier>,
    cls_cache: Cache<Option<ClassInfo>>,
    routine_cache: Cache<Option<Source>>,
    rtn_cache: Cache<Option<RoutineInfo>>,
}

impl<A: Atelier + 'static> From<A> for AtelierContext {
    fn from(atelier: A) -> Self {
        Self {
            atelier: Box::new(atelier),
            cls_cache: Cache::new(),
            routine_cache: Cache::new(),
            rtn_cache: Cache::new(),
        }
    }
}

impl IrisContext for AtelierContext {
    fn get_cls(&self, cls: &str) -> Result<Option<ClassInfo>, String> {
        self.cls_cache.get_or_try_insert(cls, || {
            Ok(class_infos(self, "= ?", &[cls])?.into_values().next())
        })
    }

    fn get_inc(&self, name: &str) -> Result<Option<Source>, String> {
        self.fetch_routine(&[format!("{name}.inc")])
    }

    // The `.int` first: it is the instance's own macro expansion of the `.mac`, so no expansion
    // (or include resolution) is needed on our side.
    fn get_rtn(&self, name: &str) -> Result<Option<RoutineInfo>, String> {
        self.rtn_cache.get_or_try_insert(name, || {
            let source = self.fetch_routine(&[format!("{name}.int"), format!("{name}.mac")])?;
            Ok(match source {
                Some(source) => Some(crate::routine_info(&source.text, source.uri)),
                // Without source, its OBJ says whether it exists.
                None => routine_names(self, &format!("{name}.obj"))?
                    .contains(name)
                    .then_some(RoutineInfo {
                        labels: None,
                        uri: None,
                    }),
            })
        })
    }
}

impl AtelierContext {
    pub(crate) fn rows(&self, sql: &str, params: &[&str]) -> Result<Vec<Value>, String> {
        let rows = self.atelier.query(sql, params)?;
        serde_json::from_str(&rows)
            .map_err(|err| format!("query answered with other than rows: {err}"))
    }

    // The first of `docs` that exists, cached under the whole candidate list.
    fn fetch_routine(&self, docs: &[String]) -> Result<Option<Source>, String> {
        self.routine_cache
            .get_or_try_insert(&docs.join("|"), || self.first_doc(docs))
    }

    // The first of `docs` that exists. The Atelier REST API has no uri scheme of its own to offer.
    pub(crate) fn first_doc(
        &self,
        docs: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> Result<Option<Source>, String> {
        for doc in docs {
            if let Some(text) = self.atelier.get_doc(doc.as_ref())? {
                return Ok(Some(Source { text, uri: None }));
            }
        }
        Ok(None)
    }
}

// The routines matching `spec` (e.g. `*.mac,*.int,*.obj`), system ones included and
// class-generated ones (`Name.1.obj`) left out.
pub(crate) fn routine_names(ctx: &AtelierContext, spec: &str) -> Result<BTreeSet<String>, String> {
    let sql = "SELECT Name FROM %Library.RoutineMgr_StudioOpenDialog(?,1,1,1,1,1,0,\
               'NOT (Name %PATTERN ''.E1\".\"0.1\"G\"1N1\".obj\"'' AND $LENGTH(Name,''.'') > 3)')";
    let rows = ctx.rows(sql, &[spec])?;
    Ok(rows
        .iter()
        .filter_map(|row| Some(doc_basename(row.get("Name")?.as_str()?)))
        .collect())
}

fn index_key(key: &str) -> IndexKey {
    let (key, collation) = match key.split_once(':') {
        Some((key, collation)) => (key, Some(collation.to_string())),
        None => (key, None),
    };
    let (property, subscript) = match key.strip_suffix(')').and_then(|k| k.split_once('(')) {
        Some((property, "KEYS")) => (property, Some(IndexSubscript::Keys)),
        Some((property, "ELEMENTS")) => (property, Some(IndexSubscript::Elements)),
        _ => (key, None),
    };
    IndexKey {
        property: property.to_string(),
        subscript,
        collation,
    }
}

fn split_classes(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

// Every class satisfying `Name {filter}` (`= ?`, `IN (?,?)`; `params` bind the `?`s) with its
// own members, keyed by name: one round trip per dictionary table for the whole set.
pub(crate) fn class_infos(
    conn: &AtelierContext,
    filter: &str,
    params: &[&str],
) -> Result<HashMap<String, ClassInfo>, String> {
    let sql = format!(
        "SELECT Name, Description, Super, IncludeCode, Import, ClassType, Deprecated, Abstract, Inheritance, \
         _Language, ProcedureBlock, PropertyClass FROM %Dictionary.CompiledClass WHERE Name {filter}"
    );
    let rows = conn.rows(&sql, params)?;
    if rows.is_empty() {
        return Ok(HashMap::new());
    }
    let members = all_mem_rows(conn, filter, params)?;
    let storages: HashMap<String, String> = conn
        .rows(
            &format!(
                "SELECT parent AS Parent, Type FROM %Dictionary.CompiledStorage WHERE parent {filter}"
            ),
            params,
        )?
        .into_iter()
        .filter_map(|row| {
            let field = |key: &str| Some(row.get(key)?.as_str()?.to_string());
            Some((field("Parent")?, field("Type").filter(|t| !t.is_empty())?))
        })
        .collect();
    Ok(assemble(rows, members, &storages))
}

fn assemble(
    class_rows: Vec<Value>,
    member_rows: Vec<MemberRow>,
    storages: &HashMap<String, String>,
) -> HashMap<String, ClassInfo> {
    let mut members: HashMap<String, Vec<MemberInfo>> = HashMap::new();
    for row in member_rows {
        if !row.name.is_empty() {
            members
                .entry(row.parent.clone())
                .or_default()
                .push(member_row_to_info(row));
        }
    }
    for members in members.values_mut() {
        drop_query_methods(members);
    }
    class_rows
        .into_iter()
        .filter_map(|row| {
            let str_field = |key: &str| row.get(key).and_then(Value::as_str).unwrap_or_default();
            let cls = str_field("Name").to_string();
            if cls.is_empty() {
                return None;
            }
            let info = ClassInfo {
                doc: str_field("Description").to_string(),
                name: cls.clone(),
                extends: split_classes(str_field("Super")),
                storage: storages.get(&cls).cloned(),
                members: members.remove(&cls).unwrap_or_default(),
                include: split_classes(str_field("IncludeCode")),
                import: Some(split_classes(str_field("Import"))).filter(|i| !i.is_empty()),
                // The Atelier REST API has no uri scheme of its own to offer here.
                uri: None,
                class_type: parsed::<cos::ClassType, _>(str_field("ClassType")),
                deprecated: row.get("Deprecated").is_some_and(is_truthy),
                abstract_: row.get("Abstract").is_some_and(is_truthy),
                inheritance: parsed::<cos::Inheritance, _>(str_field("Inheritance")),
                language: parsed::<cos::MethodLanguage, _>(str_field("_Language")),
                procedure_block: row.get("ProcedureBlock").map(is_truthy),
                property_class: split_classes(str_field("PropertyClass")),
            };
            Some((cls, info))
        })
        .collect()
}

// The methods IRIS generates from each `Query` have fixed shapes the checker knows, so they go;
// the query itself keeps `QExecute`'s view of its arguments, which has the defaults the
// query's own `FormalSpec` drops.
fn drop_query_methods(members: &mut Vec<MemberInfo>) {
    let queries: Vec<String> = members
        .iter()
        .filter(|m| matches!(m.kind, MemberKind::Query(_)))
        .map(|m| m.name.clone())
        .collect();
    for query in &queries {
        let execute = members.iter().find(|m| m.name == format!("{query}Execute"));
        let Some(MemberKind::ClassMethod(sig) | MemberKind::Method(sig)) = execute.map(|m| &m.kind)
        else {
            continue;
        };
        let (normal, variadic) = (sig.normal[1..].to_vec(), sig.variadic.clone());
        let Some(MemberKind::Query(sig)) = members
            .iter_mut()
            .find(|m| &m.name == query)
            .map(|m| &mut m.kind)
        else {
            continue;
        };
        (sig.normal, sig.variadic) = (normal, variadic);
    }
    members.retain(|m| {
        matches!(m.kind, MemberKind::Query(_))
            || !queries.iter().any(|q| {
                crate::details::QUERY_METHOD_SUFFIXES
                    .iter()
                    .any(|suffix| m.name == format!("{q}{suffix}"))
            })
    });
}

// A compiled keyword value, read by the grammar's spelling of it into the host's enum.
fn parsed<Spelling: Grammar, T: for<'a> From<&'a Spelling>>(text: &str) -> Option<T> {
    Spelling::parse(text.trim())
        .ok()
        .map(|spelling| T::from(&spelling))
}

// Uniform row shape across the member tables `all_mem_rows` unions together. IRIS
// reports an unset SQL string column as an empty string, not JSON `null` -- normalized to `None`
// here, at the boundary, so nothing downstream has to know that (or risk treating "" as a real,
// if nonsense, class/type name).
#[derive(Deserialize, Clone)]
#[serde(rename_all = "PascalCase")]
struct MemberRow {
    // IRIS names the column after its `parent` relationship; an `AS Parent` alias keeps the case.
    #[serde(default, alias = "parent")]
    parent: String,
    #[serde(default)]
    name: String,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    aliases: Option<String>,
    member_type: String,
    #[serde(deserialize_with = "empty_string_as_none")]
    description: Option<String>,
    #[serde(deserialize_with = "empty_string_as_none")]
    formal_spec: Option<String>,
    #[serde(deserialize_with = "empty_string_as_none")]
    return_type: Option<String>,
    // A parameter's value as compiled: a literal, or what an `{expression}` evaluated to.
    #[serde(default, deserialize_with = "empty_string_as_none")]
    default_value: Option<String>,
    // A property's `list`/`array`, when it is a collection.
    #[serde(default, deserialize_with = "empty_string_as_none")]
    collection: Option<String>,
    #[serde(default)]
    relationship: Value,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    cardinality: Option<String>,
    #[serde(default)]
    multi_dimensional: Value,
    class_method: Value,
    deprecated: Value,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    origin: Option<String>,
    // An index's keywords.
    #[serde(default)]
    index_abstract: Value,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    index_condition: Option<String>,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    index_coshard_with: Option<String>,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    index_data: Option<String>,
    #[serde(default)]
    index_deferred: Value,
    #[serde(default)]
    index_extent: Value,
    #[serde(default)]
    index_id_key: Value,
    #[serde(default)]
    index_internal: Value,
    #[serde(default)]
    index_primary_key: Value,
    #[serde(default)]
    index_shard_key: Value,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    index_sql_name: Option<String>,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    index_type: Option<String>,
    #[serde(default)]
    index_unique: Value,
}

// The dictionary holds a parameter's compiled value, bare; the header spells it as source, so
// anything but a number IRIS compiles bare (`inf`, `NaN` and `1.` are not) is quoted. A value with
// a line break came from an `{expression}` and has no literal spelling, so it is left out.
fn parameter_source(value: &str) -> Option<String> {
    if value.contains(['\n', '\r']) {
        None
    } else if value.parse::<f64>().is_ok()
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || b"+-.eE".contains(&b))
        && value
            .split('.')
            .skip(1)
            .all(|s| s.starts_with(|c: char| c.is_ascii_digit()))
    {
        Some(value.to_string())
    } else {
        Some(format!("\"{}\"", value.replace('"', "\"\"")))
    }
}

fn empty_string_as_none<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.filter(|s| !s.is_empty()))
}

// IRIS may serialize a SQL bit column as a JSON bool, a "0"/"1" string, or a 0/1 number.
fn is_truthy(v: &Value) -> bool {
    matches!(v, Value::Bool(true)) || v.as_str() == Some("1") || v.as_i64() == Some(1)
}

// A class's own members (`Origin = Parent`): methods, properties and parameters, then its indices
// and queries, whose generated methods ascot synthesizes. Two requests: one `UNION` of all five
// tables overflows IRIS's query stack for a batch of classes.
fn all_mem_rows(
    conn: &AtelierContext,
    filter: &str,
    params: &[&str],
) -> Result<Vec<MemberRow>, String> {
    let declared = format!(
        "SELECT Parent, Name, NULL AS Aliases, 'method' AS MemberType, Description, \
             FormalSpec, ReturnType, NULL AS Collection, 0 AS MultiDimensional, 0 AS \
             Relationship, NULL AS Cardinality, ClassMethod, Deprecated, Origin, NULL AS \
             DefaultValue FROM %Dictionary.CompiledMethod WHERE Parent {filter} AND Origin = \
             Parent UNION ALL SELECT Parent, Name, Aliases, 'property' AS MemberType, Description, \
             NULL AS FormalSpec, Type AS ReturnType, Collection, MultiDimensional, Relationship, \
             Cardinality, 0 AS ClassMethod, Deprecated, Origin, NULL AS DefaultValue FROM \
             %Dictionary.CompiledProperty WHERE Parent {filter} AND Origin = Parent UNION ALL \
             SELECT Parent, Name, NULL AS Aliases, 'parameter' AS MemberType, Description, NULL AS \
             FormalSpec, Type AS ReturnType, NULL AS Collection, 0 AS MultiDimensional, 0 AS \
             Relationship, NULL AS Cardinality, 0 AS ClassMethod, Deprecated, Origin, _Default AS \
             DefaultValue FROM %Dictionary.CompiledParameter WHERE Parent {filter} AND Origin = \
             Parent"
    );
    let generating = format!(
        "SELECT Parent, Name, NULL AS Aliases, 'index' AS MemberType, Description, \
             Properties AS FormalSpec, NULL AS ReturnType, NULL AS Collection, 0 AS \
             MultiDimensional, 0 AS Relationship, NULL AS Cardinality, 0 AS ClassMethod, 0 AS \
             Deprecated, Origin, Abstract AS IndexAbstract, Condition AS IndexCondition, \
             CoshardWith AS IndexCoshardWith, Data AS IndexData, _Deferred AS IndexDeferred, \
             Extent AS IndexExtent, IdKey AS IndexIdKey, Internal AS IndexInternal, PrimaryKey AS \
             IndexPrimaryKey, ShardKey AS IndexShardKey, SqlName AS IndexSqlName, Type AS \
             IndexType, _Unique AS IndexUnique FROM %Dictionary.CompiledIndex WHERE Parent \
             {filter} AND Origin = Parent UNION ALL SELECT Parent, Name, NULL AS Aliases, 'query' \
             AS MemberType, Description, FormalSpec, Type AS ReturnType, NULL AS Collection, 0 \
             AS MultiDimensional, 0 AS Relationship, NULL AS Cardinality, 0 AS ClassMethod, 0 AS \
             Deprecated, Origin, 0 AS IndexAbstract, NULL AS IndexCondition, NULL AS \
             IndexCoshardWith, NULL AS IndexData, 0 AS IndexDeferred, 0 AS IndexExtent, 0 AS \
             IndexIdKey, 0 AS IndexInternal, 0 AS IndexPrimaryKey, 0 AS IndexShardKey, NULL AS \
             IndexSqlName, NULL AS IndexType, 0 AS IndexUnique FROM %Dictionary.CompiledQuery \
             WHERE Parent {filter} AND Origin = Parent"
    );
    let mut rows = member_rows(conn, &declared, &[params, params, params].concat())?;
    rows.extend(member_rows(conn, &generating, &[params, params].concat())?);
    Ok(rows)
}

fn member_rows(
    conn: &AtelierContext,
    sql: &str,
    params: &[&str],
) -> Result<Vec<MemberRow>, String> {
    conn.rows(sql, params).map(|rows| {
        rows.into_iter()
            .filter_map(|row| serde_json::from_value(row).ok())
            .collect()
    })
}

fn split_aliases(aliases: Option<&str>) -> Vec<String> {
    aliases
        .map(|aliases| {
            aliases
                .split(',')
                .map(|alias| alias.chars().filter(|c| !c.is_whitespace()).collect())
                .filter(|alias: &String| !alias.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn member_row_to_info(row: MemberRow) -> MemberInfo {
    // A dictionary may report a quoted name with its quotes, `"_isLabeled"`; the name is what
    // is between them, a doubled quote standing for one.
    let name = match row.name.strip_prefix('"').and_then(|n| n.strip_suffix('"')) {
        Some(quoted) => quoted.replace("\"\"", "\""),
        None => row.name.clone(),
    };
    let aliases = split_aliases(row.aliases.as_deref());
    let origin = row.origin.clone();
    MemberInfo {
        doc: row.description.clone().unwrap_or_default(),
        name,
        aliases,
        deprecated: is_truthy(&row.deprecated),
        kind: row_to_member_kind(&row),
        uri: None,
        origin,
    }
}

// A collection property's type is the collection class -- the same mapping the parser's
// `PropertyType` gets for a `list Of X` declaration.
fn property_type(
    collection: Option<&str>,
    ty: Option<String>,
    multidimensional: bool,
) -> PropertyInfo {
    PropertyInfo {
        t: ty,
        collection: match collection.map(str::to_ascii_lowercase).as_deref() {
            Some("list") => Some(CollectionKind::List),
            Some("array") => Some(CollectionKind::Array),
            _ => None,
        },
        multidimensional,
    }
}

fn row_to_member_kind(row: &MemberRow) -> MemberKind {
    match row.member_type.as_str() {
        "property" if is_truthy(&row.relationship) => MemberKind::Relationship(RelationshipInfo {
            t: row.return_type.clone(),
            cardinality: match row
                .cardinality
                .as_deref()
                .map(str::to_ascii_lowercase)
                .as_deref()
            {
                Some("one") => Some(Cardinality::One),
                Some("many") => Some(Cardinality::Many),
                Some("parent") => Some(Cardinality::Parent),
                Some("children") => Some(Cardinality::Children),
                _ => None,
            },
        }),
        "property" => MemberKind::Property(property_type(
            row.collection.as_deref(),
            row.return_type.clone(),
            is_truthy(&row.multi_dimensional),
        )),
        "parameter" => MemberKind::Parameter(ParameterInfo {
            t: row.return_type.clone(),
            v: row.default_value.as_deref().and_then(parameter_source),
        }),
        // `Properties` is `Name,Age`, a collated key `Name:Exact`, a collection key `Tags(ELEMENTS)`;
        // empty without an `On` clause. The compiled `SqlName` defaults to the index's own name.
        "index" => MemberKind::Index(IndexInfo {
            on: row.formal_spec.as_deref().map(|keys| {
                split_classes(keys)
                    .iter()
                    .map(|key| index_key(key))
                    .collect()
            }),
            abstract_: is_truthy(&row.index_abstract),
            condition: row.index_condition.clone(),
            coshard_with: row.index_coshard_with.clone(),
            data: split_classes(row.index_data.as_deref().unwrap_or("")),
            deferred: is_truthy(&row.index_deferred),
            extent: is_truthy(&row.index_extent),
            id_key: is_truthy(&row.index_id_key),
            internal: is_truthy(&row.index_internal),
            primary_key: is_truthy(&row.index_primary_key),
            shard_key: is_truthy(&row.index_shard_key),
            sql_name: row.index_sql_name.clone().filter(|name| *name != row.name),
            type_: row
                .index_type
                .as_deref()
                .and_then(|t| cos::IndexType::parse(t.trim()).ok())
                .and_then(|t| crate::details::index_type(&t)),
            unique: is_truthy(&row.index_unique),
        }),
        "query" => {
            let (normal, variadic) = parse_formal_spec(row.formal_spec.as_deref().unwrap_or(""));
            MemberKind::Query(Signature {
                normal,
                variadic,
                t: row.return_type.clone(),
                procedure_block: None,
            })
        }
        _ => {
            let (normal, variadic) = parse_formal_spec(row.formal_spec.as_deref().unwrap_or(""));
            let info = Signature {
                normal,
                variadic,
                t: row.return_type.clone(),
                procedure_block: None,
            };
            if is_truthy(&row.class_method) {
                MemberKind::ClassMethod(info)
            } else {
                MemberKind::Method(info)
            }
        }
    }
}

// A method's `FormalSpec` as `%Dictionary.CompiledMethod` reports it: minified, e.g.
// `*out:%String,&ref:%Integer,x:%String="a",args...`.
#[derive(Grammar)]
struct CompiledFormalSpec(Option<VecSep<CompiledFormal, Comma>>);

type Comma = Raw<(Space, StringEq!(","), Space)>;

#[derive(Grammar)]
struct CompiledFormal {
    call_type: Option<CompiledCallType>,
    name: LocalVariableName,
    variadic: Option<StringEq!("...")>,
    as_type: Option<Prefix<StringEq!(":"), Type>>,
    default: Option<Prefix<StringEq!("="), CompiledDefault>>,
}

#[derive(Grammar)]
enum CompiledCallType {
    Output(StringEq!("*")),
    ByRef(StringEq!("&")),
}

// A default value, up to the next top-level comma: strings, parentheses and braces balanced,
// anything else (a macro, `..#PARAM`) taken as it comes.
#[derive(Grammar)]
struct CompiledDefault(Raw<Vec<CompiledDefaultUnit>>);

#[derive(Grammar)]
enum CompiledDefaultUnit {
    String(OSString),
    Paren(StringEq!("("), Raw<Vec<CompiledNestedUnit>>, StringEq!(")")),
    Brace(StringEq!("{"), Raw<Vec<CompiledNestedUnit>>, StringEq!("}")),
    Text(StringOf1<IsCompiledDefaultText>),
}

#[derive(Grammar)]
enum CompiledNestedUnit {
    Unit(CompiledDefaultUnit),
    Comma(StringEq!(",")),
}

tygr::char_class!(IsCompiledDefaultText, "default_char", |ch| {
    !matches!(ch, ',' | '"' | '(' | ')' | '{' | '}')
});

fn parse_formal_spec(spec: &str) -> (Vec<NormalArg>, Option<VariadicArg>) {
    let Ok(CompiledFormalSpec(Some(formals))) = CompiledFormalSpec::parse(spec) else {
        return (vec![], None);
    };
    let mut normal = vec![];
    let mut variadic = None;
    for formal in &formals.items {
        let mode = match formal.call_type {
            Some(CompiledCallType::Output(_)) => Some(RefMode::Output),
            Some(CompiledCallType::ByRef(_)) => Some(RefMode::ByRef),
            None => None,
        };
        let name = formal.name.print();
        let t = formal.as_type.as_ref().map(|t| t.deref().print());
        if formal.variadic.is_some() {
            variadic = Some(VariadicArg { mode, name, t });
        } else {
            normal.push(NormalArg {
                mode,
                name,
                t,
                default: formal.default.as_ref().map(|d| d.deref().print()),
            });
        }
    }
    (normal, variadic)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_collection_property_keeps_its_element_type() {
        let list = property_type(Some("list"), Some("%String".into()), false);
        assert_eq!(list.collection, Some(CollectionKind::List));
        assert_eq!(list.t.as_deref(), Some("%String"));
        let array = property_type(Some("array"), Some("Foo.Bar".into()), false);
        assert_eq!(array.collection, Some(CollectionKind::Array));
        assert_eq!(
            property_type(None, Some("%String".into()), false).collection,
            None
        );
    }

    #[test]
    fn a_quoted_member_name_is_what_is_between_the_quotes() {
        let row = serde_json::json!({"parent": "A", "Name": "\"_isLabeled\"", "MemberType": "method", "Description": "", "FormalSpec": "", "ReturnType": "%Boolean", "ClassMethod": 0, "Deprecated": 0});
        let info = member_row_to_info(serde_json::from_value(row).unwrap());
        assert_eq!(info.name, "_isLabeled");
    }

    #[test]
    fn assemble_groups_member_rows_by_their_parent_class() {
        let class_rows = vec![
            serde_json::json!({"Name": "A.B", "Super": "%RegisteredObject", "ClassType": ""}),
            serde_json::json!({"Name": "A.C", "Super": "", "Inheritance": "right", "ClassType": "datatype", "Deprecated": 1}),
        ];
        let member_rows: Vec<MemberRow> = [
            serde_json::json!({"parent": "A.C", "Name": "P", "MemberType": "property", "Description": "", "FormalSpec": "", "ReturnType": "%String", "ClassMethod": 0, "Deprecated": 0}),
            serde_json::json!({"parent": "A.B", "Name": "M", "MemberType": "method", "Description": "", "FormalSpec": "x:%String", "ReturnType": "%Status", "ClassMethod": 1, "Deprecated": 0}),
            serde_json::json!({"Parent": "A.B", "Name": "", "MemberType": "method", "Description": "", "FormalSpec": "", "ReturnType": "", "ClassMethod": 0, "Deprecated": 0}),
            serde_json::json!({"parent": "A.C", "Name": "ByP", "MemberType": "index", "Description": "", "FormalSpec": "P,Tags(ELEMENTS)", "ReturnType": "", "ClassMethod": 0, "Deprecated": 0}),
            serde_json::json!({"parent": "A.C", "Name": "ByExactP", "MemberType": "index", "Description": "", "FormalSpec": "P:Exact", "ReturnType": "", "ClassMethod": 0, "Deprecated": 0, "IndexType": "index", "IndexUnique": true, "IndexData": "P,Q", "IndexSqlName": "ByExactP", "IndexCondition": "P > 1"}),
        ]
        .into_iter()
        .map(|row| serde_json::from_value(row).unwrap())
        .collect();
        let storages = [("A.B".to_string(), "%Storage.Persistent".to_string())].into();
        let classes = assemble(class_rows, member_rows, &storages);
        assert_eq!(classes.len(), 2);
        let b = &classes["A.B"];
        assert_eq!(b.extends, vec!["%RegisteredObject"]);
        assert_eq!(b.storage.as_deref(), Some("%Storage.Persistent"));
        assert!(classes["A.C"].storage.is_none());
        assert_eq!(b.class_type, None);
        assert_eq!(b.members.len(), 1, "the nameless row is dropped");
        assert_eq!(b.members[0].name, "M");
        assert!(matches!(b.members[0].kind, MemberKind::ClassMethod(_)));
        let c = &classes["A.C"];
        assert!(c.deprecated);
        assert_eq!(c.class_type, Some(crate::bindings::ClassType::Datatype));
        assert_eq!(c.inheritance, Some(crate::bindings::Inheritance::Right));
        assert_eq!(b.inheritance, None);
        assert_eq!(c.members[0].name, "P");
        let key = |property: &str, subscript, collation: Option<&str>| IndexKey {
            property: property.to_string(),
            subscript,
            collation: collation.map(str::to_string),
        };
        let MemberKind::Index(by_p) = &c.members[1].kind else {
            panic!("{:?}", c.members[1].kind)
        };
        assert_eq!(
            by_p.on.as_deref(),
            Some(
                &[
                    key("P", None, None),
                    key("Tags", Some(IndexSubscript::Elements), None)
                ][..]
            )
        );
        assert!(!by_p.unique && by_p.type_.is_none());
        let MemberKind::Index(by_exact_p) = &c.members[2].kind else {
            panic!("{:?}", c.members[2].kind)
        };
        assert_eq!(
            by_exact_p.on.as_deref(),
            Some(&[key("P", None, Some("Exact"))][..])
        );
        assert!(by_exact_p.unique && by_exact_p.type_.is_none());
        assert_eq!(by_exact_p.data, ["P", "Q"]);
        assert_eq!(by_exact_p.condition.as_deref(), Some("P > 1"));
        assert!(
            by_exact_p.sql_name.is_none(),
            "the default SqlName is the index's name"
        );
    }

    #[test]
    fn a_host_answers_the_lookups_through_its_own_atelier() {
        struct Docs;
        impl Atelier for Docs {
            fn query(&self, _: &str, _: &[&str]) -> Result<String, String> {
                Ok("[]".into())
            }
            fn get_doc(&self, name: &str) -> Result<Option<String>, String> {
                Ok((name == "Util.mac").then(|| "ROUTINE Util\nFoo(a)\n Quit".into()))
            }
        }
        let iris = AtelierContext::from(Docs);
        let util = iris.get_rtn("Util").unwrap().unwrap().labels.unwrap();
        let labels: Vec<_> = util.iter().map(|label| label.name.as_str()).collect();
        assert_eq!(labels, ["Foo"]);
        assert!(iris.get_inc("Util").unwrap().is_none());
        assert!(iris.get_rtn("NoSuchRoutine").unwrap().is_none());
        assert!(iris.get_cls("No.Such.Class").unwrap().is_none());
    }

    #[test]
    fn doc_basename_strips_the_extension() {
        assert_eq!(doc_basename("Foo.Bar.inc"), "Foo.Bar");
    }

    #[test]
    fn parses_a_normal_and_variadic_arg_with_types_and_a_default() {
        let (normal, variadic) =
            parse_formal_spec(r#"*out:%String,&ref:%Integer,x:%String="a",y..."#);
        assert_eq!(normal.len(), 3);
        assert_eq!(normal[0].mode, Some(RefMode::Output));
        assert_eq!(normal[0].name, "out");
        assert_eq!(normal[0].t, Some("%String".to_string()));
        assert_eq!(normal[1].mode, Some(RefMode::ByRef));
        assert_eq!(normal[1].name, "ref");
        assert_eq!(normal[1].t, Some("%Integer".to_string()));
        assert_eq!(normal[2].mode, None);
        assert_eq!(normal[2].name, "x");
        assert_eq!(normal[2].t, Some("%String".to_string()));
        assert_eq!(normal[2].default, Some("\"a\"".to_string()));
        let variadic = variadic.unwrap();
        assert_eq!(variadic.name, "y");
    }

    #[test]
    fn parses_a_default_containing_a_comma_inside_quotes() {
        let (normal, _) = parse_formal_spec(r#"x:%String="a,b""#);
        assert_eq!(normal.len(), 1);
        assert_eq!(normal[0].name, "x");
        assert_eq!(normal[0].t, Some("%String".to_string()));
    }

    #[test]
    fn parses_a_type_containing_a_comma_inside_parens() {
        let (normal, _) = parse_formal_spec("x:%String(MAXLEN=50,MINLEN=1)");
        assert_eq!(normal.len(), 1);
        assert_eq!(normal[0].name, "x");
        assert_eq!(normal[0].t, Some("%String(MAXLEN=50,MINLEN=1)".to_string()));
    }

    #[test]
    fn empty_formal_spec_has_no_args() {
        let (normal, variadic) = parse_formal_spec("");
        assert!(normal.is_empty());
        assert!(variadic.is_none());
    }

    #[test]
    fn parses_a_default_containing_a_macro_or_a_nested_call() {
        let (normal, _) = parse_formal_spec(r#"sc:%Status=$$$OK,l=$lb(1,"a,b",$g(x)),o={{}}"#);
        let defaults: Vec<_> = normal.iter().map(|arg| arg.default.as_deref()).collect();
        assert_eq!(
            defaults,
            [Some("$$$OK"), Some(r#"$lb(1,"a,b",$g(x))"#), Some("{{}}")]
        );
    }
}
