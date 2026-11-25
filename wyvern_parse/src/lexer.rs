//! Lexer (logos).
//!
//! Design notes
//! * Only *structural* words are reserved tokens. Everything else (types, `IF`,
//!   `EXISTS`, `COLUMN`, `HEADER`, `SUMMARIZE`, every future ML / plot word...)
//!   lexes as `Ident` and is matched case-insensitively by the parser (`kw("..")`).
//!   So `date`, `value`, `year`, `count`, `model` stay usable as column names and
//!   adding a new command never touches this file.
//! * Tokens borrow from the source: zero allocation while lexing.

use crate::diagnostics::Diagnostic;
use chumsky::span::SimpleSpan;
use logos::Logos;
use std::fmt;

fn trim<'a>(s: &'a str) -> &'a str {
    &s[1..s.len() - 1]
}

#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t\r\n\f]+")]
#[logos(skip(r"--[^\n]*", allow_greedy = true))]
#[logos(skip r"/\*([^*]|\*+[^*/])*\*+/")]
pub enum Token<'a> {
    // ---- reserved keywords (case-insensitive) ----
    #[token("select", ignore(case))] Select,
    #[token("from", ignore(case))] From,
    #[token("where", ignore(case))] Where,
    #[token("group", ignore(case))] Group,
    #[token("by", ignore(case))] By,
    #[token("having", ignore(case))] Having,
    #[token("order", ignore(case))] Order,
    #[token("limit", ignore(case))] Limit,
    #[token("offset", ignore(case))] Offset,
    #[token("as", ignore(case))] As,
    #[token("and", ignore(case))] And,
    #[token("or", ignore(case))] Or,
    #[token("not", ignore(case))] Not,
    #[token("in", ignore(case))] In,
    #[token("is", ignore(case))] Is,
    #[token("null", ignore(case))] Null,
    #[token("true", ignore(case))] True,
    #[token("false", ignore(case))] False,
    #[token("like", ignore(case))] Like,
    #[token("between", ignore(case))] Between,
    #[token("case", ignore(case))] Case,
    #[token("when", ignore(case))] When,
    #[token("then", ignore(case))] Then,
    #[token("else", ignore(case))] Else,
    #[token("end", ignore(case))] End,
    #[token("join", ignore(case))] Join,
    #[token("inner", ignore(case))] Inner,
    #[token("left", ignore(case))] Left,
    #[token("right", ignore(case))] Right,
    #[token("full", ignore(case))] Full,
    #[token("cross", ignore(case))] Cross,
    #[token("outer", ignore(case))] Outer,
    #[token("on", ignore(case))] On,
    #[token("insert", ignore(case))] Insert,
    #[token("into", ignore(case))] Into,
    #[token("values", ignore(case))] Values,
    #[token("update", ignore(case))] Update,
    #[token("set", ignore(case))] Set,
    #[token("delete", ignore(case))] Delete,
    #[token("create", ignore(case))] Create,
    #[token("drop", ignore(case))] Drop,
    #[token("alter", ignore(case))] Alter,
    #[token("table", ignore(case))] Table,
    #[token("database", ignore(case))] Database,
    #[token("use", ignore(case))] Use,
    #[token("distinct", ignore(case))] Distinct,
    #[token("asc", ignore(case))] Asc,
    #[token("desc", ignore(case))] Desc,

    // ---- operators ----
    #[token("=")] #[token("==")] Equals,
    #[token("!=")] #[token("<>")] NotEq,
    #[token("<")] Lt,
    #[token("<=")] Le,
    #[token(">")] Gt,
    #[token(">=")] Ge,
    #[token("+")] Plus,
    #[token("-")] Minus,
    #[token("*")] Star,
    #[token("/")] Slash,
    #[token("%")] Percent,
    #[token("||")] Concat,

    // ---- punctuation ----
    #[token("(")] LParen,
    #[token(")")] RParen,
    #[token("[")] LBracket,
    #[token("]")] RBracket,
    #[token(",")] Comma,
    #[token(".")] Dot,
    #[token(";")] Semi,
    #[token(":")] Colon,
    #[token("::")] ColonColon,

    // ---- literals & identifiers (all borrow from the source) ----
    #[regex(r"[A-Za-z_][A-Za-z0-9_]*", |lex| lex.slice())]
    Ident(&'a str),
    /// "quoted name" or `quoted name` (lets you use reserved words / spaces as names)
    #[regex(r#""[^"]+""#, |lex| trim(lex.slice()))]
    #[regex(r"`[^`]+`", |lex| trim(lex.slice()))]
    QuotedIdent(&'a str),
    /// 'text' (raw slice; `''` escapes are resolved by the parser)
    #[regex(r"'([^']|'')*'", |lex| trim(lex.slice()))]
    Str(&'a str),
    #[regex(r"[0-9]+", |lex| lex.slice())]
    Int(&'a str),
    #[regex(r"[0-9]+\.[0-9]+([eE][+-]?[0-9]+)?|[0-9]+[eE][+-]?[0-9]+", |lex| lex.slice())]
    Float(&'a str),
}

impl fmt::Display for Token<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use Token::*;
        let s = match self {
            Select => "SELECT", From => "FROM", Where => "WHERE", Group => "GROUP",
            By => "BY", Having => "HAVING", Order => "ORDER", Limit => "LIMIT",
            Offset => "OFFSET", As => "AS", And => "AND", Or => "OR", Not => "NOT",
            In => "IN", Is => "IS", Null => "NULL", True => "TRUE", False => "FALSE",
            Like => "LIKE", Between => "BETWEEN", Case => "CASE", When => "WHEN",
            Then => "THEN", Else => "ELSE", End => "END", Join => "JOIN",
            Inner => "INNER", Left => "LEFT", Right => "RIGHT", Full => "FULL",
            Cross => "CROSS", Outer => "OUTER", On => "ON", Insert => "INSERT",
            Into => "INTO", Values => "VALUES", Update => "UPDATE", Set => "SET",
            Delete => "DELETE", Create => "CREATE", Drop => "DROP", Alter => "ALTER",
            Table => "TABLE", Database => "DATABASE", Use => "USE",
            Distinct => "DISTINCT", Asc => "ASC", Desc => "DESC",
            Equals => "=", NotEq => "!=", Lt => "<", Le => "<=", Gt => ">", Ge => ">=",
            Plus => "+", Minus => "-", Star => "*", Slash => "/", Percent => "%",
            Concat => "||", LParen => "(", RParen => ")", LBracket => "[",
            RBracket => "]", Comma => ",", Dot => ".", Semi => ";", Colon => ":",
            ColonColon => "::",
            Ident(s) => return write!(f, "`{s}`"),
            QuotedIdent(s) => return write!(f, "\"{s}\""),
            Str(s) => return write!(f, "'{s}'"),
            Int(s) | Float(s) => return write!(f, "{s}"),
        };
        f.write_str(s)
    }
}

/// Tokenise the whole input. Unknown characters become diagnostics (the parser
/// never sees them), so one typo yields a clear message, not a parse cascade.
pub fn lex(src: &str) -> (Vec<(Token<'_>, SimpleSpan)>, Vec<Diagnostic>) {
    let mut tokens = Vec::with_capacity(src.len() / 4 + 1);
    let mut diags = Vec::new();
    for (res, span) in Token::lexer(src).spanned() {
        match res {
            Ok(tok) => tokens.push((tok, span.into())),
            Err(_) => diags.push(Diagnostic {
                message: format!("unexpected input `{}`", &src[span.clone()]),
                span,
            }),
        }
    }
    (tokens, diags)
}

