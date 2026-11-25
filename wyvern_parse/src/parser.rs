//! Parsers, one module per statement family. Every statement is
//! `fn(...) -> impl Pr<'a, I, Statement<'a>>`; wiring lives in `statement.rs`.

pub mod common;
pub mod ddl;
pub mod dml;
pub mod expr;
pub mod meta;
pub mod query;
pub mod statement;

pub use statement::statement;

