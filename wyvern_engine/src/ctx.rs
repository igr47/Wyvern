//! Session state and read-only access to storage for query execution.

use crate::bail;
use crate::error::Result;
use std::collections::HashMap;
use std::sync::Arc;
use wyvern_parse::ast::ObjectName;
use wyvern_storage::{Storage, Table};

pub struct Session {
    pub current_db: String,
    /// Temporary tables: private to this session, gone when it ends.
    pub temp: HashMap<String, Arc<Table>>,
}

pub struct Ctx<'a> {
    pub storage: &'a Storage,
    pub session: &'a Session,
}

impl<'a> Ctx<'a> {
    /// `name` → temp table, then current database; `db.name` → that database.
    pub fn resolve_table(&self, n: &ObjectName) -> Result<Arc<Table>> {
        match n.schema {
            None => {
                if let Some(t) = self.session.temp.get(&n.name.to_ascii_lowercase()) {
                    return Ok(t.clone());
                }
                let Some(db) = self.storage.database(&self.session.current_db) else {
                    bail!("current database `{}` no longer exists", self.session.current_db);
                };
                match db.table(n.name) {
                    Some(t) => Ok(t),
                    None => bail!("table `{}` not found in database `{}`", n.name, db.name),
                }
            }
            Some(dbn) => {
                let Some(db) = self.storage.database(dbn) else { bail!("database `{dbn}` not found") };
                match db.table(n.name) {
                    Some(t) => Ok(t),
                    None => bail!("table `{dbn}.{}` not found", n.name),
                }
            }
        }
    }
}

