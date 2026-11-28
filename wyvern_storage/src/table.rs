//! Immutable, partitioned table snapshots.
//!
//! A table is a list of *partitions* (row groups of up to [`PARTITION_ROWS`]
//! rows). Each partition stores one chunk per column, so a scan reads only the
//! columns it needs, and per-column min/max/null statistics ("zone maps") let
//! it skip whole partitions that cannot satisfy a predicate.

use crate::codec::{decode_column, encode_column};
use crate::column::{Column, ColumnStats};
use crate::error::{Result, StorageError};
use crate::file::{ChunkLoc, DbFile};
use crate::types::{Schema, Value};
use std::cmp::Ordering;
use std::sync::Arc;

pub const PARTITION_ROWS: usize = 65_536;
/// A trailing partition smaller than this is merged with newly appended rows.
const MERGE_BELOW: usize = 8_192;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableKind {
    /// Stored in the database file.
    Persistent,
    /// Lives in memory, visible to every session while the process runs.
    InMemory,
    /// Lives in memory, private to one session.
    Temporary,
}

#[derive(Debug, Clone)]
pub(crate) enum PartStore {
    Mem(Vec<Arc<Column>>),
    Disk(Vec<ChunkLoc>),
}

#[derive(Debug, Clone)]
pub struct PartitionHandle {
    pub rows: usize,
    pub stats: Vec<ColumnStats>,
    pub(crate) store: PartStore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    Lt,
    Le,
    Gt,
    Ge,
}

/// `column <op> constant`, used purely to skip partitions.
#[derive(Debug, Clone)]
pub struct ScanPred {
    pub col: usize,
    pub op: CmpOp,
    pub value: Value,
}

impl PartitionHandle {
    pub fn may_match(&self, p: &ScanPred) -> bool {
        let Some(st) = self.stats.get(p.col) else { return true };
        if st.null_count as usize == self.rows {
            return false; // all NULL: no comparison can be true
        }
        let (Some(min), Some(max)) = (&st.min, &st.max) else { return true };
        let (Some(lo), Some(hi)) = (min.sql_cmp(&p.value), max.sql_cmp(&p.value)) else { return true };
        match p.op {
            CmpOp::Eq => lo != Ordering::Greater && hi != Ordering::Less,
            CmpOp::Lt => lo == Ordering::Less,
            CmpOp::Le => lo != Ordering::Greater,
            CmpOp::Gt => hi == Ordering::Greater,
            CmpOp::Ge => hi != Ordering::Less,
        }
    }

    pub(crate) fn disk_bytes(&self) -> u64 {
        match &self.store {
            PartStore::Disk(l) => l.iter().map(|c| c.on_disk_size()).sum(),
            PartStore::Mem(_) => 0,
        }
    }
}

pub enum Rewrite {
    Keep,
    Drop,
    /// New contents for the partition: one column per schema field.
    Replace(Vec<Column>),
}

#[derive(Debug, Clone)]
pub struct Table {
    pub name: String,
    pub kind: TableKind,
    pub schema: Schema,
    pub partitions: Vec<PartitionHandle>,
    /// Next value handed out by AUTO_INCREMENT columns.
    pub next_auto: i64,
    pub(crate) file: Option<Arc<DbFile>>,
}

pub struct ScanBatch {
    pub rows: usize,
    /// Columns in the order requested by the projection.
    pub columns: Vec<Arc<Column>>,
}

pub struct Scan {
    table: Arc<Table>,
    projection: Vec<usize>,
    preds: Vec<ScanPred>,
    next: usize,
}

impl Iterator for Scan {
    type Item = Result<ScanBatch>;
    fn next(&mut self) -> Option<Self::Item> {
        while self.next < self.table.partitions.len() {
            let h = &self.table.partitions[self.next];
            self.next += 1;
            if h.rows == 0 || !self.preds.iter().all(|p| h.may_match(p)) {
                continue;
            }
            return Some(
                self.table
                    .read_columns(h, &self.projection)
                    .map(|columns| ScanBatch { rows: h.rows, columns }),
            );
        }
        None
    }
}

impl Table {
    pub(crate) fn new(name: String, kind: TableKind, schema: Schema, file: Option<Arc<DbFile>>) -> Table {
        Table { name, kind, schema, partitions: Vec::new(), next_auto: 1, file }
    }

    pub fn row_count(&self) -> usize {
        self.partitions.iter().map(|p| p.rows).sum()
    }

    /// Streams `projection` columns partition by partition, skipping partitions
    /// whose statistics rule out `preds`.
    pub fn scan(self: &Arc<Table>, projection: Vec<usize>, preds: Vec<ScanPred>) -> Scan {
        Scan { table: self.clone(), projection, preds, next: 0 }
    }

    pub fn read_columns(&self, h: &PartitionHandle, cols: &[usize]) -> Result<Vec<Arc<Column>>> {
        match &h.store {
            PartStore::Mem(v) => Ok(cols.iter().map(|&c| v[c].clone()).collect()),
            PartStore::Disk(locs) => {
                let file = self.file.as_ref().ok_or_else(|| StorageError::Corrupt("table has no backing file".into()))?;
                cols.iter()
                    .map(|&c| Ok(Arc::new(decode_column(&file.read_chunk(locs[c])?)?)))
                    .collect()
            }
        }
    }

