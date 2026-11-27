use either::Either;
use tygr::{Grammar, Print};
use tygr_objectscript::sql::core::*;

use super::{SqlProcedureUse, Walk, Walker, name_of};

impl Walk for Statement {
    fn walk(&self, w: &mut Walker) {
        match self {
            Statement::SelectStatement(statement) => statement.walk(w),
            Statement::Insert(statement) => statement.walk(w),
            Statement::Update(statement) => statement.walk(w),
            Statement::Delete(statement) => statement.walk(w),
            Statement::ThroughStatement(thr) => w.unchecked(&thr.remote_statement.1),
            Statement::DDL(ddl) => w.unchecked(ddl),
            Statement::Declare(decl) => decl.walk(w),
            Statement::CursorStatement(ofc) => ofc.walk(w),
            Statement::TransactionStatement(tp) => tp.walk(w),
            Statement::Call(call) => call.walk(w),
            Statement::ExplainStatement(explain) => explain.walk(w),
            Statement::Truncate(_)
            | Statement::SetOption(_)
            | Statement::GrantStatement(_)
            | Statement::RevokeStatement(_)
            | Statement::SetMlConfigurationStatement(_)
            | Statement::CheckPrivilegeStatement(_)
            | Statement::Savepoint(_)
            | Statement::Lock(_)
            | Statement::Unlock(_) => {}
        }
    }
}

// The target table is the frame of the column list, the values, the `SET` and the `WHERE`: IRIS
// resolves a name in a VALUES list against it as in a SET (measured on 2026.2).
impl Walk for InsertStatement {
    fn walk(&self, w: &mut Walker) {
        w.push_frame();
        self.target.1.walk(w);
        self.values.1.walk(w);
        w.pop_frame()
    }
}

impl Walk for UpdateStatement {
    fn walk(&self, w: &mut Walker) {
        w.push_frame();
        match &self.body {
            UpdateStatementBody::Update {
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
            UpdateStatementBody::UpdateStats { table, .. } => {
                w.table(Some(name_of(&table.0)), None)
            }
        }
        w.pop_frame()
    }
}

impl Walk for DeleteStatement {
    fn walk(&self, w: &mut Walker) {
        w.push_frame();
        self.target.1.walk(w);
        self.from.walk(w);
        self.where_clause.walk(w);
        w.pop_frame()
    }
}

impl Walk for TargetTable {
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

impl Walk for InsertColumnsAndSource {
    fn walk(&self, w: &mut Walker) {
        match self {
            InsertColumnsAndSource::ColumnList(list) => list.walk(w),
            InsertColumnsAndSource::Query { columns, query } => {
                w.columns(columns);
                w.outside(&query.1)
            }
            InsertColumnsAndSource::DefaultValues { columns, .. } => w.columns(columns),
        }
    }
}

impl Walk for ColumnList {
    fn walk(&self, w: &mut Walker) {
        let ColumnList::Columns { columns, values } = self;
        w.columns(columns);
        values.1.walk(w)
    }
}

impl Walk for SetClause {
    fn walk(&self, w: &mut Walker) {
        w.column(&self.column);
        self.value.walk(w)
    }
}

impl Walk for ValuesClause {
    fn walk(&self, w: &mut Walker) {
        self.spec.walk(w)
    }
}

impl Walk for WhereCondition {
    fn walk(&self, w: &mut Walker) {
        match self {
            WhereCondition::BooleanValueExpression(condition) => condition.walk(w),
            WhereCondition::CurrentOf { condition, .. } => condition.walk(w),
        }
    }
}

impl Walk for DeclareCursor {
    fn walk(&self, w: &mut Walker) {
        w.hosts.declares = Some(self.cursor_name.1.print());
        match &self.statement {
            DeclaredStatement::ExplainStatement(statement) => statement.walk(w),
            DeclaredStatement::SelectStatement(statement) => statement.walk(w),
        }
    }
}

impl Walk for CursorStatement {
    fn walk(&self, w: &mut Walker) {
        if let CursorStatement::Fetch { cursor, into, .. } = self {
            w.hosts.fetches = Some(cursor.print());
            into.walk(w)
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

impl Walk for ExplainStatement {
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

impl Walk for TransactionStatement {
    fn walk(&self, w: &mut Walker) {
        match self {
            TransactionStatement::StartTransaction(StartTransactionStatement { modes, .. })
            | TransactionStatement::SetTransaction(SetTransactionStatement { modes, .. }) => {
                if let Some((_, modes)) = modes {
                    modes.walk(w)
                }
            }
            TransactionStatement::Rollback(_) | TransactionStatement::TransactionOption(_) => {}
        }
    }
}

impl Walk for TransactionMode {
    fn walk(&self, w: &mut Walker) {
        if let TransactionMode::DiagnosticsSize { size, .. } = self {
            size.walk(w)
        }
    }
}

impl Walk for DDL {
    fn walk(&self, w: &mut Walker) {
        match self {
            DDL::Create { what, .. } => what.walk(w),
            DDL::Alter {
                what: Altered::View(definition),
                ..
            } => definition.walk(w),
            DDL::CancelQuery {
                id,
                cancel_scope,
                timeout,
                ..
            } => {
                id.walk(w);
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
            Created::View { definition, .. } => definition.walk(w),
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
            Created::Method(routine) => routine.spec.1.walk(w),
            Created::Query(routine) => routine.spec.1.walk(w),
            Created::Routine(routine) => routine.spec.1.walk(w),
            _ => {}
        }
    }
}

impl Walk for ViewDefinition {
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
            TriggerBody::Statement { statement, .. } => statement.walk(w),
        }
    }
}

impl Walk for CodeBody {
    fn walk(&self, w: &mut Walker) {
        if let CodeBody::Sql(body) = self {
            body.walk(w)
        }
    }
}

impl Walk for MethodBody {
    fn walk(&self, w: &mut Walker) {
        if let MethodBody::Sql(body) = self {
            body.walk(w)
        }
    }
}

impl Walk for QueryBody {
    fn walk(&self, w: &mut Walker) {
        if let QueryBody::Sql(body) = self {
            body.walk(w)
        }
    }
}

impl Walk for SqlBody {
    fn walk(&self, w: &mut Walker) {
        for (_, statement, _, _) in self.body.wrapped.iter() {
            match statement {
                Either::Left(ret) => ret.value.1.walk(w),
                Either::Right(statement) => statement.walk(w),
            }
        }
    }
}

impl Walk for ReturnValue {
    fn walk(&self, w: &mut Walker) {
        match self {
            ReturnValue::Paren(value) => value.walk(w),
            ReturnValue::HostVariable(host_var) => host_var.walk(w),
            ReturnValue::Literal(..) | ReturnValue::Marker(_) => {}
        }
    }
}

impl<Body: Walk + Grammar> Walk for CodeSpecification<Body> {
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
            ModelSource::Subquery { query, .. } => query.walk(w),
            ModelSource::Inline {
                from,
                where_clause,
                group,
                having,
                tail,
            } => {
                from.walk(w);
                if let Some((_, condition)) = where_clause {
                    condition.walk(w)
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
        if let WithExpression::ValueExpression(expression) = self {
            expression.walk(w)
        }
    }
}
