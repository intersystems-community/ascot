use either::Either;
use tygr_objectscript::sql::*;

use super::{SqlProcedureUse, Walk, Walker};

impl Walk for Scalx {
    fn walk(&self, w: &mut Walker) {
        let Scalx::Legacy(terms) = self;
        terms.walk(w)
    }
}

impl Walk for Term {
    fn walk(&self, w: &mut Walker) {
        match self {
            Term::Builtin(f) => f.walk(w),
            Term::ScalarFunction(f) => f.walk(w),
            Term::Window(window) => window.walk(w),
            Term::Agg(agg) => agg.walk(w),
            Term::Unary { term, .. } | Term::PercentUnary { term, .. } => term.walk(w),
            Term::StringOperation(op) => {
                op.expr.walk(w);
                op.truncation.walk(w)
            }
            Term::XTFun(f) => f.args.walk(w),
            Term::Case(case) => case.walk(w),
            Term::OdbcEscape(escape) => escape.walk(w),
            Term::Scalx(scalx) => scalx.walk(w),
            Term::Query(query) => query.walk(w),
            Term::Call(f) => {
                let args = f.args.wrapped.as_ref().map_or(0, |args| args.items.len());
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

impl<T1: Walk + tygr::Grammar, T2: Walk + tygr::Grammar> Walk for ArgsRR<T1, T2> {
    fn walk(&self, w: &mut Walker) {
        self.arg1.walk(w);
        self.arg2.walk(w)
    }
}

impl<T1: Walk + tygr::Grammar, T2: Walk + tygr::Grammar, T3: Walk + tygr::Grammar> Walk
    for ArgsRRR<T1, T2, T3>
{
    fn walk(&self, w: &mut Walker) {
        self.arg1.walk(w);
        self.arg2.walk(w);
        self.arg3.walk(w)
    }
}

impl<
    T1: Walk + tygr::Grammar,
    T2: Walk + tygr::Grammar,
    T3: Walk + tygr::Grammar,
    T4: Walk + tygr::Grammar,
> Walk for ArgsRRRR<T1, T2, T3, T4>
{
    fn walk(&self, w: &mut Walker) {
        self.arg1.walk(w);
        self.arg2.walk(w);
        self.arg3.walk(w);
        self.arg4.walk(w)
    }
}

impl<T1: Walk + tygr::Grammar, T2: Walk + tygr::Grammar> Walk for ArgsRO<T1, T2> {
    fn walk(&self, w: &mut Walker) {
        self.arg1.walk(w);
        if let Some((_, arg2)) = &self.arg2 {
            arg2.walk(w)
        }
    }
}

impl<T1: Walk + tygr::Grammar, T2: Walk + tygr::Grammar, T3: Walk + tygr::Grammar> Walk
    for ArgsROO<T1, T2, T3>
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
    for ArgsRRO<T1, T2, T3>
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
> Walk for ArgsRROO<T1, T2, T3, T4>
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

impl Walk for ScalarFunction {
    fn walk(&self, w: &mut Walker) {
        use ScalarFunction::*;
        match self {
            DollarExtract(_, a)
            | DollarFind(_, a)
            | DollarFNumber(_, a)
            | DollarList(_, a)
            | DollarListGet(_, a)
            | Str(_, a) => a.walk(w),
            DollarJustify(_, a)
            | DollarListFind(_, a)
            | DollarTranslate(_, a)
            | CharIndex(_, a)
            | Convert(_, a)
            | IfNull(_, a)
            | LPad(_, a)
            | RPad(_, a)
            | Substr(_, a) => a.walk(w),
            DollarLength(_, a)
            | DollarListData(_, a)
            | DollarListFromString(_, a)
            | DollarListToString(_, a)
            | Embedding(_, a)
            | ToChar(_, a)
            | ToDate(_, a)
            | ToUSChar(_, a)
            | ToUSDate(_, a)
            | ToTimestamp(_, a) => a.walk(w),
            DollarListBuild(_, a) => a.walk(w),
            DollarListLength(_, a)
            | PercentObject(_, a)
            | PercentOid(_, a)
            | Abs(_, a)
            | Ascii(_, a)
            | Ceiling(_, a)
            | CharacterLength(_, a)
            | CharLength(_, a)
            | DataLength(_, a)
            | Date(_, a)
            | Day(_, a)
            | Floor(_, a)
            | IsNumeric(_, a)
            | LastDay(_, a)
            | LCase(_, a)
            | Len(_, a)
            | Length(_, a)
            | Lower(_, a)
            | LTrim(_, a)
            | Month(_, a)
            | Reverse(_, a)
            | RTrim(_, a)
            | Sign(_, a)
            | Space(_, a)
            | UCase(_, a)
            | Year(_, a) => a.walk(w),
            DollarListSame(_, a)
            | DollarMvFmt(_, a)
            | DollarMvFmts(_, a)
            | DollarMvIconv(_, a)
            | DollarMvIconvs(_, a)
            | DollarMvOconv(_, a)
            | DollarMvOconvs(_, a)
            | Similarity(_, a)
            | HashBytes(_, a)
            | IsNull(_, a)
            | Left(_, a)
            | NullIf(_, a)
            | Nvl(_, a)
            | PatIndex(_, a)
            | Power(_, a)
            | Repeat(_, a)
            | Replicate(_, a)
            | Right(_, a)
            | Round(_, a)
            | VectorCosine(_, a)
            | VectorDotProduct(_, a) => a.walk(w),
            DollarPiece(_, a) | Instr(_, a) => a.walk(w),
            Coalesce(_, a) | Decode(_, a) | Greatest(_, a) | Least(_, a) | String(_, a) => {
                a.walk(w)
            }
            CurrentTimestamp(_, a) | CurrentTime(_, a) => a.walk(w),
            GetDate(_, a) | GetUtcDate(_, a) | UnixTimestamp(_, a) => a.walk(w),
            Replace(_, a) => a.walk(w),
            Stuff(_, a) => a.walk(w),
            DateAdd(_, a) | DateDiff(_, a) => a.walk(w),
            DateName(_, a) | DateTrunc(_, a) => a.walk(w),
            DatePart(_, a) => a.walk(w),
            ToNumber(_, a) => a.walk(w),
            Position(_, a) => {
                a.needle.walk(w);
                a.haystack.walk(w)
            }
            Substring(_, a) => a.walk(w),
            TSqlDbName(..) | TSqlError(..) | TSqlFetchStatus(..) | TSqlGetUtcDate(..)
            | TSqlHostName(..) | TSqlLockTimeout(..) | TSqlNestLevel(..) | TSqlServerName(..)
            | TSqlSpid(..) | TSqlSqlStatus(..) | TSqlTranCount(..) | TSqlVersion(..)
            | LastIdentity(..) | Now(..) => {}
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
            Convert(_, a) => a.walk(w),
            JSONArray(_, a) => {
                if let Some((items, _)) = &a.wrapped {
                    items.walk(w)
                }
            }
            JSONObject(_, a) => {
                if let Some((pairs, _)) = &a.wrapped {
                    pairs.walk(w)
                }
            }
            Predict(_, a) => a.with_clause.walk(w),
            Probability(_, a) => {
                a.for_clause.walk(w);
                a.with_clause.walk(w)
            }
            SearchIndex(_, a) => {
                for (_, scalx) in &a.scalx_list {
                    scalx.walk(w)
                }
            }
            TimestampAdd(_, a) => a.walk(w),
            TimestampDiff(_, a) => a.walk(w),
            ToVector(_, a) => a.walk(w),
            Trim(_, _, a) => a.walk(w),
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

impl Walk for PredictWith {
    fn walk(&self, w: &mut Walker) {
        match self {
            PredictWith::Unnamed { scalx_list, .. } => scalx_list.walk(w),
            PredictWith::Named { assignments, .. } => assignments.walk(w),
        }
    }
}

impl Walk for PredictAssign {
    fn walk(&self, w: &mut Walker) {
        self.scalx.walk(w)
    }
}

impl Walk for SubstringArgsInternal {
    fn walk(&self, w: &mut Walker) {
        match self {
            SubstringArgsInternal::Keyword {
                expr,
                start,
                for_clause,
                ..
            } => {
                expr.walk(w);
                start.walk(w);
                if let Some((_, length)) = for_clause {
                    length.walk(w)
                }
            }
            SubstringArgsInternal::Position {
                expr,
                start,
                length,
                ..
            } => {
                expr.walk(w);
                start.walk(w);
                if let Some((_, length)) = length {
                    length.walk(w)
                }
            }
        }
    }
}

impl Walk for TrimArgs {
    fn walk(&self, w: &mut Walker) {
        match self {
            TrimArgs::SpecScalx(_, _, _, _, scalx) => scalx.walk(w),
            TrimArgs::SpecScalxScalx(_, _, chars, from) => {
                chars.walk(w);
                if let Some((_, _, _, scalx)) = from {
                    scalx.walk(w)
                }
            }
            TrimArgs::ScalxScalx(chars, scalx) => {
                if let Some((chars, ..)) = chars {
                    chars.walk(w)
                }
                scalx.walk(w)
            }
        }
    }
}

impl Walk for SQLCastExpression {
    fn walk(&self, w: &mut Walker) {
        use CastType::*;
        self.expr.walk(w);
        match &self.cast_type {
            BinaryVarying(_, _, _, a) | CharacterVarying(_, _, _, a) | CharVarying(_, _, _, a) => {
                a.walk(w)
            }
            Binary(_, a)
            | Character(_, a)
            | Char(_, a)
            | Nchar(_, a)
            | Nvarchar(_, a)
            | Varbinary(_, a)
            | Varchar(_, a) => a.walk(w),
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
        self.values.walk(w)
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

impl Walk for DTS {
    fn walk(&self, w: &mut Walker) {
        match self {
            DTS::Function(f) => f.walk(w),
            DTS::DateTime(val) => val.val_spec.walk(w),
        }
    }
}

impl Walk for ODBCFunc {
    fn walk(&self, w: &mut Walker) {
        use ODBCFunc::*;
        match self {
            Abs(_, a)
            | Acos(_, a)
            | Asin(_, a)
            | Atan(_, a)
            | Ceiling(_, a)
            | Cos(_, a)
            | Cot(_, a)
            | Exp(_, a)
            | Floor(_, a)
            | Log(_, a)
            | Log10(_, a)
            | Sin(_, a)
            | Sqrt(_, a)
            | Tan(_, a)
            | Ascii(_, a)
            | Char(_, a)
            | DayName(_, a)
            | DayOfMonth(_, a)
            | DayOfWeek(_, a)
            | DayOfYear(_, a)
            | Hour(_, a)
            | LCase(_, a)
            | Length(_, a)
            | LTrim(_, a)
            | Minute(_, a)
            | Month(_, a)
            | MonthName(_, a)
            | Quarter(_, a)
            | RTrim(_, a)
            | Second(_, a)
            | Sign(_, a)
            | Space(_, a)
            | UCase(_, a)
            | Week(_, a)
            | Year(_, a) => a.walk(w),
            Mod(_, a)
            | Power(_, a)
            | Concat(_, a)
            | IfNull(_, a)
            | Left(_, a)
            | Repeat(_, a)
            | Right(_, a)
            | Round(_, a)
            | Truncate(_, a) => a.walk(w),
            Substring(_, a) => a.walk(w),
            TimestampAdd(_, a) => a.walk(w),
            TimestampDiff(_, a) => a.walk(w),
            Convert(_, a) => a.walk(w),
            Pi(..) | CurDate(..) | CurTime(..) | Database(..) | Now(..) | User(..) => {}
        }
    }
}

impl Walk for Agg {
    fn walk(&self, w: &mut Walker) {
        match self {
            Agg::AggFuncArgs(_, _, arg) => arg.walk(w),
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
        if let Dist::Distinct { by, .. } = self {
            by.walk(w)
        }
    }
}

impl Walk for Window {
    fn walk(&self, w: &mut Walker) {
        if let Some(Either::Right(args)) = &self.arg.wrapped {
            args.walk(w)
        }
        self.partition.walk(w);
        self.order.walk(w)
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
        match self {
            Case::Searched(case) => {
                case.when_clauses.walk(w);
                case.else_clause.walk(w)
            }
            Case::Simple(case) => {
                case.expr.walk(w);
                case.when_clauses.walk(w);
                case.else_clause.walk(w)
            }
        }
    }
}

impl Walk for SQLWhenClause {
    fn walk(&self, w: &mut Walker) {
        self.condition.walk(w);
        self.result.walk(w)
    }
}

impl Walk for SQLSimpleCaseWhen {
    fn walk(&self, w: &mut Walker) {
        self.value.walk(w);
        self.result.walk(w)
    }
}

impl Walk for SQLElseClause {
    fn walk(&self, w: &mut Walker) {
        self.result.walk(w)
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
            Cond::Paren { predicate, .. } => predicate.walk(w),
            Cond::Exists { query, .. } => query.walk(w),
            Cond::ForSomeElement {
                column, predicate, ..
            } => {
                w.column(column);
                w.unchecked(predicate)
            }
            Cond::ForSome {
                tables, condition, ..
            } => {
                w.push_frame();
                tables.walk(w);
                if let Some((_, condition)) = condition {
                    condition.walk(w)
                }
                w.pop_frame()
            }
            Cond::Compare { lhs, rhs, .. }
            | Cond::StarEq { lhs, rhs, .. }
            | Cond::EqStar { lhs, rhs, .. } => {
                lhs.walk(w);
                rhs.walk(w)
            }
            Cond::Quantified { lhs, subquery, .. } => {
                lhs.walk(w);
                subquery.walk(w)
            }
            Cond::IsNull { lhs, .. } | Cond::IsJson { lhs, .. } => lhs.walk(w),
            Cond::Between {
                lhs, lower, upper, ..
            } => {
                lhs.walk(w);
                lower.walk(w);
                upper.walk(w)
            }
            Cond::Like {
                lhs,
                pattern,
                escape,
                ..
            }
            | Cond::Matches {
                lhs,
                pattern,
                escape,
                ..
            } => {
                lhs.walk(w);
                pattern.walk(w);
                escape.walk(w)
            }
            Cond::In { lhs, spec, .. } => {
                lhs.walk(w);
                spec.walk(w)
            }
            Cond::InList { lhs, list_expr, .. } => {
                lhs.walk(w);
                list_expr.walk(w)
            }
            Cond::Find { lhs, expr, .. } => {
                lhs.walk(w);
                expr.walk(w)
            }
            Cond::Pattern { lhs, pattern, .. } => {
                lhs.walk(w);
                pattern.walk(w)
            }
            Cond::Contains { lhs, list, .. } => {
                lhs.walk(w);
                list.walk(w)
            }
        }
    }
}

impl Walk for ForCnd {
    fn walk(&self, w: &mut Walker) {
        match self {
            ForCnd::Paren(_, _, predicate, _, _) => predicate.walk(w),
            ForCnd::GroupHaving { group, having, .. } => {
                group.walk(w);
                having.walk(w)
            }
            ForCnd::GroupOnly(_, _, group, _, _) => group.walk(w),
        }
    }
}

impl Walk for InSpec {
    fn walk(&self, w: &mut Walker) {
        match self {
            InSpec::Query(query) => query.walk(w),
            InSpec::Scalxes(scalxes) => scalxes.walk(w),
        }
    }
}
