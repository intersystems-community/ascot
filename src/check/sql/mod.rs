//! The checker of embedded SQL. It walks the parsed statement node by node, the way the
//! ObjectScript checker does, and records what the flow needs -- every host variable as a read or
//! a write, and the cursor the statement declares or fetches -- and the names `resolve` checks.

use std::ops::Range;

use either::Either;
use tygr::{Prefix, Print, Suffix, Vec1, Wrap};

use crate::bindings::{ClassInfo, MemberKind, SqlProjection};
use crate::check::errors::{SqlProcedureUse, TypeErrorKind};
use resolve::{Columns, Lookup, ProcInfo};
use tygr_objectscript::cos::VecSep;
use tygr_objectscript::sql::*;

mod expr;
mod query;
pub(crate) mod resolve;
mod stat;

#[derive(Default)]
pub(super) struct Hosts {
    pub reads: Vec<(String, bool)>,
    pub writes: Vec<(String, bool)>,
    pub declares: Option<String>,
    pub fetches: Option<String>,
}

// A query's bindings while it is walked: its `FROM` items, each with its table's columns when
// they are known, the CTEs of a `WITH`, and the select aliases.
#[derive(Default)]
pub(crate) struct Frame {
    pub tables: Vec<(TableRef, Option<Columns>)>,
    pub ctes: Vec<String>,
    pub aliases: Vec<String>,
    // A `SELECT` statement around its branches: it takes the first branch's names, which its
    // trailing `ORDER BY` sees and the other branches don't.
    pub branches: bool,
}

#[derive(Clone)]
pub(crate) struct TableRef {
    // `None` for a subquery, a table-valued function or a JSON_TABLE.
    pub name: Option<Name>,
    pub alias: Option<String>,
}

// A dotted name as SQL reads it, quotes dropped, at its byte range in the text.
#[derive(Clone)]
pub(crate) struct Name {
    pub parts: Vec<String>,
    pub range: Range<usize>,
}

pub(crate) struct ColumnRef {
    pub qualifier: Vec<String>,
    // `None` for `label.*`.
    pub name: Option<String>,
    pub range: Range<usize>,
}

// What the walker asks of the checker, when there is one (see `resolve`).
pub(crate) trait Resolver {
    fn table(&self, schema: &str, table: &str) -> Lookup<Option<Columns>>;
    fn procedure(&self, schema: &str, name: &str) -> Lookup<ProcInfo>;
    fn report(&self, kind: TypeErrorKind, range: &Range<usize>);
}

// An identifier's text, a delimited one without its quotes.
fn text(ident: &Ident) -> String {
    match ident {
        Ident::Simple(name) => name.print(),
        Ident::Delimited(name) => name.print().trim_matches('"').replace("\"\"", "\""),
    }
}

fn name_of(dotted: &Src<DottedIdent>) -> Name {
    Name {
        parts: dotted.parts().map(text).collect(),
        range: dotted.range(),
    }
}

// A method's or class query's SQL projection, as declared.
pub(crate) fn projection(kind: &MemberKind) -> Option<&SqlProjection> {
    match kind {
        MemberKind::Method(sig) | MemberKind::ClassMethod(sig) | MemberKind::Query(sig) => {
            sig.sql.as_ref()
        }
        _ => None,
    }
}

// The views the class's `[ SqlView ]` queries project, by name.
pub(crate) fn views(info: &ClassInfo) -> impl Iterator<Item = &str> {
    info.members.iter().filter_map(|m| match &m.kind {
        MemberKind::Query(sig) => {
            let sql = sig.sql.as_ref().filter(|sql| sql.view)?;
            Some(sql.view_name.as_deref().unwrap_or(&m.name))
        }
        _ => None,
    })
}

pub(super) fn analyze(sql: &SQL, check: Option<&dyn Resolver>) -> Hosts {
    let mut w = Walker {
        hosts: Hosts::default(),
        frames: vec![Frame::default()],
        check,
        unchecked: false,
        access: Access::Read,
    };
    sql.stat.walk(&mut w);
    w.hosts
}

#[derive(Clone, Copy, PartialEq)]
enum Access {
    Read,
    Write,
}

