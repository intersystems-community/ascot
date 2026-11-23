use tygr_objectscript::sql::*;

use super::{SqlProcedureUse, Walk, Walker, name_of};

// The CTEs are tables of every query under the `WITH`.
impl<T: Walk + tygr::Grammar> Walk for WithStat<T> {
    fn walk(&self, w: &mut Walker) {
        w.push_frame();
        if let Some((with, _)) = &self.with_clause {
            with.walk(w)
        }
        self.body.walk(w);
        w.pop_frame()
    }
}

impl Walk for WithList {
    fn walk(&self, w: &mut Walker) {
        self.0.walk(w)
    }
}

impl Walk for WithListElement {
    fn walk(&self, w: &mut Walker) {
        let cte = super::text(&self.name);
        w.current().ctes.push(cte);
        self.query.walk(w)
    }
}

// The statement around its branches; its `ORDER BY` and row limit see the first branch's names.
impl Walk for Selct {
    fn walk(&self, w: &mut Walker) {
        w.push_frame();
        w.current().branches = true;
        self.query.walk(w);
        self.order.walk(w);
        self.row_limiting_clause.walk(w);
        w.pop_frame()
    }
}

impl Walk for Query {
    fn walk(&self, w: &mut Walker) {
        self.0.walk(w)
    }
}

impl Walk for QTerm {
    fn walk(&self, w: &mut Walker) {
        match self {
            QTerm::Query(query) => query.walk(w),
            QTerm::Selex(selex) => selex.walk(w),
        }
    }
}

impl Walk for Selex {
    // In the order SQL evaluates the clauses, so that each name is bound before it is used.
    fn walk(&self, w: &mut Walker) {
        w.push_frame();
        self.from.walk(w);
        self.where_clause.walk(w);
        self.group.walk(w);
        self.having.walk(w);
        self.options.walk(w);
        self.selst.walk(w);
        // A statement's trailing `ORDER BY` parses as its last branch's; it names the first
        // branch's columns and aliases, which the statement's frame holds.
        if w.in_statement() {
            w.pop_frame();
            self.order.walk(w)
        } else {
            self.order.walk(w);
            w.pop_frame()
        }
    }
}

impl Walk for Selst {
    fn walk(&self, w: &mut Walker) {
        self.distinct.walk(w);
        self.top.walk(w);
        self.select_list.0.walk(w);
        self.targt.walk(w)
    }
}

impl Walk for SelectOption {
    fn walk(&self, w: &mut Walker) {
        match self {
            SelectOption::RowLimit(_, val, _) | SelectOption::RowOffset(_, val, _) => val.walk(w),
            _ => {}
        }
    }
}

impl Walk for TopSpec {
    fn walk(&self, w: &mut Walker) {
        if let TopSpec::ValSpec(val) = self {
            val.walk(w)
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

impl Walk for Tables {
    fn walk(&self, w: &mut Walker) {
        for tref in self.iter() {
            tref.walk(w)
        }
    }
}

impl Walk for Tref {
    fn walk(&self, w: &mut Walker) {
        match self {
            Tref::Join(table) => table.walk(w),
            Tref::OJSyntax { table, .. } => table.walk(w),
        }
    }
}

impl Walk for JTab {
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
                if let JSpec::On { condition, .. } = spec {
                    condition.walk(w)
                }
            }
        }
    }
}

impl Walk for Frmit {
    fn walk(&self, w: &mut Walker) {
        match self {
            Frmit::TVF { function, alias } => {
                let args = function.args.as_ref().map_or(0, |args| args.items.len());
                w.call(&function.table.0, args, SqlProcedureUse::Table);
                w.table(None, alias.as_ref());
                function.args.walk(w)
            }
            Frmit::Query { query, alias, .. } => {
                w.table(None, alias.as_ref());
                query.walk(w)
            }
            Frmit::JTab { joined, .. } => joined.walk(w),
            Frmit::JSONTable { alias, .. } => w.table(None, alias.as_ref()),
            Frmit::Table { name, alias } => w.table(Some(name_of(&name.0)), alias.as_ref()),
        }
    }
}

impl Walk for RowLimitingClause {
    fn walk(&self, w: &mut Walker) {
        match self {
            RowLimitingClause::LimitOffset { limit, offset } => {
                limit.walk(w);
                offset.walk(w)
            }
            RowLimitingClause::OffsetFetch { offset, fetch } => {
                offset.walk(w);
                fetch.walk(w)
            }
            RowLimitingClause::FetchOnly(fetch) => fetch.walk(w),
        }
    }
}

impl Walk for LimitSpec {
    fn walk(&self, w: &mut Walker) {
        if let LimitSpec::Val(count) = self {
            count.walk(w)
        }
    }
}

impl Walk for RowCount {
    fn walk(&self, w: &mut Walker) {
        match self {
            RowCount::Val(val) => val.walk(w),
            RowCount::Scalx(scalx) => scalx.walk(w),
        }
    }
}

impl Walk for Offset {
    fn walk(&self, w: &mut Walker) {
        self.count.walk(w)
    }
}

impl Walk for Fetch {
    fn walk(&self, w: &mut Walker) {
        self.valspec.walk(w)
    }
}
