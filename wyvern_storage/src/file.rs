//! Single-file container.
//!
//! ```text
//! [ header slot A: 512 B ][ header slot B: 512 B ][ chunk | chunk | catalog | chunk ... ]
//! ```
//! Data is append-only. A header slot points at the latest catalog; slots are
//! written alternately with a generation number and checksum, so a torn write
//! leaves the previous slot valid and the database reopens at its last commit.
//! Chunks that are no longer referenced are reclaimed by `Database::vacuum`.

use crate::column::hash_bytes;
use crate::error::{Result, StorageError};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const MAGIC: &[u8; 8] = b"WYVERNDB";
const VERSION: u32 = 1;
const SLOT: u64 = 512;
const DATA_START: u64 = 2 * SLOT;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkLoc {
    pub offset: u64,
    /// Payload length (the 8-byte checksum prefix is not included).
    pub len: u64,
}

impl ChunkLoc {
    pub fn on_disk_size(&self) -> u64 {
        self.len + 8
    }
}

struct Inner {
    file: File,
    end: u64,
    generation: u64,
}

pub struct DbFile {
    pub path: PathBuf,
    inner: Mutex<Inner>,
}

impl std::fmt::Debug for DbFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DbFile({})", self.path.display())
    }
}

struct Header {
    generation: u64,
    cat_offset: u64,
    cat_len: u64,
}

fn encode_header(h: &Header) -> [u8; 48] {
    let mut b = [0u8; 48];
    b[0..8].copy_from_slice(MAGIC);
    b[8..12].copy_from_slice(&VERSION.to_le_bytes());
    b[16..24].copy_from_slice(&h.generation.to_le_bytes());
    b[24..32].copy_from_slice(&h.cat_offset.to_le_bytes());
    b[32..40].copy_from_slice(&h.cat_len.to_le_bytes());
    let sum = hash_bytes(&b[..40]);
    b[40..48].copy_from_slice(&sum.to_le_bytes());
    b
}

fn decode_header(b: &[u8]) -> Option<Header> {
    if b.len() < 48 || &b[0..8] != MAGIC {
        return None;
    }
    let sum = u64::from_le_bytes(b[40..48].try_into().unwrap());
    if hash_bytes(&b[..40]) != sum {
        return None;
    }
    let u64_at = |i: usize| u64::from_le_bytes(b[i..i + 8].try_into().unwrap());
    Some(Header { generation: u64_at(16), cat_offset: u64_at(24), cat_len: u64_at(32) })
}

impl DbFile {
    /// Creates an empty database file (fails if it exists).
    pub fn create(path: &Path) -> Result<DbFile> {
        let mut file = OpenOptions::new().read(true).write(true).create_new(true).open(path)?;
        file.write_all(&vec![0u8; DATA_START as usize])?;
        file.sync_all()?;
        Ok(DbFile {
            path: path.to_path_buf(),
            inner: Mutex::new(Inner { file, end: DATA_START, generation: 0 }),
        })
    }

    /// Opens an existing file; returns the newest committed catalog bytes (if any).
    pub fn open(path: &Path) -> Result<(DbFile, Option<Vec<u8>>)> {
        let mut file = OpenOptions::new().read(true).write(true).open(path)?;
        let end = file.metadata()?.len();
        if end < DATA_START {
            return Err(StorageError::Corrupt(format!("{} is too small", path.display())));
        }
        let mut head = vec![0u8; DATA_START as usize];
        file.read_exact(&mut head)?;
        let a = decode_header(&head[..SLOT as usize]);
        let b = decode_header(&head[SLOT as usize..]);
        let best = match (a, b) {
            (Some(a), Some(b)) => Some(if a.generation >= b.generation { a } else { b }),
            (a, b) => a.or(b),
        };
        if best.is_none() && head.iter().any(|x| *x != 0) {
            return Err(StorageError::Corrupt(format!("{} is not a Wyvern database", path.display())));
        }
        let (generation, catalog) = match best {
            Some(h) => {
                let loc = ChunkLoc { offset: h.cat_offset, len: h.cat_len };
                let db = DbFile {
                    path: path.to_path_buf(),
                    inner: Mutex::new(Inner { file, end, generation: h.generation }),
                };
                let bytes = db.read_chunk(loc)?;
                return Ok((db, Some(bytes)));
            }
            None => (0, None),
        };
        Ok((
            DbFile { path: path.to_path_buf(), inner: Mutex::new(Inner { file, end, generation }) },
            catalog,
        ))
    }

    pub fn len(&self) -> u64 {
        self.inner.lock().unwrap().end
    }

    /// Appends `[checksum][payload]` and returns where it landed.
    pub fn write_chunk(&self, payload: &[u8]) -> Result<ChunkLoc> {
        let mut g = self.inner.lock().unwrap();
        let offset = g.end;
        let mut buf = Vec::with_capacity(payload.len() + 8);
        buf.extend_from_slice(&hash_bytes(payload).to_le_bytes());
        buf.extend_from_slice(payload);
        g.file.seek(SeekFrom::Start(offset))?;
        g.file.write_all(&buf)?;
        g.end += buf.len() as u64;
        Ok(ChunkLoc { offset, len: payload.len() as u64 })
    }

    pub fn read_chunk(&self, loc: ChunkLoc) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; loc.len as usize + 8];
        {
            let mut g = self.inner.lock().unwrap();
            g.file.seek(SeekFrom::Start(loc.offset))?;
            g.file.read_exact(&mut buf)?;
        }
        let sum = u64::from_le_bytes(buf[..8].try_into().unwrap());
        if hash_bytes(&buf[8..]) != sum {
            return Err(StorageError::Corrupt(format!("checksum mismatch at offset {}", loc.offset)));
        }
        buf.drain(..8);
        Ok(buf)
    }

    /// Durably publishes `catalog` as the new database state.
    pub fn commit_catalog(&self, catalog: &[u8]) -> Result<()> {
        let loc = self.write_chunk(catalog)?;
        let mut g = self.inner.lock().unwrap();
        g.file.sync_data()?; // data + catalog durable before the pointer moves
        g.generation += 1;
        let slot = (g.generation % 2) * SLOT;
        let hdr = encode_header(&Header { generation: g.generation, cat_offset: loc.offset, cat_len: loc.len });
        g.file.seek(SeekFrom::Start(slot))?;
        g.file.write_all(&hdr)?;
        g.file.sync_data()?;
        Ok(())
    }
}