struct Walker<'r> {
    hosts: Hosts,
    // The statement's frame first, the innermost query's last.
    frames: Vec<Frame>,
    check: Option<&'r dyn Resolver>,
    // Inside DDL or a statement for a foreign server, whose names are not this namespace's.
    unchecked: bool,
    access: Access,
}

impl Walker<'_> {
    fn unchecked<T: Walk + ?Sized>(&mut self, t: &T) {
        let was = std::mem::replace(&mut self.unchecked, true);
        t.walk(self);
        self.unchecked = was;
    }

    fn push_frame(&mut self) {
        self.frames.push(Frame::default());
    }

    // A statement's frame takes its first branch's names, for the trailing `ORDER BY`.
    fn pop_frame(&mut self) {
        let frame = self.frames.pop().expect("a frame was pushed");
        if let Some(statement) = self.frames.last_mut()
            && statement.branches
            && statement.tables.is_empty()
            && statement.aliases.is_empty()
        {
            statement.tables = frame.tables;
            statement.aliases = frame.aliases;
        }
    }

    fn current(&mut self) -> &mut Frame {
        self.frames.last_mut().expect("the statement's frame stays")
    }

    // Whether the current frame is a branch of a `SELECT` statement.
    fn in_statement(&self) -> bool {
        self.frames.len() >= 2 && self.frames[self.frames.len() - 2].branches
    }

    // Walks `t` in the enclosing frame: the query feeding an `INSERT` does not see its target.
    fn outside<T: Walk + ?Sized>(&mut self, t: &T) {
        let frame = self.frames.pop().expect("a frame was pushed");
        t.walk(self);
        self.frames.push(frame);
    }

    fn table(&mut self, name: Option<Name>, alias: Option<&Alias>) {
        if self.unchecked {
            return;
        }
        let alias = alias.map(|alias| text(&alias.name().0));
        let columns = match (&name, self.check) {
            (Some(name), Some(check)) => resolve::table(check, &self.frames, name),
            _ => None,
        };
        self.current()
            .tables
            .push((TableRef { name, alias }, columns));
    }

    fn alias(&mut self, alias: &SQLIdentNonKeyword) {
        let alias = text(&alias.0);
        self.current().aliases.push(alias);
    }

    fn column(&mut self, column: &Colmn) {
        let Some(check) = self.check.filter(|_| !self.unchecked) else {
            return;
        };
        let column = match column {
            Colmn::Str(label, ..) => ColumnRef {
                qualifier: vec![text(label)],
                name: None,
                range: label.range(),
            },
            Colmn::Arr(dotted, _) => {
                let mut qualifier: Vec<String> = dotted.parts().map(text).collect();
                let name = qualifier.pop();
                ColumnRef {
                    qualifier,
                    name,
                    range: dotted.range(),
                }
            }
        };
        if let Some(kind) = resolve::column_issue(&self.frames, &column) {
            check.report(kind, &column.range);
        }
    }

    fn call(&mut self, name: &Src<DottedIdent>, args: usize, used: SqlProcedureUse) {
        if let Some(check) = self.check.filter(|_| !self.unchecked) {
            resolve::call(check, &name_of(name), args, used);
        }
    }

    fn written<T: Walk + ?Sized>(&mut self, t: &T) {
        self.with(Access::Write, t)
    }

    fn read<T: Walk + ?Sized>(&mut self, t: &T) {
        self.with(Access::Read, t)
    }

    fn with<T: Walk + ?Sized>(&mut self, access: Access, t: &T) {
        let was = std::mem::replace(&mut self.access, access);
        t.walk(self);
        self.access = was;
    }

    fn host(&mut self, host_ref: &HostRef, subscripted: bool) {
        match host_ref {
            HostRef::InstanceVariableHostRef(_) => {}
            HostRef::PropertyHostRef(prop) => self.hosts.reads.push((prop.part1.print(), false)),
            HostRef::Plain(_, name) => self.local(name.print(), subscripted),
        }
    }

    fn local(&mut self, name: String, subscripted: bool) {
        match self.access {
            Access::Write => self.hosts.writes.push((name, subscripted)),
            Access::Read => self.hosts.reads.push((name, subscripted)),
        }
    }

    fn ident(&mut self, ident: &Ident, subscripted: bool) {
        if let Ident::Simple(name) = ident {
            self.local(name.print(), subscripted)
        }
    }
}

