//! The names embedded SQL uses, resolved against the classes: a table is a persistent class of
//! the schema's package, named after the class or its `SqlTableName`, or a view a class query
//! projects with `SqlView`; a column is a property under its `SqlFieldName` or its own name; a
//! stored procedure is a method or class query with `SqlProc`, named `Class_Member` or by its
//! `SqlName`. The workspace is asked first, then IRIS.

use std::collections::HashMap;

use crate::bindings::{self, Cardinality, CollectionKind, MemberKind};
use crate::check::env::{Class, CodeEnv, WorkspaceEnv};
use crate::check::errors::{ArgCount, SqlMemberKind, SqlProcedureUse, TypeErrorKind};
use crate::check::flow::Flow;
use crate::check::types::{ClsOrIns, Ty};
use crate::details::{MemberKindSpecificDetails, Positions};
use crate::{DocKind, sql_schema};

use super::{ColumnRef, Frame, Hosts, Name, Resolver, TableRef};
use tygr_objectscript::sql::SQL;

#[derive(Clone)]
pub(crate) enum Lookup<T> {
    Found(T),
    Missing,
    // The IRIS environment in use may hold it under a SQL name it doesn't tell (A8172).
    Unconfirmed,
}

impl<T> Lookup<T> {
    fn map<U>(self, f: impl FnOnce(T) -> U) -> Lookup<U> {
        match self {
            Lookup::Found(found) => Lookup::Found(f(found)),
            Lookup::Missing => Lookup::Missing,
            Lookup::Unconfirmed => Lookup::Unconfirmed,
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct Columns {
    pub names: Vec<String>,
    // False when a property's `SqlFieldName` may be missing: an IRIS ancestor's, from
    // declarations without SQL names, or a class nobody can see.
    pub complete: bool,
}

#[derive(Clone)]
pub(crate) struct ProcInfo {
    pub is_query: bool,
    pub takes: usize,
    pub variadic: bool,
}

// What the workspace's classes project, by upper-cased schema and name.
#[derive(Default)]
struct Index {
    classes: HashMap<(String, String), String>,
    // Several classes may claim one name; the one with a table wins at lookup.
    tables: HashMap<(String, String), Vec<Table>>,
    // An explicit `SqlName`: the class declaring it.
    procedures: HashMap<(String, String), String>,
}

// The class behind a table or a view.
#[derive(Clone)]
enum Table {
    Class(String),
    View(String),
}

#[derive(Default)]
pub(crate) struct SqlState {
    index: Option<Index>,
    // A view's or an unknown table's columns are `None`.
    tables: HashMap<String, Lookup<Option<Columns>>>,
    procedures: HashMap<String, Lookup<ProcInfo>>,
}

struct SqlMember {
    origin: String,
    name: String,
    is_query: bool,
    takes: usize,
    variadic: bool,
    sql: Option<bindings::SqlProjection>,
    from_iris: bool,
}

fn key(schema: &str, name: &str) -> (String, String) {
    (schema.to_ascii_uppercase(), name.to_ascii_uppercase())
}

// `Class_Member`, a leading `%` spelled `sys_`.
fn default_procedure_name(class: &str, member: &str) -> String {
    let class = class.rsplit('.').next().unwrap_or(class);
    match member.strip_prefix('%') {
        Some(rest) => format!("{class}_sys_{rest}"),
        None => format!("{class}_{member}"),
    }
}

// A `list Of` property is a column; an `array Of` property is a child table (see `details`).
fn is_array(ty: &Option<Ty>, element_ty: &Option<Ty>) -> bool {
    element_ty.is_some()
        && ty
            .as_ref()
            .is_some_and(|ty| !ty.to_string().ends_with("List"))
}

fn package_of(schema: &str) -> String {
    if schema.eq_ignore_ascii_case("SQLUser") {
        "User".to_string()
    } else {
        schema.replace('_', ".")
    }
}

impl<'a> WorkspaceEnv<'a> {
    fn sql_names_declared(&self) -> bool {
        self.workspace
            .iris
            .as_ref()
            .is_none_or(|iris| iris.sql_names_declared())
    }

    fn with_index<R>(&self, f: impl FnOnce(&Index) -> R) -> R {
        if self.sql.borrow().index.is_none() {
            let index = self.build_index();
            self.sql.borrow_mut().index = Some(index);
        }
        let state = self.sql.borrow();
        f(state.index.as_ref().expect("built above"))
    }

    fn build_index(&self) -> Index {
        let mut index = Index::default();
        for (_, uri) in self.workspace.class_uris() {
            let Ok(details) = self.workspace.cls_details(&uri) else {
                continue;
            };
            let name = details.name.it.clone();
            let Some((package, short)) = name.rsplit_once('.') else {
                continue;
            };
            let schema = sql_schema(package);
            index.classes.insert(key(&schema, short), name.clone());
            let table = details.keywords.sql_table_name.as_deref().unwrap_or(short);
            (index.tables.entry(key(&schema, table)).or_default()).push(Table::Class(name.clone()));
            for (member, m) in details.members.method.iter().chain(&details.members.query) {
                let sql = match &m.it.kind {
                    MemberKindSpecificDetails::MethodOrClassMethod(method) => method.sql.as_ref(),
                    MemberKindSpecificDetails::Query { sql, .. } => sql.as_ref(),
                    _ => None,
                };
                let Some(sql) = sql else {
                    continue;
                };
                if sql.procedure
                    && let Some(sql_name) = &sql.name
                {
                    index
                        .procedures
                        .insert(key(&schema, sql_name), name.clone());
                }
                if sql.view {
                    let view = sql.view_name.as_deref().unwrap_or(member);
                    (index.tables.entry(key(&schema, view)).or_default())
                        .push(Table::View(name.clone()));
                }
            }
        }
        index
    }

    // Whether the class's ancestry reaches `%Persistent`, by name or `ClassType`, or an ancestor
    // nobody can see, who may.
    fn projects_table(&self, cls: &str) -> bool {
        self.ancestry(cls).any(|(name, class)| match class {
            Some(Class::Workspace(_, details)) => {
                details.keywords.class_type == Some(bindings::ClassType::Persistent)
            }
            Some(Class::Iris(info)) => info.class_type == Some(bindings::ClassType::Persistent),
            Some(Class::Unknown(_)) | None => name == "%Library.Persistent" || name != cls,
        })
    }

    // Likewise for `%SerialObject`, whose properties flatten into the table of a class using it.
    fn is_serial(&self, cls: &str) -> bool {
        for (name, class) in self.ancestry(cls) {
            let class_type = match class {
                Some(Class::Workspace(_, details)) => details.keywords.class_type,
                Some(Class::Iris(info)) => info.class_type,
                Some(Class::Unknown(_)) | None => {
                    return name == "%Library.SerialObject"
                        || (name != cls && name != "%Library.Persistent");
                }
            };
            match class_type {
                Some(bindings::ClassType::Serial) => return true,
                Some(_) => return false,
                None => {}
            }
        }
        false
    }

    pub(crate) fn sql_table(&self, schema: &str, table: &str) -> Lookup<Option<Columns>> {
        let cache_key = format!("{schema}.{table}").to_ascii_uppercase();
        if let Some(known) = self.sql.borrow().tables.get(&cache_key) {
            return known.clone();
        }
        let lookup = self.resolve_table(schema, table);
        self.sql
            .borrow_mut()
            .tables
            .insert(cache_key, lookup.clone());
        lookup
    }

    fn resolve_table(&self, schema: &str, table: &str) -> Lookup<Option<Columns>> {
        // A dependency on the class so named, should one open.
        self.resolve_uri(DocKind::Cls, &format!("{}.{table}", package_of(schema)));
        if let Some(found) = self.workspace_table(schema, table) {
            return Lookup::Found(self.table_columns(&found));
        }
        // `Table_Column`, the child table of an `array Of` property: IRIS lists it as a class of
        // its own, with none of its columns.
        for (i, _) in table.match_indices('_') {
            let (parent, column) = (&table[..i], &table[i + 1..]);
            if let Lookup::Found(Table::Class(cls)) = self.table_class(schema, parent)
                && let Some(column) = self.array_property(&cls, column)
            {
                let names = ["ID", parent, "element_key", &column].map(str::to_string);
                return Lookup::Found(Some(Columns {
                    names: names.to_vec(),
                    complete: true,
                }));
            }
        }
        self.iris_table(schema, table)
            .map(|found| self.table_columns(&found))
    }

    fn table_columns(&self, table: &Table) -> Option<Columns> {
        match table {
            Table::View(_) => None,
            Table::Class(cls) => Some(self.sql_columns(cls)),
        }
    }

    // The class projected as the table or view, in the workspace or on IRIS.
    fn table_class(&self, schema: &str, table: &str) -> Lookup<Table> {
        match self.workspace_table(schema, table) {
            Some(found) => Lookup::Found(found),
            None => self.iris_table(schema, table),
        }
    }

    fn workspace_table(&self, schema: &str, table: &str) -> Option<Table> {
        let candidates = self.with_index(|index| index.tables.get(&key(schema, table)).cloned());
        candidates
            .unwrap_or_default()
            .into_iter()
            .find(|candidate| match candidate {
                Table::View(cls) => self.resolve(cls).is_some(),
                Table::Class(cls) => self.projects_table(cls),
            })
    }

    fn iris_table(&self, schema: &str, table: &str) -> Lookup<Table> {
        let Some(iris) = self.workspace.iris.as_ref() else {
            return Lookup::Missing;
        };
        match iris.find_cls_by_sql(schema, table) {
            Ok(Some(cls)) => {
                let is_view = match self.resolve(&cls) {
                    Some(Class::Iris(info)) => {
                        super::views(&info).any(|view| view.eq_ignore_ascii_case(table))
                    }
                    _ => false,
                };
                if is_view {
                    Lookup::Found(Table::View(cls))
                } else if self.projects_table(&cls) {
                    Lookup::Found(Table::Class(cls))
                } else {
                    Lookup::Missing
                }
            }
            Ok(None) => Lookup::Missing,
            Err(_) => Lookup::Unconfirmed,
        }
    }

    // The column name of the `array Of` property of `cls` projected as `column`.
    fn array_property(&self, cls: &str, column: &str) -> Option<String> {
        self.ancestry(cls).find_map(|(_, class)| match class {
            Some(Class::Workspace(_, details)) => {
                details.members.property.iter().find_map(|(name, m)| {
                    let MemberKindSpecificDetails::Property {
                        ty,
                        element_ty,
                        sql_field_name,
                        ..
                    } = &m.it.kind
                    else {
                        return None;
                    };
                    let own = sql_field_name.as_deref().unwrap_or(name);
                    (is_array(ty, element_ty) && own.eq_ignore_ascii_case(column))
                        .then(|| own.to_string())
                })
            }
            Some(Class::Iris(info)) => info.members.iter().find_map(|m| match &m.kind {
                MemberKind::Property(p) if p.collection == Some(CollectionKind::Array) => {
                    let own = p.sql_field_name.as_deref().unwrap_or(&m.name);
                    own.eq_ignore_ascii_case(column).then(|| own.to_string())
                }
                _ => None,
            }),
            Some(Class::Unknown(_)) | None => None,
        })
    }

    fn sql_columns(&self, cls: &str) -> Columns {
        let (mut columns, row_id) = self.property_columns(cls, 0);
        columns
            .names
            .push(row_id.unwrap_or_else(|| "ID".to_string()));
        columns
    }

    // A multidimensional or `array Of` property and the `many`/`children` side of a relationship
    // have no column; a serial property's fields come along as `Property_Field`.
    fn property_columns(&self, cls: &str, depth: usize) -> (Columns, Option<String>) {
        let declared = self.sql_names_declared();
        let mut columns = Columns {
            names: vec![],
            complete: true,
        };
        let mut row_id = None;
        for (name, class) in self.ancestry(cls) {
            match class {
                Some(Class::Workspace(_, details)) => {
                    row_id = row_id.or(details.keywords.sql_row_id_name.clone());
                    for (property, m) in &details.members.property {
                        let MemberKindSpecificDetails::Property {
                            ty,
                            element_ty,
                            multidimensional,
                            sql_field_name,
                            ..
                        } = &m.it.kind
                        else {
                            continue;
                        };
                        let many = matches!(ty, Some(Ty::OREF(ClsOrIns::Ins(c))) if c.name == "%Library.RelationshipObject");
                        if *multidimensional || many || is_array(ty, element_ty) {
                            continue;
                        }
                        let column = sql_field_name.clone().unwrap_or_else(|| property.clone());
                        if element_ty.is_none()
                            && let Some(Ty::OREF(ClsOrIns::Ins(c))) = ty
                        {
                            self.embedded_columns(&c.name, &name, &column, &mut columns, depth);
                        }
                        columns.names.push(column);
                    }
                }
                Some(Class::Iris(info)) => {
                    row_id = row_id.or(info.sql_row_id_name.clone());
                    for m in &info.members {
                        match &m.kind {
                            MemberKind::Property(p)
                                if !p.multidimensional
                                    && p.collection != Some(CollectionKind::Array) =>
                            {
                                if !declared && !m.name.starts_with('%') {
                                    columns.complete = false;
                                }
                                let column =
                                    p.sql_field_name.clone().unwrap_or_else(|| m.name.clone());
                                if p.collection.is_none()
                                    && let Some(t) = &p.t
                                {
                                    self.embedded_columns(t, &name, &column, &mut columns, depth);
                                }
                                columns.names.push(column);
                            }
                            MemberKind::Relationship(r)
                                if matches!(
                                    r.cardinality,
                                    Some(Cardinality::One | Cardinality::Parent)
                                ) =>
                            {
                                let column =
                                    r.sql_field_name.clone().unwrap_or_else(|| m.name.clone());
                                columns.names.push(column)
                            }
                            _ => {}
                        }
                    }
                }
                Some(Class::Unknown(_)) => columns.complete = false,
                None => {}
            }
        }
        (columns, row_id)
    }

    fn embedded_columns(
        &self,
        ty: &str,
        context: &str,
        prefix: &str,
        columns: &mut Columns,
        depth: usize,
    ) {
        if depth > 3 {
            return;
        }
        let ty = Ty::from(ty);
        let Ty::OREF(ClsOrIns::Ins(c)) = &ty else {
            return;
        };
        let class = self.qualify_class_name(&c.name, Some(context));
        if !self.is_serial(&class) {
            return;
        }
        let (inner, _) = self.property_columns(&class, depth + 1);
        columns.complete &= inner.complete;
        for field in inner.names {
            columns.names.push(format!("{prefix}_{field}"));
        }
    }

    pub(crate) fn sql_procedure(&self, schema: &str, name: &str) -> Lookup<ProcInfo> {
        let cache_key = format!("{schema}.{name}").to_ascii_uppercase();
        if let Some(known) = self.sql.borrow().procedures.get(&cache_key) {
            return known.clone();
        }
        let lookup = self.resolve_procedure(schema, name);
        self.sql
            .borrow_mut()
            .procedures
            .insert(cache_key, lookup.clone());
        lookup
    }

    fn resolve_procedure(&self, schema: &str, name: &str) -> Lookup<ProcInfo> {
        // `Class_Member`: the class before the first underscore, in the workspace or on IRIS.
        if let Some((short, _)) = name.split_once('_') {
            let cls = self
                .with_index(|index| index.classes.get(&key(schema, short)).cloned())
                .unwrap_or_else(|| format!("{}.{short}", package_of(schema)));
            if let Some(found) = self.procedure_in(&cls, name) {
                return Lookup::Found(found);
            }
        }
        if let Some(cls) =
            self.with_index(|index| index.procedures.get(&key(schema, name)).cloned())
            && let Some(found) = self.procedure_in(&cls, name)
        {
            return Lookup::Found(found);
        }
        let Some(iris) = self.workspace.iris.as_ref() else {
            return Lookup::Missing;
        };
        match iris.find_sql_proc(schema, name) {
            Ok(Some(cls)) => self
                .procedure_in(&cls, name)
                .map_or(Lookup::Missing, Lookup::Found),
            Ok(None) => Lookup::Missing,
            Err(_) => Lookup::Unconfirmed,
        }
    }

    // The member of `cls` or its ancestors that SQL knows as `name`. A `SqlName` applies to the
    // class declaring it; a subclass projects the member as `Subclass_Member` again.
    fn procedure_in(&self, cls: &str, name: &str) -> Option<ProcInfo> {
        let declared = self.sql_names_declared();
        self.sql_members(cls).into_iter().find_map(|member| {
            let own_name = (member.sql.as_ref())
                .filter(|_| member.origin == cls)
                .and_then(|sql| sql.name.clone());
            let effective = own_name.unwrap_or_else(|| default_procedure_name(cls, &member.name));
            if !effective.eq_ignore_ascii_case(name) {
                return None;
            }
            let is_procedure = member.sql.as_ref().is_some_and(|sql| sql.procedure)
                || (member.from_iris && !declared);
            is_procedure.then_some(ProcInfo {
                is_query: member.is_query,
                takes: member.takes,
                variadic: member.variadic,
            })
        })
    }

    fn sql_members(&self, cls: &str) -> Vec<SqlMember> {
        let mut members = vec![];
        for (origin, class) in self.ancestry(cls) {
            match class {
                Some(Class::Workspace(_, details)) => {
                    for (name, m) in details.members.method.iter().chain(&details.members.query) {
                        let (is_query, takes, variadic, sql) = match &m.it.kind {
                            MemberKindSpecificDetails::MethodOrClassMethod(method) => (
                                false,
                                method.normal_args.len(),
                                method.variadic_arg.is_some(),
                                method.sql.clone(),
                            ),
                            MemberKindSpecificDetails::Query {
                                normal_args,
                                variadic_arg,
                                sql,
                                ..
                            } => (true, normal_args.len(), variadic_arg.is_some(), sql.clone()),
                            _ => continue,
                        };
                        members.push(SqlMember {
                            origin: origin.clone(),
                            name: name.clone(),
                            is_query,
                            takes,
                            variadic,
                            sql,
                            from_iris: false,
                        });
                    }
                }
                Some(Class::Iris(info)) => {
                    for m in &info.members {
                        let (sig, is_query) = match &m.kind {
                            MemberKind::Method(sig) | MemberKind::ClassMethod(sig) => (sig, false),
                            MemberKind::Query(sig) => (sig, true),
                            _ => continue,
                        };
                        members.push(SqlMember {
                            origin: origin.clone(),
                            name: m.name.clone(),
                            is_query,
                            takes: sig.normal.len(),
                            variadic: sig.variadic.is_some(),
                            sql: sig.sql.clone(),
                            from_iris: true,
                        });
                    }
                }
                Some(Class::Unknown(_)) | None => {}
            }
        }
        members
    }
}

// `schema.name` as written; an unqualified name is in the resolver's default schema.
fn qualified(check: &dyn Resolver, parts: &[String]) -> Option<(String, String)> {
    match parts {
        [name] => Some((check.default_schema(), name.clone())),
        [schema, name] => Some((schema.clone(), name.clone())),
        _ => None,
    }
}

// Whether the label names the table: its alias when it has one, else its name's last parts.
fn labeled(table: &TableRef, label: &[String]) -> bool {
    match (&table.alias, &table.name) {
        (Some(alias), _) => matches!(label, [only] if only.eq_ignore_ascii_case(alias)),
        (None, Some(name)) => {
            label.len() <= name.parts.len()
                && (name.parts[name.parts.len() - label.len()..].iter())
                    .zip(label)
                    .all(|(part, written)| part.eq_ignore_ascii_case(written))
        }
        (None, None) => false,
    }
}

fn has_column(columns: &Columns, name: &str) -> bool {
    columns
        .names
        .iter()
        .any(|column| column.eq_ignore_ascii_case(name))
}

// `%ID` and the other `%` pseudo-columns, and the class name column every table has.
fn pseudo_column(name: &str) -> bool {
    name.starts_with('%') || name.eq_ignore_ascii_case("x__classname")
}

struct Checker<'c, 'a> {
    code: &'c CodeEnv<'a>,
    flow: &'c Flow<'a>,
}

impl Resolver for Checker<'_, '_> {
    // The class's package's schema, which IRIS 2026.2 searches alone: not `SQLUser` after it, nor a
    // parent package's schema, whatever `SqlSchemaName` the class has. A routine's is `SQLUser`.
    fn default_schema(&self) -> String {
        match self.code.this() {
            Ok(this) => sql_schema(this.name().rsplit_once('.').map_or("User", |(p, _)| p)),
            Err(_) => "SQLUser".to_string(),
        }
    }

    fn table(&self, schema: &str, table: &str) -> Lookup<Option<Columns>> {
        self.code.sql_table(schema, table)
    }

    fn procedure(&self, schema: &str, name: &str) -> Lookup<ProcInfo> {
        self.code.sql_procedure(schema, name)
    }

    fn report(&self, kind: TypeErrorKind, range: &std::ops::Range<usize>) {
        self.flow
            .add_soft_diagnostic(kind.at(self.code.span(range.start, range.end)).into())
    }
}

// Walks the statement, checking its names as they come, and returns its host variables.
pub(in crate::check) fn check(code: &CodeEnv<'_>, flow: &mut Flow<'_>, statement: &SQL) -> Hosts {
    super::analyze(statement, Some(&Checker { code, flow }))
}

// A table's columns when they are known: not a CTE's, a view's, or a table nobody has, which is
// reported here.
pub(super) fn table(check: &dyn Resolver, frames: &[Frame], name: &Name) -> Option<Columns> {
    if let [cte] = name.parts.as_slice()
        && (frames.iter()).any(|frame| frame.ctes.iter().any(|c| c.eq_ignore_ascii_case(cte)))
    {
        return None;
    }
    let (schema, table) = qualified(check, &name.parts)?;
    let written = name.parts.join(".");
    match check.table(&schema, &table) {
        Lookup::Found(columns) => columns,
        Lookup::Missing => {
            check.report(
                TypeErrorKind::SqlTableNotFound { table: written },
                &name.range,
            );
            None
        }
        Lookup::Unconfirmed => {
            let what = format!("the table '{written}'");
            check.report(TypeErrorKind::SqlUnconfirmed { what }, &name.range);
            None
        }
    }
}

pub(super) fn call(check: &dyn Resolver, name: &Name, args: usize, used: SqlProcedureUse) {
    let Some((schema, procedure)) = qualified(check, &name.parts) else {
        return;
    };
    // A function without a schema may be a built-in the grammar doesn't list.
    if used == SqlProcedureUse::Function && name.parts.len() < 2 {
        return;
    }
    let written = name.parts.join(".");
    let kind = match check.procedure(&schema, &procedure) {
        Lookup::Missing => TypeErrorKind::SqlProcedureNotFound {
            name: written,
            used,
        },
        Lookup::Unconfirmed => TypeErrorKind::SqlUnconfirmed {
            what: format!("the {used} '{written}'"),
        },
        Lookup::Found(procedure) => match (procedure.is_query, used) {
            (true, SqlProcedureUse::Function) | (false, SqlProcedureUse::Table) => {
                TypeErrorKind::SqlProcedureMisused {
                    name: written,
                    actual: if procedure.is_query {
                        SqlMemberKind::Query
                    } else {
                        SqlMemberKind::Method
                    },
                    used,
                }
            }
            _ if args > procedure.takes && !procedure.variadic => {
                TypeErrorKind::SqlTooManyArguments {
                    name: written,
                    takes: ArgCount(procedure.takes),
                }
            }
            _ => return,
        },
    };
    check.report(kind, &name.range);
}

// A label naming no table in scope, or a field no table of a frame has, the innermost frame
// first, where every table's columns are known.
pub(super) fn column_issue(frames: &[Frame], column: &ColumnRef) -> Option<TypeErrorKind> {
    // A statement's frame holds its first branch's names for the trailing `ORDER BY`, which is
    // checked in it; a branch under it does not see them.
    let frames: Vec<&Frame> = (frames.iter().rev().enumerate())
        .filter(|(i, frame)| *i == 0 || !frame.branches)
        .map(|(_, frame)| frame)
        .collect();
    if frames.iter().all(|frame| frame.tables.is_empty()) {
        return None;
    }
    if !column.qualifier.is_empty() {
        let found = (frames.iter()).find_map(|frame| {
            (frame.tables.iter()).find(|(table, _)| labeled(table, &column.qualifier))
        });
        let Some((_, columns)) = found else {
            return Some(TypeErrorKind::SqlLabelNotFound {
                label: column.qualifier.join("."),
            });
        };
        let (Some(name), Some(columns)) = (&column.name, columns) else {
            return None;
        };
        return field_issue(columns, name, &column.qualifier.join("."));
    }
    let name = column.name.as_ref()?;
    if pseudo_column(name) {
        return None;
    }
    for frame in &frames {
        if frame
            .aliases
            .iter()
            .any(|alias| alias.eq_ignore_ascii_case(name))
        {
            return None;
        }
        let mut incomplete = false;
        for (_, columns) in &frame.tables {
            let Some(columns) = columns else {
                return None;
            };
            if has_column(columns, name) {
                return None;
            }
            incomplete |= !columns.complete;
        }
        if incomplete {
            let what = format!("the field '{name}' in {}", tables_text(frame));
            return Some(TypeErrorKind::SqlUnconfirmed { what });
        }
    }
    Some(TypeErrorKind::SqlFieldNotFound {
        field: name.clone(),
        tables: tables_text(frames[0]),
    })
}

fn tables_text(frame: &Frame) -> String {
    let names: Vec<String> = (frame.tables.iter())
        .filter_map(|(table, _)| table.name.as_ref().map(|name| name.parts.join(".")))
        .collect();
    names.join(", ")
}

fn field_issue(columns: &Columns, name: &str, table: &str) -> Option<TypeErrorKind> {
    if has_column(columns, name) || pseudo_column(name) {
        None
    } else if columns.complete {
        Some(TypeErrorKind::SqlFieldNotFound {
            field: format!("{table}.{name}"),
            tables: table.to_string(),
        })
    } else {
        Some(TypeErrorKind::SqlUnconfirmed {
            what: format!("the field '{name}' in {table}"),
        })
    }
}
