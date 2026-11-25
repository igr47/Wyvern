//! SELECT. Two equivalent shapes sharing the same clause parsers:
//!
//!   SELECT a, sum(b) FROM t WHERE .. GROUP BY .. HAVING .. ORDER BY .. LIMIT n
//!   FROM t WHERE .. GROUP BY ALL ORDER BY .. LIMIT n SELECT a, sum(b)   -- FROM-first
//!
//! FROM-first reads in execution order, and a bare `FROM t` is `SELECT * FROM t`.

use super::common::*;
use super::expr::{expr, order_by, where_clause};
use crate::ast::*;
use crate::lexer::Token as T;
use chumsky::prelude::*;

struct Tail<'a> {
    filter: Option<Expr<'a>>,
    group_by: Option<GroupBy<'a>>,
    having: Option<Expr<'a>>,
    order_by: Vec<OrderItem<'a>>,
    limit: Option<u64>,
    offset: Option<u64>,
}

fn assemble<'a>(
    distinct: bool,
    projection: Vec<SelectItem<'a>>,
    from: Option<(TableRef<'a>, Vec<Join<'a>>)>,
    t: Tail<'a>,
) -> Select<'a> {
    let (from, joins) = match from {
        Some((f, j)) => (Some(f), j),
        None => (None, Vec::new()),
    };
    Select {
        distinct,
        projection,
        from,
        joins,
        filter: t.filter,
        group_by: t.group_by,
        having: t.having,
        order_by: t.order_by,
        limit: t.limit,
        offset: t.offset,
    }
}

pub fn select_stmt<'a, I: Inp<'a>>() -> impl Pr<'a, I, Select<'a>> {
    recursive(|query| {
        let expr = expr(query.clone());

        // ---- projection
        let as_alias = just(T::As).ignore_then(ident());
        let item_alias = as_alias.clone().or(ident());
        let item = choice((
            just(T::Star)
                .ignore_then(kw("exclude").ignore_then(parens(comma_list(ident()))).or_not())
                .map(|ex| SelectItem::Wildcard { exclude: ex.unwrap_or_default() }),
            expr.clone()
                .then(item_alias.or_not())
                .map(|(expr, alias)| SelectItem::Expr { expr, alias }),
        ));
        let projection = just(T::Distinct)
            .or_not()
            .map(|d| d.is_some())
            .then(comma_list(item));

        // ---- sources
        let table_alias = as_alias.or(ident().filter(|s| !s.eq_ignore_ascii_case("using")));
        let table_ref = choice((
            parens(query.clone()).map(|q| TableFactor::Subquery(Box::new(q))),
            object_name().map(TableFactor::Table),
        ))
        .then(table_alias.or_not())
        .map(|(factor, alias)| TableRef { factor, alias });

        let join_kind = choice((
            just(T::Cross).ignore_then(just(T::Join)).to(JoinKind::Cross),
            just(T::Left).then(just(T::Outer).or_not()).then(just(T::Join)).to(JoinKind::Left),
            just(T::Right).then(just(T::Outer).or_not()).then(just(T::Join)).to(JoinKind::Right),
            just(T::Full).then(just(T::Outer).or_not()).then(just(T::Join)).to(JoinKind::Full),
            just(T::Inner).or_not().then(just(T::Join)).to(JoinKind::Inner),
        ));
        let join_constraint = choice((
            just(T::On).ignore_then(expr.clone()).map(JoinConstraint::On),
            kw("using").ignore_then(parens(comma_list(ident()))).map(JoinConstraint::Using),
        ));
        let join = join_kind
            .then(table_ref.clone())
            .then(join_constraint.or_not())
            .map(|((kind, table), constraint)| Join { kind, table, constraint });
        let source = table_ref.then(join.repeated().collect::<Vec<_>>());

        // ---- tail clauses (shared by both shapes)
        let group_by = just(T::Group).ignore_then(just(T::By)).ignore_then(choice((
            kw("all").to(GroupBy::All),
            comma_list(expr.clone()).map(GroupBy::Exprs),
        )));
        let having = just(T::Having).ignore_then(expr.clone());
        let tail = group((
            where_clause(expr.clone()).or_not(),
            group_by.or_not(),
            having.or_not(),
            order_by(expr).or_not(),
            just(T::Limit).ignore_then(uint()).or_not(),
            just(T::Offset).ignore_then(uint()).or_not(),
        ))
        .map(|(filter, group_by, having, order_by, limit, offset)| Tail {
            filter,
            group_by,
            having,
            order_by: order_by.unwrap_or_default(),
            limit,
            offset,
        });

        // ---- SELECT-first
        let select_first = just(T::Select)
            .ignore_then(projection.clone())
            .then(just(T::From).ignore_then(source.clone()).or_not())
            .then(tail.clone())
            .map(|(((distinct, proj), from), t)| assemble(distinct, proj, from, t));

        // ---- FROM-first (projection optional, defaults to `*`)
        let from_first = just(T::From)
            .ignore_then(source)
            .then(tail)
            .then(just(T::Select).ignore_then(projection).or_not())
            .map(|((from, t), proj)| {
                let (distinct, proj) =
                    proj.unwrap_or_else(|| (false, vec![SelectItem::Wildcard { exclude: vec![] }]));
                assemble(distinct, proj, Some(from), t)
            });

        select_first.or(from_first)
    })
}

