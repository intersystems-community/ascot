use either::Either;
use tygr_objectscript::sql::core::*;

use super::{SqlProcedureUse, Walk, Walker};

impl<B: tygr::Grammar + Default> Walk for ValueExpression<B> {
    fn walk(&self, w: &mut Walker) {
        self.first.walk(w);
        for (.., operand) in &self.rest {
            operand.walk(w)
        }
    }
}

impl<B: tygr::Grammar + Default> Walk for Operand<B> {
    fn walk(&self, w: &mut Walker) {
        match self {
            Operand::Paren(_, expression) => expression.walk(w),
            Operand::Plain(_, term) => term.walk(w),
        }
    }
}

impl Walk for ValueExpressionPrimary {
    fn walk(&self, w: &mut Walker) {
        match self {
            ValueExpressionPrimary::Builtin(f) => f.walk(w),
            ValueExpressionPrimary::WindowFunction(window) => window.walk(w),
            ValueExpressionPrimary::AggregateFunction(agg) => agg.walk(w),
            ValueExpressionPrimary::Unary { term, .. } => term.walk(w),
            ValueExpressionPrimary::ExtrinsicFunction(f) => f.args.walk(w),
            ValueExpressionPrimary::Case(case) => case.walk(w),
            ValueExpressionPrimary::Query(query) => query.walk(w),
            ValueExpressionPrimary::Call(f) => {
                let args = f.args.1.wrapped.as_ref().map_or(0, |args| args.items.len());
                w.call(&f.name.name, args, SqlProcedureUse::Function);
                f.args.walk(w)
            }
            ValueExpressionPrimary::HostVariable(host_var) => host_var.walk(w),
            ValueExpressionPrimary::Column(column) => w.column(column),
            ValueExpressionPrimary::Macro(..)
            | ValueExpressionPrimary::Literal(_)
            | ValueExpressionPrimary::ParameterMarker(_)
            | ValueExpressionPrimary::SpecialSystemVariable(_)
            | ValueExpressionPrimary::Niladic(..)
            | ValueExpressionPrimary::Null(_) => {}
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
                for (_, expression) in &a.1.wrapped.scalx_list {
                    expression.walk(w)
                }
            }
            TimestampAdd(_, a) => a.walk(w),
            TimestampDiff(_, a) => a.walk(w),
            ToVector(_, a) => a.walk(w),
            Trim(_, a) => a.walk(w),
            Odbc(a) => a.walk(w),
            CurrentTimestamp(_, a) | CurrentTime(_, a) => a.walk(w),
        }
    }
}

impl Walk for ODBCOnlyFunction {
    fn walk(&self, w: &mut Walker) {
        match self {
            ODBCOnlyFunction::Convert(_, a) => a.walk(w),
            ODBCOnlyFunction::IfNull(_, a) => a.walk(w),
            ODBCOnlyFunction::User(..) => {}
        }
    }
}

impl Walk for DatePart {
    fn walk(&self, w: &mut Walker) {
        if let DatePart::HostVariable(host_var) = self {
            host_var.walk(w)
        }
    }
}

impl Walk for ExtendedDatePart {
    fn walk(&self, w: &mut Walker) {
        if let ExtendedDatePart::HostVariable(host_var) = self {
            host_var.walk(w)
        }
    }
}

