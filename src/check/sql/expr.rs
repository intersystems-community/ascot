use either::Either;
use tygr_objectscript::sql::core::*;

use super::{SqlProcedureUse, Walk, Walker};

impl Walk for Scalx {
    fn walk(&self, w: &mut Walker) {
        let Scalx(terms) = self;
        terms.walk(w)
    }
}

impl Walk for Term {
    fn walk(&self, w: &mut Walker) {
        match self {
            Term::Builtin(f) => f.walk(w),
            Term::Window(window) => window.walk(w),
            Term::Agg(agg) => agg.walk(w),
            Term::Unary { term, .. } => term.walk(w),
            Term::XTFun(f) => f.args.walk(w),
            Term::Case(case) => case.walk(w),
            Term::Scalx(scalx) => scalx.walk(w),
            Term::Query(query) => query.walk(w),
            Term::Call(f) => {
                let args = f.args.1.wrapped.as_ref().map_or(0, |args| args.items.len());
                w.call(&f.name.0, args, SqlProcedureUse::Function);
                f.args.walk(w)
            }
            Term::HostVar(host_var) => host_var.walk(w),
            Term::Column(column) => w.column(column),
            Term::Macro(..)
            | Term::Literal(_)
            | Term::ParameterMarker(_)
            | Term::SpecialSystemVariable(_)
            | Term::Niladic(..)
            | Term::Null(_) => {}
        }
    }
}

impl<T1: Walk + tygr::Grammar, T2: Walk + tygr::Grammar> Walk for RR<T1, T2> {
    fn walk(&self, w: &mut Walker) {
        self.arg1.walk(w);
        self.arg2.walk(w)
    }
}

impl<T1: Walk + tygr::Grammar, T2: Walk + tygr::Grammar, T3: Walk + tygr::Grammar> Walk
    for RRR<T1, T2, T3>
{
    fn walk(&self, w: &mut Walker) {
        self.arg1.walk(w);
        self.arg2.walk(w);
        self.arg3.walk(w)
    }
}

impl<T1: Walk + tygr::Grammar, T2: Walk + tygr::Grammar> Walk for RO<T1, T2> {
    fn walk(&self, w: &mut Walker) {
        self.arg1.walk(w);
        if let Some((_, arg2)) = &self.arg2 {
            arg2.walk(w)
        }
    }
}

impl<T1: Walk + tygr::Grammar, T2: Walk + tygr::Grammar, T3: Walk + tygr::Grammar> Walk
    for ROO<T1, T2, T3>
{
    fn walk(&self, w: &mut Walker) {
        self.arg1.walk(w);
        if let Some((_, arg2, arg3)) = &self.rest {
            arg2.walk(w);
            if let Some((_, arg3)) = arg3 {
                arg3.walk(w)
            }
        }
    }
}

impl<T1: Walk + tygr::Grammar, T2: Walk + tygr::Grammar, T3: Walk + tygr::Grammar> Walk
    for RRO<T1, T2, T3>
{
    fn walk(&self, w: &mut Walker) {
        self.arg1.walk(w);
        self.arg2.walk(w);
        if let Some((_, arg3)) = &self.arg3 {
            arg3.walk(w)
        }
    }
}

impl<
    T1: Walk + tygr::Grammar,
    T2: Walk + tygr::Grammar,
    T3: Walk + tygr::Grammar,
    T4: Walk + tygr::Grammar,
> Walk for RROO<T1, T2, T3, T4>
{
    fn walk(&self, w: &mut Walker) {
        self.arg1.walk(w);
        self.arg2.walk(w);
        if let Some((_, arg3, arg4)) = &self.rest {
            arg3.walk(w);
            if let Some((_, arg4)) = arg4 {
                arg4.walk(w)
            }
        }
    }
}

