//! Compact little-endian binary encoding for columns, values, schemas and stats.

use crate::column::{Column, ColumnData, ColumnStats};
use crate::error::{Result, StorageError};
use crate::types::{DataType, Field, Schema, Value};

pub struct Writer(pub Vec<u8>);

impl Writer {
    pub fn new() -> Self {
        Writer(Vec::new())
    }
    pub fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    pub fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub fn i64(&mut self, v: i64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    pub fn bytes(&mut self, b: &[u8]) {
        self.u32(b.len() as u32);
        self.0.extend_from_slice(b);
    }
    pub fn str(&mut self, s: &str) {
        self.bytes(s.as_bytes());
    }
}

pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

fn eof() -> StorageError {
    StorageError::Corrupt("unexpected end of data".into())
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Reader { buf, pos: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or_else(eof)?;
        let s = self.buf.get(self.pos..end).ok_or_else(eof)?;
        self.pos = end;
        Ok(s)
    }
    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    pub fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    pub fn bytes(&mut self) -> Result<&'a [u8]> {
        let n = self.u32()? as usize;
        self.take(n)
    }
    pub fn str(&mut self) -> Result<String> {
        String::from_utf8(self.bytes()?.to_vec()).map_err(|_| StorageError::Corrupt("invalid utf-8".into()))
    }
}

// ------------------------------------------------------------------ data types
fn dtype_tag(t: DataType) -> u8 {
    match t {
        DataType::Bool => 0,
        DataType::Int => 1,
        DataType::Float => 2,
        DataType::Decimal { .. } => 3,
        DataType::Text => 4,
        DataType::Date => 5,
        DataType::DateTime => 6,
        DataType::Uuid => 7,
    }
}

pub fn write_dtype(w: &mut Writer, t: DataType) {
    w.u8(dtype_tag(t));
    if let DataType::Decimal { precision, scale } = t {
        w.u8(precision.map_or(255, |p| p));
        w.u8(scale.map_or(255, |s| s));
    }
}

pub fn read_dtype(r: &mut Reader) -> Result<DataType> {
    Ok(match r.u8()? {
        0 => DataType::Bool,
        1 => DataType::Int,
        2 => DataType::Float,
        3 => {
            let p = r.u8()?;
            let s = r.u8()?;
            DataType::Decimal {
                precision: (p != 255).then_some(p),
                scale: (s != 255).then_some(s),
            }
        }
        4 => DataType::Text,
        5 => DataType::Date,
        6 => DataType::DateTime,
        7 => DataType::Uuid,
        t => return Err(StorageError::Corrupt(format!("unknown type tag {t}"))),
    })
}

// ---------------------------------------------------------------------- values
pub fn write_value(w: &mut Writer, v: &Value) {
    match v {
        Value::Null => w.u8(0),
        Value::Bool(b) => {
            w.u8(1);
            w.u8(*b as u8)
        }
        Value::Int(i) => {
            w.u8(2);
            w.i64(*i)
        }
        Value::Float(f) => {
            w.u8(3);
            w.u64(f.to_bits())
        }
        Value::Text(s) => {
            w.u8(4);
            w.str(s)
        }
        Value::Date(d) => {
            w.u8(5);
            w.i64(*d as i64)
        }
        Value::DateTime(d) => {
            w.u8(6);
            w.i64(*d)
        }
        Value::Uuid(u) => {
            w.u8(7);
            w.u64(*u as u64);
            w.u64((*u >> 64) as u64)
        }
    }
}

pub fn read_value(r: &mut Reader) -> Result<Value> {
    Ok(match r.u8()? {
        0 => Value::Null,
        1 => Value::Bool(r.u8()? != 0),
        2 => Value::Int(r.i64()?),
        3 => Value::Float(f64::from_bits(r.u64()?)),
        4 => Value::Text(r.str()?),
        5 => Value::Date(r.i64()? as i32),
        6 => Value::DateTime(r.i64()?),
        7 => {
            let lo = r.u64()? as u128;
            let hi = r.u64()? as u128;
            Value::Uuid(lo | (hi << 64))
        }
        t => return Err(StorageError::Corrupt(format!("unknown value tag {t}"))),
    })
}

pub fn write_stats(w: &mut Writer, s: &ColumnStats) {
    w.u32(s.null_count);
    write_value(w, s.min.as_ref().unwrap_or(&Value::Null));
    write_value(w, s.max.as_ref().unwrap_or(&Value::Null));
}

