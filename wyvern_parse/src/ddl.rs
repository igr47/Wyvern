//! CREATE / USE / DROP / ALTER

use super::common::*;
use crate::ast::*;
use crate::lexer::Token as T;
use chumsky::prelude::*;

pub fn create_database<'a, I: Inp<'a>>() -> impl Pr<'a, I, Statement<'a>> {
    just(T::Create)
        .ignore_then(just(T::Database))
        .ignore_then(if_not_exists())
        .then(ident())
        .map(|(if_not_exists, name)| Statement::CreateDatabase { name, if_not_exists })
}

/// `USE shop` or `USE DATABASE shop`
pub fn use_database<'a, I: Inp<'a>>() -> impl Pr<'a, I, Statement<'a>> {
    just(T::Use)
        .ignore_then(just(T::Database).or_not())
        .ignore_then(ident())
        .map(|name| Statement::UseDatabase { name })
}

pub fn drop_database<'a, I: Inp<'a>>() -> impl Pr<'a, I, Statement<'a>> {
    just(T::Drop)
        .ignore_then(just(T::Database))
        .ignore_then(if_exists())
        .then(ident())
        .map(|(if_exists, name)| Statement::DropDatabase { name, if_exists })
}

/// `DROP TABLE [IF EXISTS] a, b, c`
pub fn drop_table<'a, I: Inp<'a>>() -> impl Pr<'a, I, Statement<'a>> {
    just(T::Drop)
        .ignore_then(just(T::Table))
        .ignore_then(if_exists())
        .then(comma_list(object_name()))
        .map(|(if_exists, names)| Statement::DropTable { names, if_exists })
}

/// `name TYPE [PRIMARY KEY] [NOT NULL] [UNIQUE] [AUTO_INCREMENT] [DEFAULT e] [CHECK (e)]`
pub fn column_def<'a, I: Inp<'a>>(
    expr: impl Pr<'a, I, Expr<'a>> + 'a,
) -> impl Pr<'a, I, ColumnDef<'a>> {
    let constraint = choice((
        kw("primary").ignore_then(kw("key")).to(ColumnConstraint::PrimaryKey),
        just(T::Not).ignore_then(just(T::Null)).to(ColumnConstraint::NotNull),
        just(T::Null).to(ColumnConstraint::Null),
        kw("unique").to(ColumnConstraint::Unique),
        kw("auto_increment").or(kw("autoincrement")).to(ColumnConstraint::AutoIncrement),
        kw("default").ignore_then(expr.clone()).map(ColumnConstraint::Default),
        kw("check").ignore_then(parens(expr)).map(ColumnConstraint::Check),
    ));
    ident()
        .then(data_type())
        .then(constraint.repeated().collect::<Vec<_>>())
        .map(|((name, ty), constraints)| ColumnDef { name, ty, constraints })
}

/// ```text
/// CREATE [OR REPLACE] [TEMP | IN MEMORY] TABLE [IF NOT EXISTS] t (col type ..., ...)
/// CREATE TABLE t FROM 'file.csv' [WITH HEADER] [DELIMITER ';']   -- schema inferred
/// CREATE TABLE t AS SELECT ...
/// ```
pub fn create_table<'a, I: Inp<'a>>(
    expr: impl Pr<'a, I, Expr<'a>> + 'a,
    query: impl Pr<'a, I, Select<'a>> + 'a,
) -> impl Pr<'a, I, Statement<'a>> {
    let or_replace = just(T::Or).ignore_then(kw("replace")).or_not().map(|o| o.is_some());
    let kind = choice((
        kw("temp").or(kw("temporary")).to(TableKind::Temporary),
        kw("memory")
            .or(kw("in_memory"))
            .or(just(T::In).ignore_then(kw("memory")))
            .to(TableKind::InMemory),
    ))
    .or_not()
    .map(|k| k.unwrap_or(TableKind::Persistent));

    let body = choice((
        parens(comma_list(column_def(expr))).map(TableBody::Columns),
        just(T::From).ignore_then(file_spec()).map(TableBody::File),
        just(T::As).ignore_then(query).map(|q| TableBody::Query(Box::new(q))),
    ));

    just(T::Create)
        .ignore_then(or_replace)
        .then(kind)
        .then_ignore(just(T::Table))
        .then(if_not_exists())
        .then(object_name())
        .then(body)
        .map(|((((or_replace, kind), if_not_exists), name), body)| {
            Statement::CreateTable(CreateTable { name, kind, or_replace, if_not_exists, body })
        })
}

/// ```text
/// ALTER TABLE t ADD [COLUMN] c TYPE, DROP [COLUMN] c, RENAME [COLUMN] a TO b,
///               RENAME TO new_name, MODIFY [COLUMN] c TYPE, ALTER COLUMN c [TYPE] TYPE
/// ```
/// Several comma-separated actions may follow one `ALTER TABLE`.
pub fn alter_table<'a, I: Inp<'a>>(
    expr: impl Pr<'a, I, Expr<'a>> + 'a,
) -> impl Pr<'a, I, Statement<'a>> {
    let col = kw("column").or_not();
    let action = choice((
        kw("add")
            .ignore_then(col.clone())
            .ignore_then(column_def(expr))
            .map(AlterAction::AddColumn),
        just(T::Drop)
            .ignore_then(col.clone())
            .ignore_then(if_exists())
            .then(ident())
            .map(|(if_exists, name)| AlterAction::DropColumn { name, if_exists }),
        kw("rename").ignore_then(choice((
            kw("to").ignore_then(ident()).map(AlterAction::RenameTable),
            col.clone()
                .ignore_then(ident())
                .then_ignore(kw("to"))
                .then(ident())
                .map(|(from, to)| AlterAction::RenameColumn { from, to }),
        ))),
        kw("modify")
            .ignore_then(col.clone())
            .ignore_then(ident())
            .then(data_type())
            .map(|(name, ty)| AlterAction::ModifyColumn { name, ty }),
        just(T::Alter)
            .ignore_then(kw("column"))
            .ignore_then(ident())
            .then_ignore(kw("type").or_not())
            .then(data_type())
            .map(|(name, ty)| AlterAction::ModifyColumn { name, ty }),
    ));

    just(T::Alter)
        .ignore_then(just(T::Table))
        .ignore_then(object_name())
        .then(comma_list(action))
        .map(|(table, actions)| Statement::AlterTable(AlterTable { table, actions }))
}

