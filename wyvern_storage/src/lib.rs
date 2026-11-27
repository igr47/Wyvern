//! # wyvern_storage 
//!
//! Columnar, partitioned storage for the wyvern analytical database.
//!
//! * [`Storage`]  a folder of databases (`<name>.wyv`), or a purely in-memory instance 
//! * [`Database`] one file holding many tables; also owns in-memory tables 
//! * ['Table']    immutable snapshots of a table: schema + partitions (row groups)
//! * [`Column`]   typed vector + null mask, the unit of data exchange with the engine 
//!
//! Tables are copy-on-write: every mutation returns a new `Table` shares 
//! unchanged partitions, so readers never block writers and a statement is 
//! atomic (the catalog pointer flips only at [`Database::commit`]).

mod codec;
pub mod column;
pub mod error;
pub mod tempral;
