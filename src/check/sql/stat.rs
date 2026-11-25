use either::Either;
use tygr::Print;
use tygr_objectscript::sql::{Space, core::*};

use super::{SqlProcedureUse, Walk, Walker, name_of};

impl Walk for Stat {
    fn walk(&self, w: &mut Walker) {
        match self {
            Stat::Selct(stat) => stat.walk(w),
            Stat::Insert(stat) => stat.walk(w),
            Stat::Update(stat) => stat.walk(w),
            Stat::Delete(stat) => stat.walk(w),
            Stat::Thr(thr) => w.unchecked(&thr.remote_statement),
            Stat::DDL(ddl) => w.unchecked(ddl),
            Stat::Declare(decl) => decl.walk(w),
            Stat::OFC(ofc) => ofc.walk(w),
            Stat::TpRelated(tp) => tp.walk(w),
            Stat::Get(get) => {
                if let Some((_, _, _, var, ..)) = &get.assignment {
                    w.written(var)
                }
            }
            Stat::Call(call) => call.walk(w),
            Stat::Explain(explain) => explain.walk(w),
            Stat::Truncate(_)
            | Stat::SetOption(_)
            | Stat::Grant(_)
            | Stat::Revok(_)
            | Stat::Setml(_)
            | Stat::ChkPrv(_) => {}
        }
    }
}

// The target table is the frame of the column list, the values, the `SET` and the `WHERE`: IRIS
// resolves a name in a VALUES list against it as in a SET (measured on 2026.2).
impl Walk for Insrt {
    fn walk(&self, w: &mut Walker) {
        w.push_frame();
        self.target.1.walk(w);
        self.values.1.walk(w);
        w.pop_frame()
    }
}

impl Walk for Updat {
    fn walk(&self, w: &mut Walker) {
        w.push_frame();
        match self {
            Updat::Update {
                target,
                assignment,
                from,
                where_clause,
                ..
            } => {
                target.1.walk(w);
                from.walk(w);
                where_clause.walk(w);
                assignment.1.walk(w)
            }
            Updat::UpdateStats { table, .. } => w.table(Some(name_of(&table.0)), None),
        }
        w.pop_frame()
    }
}

impl Walk for Delet {
    fn walk(&self, w: &mut Walker) {
        w.push_frame();
        self.target.1.walk(w);
        self.where_clause.walk(w);
        w.pop_frame()
    }
}

impl Walk for IUDTarget {
    fn walk(&self, w: &mut Walker) {
        match self {
            IUDTarget::Table { name, alias } => w.table(Some(name_of(&name.0)), alias.as_ref()),
            IUDTarget::Subquery { query, alias } => {
                w.table(None, alias.as_ref());
                query.walk(w)
            }
        }
    }
}

impl Walk for InsertValues {
    fn walk(&self, w: &mut Walker) {
        match self {
            InsertValues::ColumnList { columns, values } => column_list(w, columns, values),
            InsertValues::DefaultValues(..) => {}
        }
    }
}

impl Walk for Asign {
    fn walk(&self, w: &mut Walker) {
        match self {
            Asign::SetList { assignments, .. } => assignments.1.walk(w),
            Asign::ColumnList { columns, values } => column_list(w, columns, values),
        }
    }
}

fn column_list(
    w: &mut Walker,
    columns: &Option<SQLColumnListParen>,
    values: &(Space, ValueSource),
) {
    for column in columns.iter().flat_map(|list| list.1.wrapped.items.iter()) {
        w.column(column)
    }
    match &values.1 {
        query @ ValueSource::Query { .. } => w.outside(query),
        values => values.walk(w),
    }
}

impl Walk for SQLSetAssignment {
    fn walk(&self, w: &mut Walker) {
        w.column(&self.column);
        self.value.walk(w)
    }
}

impl Walk for ValueSource {
    fn walk(&self, w: &mut Walker) {
        match self {
            ValueSource::Query { query, order } => {
                query.walk(w);
                order.walk(w)
            }
            ValueSource::ValSpec { spec, .. } => spec.walk(w),
        }
    }
}

impl Walk for WhereWerop {
    fn walk(&self, w: &mut Walker) {
        match &self.werop {
            Werop::Prdct(prdct) => prdct.walk(w),
            Werop::CurrentOf { prdct, .. } => prdct.walk(w),
        }
    }
}

impl Walk for Decl {
    fn walk(&self, w: &mut Walker) {
        match self {
            Decl::Explain {
                cursor_name,
                statement,
                ..
            } => {
                w.hosts.declares = Some(cursor_name.1.print());
                statement.walk(w)
            }
            Decl::Selct {
                cursor_name,
                statement,
                ..
            } => {
                w.hosts.declares = Some(cursor_name.1.print());
                statement.walk(w)
            }
        }
    }
}

impl Walk for OFC {
    fn walk(&self, w: &mut Walker) {
        if let OFC::Fetch { cursor, targt, .. } = self {
            w.hosts.fetches = Some(cursor.print());
            targt.walk(w)
        }
    }
}