trait Walk {
    fn walk(&self, w: &mut Walker);
}

macro_rules! leaf {
    ($($t:ty),*) => {$(impl Walk for $t { fn walk(&self, _: &mut Walker) {} })*};
}
leaf!(
    Integer,
    VectorDatyp,
    TSAddInterval,
    TSDiffInterval,
    SQLConvertFunc
);

impl<T: Walk> Walk for Option<T> {
    fn walk(&self, w: &mut Walker) {
        if let Some(t) = self {
            t.walk(w)
        }
    }
}

impl<T: Walk> Walk for Vec<T> {
    fn walk(&self, w: &mut Walker) {
        for t in self {
            t.walk(w)
        }
    }
}

impl<T: Walk> Walk for Vec1<T> {
    fn walk(&self, w: &mut Walker) {
        for t in self.iter() {
            t.walk(w)
        }
    }
}

impl<T: Walk> Walk for Box<T> {
    fn walk(&self, w: &mut Walker) {
        (**self).walk(w)
    }
}

impl<T: Walk> Walk for Src<T> {
    fn walk(&self, w: &mut Walker) {
        self.node.walk(w)
    }
}

impl<T: Walk, S> Walk for VecSep<T, S> {
    fn walk(&self, w: &mut Walker) {
        self.items.walk(w)
    }
}

impl<P, T: Walk, S> Walk for Wrap<P, T, S> {
    fn walk(&self, w: &mut Walker) {
        self.wrapped.walk(w)
    }
}

impl<P, T: Walk> Walk for Prefix<P, T> {
    fn walk(&self, w: &mut Walker) {
        self.prefixed.walk(w)
    }
}

impl<T: Walk, S> Walk for Suffix<T, S> {
    fn walk(&self, w: &mut Walker) {
        self.suffixed.walk(w)
    }
}

impl<A: Walk, B: Walk> Walk for Either<A, B> {
    fn walk(&self, w: &mut Walker) {
        match self {
            Either::Left(a) => a.walk(w),
            Either::Right(b) => b.walk(w),
        }
    }
}

impl<T: Walk + tygr::Grammar> Walk for OptionallyWrapped<T> {
    fn walk(&self, w: &mut Walker) {
        match self {
            OptionallyWrapped::Wrapped(t) => t.walk(w),
            OptionallyWrapped::Scalar(t) => t.walk(w),
        }
    }
}

impl Walk for HostVar {
    fn walk(&self, w: &mut Walker) {
        w.host(&self.host_ref, self.subs.is_some());
        w.read(&self.subs)
    }
}

impl Walk for Sub {
    fn walk(&self, w: &mut Walker) {
        match self {
            Sub::Literal(_) => {}
            Sub::Var(var) => var.walk(w),
        }
    }
}

impl Walk for Var {
    fn walk(&self, w: &mut Walker) {
        match self {
            Var::HostRef(host_ref) => w.host(host_ref, false),
            Var::Ident(ident) => w.ident(ident, false),
        }
    }
}

impl Walk for Param {
    fn walk(&self, w: &mut Walker) {
        match self {
            Param::HostVar(host_var) => host_var.walk(w),
            Param::Ident(ident, subs) => {
                w.ident(ident, subs.is_some());
                w.read(subs)
            }
        }
    }
}

impl Walk for ValSpec {
    fn walk(&self, w: &mut Walker) {
        match self {
            ValSpec::Scalx { exprs, .. } => exprs.walk(w),
            ValSpec::Var { var, subs } => {
                var.walk(w);
                subs.walk(w)
            }
        }
    }
}

impl Walk for DTSValSpec {
    fn walk(&self, w: &mut Walker) {
        match self {
            DTSValSpec::Literal(_) => {}
            DTSValSpec::HostVar(host_var) => host_var.walk(w),
        }
    }
}

