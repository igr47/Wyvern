//! The only place that knows the full statement list.
//! To add a command: write `fn my_cmd() -> impl Pr<.., Statement>` in a module,
//! add one line to the `choice` below (and a variant to `ast::Statement`).

use super::{common::*, ddl, dml, expr::expr, meta, query::select_stmt};
use crate::ast::Statement;
use chumsky::prelude::*;

pub fn statement<'a, I: Inp<'a>>() -> impl Pr<'a, I, Statement<'a>> {
    // Built once, cloned (cheap, Rc-based) into every statement that needs them.
    let query = select_stmt();
    let expr = expr(query.clone());

    choice((
        query.clone().map(|q| Statement::Select(Box::new(q))),
        ddl::create_database(),
        ddl::use_database(),
        ddl::drop_database(),
        ddl::create_table(expr.clone(), query.clone()),
        ddl::drop_table(),
        ddl::alter_table(expr.clone()),
        dml::insert(expr.clone(), query.clone()),
        dml::update(expr.clone()),
        dml::delete(expr),
        dml::load(),
        meta::show(),
        meta::describe(),
        meta::summarize(query),
    ))
    .then_ignore(end())
}

