//! Introspection & quick analysis commands. All use *soft* keywords, so adding
//! one is a single function here + one line in `statement.rs`.

use super::common::*;
use crate::ast::*;
use crate::lexer::Token as T;
use chumsky::prelude::*;

/// `SHOW DATABASES` | `SHOW TABLES`
pub fn show<'a, I: Inp<'a>>() -> impl Pr<'a, I, Statement<'a>> {
    kw("show")
        .ignore_then(choice((
            kw("databases").to(ShowTarget::Databases),
            kw("tables").to(ShowTarget::Tables),
        )))
        .map(Statement::Show)
}

/// `DESCRIBE t` (also `DESC t`): column names, types, constraints
pub fn describe<'a, I: Inp<'a>>() -> impl Pr<'a, I, Statement<'a>> {
    kw("describe")
        .or(just(T::Desc).ignored())
        .ignore_then(object_name())
        .map(|table| Statement::Describe { table })
}

/// `SUMMARIZE t` | `SUMMARIZE FROM t WHERE ..`: one-line data profile per column
/// (count, nulls, distinct, min/max, mean, std, quartiles). One command instead of
/// a dozen hand-written aggregates.
pub fn summarize<'a, I: Inp<'a>>(
    query: impl Pr<'a, I, Select<'a>> + 'a,
) -> impl Pr<'a, I, Statement<'a>> {
    kw("summarize")
        .ignore_then(choice((
            query.map(|q| Relation::Query(Box::new(q))),
            object_name().map(Relation::Table),
        )))
        .map(Statement::Summarize)
}