impl Walk for Builtin {
    fn walk(&self, w: &mut Walker) {
        use Builtin::*;
        match self {
            XmlConcat(x) => x.walk(w),
            XmlElement(x) => x.walk(w),
            XmlForest(x) => x.walk(w),
            Cast(_, cast) => cast.walk(w),
            DateAdd(_, a) | DateDiff(_, a) => a.walk(w),
            DateName(_, a) | DateTrunc(_, a) => a.walk(w),
            DatePart(_, a) => a.walk(w),
            JSONArray(_, a) => {
                if let Some((items, _)) = &a.1.wrapped {
                    items.walk(w)
                }
            }
            JSONObject(_, a) => {
                if let Some((pairs, _)) = &a.1.wrapped {
                    pairs.walk(w)
                }
            }
            Predict(_, a) => with(w, &a.1.wrapped.with_clause),
            Probability(_, a) => {
                a.1.wrapped.for_clause.walk(w);
                with(w, &a.1.wrapped.with_clause)
            }
            SearchIndex(_, a) => {
                for (_, scalx) in &a.1.wrapped.scalx_list {
                    scalx.walk(w)
                }
            }
            TimestampAdd(_, a) => a.walk(w),
            TimestampDiff(_, a) => a.walk(w),
            ToVector(_, a) => a.walk(w),
            Trim(_, a) => a.walk(w),
            Odbc(a) => a.walk(w),
            CurrentTimestamp(_, a) | CurrentTime(_, a) => a.walk(w),
            DollarExtract(_, a) | DollarFNumber(_, a) | DollarList(_, a) | DollarListGet(_, a) => {
                a.walk(w)
            }
            DollarFind(_, a)
            | DollarJustify(_, a)
            | DollarListFind(_, a)
            | DollarTranslate(_, a) => a.walk(w),
            DollarLength(_, a)
            | DollarListData(_, a)
            | DollarListFromString(_, a)
            | DollarListToString(_, a) => a.walk(w),
            DollarListBuild(_, a) => a.walk(w),
            DollarListLength(_, a) => a.walk(w),
            DollarListSame(_, a)
            | DollarMvFmt(_, a)
            | DollarMvFmts(_, a)
            | DollarMvIconv(_, a)
            | DollarMvIconvs(_, a)
            | DollarMvOconv(_, a)
            | DollarMvOconvs(_, a) => a.walk(w),
            DollarPiece(_, a) => a.walk(w),
            TSqlObjectId(_, a) | TSqlObjectName(_, a) => a.walk(w),
            TSqlDbName(..) | TSqlError(..) | TSqlFetchStatus(..) | TSqlGetUtcDate(..)
            | TSqlHostName(..) | TSqlLockTimeout(..) | TSqlNestLevel(..) | TSqlServerName(..)
            | TSqlSpid(..) | TSqlSqlStatus(..) | TSqlTranCount(..) | TSqlVersion(..)
            | TSqlNewId(..) => {}
        }
    }
}

impl Walk for ODBCOnlyFunc {
    fn walk(&self, w: &mut Walker) {
        match self {
            ODBCOnlyFunc::Convert(_, a) => a.walk(w),
            ODBCOnlyFunc::IfNull(_, a) => a.walk(w),
            ODBCOnlyFunc::User(..) => {}
        }
    }
}

impl Walk for DatePart {
    fn walk(&self, w: &mut Walker) {
        if let DatePart::HostVar(host_var) = self {
            host_var.walk(w)
        }
    }
}

impl Walk for DatePartPart {
    fn walk(&self, w: &mut Walker) {
        if let DatePartPart::HostVar(host_var) = self {
            host_var.walk(w)
        }
    }
}

impl Walk for SQLJSONKeyValuePairs {
    fn walk(&self, w: &mut Walker) {
        for (key, _, value) in &self.0.items {
            key.walk(w);
            value.walk(w)
        }
    }
}

fn with<K>(w: &mut Walker, clause: &Option<(K, PredictWith)>) {
    if let Some((_, with)) = clause {
        with.walk(w)
    }
}

