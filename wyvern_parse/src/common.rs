//! Shared building blocks. Everything reusable (lists, parens, names, types,
//! file options, IF [NOT] EXISTS) lives here exactly once.

use crate::ast::*;
use crate::lexer::Token as T;
use chumsky::{input::ValueInput, prelude::*};
use std::borrow::Cow;

pub type Err<'a> = extra::Err<Rich<'a, T<'a>>>;

/// Input = stream of `Token`s with `SimpleSpan`s.
pub trait Inp<'a>: ValueInput<'a, Token = T<'a>, Span = SimpleSpan> {}
impl<'a, X> Inp<'a> for X where X: ValueInput<'a, Token = T<'a>, Span = SimpleSpan> {}

/// "A cloneable parser producing `O`" — keeps every signature short.
pub trait Pr<'a, I: Inp<'a>, O>: Parser<'a, I, O, Err<'a>> + Clone {}
impl<'a, I: Inp<'a>, O, X> Pr<'a, I, O> for X where X: Parser<'a, I, O, Err<'a>> + Clone {}

// ---------------------------------------------------------------- primitives
/// Soft (contextual) keyword: an identifier matched case-insensitively.
pub fn kw<'a, I: Inp<'a>>(word: &'static str) -> impl Pr<'a, I, ()> {
    select! { T::Ident(s) if s.eq_ignore_ascii_case(word) => () }
}

pub fn ident<'a, I: Inp<'a>>() -> impl Pr<'a, I, &'a str> {
    select! { T::Ident(s) => s, T::QuotedIdent(s) => s }
}

/// `name` or `db.name`
pub fn object_name<'a, I: Inp<'a>>() -> impl Pr<'a, I, ObjectName<'a>> {
    ident()
        .then(just(T::Dot).ignore_then(ident()).or_not())
        .map(|(a, b)| match b {
            Some(name) => ObjectName { schema: Some(a), name },
            None => ObjectName { schema: None, name: a },
        })
}

pub fn unescape<'a>(s: &'a str) -> Cow<'a, str> {
    if s.contains("''") {
        Cow::Owned(s.replace("''", "'"))
    } else {
        Cow::Borrowed(s)
    }
}

pub fn string<'a, I: Inp<'a>>() -> impl Pr<'a, I, Cow<'a, str>> {
    select! { T::Str(s) => unescape(s) }
}

pub fn uint<'a, I: Inp<'a>>() -> impl Pr<'a, I, u64> {
    select! { T::Int(s) => s }.try_map(|s, span| {
        s.parse::<u64>()
            .map_err(|e| Rich::custom(span, format!("invalid integer `{s}`: {e}")))
    })
}

// ------------------------------------------------------------------ combinators
pub fn comma_list<'a, I: Inp<'a>, O>(item: impl Pr<'a, I, O>) -> impl Pr<'a, I, Vec<O>> {
    item.separated_by(just(T::Comma)).at_least(1).collect::<Vec<_>>()
}

pub fn parens<'a, I: Inp<'a>, O>(inner: impl Pr<'a, I, O>) -> impl Pr<'a, I, O> {
    inner.delimited_by(just(T::LParen), just(T::RParen))
}

pub fn if_exists<'a, I: Inp<'a>>() -> impl Pr<'a, I, bool> {
    kw("if").ignore_then(kw("exists")).or_not().map(|o| o.is_some())
}

pub fn if_not_exists<'a, I: Inp<'a>>() -> impl Pr<'a, I, bool> {
    kw("if")
        .ignore_then(just(T::Not))
        .ignore_then(kw("exists"))
        .or_not()
        .map(|o| o.is_some())
}

// ------------------------------------------------------------------ data types
/// `INT`, `TEXT`, `VARCHAR(255)`, `DECIMAL(10, 2)` ... resolved by `DataType::from_name`.
pub fn data_type<'a, I: Inp<'a>>() -> impl Pr<'a, I, DataType> {
    let n = select! { T::Int(s) => s }.try_map(|s, span| {
        s.parse::<u32>().map_err(|e| Rich::custom(span, e.to_string()))
    });
    ident()
        .then(parens(n.separated_by(just(T::Comma)).collect::<Vec<_>>()).or_not())
        .try_map(|(name, args), span| {
            DataType::from_name(name, &args.unwrap_or_default())
                .ok_or_else(|| Rich::custom(span, format!("unknown data type `{name}`")))
        })
}

// ------------------------------------------------------------------ file sources
/// `[WITH] [HEADER | NO HEADER] [DELIMITER ';'] [FORMAT csv]` — shared by
/// `CREATE TABLE .. FROM` and `LOAD`.
pub fn file_options<'a, I: Inp<'a>>() -> impl Pr<'a, I, Vec<FileOpt<'a>>> {
    let opt = choice((
        kw("header").to(FileOpt::Header(true)),
        kw("no").ignore_then(kw("header")).to(FileOpt::Header(false)),
        kw("delimiter").ignore_then(string()).map(FileOpt::Delimiter),
        kw("format").ignore_then(ident()).map(FileOpt::Format),
    ));
    kw("with").or_not().ignore_then(opt.repeated().collect::<Vec<_>>())
}

pub fn file_spec<'a, I: Inp<'a>>() -> impl Pr<'a, I, FileSpec<'a>> {
    string().then(file_options()).map(|(path, opts)| FileSpec::new(path, opts))
}

