//! A folder of databases: `<root>/<name>.wyv`, one file per database.

use crate::database::Database;
use crate::error::{Result, StorageError};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const EXT: &str = "wyv";

#[derive(Debug)]
pub struct Storage {
    root: Option<PathBuf>,
    dbs: BTreeMap<String, Database>,
}

fn validate(name: &str) -> Result<()> {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err(StorageError::Invalid(format!(
            "invalid database name `{name}` (use letters, digits, `_` or `-`)"
        )));
    }
    Ok(())
}

impl Storage {
    /// Opens (creating if needed) the data folder and every database in it.
    pub fn open(dir: impl AsRef<Path>) -> Result<Storage> {
        let root = dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&root)?;
        let mut dbs = BTreeMap::new();
        for entry in std::fs::read_dir(&root)? {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) != Some(EXT) {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                dbs.insert(stem.to_ascii_lowercase(), Database::open(stem, &path)?);
            }
        }
        Ok(Storage { root: Some(root), dbs })
    }

    /// Nothing touches disk; every database lives in memory.
    pub fn in_memory() -> Storage {
        Storage { root: None, dbs: BTreeMap::new() }
    }

    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    pub fn database_names(&self) -> Vec<String> {
        self.dbs.values().map(|d| d.name.clone()).collect()
    }

    pub fn database(&self, name: &str) -> Option<&Database> {
        self.dbs.get(&name.to_ascii_lowercase())
    }

    pub fn database_mut(&mut self, name: &str) -> Option<&mut Database> {
        self.dbs.get_mut(&name.to_ascii_lowercase())
    }

    /// Returns `true` if a database was created.
    pub fn create_database(&mut self, name: &str, if_not_exists: bool) -> Result<bool> {
        validate(name)?;
        let key = name.to_ascii_lowercase();
        if self.dbs.contains_key(&key) {
            return if if_not_exists { Ok(false) } else { Err(StorageError::AlreadyExists(format!("database `{name}`"))) };
        }
        let db = match &self.root {
            Some(root) => Database::create(name, &root.join(format!("{key}.{EXT}")))?,
            None => Database::in_memory(name),
        };
        self.dbs.insert(key, db);
        Ok(true)
    }

    /// Returns `true` if a database was dropped (its file is deleted).
    pub fn drop_database(&mut self, name: &str, if_exists: bool) -> Result<bool> {
        let key = name.to_ascii_lowercase();
        if !self.dbs.contains_key(&key) {
            return if if_exists { Ok(false) } else { Err(StorageError::NotFound(format!("database `{name}`"))) };
        }
        self.dbs.remove(&key);
        if let Some(root) = &self.root {
            std::fs::remove_file(root.join(format!("{key}.{EXT}")))?;
        }
        Ok(true)
    }
}

