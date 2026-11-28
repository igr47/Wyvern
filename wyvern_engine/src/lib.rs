//! # wyvern_engine
//!
//! Executes parsed DataQl statements against `wyvern_storage`.
//!
//! ```no_run
//! let mut engine = wyvern_engine::Engine::open("./data")?;
//! for out in engine.execute("CREATE TABLE t (a INT); INSERT INTO t VALUES (1),(2); SELECT sum(a)
//! FROM t")? {
//!     println!("{out}");
//! }
//!
//! # Ok::<(), wyvern_engine::EngineError>(())
//! ```
//!
//! Layout 
//! * `select`, `join`, `window`, `agg`  query execution (streaming, vectorised)
//! * `bind`, `eval`, `scalar`, `cast`   expressions 
//! * `ddl`, `dml`, `meta`, `io`         statements and csv import 
//! * `ctx`, `batch`, `result`           session state and data carriers

pub mod error;