impl Walk for PredictWith {
    fn walk(&self, w: &mut Walker) {
        match self {
            PredictWith::Unnamed(list) => list.walk(w),
            PredictWith::Named(assignments) => assignments.walk(w),
        }
    }
}

impl Walk for PredictAssign {
    fn walk(&self, w: &mut Walker) {
        self.scalx.walk(w)
    }
}

impl Walk for TrimArgs {
    fn walk(&self, w: &mut Walker) {
        let TrimHow::SpecChars(_, _, chars, _) = &self.how;
        chars.walk(w);
        self.expr.walk(w)
    }
}

impl Walk for SQLCastExpression {
    fn walk(&self, w: &mut Walker) {
        use CastType::*;
        self.expr.walk(w);
        match &self.cast_type {
            Binary(_, _, a) | Character(_, _, a) | Char(_, _, a) => a.walk(w),
            Nchar(_, a) | Nvarchar(_, a) | Varbinary(_, a) | Varchar(_, a) => a.walk(w),
            Decimal(_, a) | Dec(_, a) | Numeric(_, a) => {
                if let Some(a) = a {
                    a.wrapped.0.walk(w);
                    if let Some((_, scale)) = &a.wrapped.1 {
                        scale.walk(w)
                    }
                }
            }
            Bigint(_) | Bit(_) | Datetime(_) | Date(_) | Double(_) | Float(_) | Guid(_)
            | Integer(_) | Int(_) | Money(_) | Posixtime(_) | Real(_) | Smalldatetime(_)
            | Smallint(_) | Smallmoney(_) | Timestamp(_) | Time(_) | Tinyint(_) => {}
        }
    }
}

impl Walk for XMLElement {
    fn walk(&self, w: &mut Walker) {
        for (_, value) in &self.args.1.wrapped.1 {
            value.walk(w)
        }
    }
}

impl Walk for XMLValue {
    fn walk(&self, w: &mut Walker) {
        match self {
            XMLValue::Concat(concat) => concat.walk(w),
            XMLValue::Attributes(attrs) => attrs.attrs.walk(w),
            XMLValue::Expr(scalx) => scalx.walk(w),
            XMLValue::Value2(value) => value.walk(w),
        }
    }
}

impl Walk for XMLValue2 {
    fn walk(&self, w: &mut Walker) {
        match self {
            XMLValue2::Element(element) => element.walk(w),
            XMLValue2::Forest(forest) => forest.walk(w),
            XMLValue2::Comment(_) => {}
        }
    }
}

impl Walk for SQLXmlAttribute {
    fn walk(&self, w: &mut Walker) {
        self.expr.walk(w)
    }
}

impl Walk for XMLForest {
    fn walk(&self, w: &mut Walker) {
        self.items.walk(w)
    }
}

impl Walk for XMLConcat {
    fn walk(&self, w: &mut Walker) {
        self.values.walk(w)
    }
}

impl Walk for Agg {
    fn walk(&self, w: &mut Walker) {
        match self {
            Agg::AggFuncArgs { arg, .. } => arg.walk(w),
            Agg::CountStar(..) => {}
        }
    }
}

impl Walk for AggArg {
    fn walk(&self, w: &mut Walker) {
        self.distinct.walk(w);
        self.expr.walk(w);
        self.foreach.walk(w)
    }
}

impl Walk for Dist {
    fn walk(&self, w: &mut Walker) {
        self.by.walk(w)
    }
}

impl Walk for Window {
    fn walk(&self, w: &mut Walker) {
        self.call.walk(w);
        let (partition, order, _) = &self.spec.wrapped;
        partition.walk(w);
        order.walk(w)
    }
}

