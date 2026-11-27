use std::fmt;

#[derive(Debug)]
pub enum StorageError {
    Io(std::io::Error),
    /// The file is not a Wyvern database, or is damaged.
    Corrupt(String),
    NotFound(String),
    AlreadyExists(String),
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, StorageError>;

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageError::Io(e) => write!(f, "io error: {e}"),
            StorageError::Corrupt(m) => write!(f, "corrupt database file: {m}"),
            StorageError::NotFound(m) => write!(f, "{m} not found"),
            StorageError::AlreadyExists(m) => write!(f, "{m} already exists"),
            StorageError::Invalid(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for StorageError {}

impl From<std::io::Error> for StorageError {
    fn from(e: std::io::Error) -> Self {
        StorageError::Io(e)
    }
}

