use tygr_objectscript::sql::core::*;

use super::{SqlProcedureUse, Walk, Walker, name_of};

// The CTEs are tables of every query under the `WITH`.
impl<T: Walk + tygr::Grammar> Walk for WithStatement<T> {
    fn walk(&self, w: &mut Walker) {
        w.push_frame();
        if let Some((with, _)) = &self.with_clause {
            with.walk(w)
        }
        self.body.walk(w);
        w.pop_frame()
    }
}

impl Walk for WithListElement {
    fn walk(&self, w: &mut Walker) {
        let cte = super::text(&self.name);
        w.current().ctes.push(cte);
        self.query.walk(w)
    }
}

// The CTEs are tables of every query under the `WITH`; the statement around its branches, whose
// `ORDER BY` and row limit see the first branch's names.
impl Walk for SelectStatement {
    fn walk(&self, w: &mut Walker) {
        w.push_frame();
        if let Some((with, _)) = &self.with_clause {
            with.walk(w)
        }
        w.push_frame();
        w.current().branches = true;
        self.query.walk(w);
        self.tail.walk(w);
        w.pop_frame();
        w.pop_frame()
    }
}

impl Walk for Query {
    fn walk(&self, w: &mut Walker) {
        self.0.walk(w)
    }
}

impl Walk for QueryTerm {
    fn walk(&self, w: &mut Walker) {
        match self {
            QueryTerm::Query(query) => query.walk(w),
            QueryTerm::QuerySpecification(selex) => selex.walk(w),
        }
    }
}

impl Walk for QuerySpecification {
    // In the order SQL evaluates the clauses, so that each name is bound before it is used.
    fn walk(&self, w: &mut Walker) {
        w.push_frame();
        self.from.walk(w);
        self.where_clause.walk(w);
        self.group.walk(w);
        self.having.walk(w);
        for (_, option) in &self.options {
            option.walk(w)
        }
        self.selst.walk(w);
        self.tail.walk(w);
        w.pop_frame()
    }
}

impl Walk for SelectList {
    fn walk(&self, w: &mut Walker) {
        self.quantifier.walk(w);
        if let Some((_, top, _)) = &self.top {
            top.walk(w)
        }
        self.select_list.walk(w);
        self.targt.walk(w)
    }
}

impl Walk for SelectOption {
    fn walk(&self, w: &mut Walker) {
        match self {
            SelectOption::RowLimit(_, val) | SelectOption::RowOffset(_, val) => val.walk(w),
            _ => {}
        }
    }
}

impl Walk for TopSpecification {
    fn walk(&self, w: &mut Walker) {
        if let TopSpecification::Count(count) = self {
            count.walk(w)
        }
    }
}

impl Walk for SelectItem {
    fn walk(&self, w: &mut Walker) {
        if let SelectItem::Expr { expr, alias } = self {
            expr.walk(w);
            if let Some(alias) = alias {
                w.alias(&alias.name)
            }
        }
    }
}

impl Walk for FromClause {
    fn walk(&self, w: &mut Walker) {
        self.tables.walk(w)
    }
}

impl<B: tygr::Grammar + Default> Walk for TableReferenceList<B> {
    fn walk(&self, w: &mut Walker) {
        self.first.walk(w)
    }
}

impl<B: tygr::Grammar + Default> Walk for TableReference<B> {
    fn walk(&self, w: &mut Walker) {
        let TableReference::Join(table) = self;
        table.walk(w)
    }
}

impl<B: tygr::Grammar + Default> Walk for JoinedTable<B> {
    fn walk(&self, w: &mut Walker) {
        self.formit.walk(w);
        for (_, join) in &self.joins {
            join.walk(w)
        }
    }
}

impl Walk for Join {
    fn walk(&self, w: &mut Walker) {
        match self {
            Join::Cross { table, .. } | Join::Natural { table, .. } => table.walk(w),
            Join::Qualified { table, spec, .. } => {
                table.walk(w);
                if let (_, JoinSpecification::On { condition, .. }) = spec {
                    condition.walk(w)
                }
            }
        }
    }
}

impl<B: tygr::Grammar + Default> Walk for TablePrimary<B> {
    fn walk(&self, w: &mut Walker) {
        match self {
            TablePrimary::TableValuedFunction(_, function, alias) => {
                let args = (function.args.wrapped.as_ref()).map_or(0, |(args, _)| args.items.len());
                w.call(&function.table.0, args, SqlProcedureUse::Table);
                w.table(None, alias.as_ref());
                if let Some((args, _)) = &function.args.wrapped {
                    args.walk(w)
                }
            }
            TablePrimary::Lateral(_, _, _, query, alias) | TablePrimary::Query(_, query, alias) => {
                w.table(None, alias.as_ref());
                query.walk(w)
            }
            TablePrimary::JoinedTable(_, joined) => joined.walk(w),
            TablePrimary::JSONTable(_, _, alias) => w.table(None, alias.as_ref()),
            TablePrimary::Table(_, name, alias) => w.table(Some(name_of(&name.0)), alias.as_ref()),
        }
    }
}

impl Walk for Tail {
    fn walk(&self, w: &mut Walker) {
        self.order.walk(w);
        self.limit.walk(w)
    }
}

// The query a statement reads, whose `ORDER BY` sees the first branch's names.
impl Walk for SourceQuery {
    fn walk(&self, w: &mut Walker) {
        w.push_frame();
        w.current().branches = true;
        self.query.walk(w);
        self.order.walk(w);
        w.pop_frame()
    }
}

impl Walk for QueryExpression {
    fn walk(&self, w: &mut Walker) {
        self.0.walk(w)
    }
}

impl Walk for RowLimitingClause {
    fn walk(&self, w: &mut Walker) {
        let RowLimitingClause::LimitOffset { limit, offset } = self;
        limit.walk(w);
        offset.walk(w)
    }
}

impl Walk for LimitSpecification {
    fn walk(&self, w: &mut Walker) {
        if let LimitSpecification::Val(count) = self {
            count.walk(w)
        }
    }
}

impl Walk for Count {
    fn walk(&self, w: &mut Walker) {
        match self {
            Count::HostVariable(host_var) => host_var.walk(w),
            Count::Literal(_) | Count::Marker(_) => {}
        }
    }
}

impl Walk for ResultOffsetClause {
    fn walk(&self, w: &mut Walker) {
        self.count.walk(w)
    }
}