impl Walk for JSONKeyValuePairs {
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

impl Walk for PredictAssignment {
    fn walk(&self, w: &mut Walker) {
        self.expression.walk(w)
    }
}

impl Walk for TrimArgs {
    fn walk(&self, w: &mut Walker) {
        let TrimArgs::SpecChars(_, chars, _, _, expr) = self;
        chars.walk(w);
        expr.walk(w)
    }
}

impl Walk for CastArgs {
    fn walk(&self, w: &mut Walker) {
        use CastTarget::*;
        self.expr.walk(w);
        match &self.cast_type {
            Binary(_, a)
            | Char(_, a)
            | Nchar(_, a)
            | Nvarchar(_, a)
            | Varbinary(_, a)
            | Varchar(_, a) => a.walk(w),
            Decimal(_, a) | Numeric(_, a) => {
                if let Some(a) = a {
                    a.wrapped.0.walk(w);
                    if let Some((_, scale)) = &a.wrapped.1 {
                        scale.walk(w)
                    }
                }
            }
            Bigint(_) | Bit(_) | Datetime(_) | Date(_) | Double(_) | Float(_) | Guid(_)
            | Integer(_) | Money(_) | Posixtime(_) | Real(_) | Smalldatetime(_) | Smallint(_)
            | Smallmoney(_) | Timestamp(_) | Time(_) | Tinyint(_) => {}
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
            XMLValue::Expr(expression) => expression.walk(w),
            XMLValue::Value2(value) => value.walk(w),
        }
    }
}

impl Walk for XMLContent {
    fn walk(&self, w: &mut Walker) {
        match self {
            XMLContent::Element(element) => element.walk(w),
            XMLContent::Forest(forest) => forest.walk(w),
            XMLContent::Comment(..) => {}
        }
    }
}

impl Walk for XMLAttribute {
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

impl Walk for AggregateFunction {
    fn walk(&self, w: &mut Walker) {
        match self {
            AggregateFunction::AggFuncArgs { arg, .. } => arg.walk(w),
            AggregateFunction::CountStar(..) => {}
        }
    }
}

impl Walk for AggregateFunctionArgs {
    fn walk(&self, w: &mut Walker) {
        self.quantifier.walk(w);
        self.expr.walk(w);
        self.foreach.walk(w)
    }
}

impl Walk for SetQuantifier {
    fn walk(&self, w: &mut Walker) {
        if let SetQuantifier::Distinct { by, .. } = self {
            by.walk(w)
        }
    }
}

impl Walk for WindowFunction {
    fn walk(&self, w: &mut Walker) {
        self.call.walk(w);
        let (partition, order, _) = &self.spec.wrapped;
        partition.walk(w);
        order.walk(w)
    }
}

impl Walk for WindowFunctionType {
    fn walk(&self, w: &mut Walker) {
        use WindowFunctionType::*;
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

impl Walk for SortSpecification {
    fn walk(&self, w: &mut Walker) {
        if let SortKey::ValueExpression(expression) = &self.expr {
            expression.walk(w)
        }
    }
}

impl Walk for Case {
    fn walk(&self, w: &mut Walker) {
        match &self.body {
            CaseBody::Searched(whens) => whens.walk(w),
            CaseBody::Simple(operand, whens) => {
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

impl Walk for ElseClause {
    fn walk(&self, w: &mut Walker) {
        self.result.walk(w)
    }
}

impl<B: tygr::Grammar + Default> Walk for BooleanValueExpression<B> {
    fn walk(&self, w: &mut Walker) {
        self.first.walk(w);
        for OrBooleanTerm::Or(_, _, and) in &self.rest {
            and.walk(w)
        }
    }
}

impl<B: tygr::Grammar + Default> Walk for BooleanTerm<B> {
    fn walk(&self, w: &mut Walker) {
        self.first.walk(w);
        for AndBooleanFactor::And(_, _, cond) in &self.rest {
            cond.walk(w)
        }
    }
}

impl<B: tygr::Grammar + Default> Walk for BooleanFactor<B> {
    fn walk(&self, w: &mut Walker) {
        match self {
            BooleanFactor::Paren(_, condition) => condition.walk(w),
            BooleanFactor::Plain(_, factor) => factor.walk(w),
        }
    }
}

impl Walk for Factor {
    fn walk(&self, w: &mut Walker) {
        use Factor::*;
        match self {
            Not(_, condition) => condition.walk(w),
            Exists(_, _, _, query) => query.walk(w),
            ForSomeElement {
                column, predicate, ..
            } => {
                w.column(&column.wrapped);
                w.unchecked(&predicate.1)
            }
            ForSome {
                tables, condition, ..
            } => {
                w.push_frame();
                tables.walk(w);
                if let Some((_, condition)) = condition {
                    condition.walk(w)
                }
                w.pop_frame()
            }
            StarEq(_, lhs, _, _, _, rhs)
            | EqStar(_, lhs, _, _, _, rhs)
            | Compare(_, lhs, _, _, _, rhs) => {
                lhs.walk(w);
                rhs.walk(w)
            }
            IsNull(_, lhs, ..) | IsJSON(_, lhs, ..) => lhs.walk(w),
            Between(_, lhs, _, _, lower, _, _, upper) => {
                lhs.walk(w);
                lower.walk(w);
                upper.walk(w)
            }
            Like(_, lhs, _, _, pattern, escape) => {
                lhs.walk(w);
                pattern.walk(w);
                escape.walk(w)
            }
            In(_, lhs, _, _, _, list) => {
                lhs.walk(w);
                list.walk(w)
            }
            InList(_, lhs, _, _, expr, _)
            | Find(_, lhs, _, _, expr, _)
            | Pattern(_, lhs, _, _, expr) => {
                lhs.walk(w);
                expr.walk(w)
            }
            Contains(_, lhs, _, _, list) => {
                lhs.walk(w);
                list.walk(w)
            }
            Quantified(_, lhs, _, _, _, _, _, query) => {
                lhs.walk(w);
                query.walk(w)
            }
        }
    }
}

impl Walk for ForSomeCondition {
    fn walk(&self, w: &mut Walker) {
        match self {
            ForSomeCondition::BooleanValueExpression(predicate) => predicate.walk(w),
            ForSomeCondition::GroupHaving(group, having) => {
                if let Some((group, _)) = group {
                    group.walk(w)
                }
                having.walk(w)
            }
        }
    }
}
