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
            Stat::Thr(thr) => w.unchecked(&thr.remote_statement.1),
            Stat::DDL(ddl) => w.unchecked(ddl),
            Stat::Declare(decl) => decl.walk(w),
            Stat::OFC(ofc) => ofc.walk(w),
            Stat::TpRelated(tp) => tp.walk(w),
            Stat::Call(call) => call.walk(w),
            Stat::Explain(explain) => explain.walk(w),
            Stat::Truncate(_)
            | Stat::SetOption(_)
            | Stat::Grant(_)
            | Stat::Revok(_)
            | Stat::Setml(_)
            | Stat::ChkPrv(_)
            | Stat::Savepoint(_)
            | Stat::Lock(_) => {}
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
        match &self.body.1 {
            UpdatBody::Update {
                target,
                assignment,
                from,
                where_clause,
                ..
            } => {
                target.walk(w);
                from.walk(w);
                where_clause.walk(w);
                assignment.1.walk(w)
            }
            UpdatBody::UpdateStats { table, .. } => w.table(Some(name_of(&table.0)), None),
        }
        w.pop_frame()
    }
}

impl Walk for Delet {
    fn walk(&self, w: &mut Walker) {
        w.push_frame();
        self.target.1.walk(w);
        self.from.walk(w);
        self.where_clause.walk(w);
        w.pop_frame()
    }
}

impl Walk for IUDTarget {
    fn walk(&self, w: &mut Walker) {
        let alias = self.alias.as_ref();
        match &self.target {
            TableOrSubquery::Table(name) => w.table(Some(name_of(&name.0)), alias),
            TableOrSubquery::Subquery(query) => {
                w.table(None, alias);
                query.walk(w)
            }
        }
    }
}

impl Walk for InsertValues {
    fn walk(&self, w: &mut Walker) {
        match self {
            InsertValues::Asign(values) => values.walk(w),
            InsertValues::DefaultValues { columns, .. } => {
                for column in columns.iter().flat_map(|list| list.1.wrapped.items.iter()) {
                    w.column(column)
                }
            }
        }
    }
}

impl Walk for Asign {
    fn walk(&self, w: &mut Walker) {
        let Asign::ColumnList { columns, values } = self;
        column_list(w, columns, values)
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
        query @ ValueSource::Query(_) => w.outside(query),
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
            ValueSource::Query(query) => query.walk(w),
            ValueSource::ValSpec { spec, .. } => spec.walk(w),
        }
    }
}

impl Walk for Werop {
    fn walk(&self, w: &mut Walker) {
        match self {
            Werop::Prdct(prdct) => prdct.walk(w),
            Werop::CurrentOf { prdct, .. } => prdct.walk(w),
        }
    }
}

impl Walk for Decl {
    fn walk(&self, w: &mut Walker) {
        w.hosts.declares = Some(self.cursor_name.1.print());
        match &self.statement {
            DeclStatement::Explain(statement) => statement.walk(w),
            DeclStatement::Selct(statement) => statement.walk(w),
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
        w.call(&self.routine.1, args, SqlProcedureUse::Call);
        self.args.walk(w);
        if let Some((.., names)) = &self.using {
            for name in &names.items {
                if let Either::Left(host) = name {
                    w.read(host)
                }
            }
        }
        if let Some((Either::Left(host), ..)) = &self.output {
            w.written(host)
        }
    }
}

impl Walk for Explain {
    fn walk(&self, w: &mut Walker) {
        for (_, option) in &self.options {
            if let ExplainOption::Into(_, _, params) = option {
                for param in &params.items {
                    w.written(param)
                }
            }
        }
        self.statement.1.walk(w)
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
            DDL::Create { what, .. } => what.walk(w),
            DDL::Alter {
                what: Altered::View(def),
                ..
            } => def.walk(w),
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
                if let Some((.., timeout)) = timeout {
                    timeout.walk(w)
                }
            }
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
            _ => {}
        }
    }
}

impl Walk for Created {
    fn walk(&self, w: &mut Walker) {
        match self {
            Created::View { def, .. } => def.walk(w),
            Created::TableAs {
                with_clause, query, ..
            } => {
                if let Some((with, _)) = with_clause {
                    with.walk(w)
                }
                query.walk(w)
            }
            Created::Trigger { when, body, .. } => {
                if let Some((.., when)) = when {
                    when.walk(w)
                }
                body.1.walk(w)
            }
            Created::Model {
                source: Some((_, source)),
                ..
            } => source.walk(w),
            Created::Routine(routine) => routine.spec.1.walk(w),
            _ => {}
        }
    }
}

impl Walk for ViewDef {
    fn walk(&self, w: &mut Walker) {
        if let Some((with, _)) = &self.with_clause {
            with.walk(w)
        }
        self.query.walk(w)
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
            for (_, statement, _, _) in body.wrapped.iter() {
                match statement {
                    Either::Left(ret) => ret.value.1.walk(w),
                    Either::Right(stat) => stat.walk(w),
                }
            }
        }
    }
}

impl Walk for ReturnValue {
    fn walk(&self, w: &mut Walker) {
        match self {
            ReturnValue::Paren(value) => value.walk(w),
            ReturnValue::HostVar(host_var) => host_var.walk(w),
            ReturnValue::Literal(..) | ReturnValue::Marker(_) => {}
        }
    }
}

impl Walk for CodeSpecification {
    fn walk(&self, w: &mut Walker) {
        self.params.walk(w);
        self.body.1.walk(w)
    }
}

impl Walk for ParameterDeclaration {
    fn walk(&self, _: &mut Walker) {}
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
                tail,
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
                tail.walk(w)
            }
        }
    }
}

impl Walk for TrainWith {
    fn walk(&self, w: &mut Walker) {
        if let TrainWith::Assignment(assignment) = self {
            for (_, _, value) in &assignment.items {
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