impl Walk for ColTrun {
    fn walk(&self, w: &mut Walker) {
        match self {
            ColTrun::Const(_) => {}
            ColTrun::HostVar(host_var) => host_var.walk(w),
        }
    }
}

impl Walk for DatePartCommon {
    fn walk(&self, w: &mut Walker) {
        if let DatePartCommon::HostVar(host_var) = self {
            host_var.walk(w)
        }
    }
}

impl Walk for DatePartPartRaw {
    fn walk(&self, w: &mut Walker) {
        if let DatePartPartRaw::Common(common) = self {
            common.walk(w)
        }
    }
}

impl Walk for DatePartPart {
    fn walk(&self, w: &mut Walker) {
        self.0.walk(w)
    }
}

impl Walk for Targt {
    fn walk(&self, w: &mut Walker) {
        if let Some((_, targt)) = &self.0 {
            match targt {
                TargtEnum::Into(into) => w.written(into),
                TargtEnum::IntoList(into) => w.written(&into.prefixed.1),
            }
        }
    }
}

impl Walk for HostList {
    fn walk(&self, w: &mut Walker) {
        self.0.walk(w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tygr::Parse;
    use tygr_objectscript::sql::desugar::Desugar;

    fn parse(text: &str) -> SQL {
        SQL::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"))
    }

    // `text` and the core text it desugars to, which the checker will move to: the walker, on the
    // full grammar still, finds the same host variables in both and reports the same codes.
    fn on_both(text: &str, mock: Option<&Mock>) -> (Hosts, Vec<String>) {
        let run = |text: &str| {
            let hosts = analyze(&parse(text), mock.map(|mock| mock as &dyn Resolver));
            (hosts, mock.map_or_else(Vec::new, |mock| mock.0.take()))
        };
        fn codes(reports: &[String]) -> Vec<&str> {
            reports.iter().map(|report| &report[..5]).collect()
        }
        let (hosts, reports) = run(text);
        let (core_hosts, core_reports) = run(&parse(text).desugar().print());
        assert_eq!(names(&hosts.reads), names(&core_hosts.reads), "{text}");
        assert_eq!(names(&hosts.writes), names(&core_hosts.writes), "{text}");
        assert_eq!(codes(&reports), codes(&core_reports), "{text}");
        (hosts, reports)
    }

    fn hosts_of(text: &str) -> Hosts {
        on_both(text, None).0
    }

    // Two tables and a view, a method and a class query; what gets reported, as `CODE at offset`.
    struct Mock(std::cell::RefCell<Vec<String>>);

    impl Resolver for Mock {
        fn table(&self, schema: &str, table: &str) -> Lookup<Option<Columns>> {
            let columns = |names: &[&str]| {
                Some(Columns {
                    names: names.iter().map(|n| n.to_string()).collect(),
                    complete: true,
                })
            };
            match (schema, table.to_ascii_uppercase().as_str()) {
                ("Demo", "T") => Lookup::Found(columns(&["Name", "Age", "ID"])),
                ("Demo", "U") => Lookup::Found(columns(&["Name", "Title", "ID"])),
                ("Demo", "V") => Lookup::Found(None),
                _ => Lookup::Missing,
            }
        }

        fn procedure(&self, _: &str, name: &str) -> Lookup<ProcInfo> {
            match name.to_ascii_uppercase().as_str() {
                "T_COUNT" => Lookup::Found(ProcInfo {
                    is_query: false,
                    takes: 0,
                    variadic: false,
                }),
                "T_BYNAME" => Lookup::Found(ProcInfo {
                    is_query: true,
                    takes: 1,
                    variadic: false,
                }),
                _ => Lookup::Missing,
            }
        }

        fn report(&self, kind: TypeErrorKind, range: &Range<usize>) {
            self.0
                .borrow_mut()
                .push(format!("{} at {}", kind.code(), range.start));
        }
    }

    fn reports(text: &str) -> Vec<String> {
        on_both(text, Some(&Mock(Default::default()))).1
    }

    #[test]
    fn every_table_column_and_procedure_of_a_statement() {
        let fine = [
            // A correlated reference reaches the enclosing query's names.
            "SELECT Name INTO :n FROM Demo.T p WHERE EXISTS (SELECT 1 FROM Demo.U u WHERE u.Title = p.Name)",
            // A trailing `ORDER BY` names the first branch's columns and aliases.
            "SELECT Name AS N FROM Demo.T UNION SELECT Name FROM Demo.U ORDER BY N",
            // A CTE's columns are not known, nor a view's.
            "WITH C AS (SELECT Name FROM Demo.T) SELECT Nope FROM C",
            "SELECT V.Nope FROM Demo.V",
            "SELECT Name FROM Demo.T t WHERE FOR SOME (Demo.U e) (e.Title = t.Name)",
            "INSERT INTO Demo.T (Name) SELECT Name FROM Demo.U",
            "SELECT %ID, x__classname FROM Demo.T t WHERE t.* IS NULL",
            "SELECT Demo.T_Count() FROM Demo.T",
            // A function without a schema may be a built-in.
            "SELECT Nope() FROM Demo.T",
            "THROUGH FOREIGN SERVER s SELECT Nope FROM Demo.Nope",
            "CREATE VIEW v AS SELECT Nope FROM Demo.Nope",
        ];
        for text in fine {
            assert_eq!(reports(text), Vec::<String>::new(), "{text}");
        }
        let wrong = [
            ("SELECT Name FROM Demo.Nope", "Demo.Nope", "A3261"),
            ("SELECT Nope FROM Demo.T", "Nope", "A7340"),
            ("SELECT p.Age FROM Demo.U p", "p.Age", "A7340"),
            (
                "SELECT Name FROM Demo.T p WHERE EXISTS (SELECT 1 FROM Demo.U u WHERE q.Name = 1)",
                "q.Name",
                "A5086",
            ),
            // A branch does not see the first branch's names; the `ORDER BY` does.
            (
                "SELECT Name FROM Demo.T UNION SELECT Age FROM Demo.U",
                "Age",
                "A7340",
            ),
            (
                "SELECT Name AS N FROM Demo.T UNION SELECT Name FROM Demo.U ORDER BY Nope",
                "Nope",
                "A7340",
            ),
            ("INSERT INTO Demo.T (Title) VALUES (1)", "Title", "A7340"),
            // The query feeding an `INSERT` does not see the target.
            (
                "INSERT INTO Demo.T (Name) SELECT Age FROM Demo.U",
                "Age",
                "A7340",
            ),
            (
                "SELECT Demo.T_Count(1) FROM Demo.T",
                "Demo.T_Count",
                "A1935",
            ),
            (
                "SELECT Demo.T_ByName('x') FROM Demo.T",
                "Demo.T_ByName",
                "A6418",
            ),
            ("CALL Demo.Nope()", "Demo.Nope", "A2957"),
        ];
        for (text, at, code) in wrong {
            let expected = format!("{code} at {}", text.find(at).unwrap());
            assert_eq!(reports(text), [expected], "{text}");
        }
    }

    fn names(occurrences: &[(String, bool)]) -> Vec<&str> {
        occurrences.iter().map(|(name, _)| name.as_str()).collect()
    }

    #[test]
    fn every_role_of_a_host_variable() {
        let h = hosts_of("SELECT Name INTO :name, :o.Prop FROM T WHERE Id = :id AND Age > :a(:i)");
        assert_eq!(names(&h.reads), ["id", "a", "i", "o"]);
        assert_eq!(h.writes, [("name".to_string(), false)]);
        assert!(h.reads.iter().any(|(n, s)| n == "a" && *s));

        let h = hosts_of("SELECT Name %INTOLIST :list(1) FROM T");
        assert_eq!(h.writes, [("list".to_string(), true)]);

        let h = hosts_of("DECLARE C1 CURSOR FOR SELECT Name INTO :n FROM T WHERE Id = :id");
        assert_eq!(h.declares.as_deref(), Some("C1"));
        assert_eq!(names(&h.reads), ["id"]);
        assert_eq!(names(&h.writes), ["n"]);

        let h = hosts_of("FETCH C1 INTO :f");
        assert_eq!(h.fetches.as_deref(), Some("C1"));
        assert_eq!(names(&h.writes), ["f"]);
        assert!(hosts_of("OPEN C1").writes.is_empty());

        let h = hosts_of(":ret = CALL Sample.Proc(:x, :y)");
        assert_eq!(names(&h.reads), ["x", "y"]);
        assert_eq!(names(&h.writes), ["ret"]);

        let h = hosts_of("INSERT INTO T (A, B) VALUES (:a, :b + 1)");
        assert_eq!(names(&h.reads), ["a", "b"]);
        assert!(h.writes.is_empty());

        let h = hosts_of("INSERT INTO T (A) SELECT X FROM U WHERE Y = :y");
        assert_eq!(names(&h.reads), ["y"]);

        let h =
            hosts_of("UPDATE T SET A = :a, B = (SELECT MAX(X) FROM U WHERE Z = :z) WHERE Id = :id");
        assert_eq!(names(&h.reads), ["id", "a", "z"]);

        let h = hosts_of(
            "SELECT X INTO :x FROM T WHERE Y IN (SELECT Y FROM U WHERE V = :v) UNION SELECT X FROM W WHERE Q = :q",
        );
        assert_eq!(names(&h.reads), ["v", "q"]);
        assert_eq!(names(&h.writes), ["x"]);

        let h = hosts_of("WITH C AS (SELECT X FROM T WHERE Y = :y) SELECT X INTO :x FROM C");
        assert_eq!(names(&h.reads), ["y"]);
        assert_eq!(names(&h.writes), ["x"]);

        let h = hosts_of("DELETE FROM T WHERE Id = :i%Id OR Name = :r%Name");
        assert!(h.reads.is_empty());
    }

    // Every `:name` a colon scan of the text finds, the walker finds too: no node that can hold
    // a host variable is skipped.
    #[test]
    fn the_walker_reaches_every_host_variable_of_the_statements_on_disk() {
        let Ok(text) = std::fs::read_to_string("/tmp/esql-statements.json") else {
            return;
        };
        let statements: Vec<String> = serde_json::from_str(&text).unwrap();
        let (mut parsed, mut missing) = (0, vec![]);
        for (i, statement) in statements.iter().enumerate() {
            let Ok(sql) = SQL::parse(statement) else {
                continue;
            };
            parsed += 1;
            let h = analyze(&sql, None);
            let found: Vec<&str> = h
                .reads
                .iter()
                .chain(&h.writes)
                .map(|(n, _)| n.as_str())
                .collect();
            for name in colon_scan(statement) {
                if !found.contains(&name.as_str()) {
                    missing.push(format!(
                        "{i}: {name} in {}",
                        &statement[..statement.len().min(80)]
                    ));
                }
            }
        }
        assert!(parsed > 0);
        assert!(missing.is_empty(), "{}", missing.join("\n"));
    }

    fn colon_scan(sql: &str) -> Vec<String> {
        let bytes = sql.as_bytes();
        let word_end = |mut j: usize| {
            while j < bytes.len()
                && (bytes[j].is_ascii_alphanumeric() || matches!(bytes[j], b'%' | b'_'))
            {
                j += 1;
            }
            j
        };
        let (mut names, mut i) = (vec![], 0);
        while i < bytes.len() {
            let c = bytes[i];
            if c == b'\'' || c == b'"' {
                i += 1 + bytes[i + 1..]
                    .iter()
                    .position(|&b| b == c)
                    .map_or(bytes.len(), |n| n + 1);
            } else if bytes[i..].starts_with(b"--") {
                i += bytes[i..]
                    .iter()
                    .position(|&b| b == b'\n')
                    .unwrap_or(bytes.len() - i);
            } else if bytes[i..].starts_with(b"/*") {
                i += sql[i..].find("*/").map_or(bytes.len() - i, |n| n + 2);
            } else if c == b':'
                && bytes
                    .get(i + 1)
                    .is_some_and(|n| n.is_ascii_alphabetic() || *n == b'%')
                && sql[..i].trim_end().as_bytes().last() != Some(&b'"')
            {
                let end = word_end(i + 1);
                let name = &sql[i + 1..end];
                if !name[1..].contains('%') {
                    names.push(name.to_string());
                }
                i = end;
                continue;
            }
            i += 1;
        }
        names
    }
}
