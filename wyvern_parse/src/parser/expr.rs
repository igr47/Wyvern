//! Expression grammar. Precedence (low -> high):
//!   OR < AND < NOT < comparison/IS/IN/BETWEEN/LIKE < + - || < * / % < unary - < ::cast

use super::common::*;
use crate::ast::*;
use crate::lexer::Token as T;
use chumsky::prelude::*;

enum Pred<'a> {
    InList(Vec<Expr<'a>>),
    InQuery(Box<Select<'a>>),
    Between(Expr<'a>, Expr<'a>),
    Like { pattern: Expr<'a>, ci: bool },
}

enum Suffix<'a> {
    Cmp(BinaryOp, Expr<'a>),
    IsNull(bool),
    Pred(bool, Pred<'a>),
}

fn apply_suffix<'a>(lhs: Expr<'a>, s: Suffix<'a>) -> Expr<'a> {
    let l = Box::new(lhs);
    match s {
        Suffix::Cmp(op, r) => Expr::Binary { op, left: l, right: Box::new(r) },
        Suffix::IsNull(negated) => Expr::IsNull { expr: l, negated },
        Suffix::Pred(negated, p) => match p {
            Pred::InList(list) => Expr::InList { expr: l, list, negated },
            Pred::InQuery(query) => Expr::InSubquery { expr: l, query, negated },
            Pred::Between(lo, hi) => Expr::Between { expr: l, low: Box::new(lo), high: Box::new(hi), negated },
            Pred::Like { pattern, ci } => Expr::Like { expr: l, pattern: Box::new(pattern), negated, case_insensitive: ci },
        },
    }
}

/// One left-associative precedence level.
fn binary_level<'a, I: Inp<'a>>(
    next: impl Pr<'a, I, Expr<'a>> + 'a,
    op: impl Pr<'a, I, BinaryOp> + 'a,
) -> impl Pr<'a, I, Expr<'a>> {
    next.clone().foldl(op.then(next).repeated(), |l, (op, r)| Expr::Binary {
        op,
        left: Box::new(l),
        right: Box::new(r),
    })
}

/// `ORDER BY a [ASC|DESC], ...` (used by SELECT and by window specs)
pub fn order_by<'a, I: Inp<'a>>(
    expr: impl Pr<'a, I, Expr<'a>> + 'a,
) -> impl Pr<'a, I, Vec<OrderItem<'a>>> {
    let dir = choice((just(T::Asc).to(false), just(T::Desc).to(true)))
        .or_not()
        .map(|d| d.unwrap_or(false));
    just(T::Order).ignore_then(just(T::By)).ignore_then(comma_list(
        expr.then(dir).map(|(expr, descending)| OrderItem { expr, descending }),
    ))
}

pub fn where_clause<'a, I: Inp<'a>>(
    expr: impl Pr<'a, I, Expr<'a>> + 'a,
) -> impl Pr<'a, I, Expr<'a>> {
    just(T::Where).ignore_then(expr)
}

/// `query` is the (recursive) SELECT parser, needed for subqueries.
pub fn expr<'a, I: Inp<'a>>(
    query: impl Pr<'a, I, Select<'a>> + 'a,
) -> impl Pr<'a, I, Expr<'a>> {
    recursive(|expr| {
        // ---- atoms
        let int = select! { T::Int(s) => s }.try_map(|s, span| {
            s.parse::<i64>()
                .map(Literal::Int)
                .map_err(|e| Rich::custom(span, format!("invalid integer `{s}`: {e}")))
        });
        let float = select! { T::Float(s) => s }.try_map(|s, span| {
            s.parse::<f64>().map(Literal::Float).map_err(|e| Rich::custom(span, e.to_string()))
        });
        let literal = choice((
            string().map(Literal::Str),
            float,
            int,
            just(T::True).to(Literal::Bool(true)),
            just(T::False).to(Literal::Bool(false)),
            just(T::Null).to(Literal::Null),
        ))
        .map(Expr::Literal);

        let case = just(T::Case)
            .ignore_then(expr.clone().or_not())
            .then(
                just(T::When)
                    .ignore_then(expr.clone())
                    .then_ignore(just(T::Then))
                    .then(expr.clone())
                    .repeated()
                    .at_least(1)
                    .collect::<Vec<_>>(),
            )
            .then(just(T::Else).ignore_then(expr.clone()).or_not())
            .then_ignore(just(T::End))
            .map(|((operand, branches), else_branch)| Expr::Case {
                operand: operand.map(Box::new),
                branches,
                else_branch: else_branch.map(Box::new),
            });

        let cast = kw("cast")
            .ignore_then(parens(expr.clone().then_ignore(just(T::As)).then(data_type())))
            .map(|(e, ty)| Expr::Cast { expr: Box::new(e), ty });

        let exists = kw("exists")
            .ignore_then(parens(query.clone()))
            .map(|q| Expr::Exists(Box::new(q)));

        // function call: f(a, b) | f(*) | f(DISTINCT a) [OVER (PARTITION BY .. ORDER BY ..)]
        let func_name = ident().or(select! { T::Left => "left", T::Right => "right" });
        let call_args = choice((
            just(T::Star).to((false, true, Vec::new())),
            just(T::Distinct)
                .or_not()
                .map(|d| d.is_some())
                .then(expr.clone().separated_by(just(T::Comma)).allow_trailing().collect::<Vec<_>>())
                .map(|(d, args)| (d, false, args)),
        ));
        let window = kw("over")
            .ignore_then(parens(
                kw("partition")
                    .ignore_then(just(T::By))
                    .ignore_then(comma_list(expr.clone()))
                    .or_not()
                    .then(order_by(expr.clone()).or_not()),
            ))
            .map(|(p, o)| WindowSpec {
                partition_by: p.unwrap_or_default(),
                order_by: o.unwrap_or_default(),
            });
        let call = func_name
            .then(parens(call_args))
            .then(window.or_not())
            .map(|((name, (distinct, star, args)), over)| {
                Expr::Function(FunctionCall { name, args, distinct, star, over })
            });

        let column = ident()
            .then(just(T::Dot).ignore_then(ident()).or_not())
            .map(|(a, b)| match b {
                Some(name) => Expr::Column { table: Some(a), name },
                None => Expr::Column { table: None, name: a },
            });

        let subquery = parens(query.clone()).map(|q| Expr::Subquery(Box::new(q)));

        let atom = choice((
            literal,
            case,
            cast,
            exists,
            call, // must precede `column` (both start with an identifier)
            column,
            subquery, // must precede `( expr )`
            parens(expr.clone()),
        ));

        // ---- postfix `x::type`, prefix `-`
        let postfix = atom.foldl(
            just(T::ColonColon).ignore_then(data_type()).repeated(),
            |e, ty| Expr::Cast { expr: Box::new(e), ty },
        );
        let unary = just(T::Minus).repeated().foldr(postfix, |_, rhs| Expr::Unary {
            op: UnaryOp::Neg,
            expr: Box::new(rhs),
        });

        // ---- arithmetic
        let product = binary_level(
            unary,
            choice((
                just(T::Star).to(BinaryOp::Mul),
                just(T::Slash).to(BinaryOp::Div),
                just(T::Percent).to(BinaryOp::Mod),
            )),
        );
        let sum = binary_level(
            product,
            choice((
                just(T::Plus).to(BinaryOp::Add),
                just(T::Minus).to(BinaryOp::Sub),
                just(T::Concat).to(BinaryOp::Concat),
            )),
        );

        // ---- comparison & predicates
        let cmp_op = choice((
            just(T::Equals).to(BinaryOp::Eq),
            just(T::NotEq).to(BinaryOp::Ne),
            just(T::Le).to(BinaryOp::Le),
            just(T::Ge).to(BinaryOp::Ge),
            just(T::Lt).to(BinaryOp::Lt),
            just(T::Gt).to(BinaryOp::Gt),
        ));
        let in_pred = just(T::In).ignore_then(parens(choice((
            query.clone().map(|q| Pred::InQuery(Box::new(q))),
            comma_list(expr.clone()).map(Pred::InList),
        ))));
        let between = just(T::Between)
            .ignore_then(sum.clone())
            .then_ignore(just(T::And))
            .then(sum.clone())
            .map(|(lo, hi)| Pred::Between(lo, hi));
        let like = choice((just(T::Like).to(false), kw("ilike").to(true)))
            .then(sum.clone())
            .map(|(ci, pattern)| Pred::Like { pattern, ci });

        let suffix = choice((
            cmp_op.then(sum.clone()).map(|(op, r)| Suffix::Cmp(op, r)),
            just(T::Is)
                .ignore_then(just(T::Not).or_not())
                .then_ignore(just(T::Null))
                .map(|n| Suffix::IsNull(n.is_some())),
            just(T::Not)
                .or_not()
                .then(choice((in_pred, between, like)))
                .map(|(n, p)| Suffix::Pred(n.is_some(), p)),
        ));
        let comparison = sum.foldl(suffix.repeated(), apply_suffix);

        // ---- boolean logic
        let not = just(T::Not).repeated().foldr(comparison, |_, rhs| Expr::Unary {
            op: UnaryOp::Not,
            expr: Box::new(rhs),
        });
        let and = binary_level(not, just(T::And).to(BinaryOp::And));
        binary_level(and, just(T::Or).to(BinaryOp::Or))
    })
}