impl Walk for WindowCall {
    fn walk(&self, w: &mut Walker) {
        use WindowCall::*;
        match self {
            RowNumber(..) | Rank(..) | DenseRank(..) | PercentRank(..) | CumeDist(..) => {}
            NTile(_, a)
            | FirstValue(_, a)
            | LastValue(_, a)
            | Sum(_, a)
            | Avg(_, a)
            | Min(_, a)
            | Max(_, a) => a.walk(w),
            NthValue(_, a) => a.walk(w),
            Lead(_, a) | Lag(_, a) => a.walk(w),
            Count(_, a) => {
                if let Either::Right(arg) = &a.1.wrapped {
                    arg.walk(w)
                }
            }
        }
    }
}

impl Walk for Ordir {
    fn walk(&self, w: &mut Walker) {
        if let Ordit::Scalx(scalx) = &self.expr {
            scalx.walk(w)
        }
    }
}

impl Walk for Case {
    fn walk(&self, w: &mut Walker) {
        match &self.body {
            CaseBody::Searched(whens) => whens.walk(w),
            CaseBody::Simple(_, operand, whens) => {
                operand.walk(w);
                whens.walk(w)
            }
        }
        self.else_clause.walk(w)
    }
}

impl<T: Walk + tygr::Grammar> Walk for When<T> {
    fn walk(&self, w: &mut Walker) {
        self.operand.walk(w);
        self.result.walk(w)
    }
}

impl Walk for Implied<SQLElseClause> {
    fn walk(&self, w: &mut Walker) {
        let Implied::Spelled(_, else_clause) = self;
        else_clause.result.walk(w)
    }
}

impl Walk for Prdct {
    fn walk(&self, w: &mut Walker) {
        self.0.walk(w)
    }
}

impl Walk for And {
    fn walk(&self, w: &mut Walker) {
        self.0.walk(w)
    }
}

impl Walk for Cond {
    fn walk(&self, w: &mut Walker) {
        match self {
            Cond::Not { condition, .. } => condition.walk(w),
            Cond::Paren(predicate) => predicate.walk(w),
            Cond::Predicate { predicate, .. } => predicate.walk(w),
        }
    }
}

impl Walk for Predicate {
    fn walk(&self, w: &mut Walker) {
        match self {
            Predicate::Exists { query, .. } => query.walk(w),
            Predicate::ForSomeElement {
                column, predicate, ..
            } => {
                w.column(&column.wrapped);
                w.unchecked(&predicate.1)
            }
            Predicate::ForSome {
                tables, condition, ..
            } => {
                w.push_frame();
                tables.walk(w);
                if let Some((_, condition)) = condition {
                    condition.walk(w)
                }
                w.pop_frame()
            }
            Predicate::Comparison { lhs, tail, .. } => {
                lhs.walk(w);
                tail.walk(w)
            }
        }
    }
}

impl Walk for Comparison {
    fn walk(&self, w: &mut Walker) {
        match self {
            Comparison::Scalx { expr, .. }
            | Comparison::StarEq { scalx: expr, .. }
            | Comparison::EqStar { scalx: expr, .. } => expr.walk(w),
            Comparison::Query { expr, .. } => expr.walk(w),
            Comparison::IsNull { .. } | Comparison::IsJSON { .. } => {}
            Comparison::Between { lower, upper, .. } => {
                lower.walk(w);
                upper.walk(w)
            }
            Comparison::Like {
                pattern, escape, ..
            }
            | Comparison::Matches {
                pattern, escape, ..
            } => {
                pattern.walk(w);
                escape.walk(w)
            }
            Comparison::In { list, .. } => list.walk(w),
            Comparison::InList { list_expr, .. } => list_expr.walk(w),
            Comparison::Find { expr, .. } => expr.walk(w),
            Comparison::Pattern { pattern, .. } => pattern.walk(w),
            Comparison::Contains { list, .. } => list.walk(w),
        }
    }
}

impl Walk for ForCnd {
    fn walk(&self, w: &mut Walker) {
        match self {
            ForCnd::Prdct(predicate) => predicate.walk(w),
            ForCnd::GroupHaving(group, having) => {
                group.walk(w);
                having.walk(w)
            }
        }
    }
}