pub fn read_stats(r: &mut Reader) -> Result<ColumnStats> {
    let null_count = r.u32()?;
    let min = read_value(r)?;
    let max = read_value(r)?;
    Ok(ColumnStats {
        null_count,
        min: (!min.is_null()).then_some(min),
        max: (!max.is_null()).then_some(max),
    })
}

// ---------------------------------------------------------------------- schema
pub fn write_schema(w: &mut Writer, s: &Schema) {
    w.u32(s.fields.len() as u32);
    for f in &s.fields {
        w.str(&f.name);
        write_dtype(w, f.dtype);
        let flags = f.nullable as u8
            | (f.primary_key as u8) << 1
            | (f.unique as u8) << 2
            | (f.auto_increment as u8) << 3;
        w.u8(flags);
        write_value(w, f.default.as_ref().unwrap_or(&Value::Null));
    }
}

pub fn read_schema(r: &mut Reader) -> Result<Schema> {
    let n = r.u32()? as usize;
    let mut fields = Vec::with_capacity(n);
    for _ in 0..n {
        let name = r.str()?;
        let dtype = read_dtype(r)?;
        let flags = r.u8()?;
        let default = read_value(r)?;
        fields.push(Field {
            name,
            dtype,
            nullable: flags & 1 != 0,
            primary_key: flags & 2 != 0,
            unique: flags & 4 != 0,
            auto_increment: flags & 8 != 0,
            default: (!default.is_null()).then_some(default),
        });
    }
    Ok(Schema { fields })
}

// --------------------------------------------------------------------- columns
/// `[type][rows u32][has_nulls u8][null mask bytes?][data]`
pub fn encode_column(c: &Column) -> Vec<u8> {
    let n = c.len();
    let mut w = Writer(Vec::with_capacity(16 + n * 8));
    w.u8(dtype_tag(c.dtype()));
    w.u32(n as u32);
    match &c.nulls {
        Some(m) => {
            w.u8(1);
            w.0.extend(m.iter().map(|b| *b as u8));
        }
        None => w.u8(0),
    }
    match &c.data {
        ColumnData::Bool(v) => w.0.extend(v.iter().map(|b| *b as u8)),
        ColumnData::Int(v) | ColumnData::DateTime(v) => {
            for x in v {
                w.0.extend_from_slice(&x.to_le_bytes());
            }
        }
        ColumnData::Float(v) => {
            for x in v {
                w.0.extend_from_slice(&x.to_le_bytes());
            }
        }
        ColumnData::Date(v) => {
            for x in v {
                w.0.extend_from_slice(&x.to_le_bytes());
            }
        }
        ColumnData::Uuid(v) => {
            for x in v {
                w.0.extend_from_slice(&x.to_le_bytes());
            }
        }
        ColumnData::Text(v) => {
            for s in v {
                w.u32(s.len() as u32);
            }
            for s in v {
                w.0.extend_from_slice(s.as_bytes());
            }
        }
    }
    w.0
}

pub fn decode_column(buf: &[u8]) -> Result<Column> {
    let mut r = Reader::new(buf);
    let tag = r.u8()?;
    let n = r.u32()? as usize;
    let nulls = if r.u8()? == 1 {
        Some(r.take(n)?.iter().map(|b| *b != 0).collect::<Vec<_>>())
    } else {
        None
    };
    macro_rules! fixed {
        ($ty:ty, $w:expr, $variant:ident) => {{
            let raw = r.take(n * $w)?;
            ColumnData::$variant(raw.chunks_exact($w).map(|c| <$ty>::from_le_bytes(c.try_into().unwrap())).collect())
        }};
    }
    let data = match tag {
        0 => ColumnData::Bool(r.take(n)?.iter().map(|b| *b != 0).collect()),
        1 => fixed!(i64, 8, Int),
        2 => fixed!(f64, 8, Float),
        5 => fixed!(i32, 4, Date),
        6 => fixed!(i64, 8, DateTime),
        7 => fixed!(u128, 16, Uuid),
        4 => {
            let lens: Vec<usize> = r.take(n * 4)?.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap()) as usize).collect();
            let mut out = Vec::with_capacity(n);
            for l in lens {
                let b = r.take(l)?;
                out.push(String::from_utf8(b.to_vec()).map_err(|_| StorageError::Corrupt("invalid utf-8".into()))?);
            }
            ColumnData::Text(out)
        }
        t => return Err(StorageError::Corrupt(format!("unknown column tag {t}"))),
    };
    Ok(Column { data, nulls })
}

