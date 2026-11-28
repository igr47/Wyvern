use std::fmt;
use wyvern_parse::diagnostics::Diagnostic;
use wyvern_storage::StorageError;

#[derive(Debug)]
pub enum EngineError {
    /// Syntax errors from the parser (all statements of the script).
    Parse(Vec<Diagnostic>),
    Storage(StorageError),
    /// Semantic or runtime error (unknown column, type mismatch, constraint violation...).
    Msg(String),
}

pub type Result<T> = std::result::Result<T, EngineError>;

impl EngineError {
    pub fn msg(s: impl Into<String>) -> EngineError {
        EngineError::Msg(s.into())
    }

    /// Multi-line, human-friendly rendering (syntax errors show the source line).
    pub fn render(&self, src: &str) -> String {
        match self {
            EngineError::Parse(d) => d.iter().map(|d| d.render(src)).collect::<Vec<_>>().join("\n"),
            other => format!("error: {other}"),
        }
    }
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::Parse(d) => {
                let first = d.first().map(|d| d.message.as_str()).unwrap_or("syntax error");
                write!(f, "syntax error: {first}")
            }
            EngineError::Storage(e) => write!(f, "{e}"),
            EngineError::Msg(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for EngineError {}

impl From<StorageError> for EngineError {
    fn from(e: StorageError) -> Self {
        EngineError::Storage(e)
    }
}

/// `bail!("fmt", args..)` returns `Err(EngineError::Msg(..))`.
#[macro_export]
macro_rules! bail {
    ($($t:tt)*) => { return Err($crate::error::EngineError::Msg(format!($($t)*))) };
}