    fn read_all(&self, h: &PartitionHandle) -> Result<Vec<Arc<Column>>> {
        let all: Vec<usize> = (0..self.schema.len()).collect();
        self.read_columns(h, &all)
    }

    fn make_handle(&self, cols: Vec<Column>) -> Result<PartitionHandle> {
        let rows = cols.first().map_or(0, |c| c.len());
        if cols.len() != self.schema.len() || cols.iter().any(|c| c.len() != rows) {
            return Err(StorageError::Invalid("partition columns do not match the table schema".into()));
        }
        let stats = cols.iter().map(|c| c.stats()).collect();
        let store = match (&self.file, self.kind) {
            (Some(file), TableKind::Persistent) => {
                let mut locs = Vec::with_capacity(cols.len());
                for c in &cols {
                    locs.push(file.write_chunk(&encode_column(c))?);
                }
                PartStore::Disk(locs)
            }
            _ => PartStore::Mem(cols.into_iter().map(Arc::new).collect()),
        };
        Ok(PartitionHandle { rows, stats, store })
    }

    /// Returns a new table with `cols` (one per schema field) appended.
    pub fn append(&self, mut cols: Vec<Column>) -> Result<Table> {
        let n = cols.first().map_or(0, |c| c.len());
        if n == 0 {
            return Ok(self.clone());
        }
        let mut t = self.clone();
        // Fold a small trailing partition into the new rows (keeps partitions dense).
        if let Some(last) = t.partitions.last() {
            if last.rows < MERGE_BELOW {
                let old = self.read_all(last)?;
                cols = old
                    .iter()
                    .zip(cols)
                    .map(|(o, c)| Column::concat(c.dtype(), &[o.as_ref(), &c]))
                    .collect();
                t.partitions.pop();
            }
        }
        let total = cols[0].len();
        let mut start = 0;
        while start < total {
            let len = PARTITION_ROWS.min(total - start);
            let part: Vec<Column> = if start == 0 && len == total {
                std::mem::take(&mut cols)
            } else {
                cols.iter().map(|c| c.slice(start, len)).collect()
            };
            t.partitions.push(t.make_handle(part)?);
            start += len;
        }
        Ok(t)
    }

    /// Rebuilds the table partition by partition (UPDATE / DELETE).
    pub fn rewrite(&self, mut f: impl FnMut(&Table, &PartitionHandle) -> Result<Rewrite>) -> Result<Table> {
        let mut t = self.clone();
        t.partitions.clear();
        for h in &self.partitions {
            match f(self, h)? {
                Rewrite::Keep => t.partitions.push(h.clone()),
                Rewrite::Drop => {}
                Rewrite::Replace(cols) => {
                    if cols.first().map_or(0, |c| c.len()) > 0 {
                        t.partitions.push(t.make_handle(cols)?);
                    }
                }
            }
        }
        Ok(t)
    }

    /// Rebuilds every partition under a new schema (ALTER TABLE ADD / MODIFY).
    pub fn transform(
        &self,
        schema: Schema,
        mut f: impl FnMut(Vec<Arc<Column>>, usize) -> Result<Vec<Column>>,
    ) -> Result<Table> {
        let mut t = self.clone();
        t.schema = schema;
        t.partitions.clear();
        for h in &self.partitions {
            let cols = f(self.read_all(h)?, h.rows)?;
            t.partitions.push(t.make_handle(cols)?);
        }
        Ok(t)
    }

    /// Cheap (metadata only): the column's chunks are simply forgotten.
    pub fn drop_column(&self, idx: usize) -> Table {
        let mut t = self.clone();
        t.schema.fields.remove(idx);
        for p in &mut t.partitions {
            p.stats.remove(idx);
            match &mut p.store {
                PartStore::Mem(v) => {
                    v.remove(idx);
                }
                PartStore::Disk(l) => {
                    l.remove(idx);
                }
            }
        }
        t
    }

    pub fn renamed(&self, name: String) -> Table {
        let mut t = self.clone();
        t.name = name;
        t
    }

    pub fn with_schema(&self, schema: Schema) -> Table {
        let mut t = self.clone();
        t.schema = schema;
        t
    }

    pub fn with_next_auto(&self, next: i64) -> Table {
        let mut t = self.clone();
        t.next_auto = next;
        t
    }

    /// Rewrites the table into dense, full partitions (used by VACUUM).
    pub fn compacted(&self, new_file: Option<Arc<DbFile>>) -> Result<Table> {
        let mut t = self.clone();
        t.file = new_file;
        t.partitions.clear();
        let n = self.schema.len();
        let mut buf: Vec<Column> = self.schema.fields.iter().map(|f| Column::new(f.dtype)).collect();
        let mut buffered = 0usize;
        for h in &self.partitions {
            let cols = self.read_all(h)?;
            for (b, c) in buf.iter_mut().zip(&cols) {
                b.append(c);
            }
            buffered += h.rows;
            if buffered >= PARTITION_ROWS {
                let full = std::mem::replace(
                    &mut buf,
                    self.schema.fields.iter().map(|f| Column::new(f.dtype)).collect(),
                );
                t.partitions.push(t.make_handle(full)?);
                buffered = 0;
            }
        }
        if buffered > 0 && n > 0 {
            t.partitions.push(t.make_handle(buf)?);
        }
        Ok(t)
    }
}