impl Walk for CallStatement {
    fn walk(&self, w: &mut Walker) {
        let args = self
            .args
            .1
            .wrapped
            .as_ref()
            .map_or(0, |args| args.items.len());
        w.call(&self.routine.1.0.0, args, SqlProcedureUse::Call);
        self.args.walk(w);
        if let Some((.., name)) = &self.using {
            w.hosts.reads.push((name.print(), false))
        }
        if let Some((_, name, ..)) = &self.output {
            w.hosts.writes.push((name.print(), false))
        }
    }
}

impl Walk for Explain {
    fn walk(&self, w: &mut Walker) {
        if let Some((.., param)) = &self.into {
            w.written(param)
        }
        match &*self.statement.1 {
            ExplainSQLStatement::Select(stat) => stat.walk(w),
            ExplainSQLStatement::Update(stat) => stat.walk(w),
            ExplainSQLStatement::Delete(stat) => stat.walk(w),
        }
    }
}

impl Walk for TpRelated {
    fn walk(&self, w: &mut Walker) {
        match self {
            TpRelated::StartTransaction(tp) => tp.modes.walk(w),
            TpRelated::SetTransaction(tp) => {
                for mode in tp.modes.items.iter().flatten() {
                    mode.1.walk(w)
                }
            }
            TpRelated::Rollback(_) | TpRelated::TPOption(_) => {}
        }
    }
}

impl Walk for TRMode {
    fn walk(&self, w: &mut Walker) {
        if let TRMode::DiagnosticsSize { size, .. } = self {
            size.walk(w)
        }
    }
}

impl Walk for DDL {
    fn walk(&self, w: &mut Walker) {
        match self {
            DDL::CreOrAltView { query, .. } => query.walk(w),
            DDL::CancelQuery {
                val,
                cancel_scope,
                timeout,
                ..
            } => {
                val.walk(w);
                if let Some((_, scope)) = cancel_scope {
                    scope.walk(w)
                }
                if let Some((_, timeout)) = timeout {
                    timeout.walk(w)
                }
            }
            DDL::CreateTableAs {
                with_clause, query, ..
            } => {
                if let Some((with, _)) = with_clause {
                    with.walk(w)
                }
                query.walk(w)
            }
            DDL::CreateTrigger { when, body, .. } => {
                if let Some((.., when)) = when {
                    when.walk(w)
                }
                body.1.walk(w)
            }
            DDL::CreateModel {
                source: Some((_, source)),
                ..
            } => source.walk(w),
            DDL::TrainModel {
                with_clause,
                source,
                ..
            }
            | DDL::ValidateModel {
                with_clause,
                source,
                ..
            } => {
                if let Some((_, with)) = with_clause {
                    with.walk(w)
                }
                if let Some((_, source)) = source {
                    source.walk(w)
                }
            }
            DDL::CreateFunction { spec, .. }
            | DDL::CreateProcedure { spec, .. }
            | DDL::CreateQuery { spec, .. }
            | DDL::CreateMethod { spec, .. } => spec.1.walk(w),
            DDL::CreateAggregate { params, .. } => params.walk(w),
            _ => {}
        }
    }
}

impl Walk for TriggerBody {
    fn walk(&self, w: &mut Walker) {
        match self {
            TriggerBody::Code(body) => body.walk(w),
            TriggerBody::Stat { stat, .. } => stat.walk(w),
        }
    }
}

impl Walk for CodeBody {
    fn walk(&self, w: &mut Walker) {
        if let CodeBody::Sql { body, .. } = self {
            for (_, statement, _, _) in &body.wrapped {
                match statement {
                    Either::Left(ret) => {
                        if let Some(ReturnValue::HostRef(host_ref)) = &ret.value {
                            w.host(host_ref, false)
                        }
                    }
                    Either::Right(stat) => stat.walk(w),
                }
            }
        }
    }
}

impl Walk for CodeSpecification {
    fn walk(&self, w: &mut Walker) {
        self.params.walk(w);
        self.body.1.walk(w)
    }
}

impl Walk for ParameterList {
    fn walk(&self, w: &mut Walker) {
        self.0.walk(w)
    }
}

impl Walk for ParameterDeclaration {
    fn walk(&self, w: &mut Walker) {
        match &self.default {
            Some((_, DefaultClause::Default { value, .. }))
            | Some((_, DefaultClause::Expr(value))) => value.walk(w),
            None => {}
        }
    }
}

impl Walk for ModelSource {
    fn walk(&self, w: &mut Walker) {
        match self {
            ModelSource::Subquery { selex, .. } => selex.walk(w),
            ModelSource::Inline {
                from,
                where_clause,
                group,
                having,
                order,
            } => {
                from.walk(w);
                if let Some((_, prdct)) = where_clause {
                    prdct.walk(w)
                }
                if let Some((_, group)) = group {
                    group.walk(w)
                }
                if let Some((_, having)) = having {
                    having.walk(w)
                }
                if let Some((_, order)) = order {
                    order.walk(w)
                }
            }
        }
    }
}

impl Walk for TrainWith {
    fn walk(&self, w: &mut Walker) {
        if let TrainWith::Assignment(assignment) = self {
            assignment.first_val.walk(w);
            for (_, _, _, value) in &assignment.rest {
                value.walk(w)
            }
        }
    }
}

impl Walk for WithExpression {
    fn walk(&self, w: &mut Walker) {
        if let WithExpression::Scalx(scalx) = self {
            scalx.walk(w)
        }
    }
}
