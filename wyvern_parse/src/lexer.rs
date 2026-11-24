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
#[logos(skip r"--[^\n]*")]
#[logos(skip r"/\*([^*]|\*+[^*/])*\*+/")]
pub enum Token<'a> {
    // ---- reserved keywords (case-insensitive) ----
    #[token("select", ignore(ascii_case))] Select,
    #[token("from", ignore(ascii_case))] From,
    #[token("where", ignore(ascii_case))] Where,
    #[token("group", ignore(ascii_case))] Group,
    #[token("by", ignore(ascii_case))] By,
    #[token("having", ignore(ascii_case))] Having,
    #[token("order", ignore(ascii_case))] Order,
    #[token("limit", ignore(ascii_case))] Limit,
    #[token("offset", ignore(ascii_case))] Offset,
    #[token("as", ignore(ascii_case))] As,
    #[token("and", ignore(ascii_case))] And,
    #[token("or", ignore(ascii_case))] Or,
    #[token("not", ignore(ascii_case))] Not,
    #[token("in", ignore(ascii_case))] In,
    #[token("is", ignore(ascii_case))] Is,
    #[token("null", ignore(ascii_case))] Null,
    #[token("true", ignore(ascii_case))] True,
    #[token("false", ignore(ascii_case))] False,
    #[token("like", ignore(ascii_case))] Like,
    #[token("between", ignore(ascii_case))] Between,
    #[token("case", ignore(ascii_case))] Case,
    #[token("when", ignore(ascii_case))] When,
    #[token("then", ignore(ascii_case))] Then,
    #[token("else", ignore(ascii_case))] Else,
    #[token("end", ignore(ascii_case))] End,
    #[token("join", ignore(ascii_case))] Join,
    #[token("inner", ignore(ascii_case))] Inner,
    #[token("left", ignore(ascii_case))] Left,
    #[token("right", ignore(ascii_case))] Right,
    #[token("full", ignore(ascii_case))] Full,
    #[token("cross", ignore(ascii_case))] Cross,
    #[token("outer", ignore(ascii_case))] Outer,
    #[token("on", ignore(ascii_case))] On,
    #[token("insert", ignore(ascii_case))] Insert,
    #[token("into", ignore(ascii_case))] Into,
    #[token("values", ignore(ascii_case))] Values,
    #[token("update", ignore(ascii_case))] Update,
    #[token("set", ignore(ascii_case))] Set,
    #[token("delete", ignore(ascii_case))] Delete,
    #[token("create", ignore(ascii_case))] Create,
    #[token("drop", ignore(ascii_case))] Drop,
    #[token("alter", ignore(ascii_case))] Alter,
    #[token("table", ignore(ascii_case))] Table,
    #[token("database", ignore(ascii_case))] Database,
    #[token("use", ignore(ascii_case))] Use,
    #[token("distinct", ignore(ascii_case))] Distinct,
    #[token("asc", ignore(ascii_case))] Asc,
    #[token("desc", ignore(ascii_case))] Desc,

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

