//! INSERT / UPDATE / DELETE / LOAD

use super::common::*;
use super::expr::where_clause;
use crate::ast::*;
use crate::lexer::Token as T;
use chumsky::prelude::*;

/// `INSERT INTO t [(a, b)] VALUES (..), (..)`  |  `INSERT INTO t [(a, b)] SELECT ..`
pub fn insert<'a, I: Inp<'a>>(
    expr: impl Pr<'a, I, Expr<'a>> + 'a,
    query: impl Pr<'a, I, Select<'a>> + 'a,
) -> impl Pr<'a, I, Statement<'a>> {
    let values = just(T::Values).ignore_then(comma_list(parens(comma_list(expr))));
    let source = choice((
        values.map(InsertSource::Values),
        query.map(|q| InsertSource::Query(Box::new(q))),
    ));
    just(T::Insert)
        .ignore_then(just(T::Into).or_not())
        .ignore_then(object_name())
        .then(parens(comma_list(ident())).or_not())
        .then(source)
        .map(|((table, columns), source)| {
            Statement::Insert(Insert { table, columns: columns.unwrap_or_default(), source })
        })
}

/// `UPDATE t SET a = e, b = e [WHERE ..]`
pub fn update<'a, I: Inp<'a>>(expr: impl Pr<'a, I, Expr<'a>> + 'a) -> impl Pr<'a, I, Statement<'a>> {
    let assignment = ident().then_ignore(just(T::Equals)).then(expr.clone());
    just(T::Update)
        .ignore_then(object_name())
        .then_ignore(just(T::Set))
        .then(comma_list(assignment))
        .then(where_clause(expr).or_not())
        .map(|((table, assignments), filter)| {
            Statement::Update(Update { table, assignments, filter })
        })
}

/// `DELETE FROM t [WHERE ..]`
pub fn delete<'a, I: Inp<'a>>(expr: impl Pr<'a, I, Expr<'a>> + 'a) -> impl Pr<'a, I, Statement<'a>> {
    just(T::Delete)
        .ignore_then(just(T::From))
        .ignore_then(object_name())
        .then(where_clause(expr).or_not())
        .map(|(table, filter)| Statement::Delete(Delete { table, filter }))
}

/// `LOAD 'file.csv' INTO t [WITH HEADER] [DELIMITER ';']`
/// Appends file rows to `t` (the executor creates `t` with an inferred schema if missing).
pub fn load<'a, I: Inp<'a>>() -> impl Pr<'a, I, Statement<'a>> {
    kw("load")
        .ignore_then(string())
        .then_ignore(just(T::Into))
        .then(object_name())
        .then(file_options())
        .map(|((path, table), opts)| {
            Statement::Load(Load { table, file: FileSpec::new(path, opts) })
        })
}

