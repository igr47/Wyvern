//! DataQL: a declarative, SQL-flavoured language for fast data analysis.
//!
//! Pipeline:  source text --logos--> tokens --split on `;`--> chumsky --> AST
//!
//! * `lexer`       zero-copy tokens (`&str` slices into the source)
//! * `ast`         borrowed AST (no String allocations for names)
//! * `parser`      chumsky parsers, one module per statement family
//! * `diagnostics` error rendering

pub mod ast;
pub mod diagnostics;
pub mod lexer;
pub mod parser;

use ast::{Span, Spanned, Statement};
use chumsky::{input::Stream, prelude::*};
use diagnostics::Diagnostic;
use lexer::Token;

/// Parse a whole script (many `;`-separated statements).
///
/// Statements are split on `;` *tokens* before parsing, so a syntax error in one
/// statement never hides errors in the next, and every statement is parsed
/// independently (easy to parallelise later). Strings are single tokens, so a
/// `;` inside a string literal can never split a statement.
pub fn parse_script<'src>(
    src: &'src str,
) -> Result<Vec<Spanned<Statement<'src>>>, Vec<Diagnostic>> {
    let (tokens, mut diags) = lexer::lex(src);
    let parser = parser::statement();
    let mut out = Vec::new();

    for chunk in tokens.split(|(t, _)| matches!(t, Token::Semi)) {
        let (Some(first), Some(last)) = (chunk.first(), chunk.last()) else {
            continue;
        };
        let whole: Span = (first.1.start..last.1.end).into();
        let eoi: Span = (last.1.end..last.1.end).into();

        // Own the chunk so the Stream doesn't borrow `tokens`.
        let owned: Vec<(Token<'src>, Span)> = chunk.to_vec();
        let input = Stream::from_iter(owned.into_iter())
            .map(eoi, |(t, s)| (t, s));

        let (stmt, errs) = parser.parse(input).into_output_errors();
        diags.extend(errs.iter().map(|e| Diagnostic {
            span: e.span().into_range(),
            message: e.reason().to_string(),
        }));
        if let Some(node) = stmt {
            out.push(Spanned { node, span: whole });
        }
    }

    if diags.is_empty() { Ok(out) } else { Err(diags) }
}
