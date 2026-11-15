//! The checker of embedded SQL. It walks the parsed statement node by node, the way the
//! ObjectScript checker does, and today records what the flow needs: every host variable as a
//! read or a write, and the cursor the statement declares or fetches.

use either::Either;
use tygr::{Prefix, Print, Suffix, Vec1, Wrap};
use tygr_objectscript::cos::VecSep;
use tygr_objectscript::sql::*;

mod expr;
mod query;
mod stat;

#[derive(Default)]
pub(super) struct Hosts {
    pub reads: Vec<(String, bool)>,
    pub writes: Vec<(String, bool)>,
    pub declares: Option<String>,
    pub fetches: Option<String>,
}

pub(super) fn hosts(sql: &SQL) -> Hosts {
    let mut w = Walker {
        hosts: Hosts::default(),
        into: false,
    };
    sql.stat.walk(&mut w);
    w.hosts
}

struct Walker {
    hosts: Hosts,
    into: bool,
}

impl Walker {
    fn targets<T: Walk + ?Sized>(&mut self, t: &T) {
        self.mode(true, t)
    }

    fn inputs<T: Walk + ?Sized>(&mut self, t: &T) {
        self.mode(false, t)
    }

    fn mode<T: Walk + ?Sized>(&mut self, into: bool, t: &T) {
        let was = std::mem::replace(&mut self.into, into);
        t.walk(self);
        self.into = was;
    }

    fn host(&mut self, host_ref: &HostRef, subscripted: bool) {
        match host_ref {
            HostRef::InstanceVariableHostRef(_) => {}
            HostRef::PropertyHostRef(prop) => self.hosts.reads.push((prop.part1.print(), false)),
            HostRef::Plain(_, name) => self.local(name.print(), subscripted),
        }
    }

    fn local(&mut self, name: String, subscripted: bool) {
        if self.into {
            self.hosts.writes.push((name, subscripted))
        } else {
            self.hosts.reads.push((name, subscripted))
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
        w.inputs(&self.subs)
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
                w.inputs(subs)
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
                TargtEnum::Into(into) => w.targets(into),
                TargtEnum::IntoList(into) => w.targets(&into.prefixed.1),
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

    fn hosts_of(text: &str) -> Hosts {
        hosts(&SQL::parse(text).unwrap_or_else(|e| panic!("{text}: {e}")))
    }

    fn names(occurrences: &[(String, bool)]) -> Vec<&str> {
        occurrences.iter().map(|(name, _)| name.as_str()).collect()
    }

    #[test]
    fn every_role_of_a_host_variable() {
        let h = hosts_of("SELECT Name INTO :name, :o.Prop FROM T WHERE Id = :id AND Age > :a(:i)");
        assert_eq!(names(&h.reads), ["o", "id", "a", "i"]);
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
        assert_eq!(names(&h.reads), ["a", "z", "id"]);

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
            let h = hosts(&sql);
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
