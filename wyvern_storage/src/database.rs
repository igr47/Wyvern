//! One database = one file. Holds persistent tables (in the file) and
//! in-memory tables (process lifetime). Temporary tables are owned by sessions.

use crate::codec::*;
use crate::column::ColumnStats;
use crate::error::{Result, StorageError};
use crate::file::{ChunkLoc, DbFile};
use crate::table::{PartStore, PartitionHandle, Table, TableKind};
use crate::types::Schema;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Compact automatically when the file is mostly dead space.
const AUTO_VACUUM_MIN_BYTES: u64 = 8 << 20;

#[derive(Debug)]
pub struct Database {
    pub name: String,
    path: Option<PathBuf>,
    file: Option<Arc<DbFile>>,
    tables: BTreeMap<String, Arc<Table>>,
    dirty: bool,
}

impl Database {
    pub fn in_memory(name: &str) -> Database {
        Database { name: name.to_string(), path: None, file: None, tables: BTreeMap::new(), dirty: false }
    }

    pub fn create(name: &str, path: &Path) -> Result<Database> {
        let file = Arc::new(DbFile::create(path)?);
        let mut db = Database {
            name: name.to_string(),
            path: Some(path.to_path_buf()),
            file: Some(file),
            tables: BTreeMap::new(),
            dirty: true,
        };
        db.commit()?;
        Ok(db)
    }

    pub fn open(name: &str, path: &Path) -> Result<Database> {
        let (file, catalog) = DbFile::open(path)?;
        let file = Arc::new(file);
        let tables = match catalog {
            Some(bytes) => decode_catalog(&bytes, &file)?,
            None => BTreeMap::new(),
        };
        Ok(Database { name: name.to_string(), path: Some(path.to_path_buf()), file: Some(file), tables, dirty: false })
    }

    pub fn is_persistent(&self) -> bool {
        self.file.is_some()
    }

    pub fn file_size(&self) -> Option<u64> {
        self.file.as_ref().map(|f| f.len())
    }

    pub fn table(&self, name: &str) -> Option<Arc<Table>> {
        self.tables.get(&name.to_ascii_lowercase()).cloned()
    }

    pub fn table_names(&self) -> Vec<String> {
        self.tables.values().map(|t| t.name.clone()).collect()
    }

    /// An empty table bound to this database (not registered until `put_table`).
    pub fn new_table(&self, name: &str, kind: TableKind, schema: Schema) -> Table {
        let file = if kind == TableKind::Persistent { self.file.clone() } else { None };
        Table::new(name.to_string(), kind, schema, file)
    }

    pub fn put_table(&mut self, t: Table) {
        let key = t.name.to_ascii_lowercase();
        let was_persistent = self.tables.get(&key).is_some_and(|o| o.kind == TableKind::Persistent);
        if t.kind == TableKind::Persistent || was_persistent {
            self.dirty = true;
        }
        self.tables.insert(key, Arc::new(t));
    }

    pub fn remove_table(&mut self, name: &str) -> Option<Arc<Table>> {
        let t = self.tables.remove(&name.to_ascii_lowercase());
        if t.as_ref().is_some_and(|t| t.kind == TableKind::Persistent) {
            self.dirty = true;
        }
        t
    }

    /// Atomically publishes all staged changes. Call once per statement.
    pub fn commit(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }
        if let Some(file) = &self.file {
            file.commit_catalog(&encode_catalog(&self.tables))?;
        }
        self.dirty = false;
        let live: u64 = self
            .tables
            .values()
            .filter(|t| t.kind == TableKind::Persistent)
            .flat_map(|t| t.partitions.iter())
            .map(|p| p.disk_bytes())
            .sum();
        if let Some(len) = self.file_size() {
            if len > AUTO_VACUUM_MIN_BYTES && len > live * 2 {
                self.vacuum()?;
            }
        }
        Ok(())
    }

    /// Rewrites the file keeping only live data, merging small partitions.
    pub fn vacuum(&mut self) -> Result<()> {
        let (Some(path), Some(_)) = (self.path.clone(), self.file.clone()) else { return Ok(()) };
        let tmp = path.with_extension("wyv.tmp");
        let _ = std::fs::remove_file(&tmp);
        let new_file = Arc::new(DbFile::create(&tmp)?);
        let mut compacted = BTreeMap::new();
        for (k, t) in &self.tables {
            if t.kind == TableKind::Persistent {
                compacted.insert(k.clone(), Arc::new(t.compacted(Some(new_file.clone()))?));
            }
        }
        new_file.commit_catalog(&encode_catalog(&compacted))?;
        drop(compacted);
        drop(new_file);
        std::fs::rename(&tmp, &path)?;
        let (file, catalog) = DbFile::open(&path)?;
        let file = Arc::new(file);
        let mut tables = match catalog {
            Some(b) => decode_catalog(&b, &file)?,
            None => BTreeMap::new(),
        };
        for (k, t) in &self.tables {
            if t.kind != TableKind::Persistent {
                tables.insert(k.clone(), t.clone());
            }
        }
        self.tables = tables;
        self.file = Some(file);
        Ok(())
    }
}

// ------------------------------------------------------------------- catalog
fn encode_catalog(tables: &BTreeMap<String, Arc<Table>>) -> Vec<u8> {
    let mut w = Writer::new();
    let persistent: Vec<_> = tables.values().filter(|t| t.kind == TableKind::Persistent).collect();
    w.u32(persistent.len() as u32);
    for t in persistent {
        w.str(&t.name);
        write_schema(&mut w, &t.schema);
        w.i64(t.next_auto);
        w.u32(t.partitions.len() as u32);
        for p in &t.partitions {
            w.u32(p.rows as u32);
            if let PartStore::Disk(locs) = &p.store {
                for (loc, st) in locs.iter().zip(&p.stats) {
                    w.u64(loc.offset);
                    w.u64(loc.len);
                    write_stats(&mut w, st);
                }
            }
        }
    }
    w.0
}

fn decode_catalog(bytes: &[u8], file: &Arc<DbFile>) -> Result<BTreeMap<String, Arc<Table>>> {
    let mut r = Reader::new(bytes);
    let mut out = BTreeMap::new();
    for _ in 0..r.u32()? {
        let name = r.str()?;
        let schema = read_schema(&mut r)?;
        let next_auto = r.i64()?;
        let ncols = schema.len();
        let nparts = r.u32()? as usize;
        let mut partitions = Vec::with_capacity(nparts);
        for _ in 0..nparts {
            let rows = r.u32()? as usize;
            let mut locs = Vec::with_capacity(ncols);
            let mut stats: Vec<ColumnStats> = Vec::with_capacity(ncols);
            for _ in 0..ncols {
                let offset = r.u64()?;
                let len = r.u64()?;
                locs.push(ChunkLoc { offset, len });
                stats.push(read_stats(&mut r)?);
            }
            partitions.push(PartitionHandle { rows, stats, store: PartStore::Disk(locs) });
        }
        let mut t = Table::new(name.clone(), TableKind::Persistent, schema, Some(file.clone()));
        t.partitions = partitions;
        t.next_auto = next_auto;
        out.insert(name.to_ascii_lowercase(), Arc::new(t));
    }
    Ok(out)
}

#[allow(dead_code)]
fn _assert_error_used(_: StorageError) {}

