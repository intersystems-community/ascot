use tygr_objectscript::sql::*;

use super::{Walk, Walker};

impl<T: Walk + tygr::Grammar> Walk for WithStat<T> {
    fn walk(&self, w: &mut Walker) {
        if let Some((with, _)) = &self.with_clause {
            with.walk(w)
        }
        self.body.walk(w)
    }
}

impl Walk for WithList {
    fn walk(&self, w: &mut Walker) {
        self.0.walk(w)
    }
}

impl Walk for WithListElement {
    fn walk(&self, w: &mut Walker) {
        self.query.walk(w)
    }
}

impl Walk for Selct {
    fn walk(&self, w: &mut Walker) {
        self.query.walk(w);
        self.order.walk(w);
        self.row_limiting_clause.walk(w)
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
    fn walk(&self, w: &mut Walker) {
        self.selst.walk(w);
        self.from.walk(w);
        self.where_clause.walk(w);
        self.group.walk(w);
        self.having.walk(w);
        self.order.walk(w)
    }
}

impl Walk for Selst {
    fn walk(&self, w: &mut Walker) {
        self.options.walk(w);
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
        if let SelectItem::Expr { expr, .. } = self {
            expr.walk(w)
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
        self.items.walk(w)
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
        self.joins.walk(w)
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
            Frmit::TVF { function, .. } => function.args.walk(w),
            Frmit::Query { query, .. } => query.walk(w),
            Frmit::JTab { joined, .. } => joined.walk(w),
            Frmit::JSONTable { .. } | Frmit::Table { .. } => {}
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

impl Walk for Fetch {
    fn walk(&self, w: &mut Walker) {
        self.valspec.walk(w)
    }
}
