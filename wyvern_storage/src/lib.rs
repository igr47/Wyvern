//! # wyvern_storage
//!
//! Columnar, partitioned storage for the Wyvern analytical database.
//!
//! * [`Storage`]   a folder of databases (`<name>.wyv`), or a purely in-memory instance
//! * [`Database`]  one file holding many tables; also owns in-memory tables
//! * [`Table`]     immutable snapshot of a table: schema + partitions (row groups)
//! * [`Column`]    typed vector + null mask, the unit of data exchange with the engine
//!
//! Tables are copy-on-write: every mutation returns a new `Table` that shares
//! unchanged partitions, so readers never block writers and a statement is
//! atomic (the catalog pointer flips only at [`Database::commit`]).

mod codec;
pub mod column;
pub mod database;
pub mod error;
mod file;
pub mod storage;
pub mod table;
pub mod temporal;
pub mod types;

pub use column::{Column, ColumnData, ColumnStats};
pub use database::Database;
pub use error::{Result, StorageError};
pub use storage::Storage;
pub use table::{CmpOp, PartitionHandle, Rewrite, Scan, ScanBatch, ScanPred, Table, TableKind, PARTITION_ROWS};
pub use types::{DataType, Field, Key, Schema, Value};

