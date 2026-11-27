 //! Columnar vectors: a typed `Vec` plus an optional null mask (`true` = NULL).

use crate::temporal::MICROS_PER_DAY;
use crate::types::{DataType, Value};
use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq)]
pub enum ColumnData {
    Bool(Vec<bool>),
    Int(Vec<i64>),
    Float(Vec<f64>),
    Text(Vec<String>),
    Date(Vec<i32>),
    DateTime(Vec<i64>),
    Uuid(Vec<u128>),
}

/// Run `$b` on the inner vector, whatever its type.
macro_rules! each {
    ($d:expr, $v:ident => $b:expr) => {
        match $d {
            ColumnData::Bool($v) => $b,
            ColumnData::Int($v) => $b,
            ColumnData::Float($v) => $b,
            ColumnData::Text($v) => $b,
            ColumnData::Date($v) => $b,
            ColumnData::DateTime($v) => $b,
            ColumnData::Uuid($v) => $b,
        }
    };
}

/// Like `each!` but re-wraps the result in the same variant.
macro_rules! map_data {
    ($d:expr, $v:ident => $b:expr) => {
        match $d {
            ColumnData::Bool($v) => ColumnData::Bool($b),
            ColumnData::Int($v) => ColumnData::Int($b),
            ColumnData::Float($v) => ColumnData::Float($b),
            ColumnData::Text($v) => ColumnData::Text($b),
            ColumnData::Date($v) => ColumnData::Date($b),
            ColumnData::DateTime($v) => ColumnData::DateTime($b),
            ColumnData::Uuid($v) => ColumnData::Uuid($b),
        }
    };
}

impl ColumnData {
    pub fn empty(dtype: DataType, cap: usize) -> ColumnData {
        match dtype.canonical() {
            DataType::Bool => ColumnData::Bool(Vec::with_capacity(cap)),
            DataType::Int => ColumnData::Int(Vec::with_capacity(cap)),
            DataType::Float | DataType::Decimal { .. } => ColumnData::Float(Vec::with_capacity(cap)),
            DataType::Text => ColumnData::Text(Vec::with_capacity(cap)),
            DataType::Date => ColumnData::Date(Vec::with_capacity(cap)),
            DataType::DateTime => ColumnData::DateTime(Vec::with_capacity(cap)),
            DataType::Uuid => ColumnData::Uuid(Vec::with_capacity(cap)),
        }
    }
    pub fn len(&self) -> usize {
        each!(self, v => v.len())
    }
    pub fn dtype(&self) -> DataType {
        match self {
            ColumnData::Bool(_) => DataType::Bool,
            ColumnData::Int(_) => DataType::Int,
            ColumnData::Float(_) => DataType::Float,
            ColumnData::Text(_) => DataType::Text,
            ColumnData::Date(_) => DataType::Date,
            ColumnData::DateTime(_) => DataType::DateTime,
            ColumnData::Uuid(_) => DataType::Uuid,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct ColumnStats {
    pub null_count: u32,
    pub min: Option<Value>,
    pub max: Option<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    pub data: ColumnData,
    /// `nulls[i] == true` means row `i` is NULL. `None` = no NULLs at all.
    pub nulls: Option<Vec<bool>>,
}

#[inline]
fn mix(h: u64, x: u64) -> u64 {
    (h.rotate_left(5) ^ x).wrapping_mul(0x517c_c1b7_2722_0a95)
}

pub fn hash_bytes(bytes: &[u8]) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    let mut chunks = bytes.chunks_exact(8);
    for c in &mut chunks {
        h = mix(h, u64::from_le_bytes(c.try_into().unwrap()));
    }
    for b in chunks.remainder() {
        h = mix(h, *b as u64);
    }
    mix(h, bytes.len() as u64)
}

#[inline]
fn float_bits(x: f64) -> u64 {
    if x == 0.0 { 0 } else if x.is_nan() { f64::NAN.to_bits() } else { x.to_bits() }
}

impl Column {
    pub fn new(dtype: DataType) -> Column {
        Column::with_capacity(dtype, 0)
    }
    pub fn with_capacity(dtype: DataType, cap: usize) -> Column {
        Column { data: ColumnData::empty(dtype, cap), nulls: None }
    }
    pub fn from_data(data: ColumnData) -> Column {
        Column { data, nulls: None }
    }
    /// Builds a column, dropping the mask when it contains no NULLs.
    pub fn from_parts(data: ColumnData, nulls: Option<Vec<bool>>) -> Column {
        let nulls = nulls.filter(|n| n.iter().any(|x| *x));
        Column { data, nulls }
    }
    pub fn from_ints(v: Vec<i64>) -> Column {
        Column::from_data(ColumnData::Int(v))
    }
    pub fn from_floats(v: Vec<f64>) -> Column {
        Column::from_data(ColumnData::Float(v))
    }
    pub fn from_bools(v: Vec<bool>) -> Column {
        Column::from_data(ColumnData::Bool(v))
    }
    pub fn from_strings(v: Vec<String>) -> Column {
        Column::from_data(ColumnData::Text(v))
    }

    pub fn from_values(dtype: DataType, values: Vec<Value>) -> Result<Column, String> {
        let mut c = Column::with_capacity(dtype, values.len());
        for v in values {
            c.push(v)?;
        }
        Ok(c)
    }

    pub fn constant(dtype: DataType, v: &Value, n: usize) -> Result<Column, String> {
        let mut c = Column::with_capacity(dtype, n);
        for _ in 0..n {
            c.push(v.clone())?;
        }
        Ok(c)
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn dtype(&self) -> DataType {
        self.data.dtype()
    }
    #[inline]
    pub fn is_null(&self, i: usize) -> bool {
        match &self.nulls {
            Some(n) => n[i],
            None => false,
        }
    }
    pub fn null_count(&self) -> usize {
        self.nulls.as_ref().map_or(0, |n| n.iter().filter(|x| **x).count())
    }

    pub fn get(&self, i: usize) -> Value {
        if self.is_null(i) {
            return Value::Null;
        }
        match &self.data {
            ColumnData::Bool(v) => Value::Bool(v[i]),
            ColumnData::Int(v) => Value::Int(v[i]),
            ColumnData::Float(v) => Value::Float(v[i]),
            ColumnData::Text(v) => Value::Text(v[i].clone()),
            ColumnData::Date(v) => Value::Date(v[i]),
            ColumnData::DateTime(v) => Value::DateTime(v[i]),
            ColumnData::Uuid(v) => Value::Uuid(v[i]),
        }
    }

    /// Numeric view of row `i` (ints widen to f64); `None` for NULL / non-numeric.
    #[inline]
    pub fn f64_at(&self, i: usize) -> Option<f64> {
        if self.is_null(i) {
            return None;
        }
        match &self.data {
            ColumnData::Int(v) => Some(v[i] as f64),
            ColumnData::Float(v) => Some(v[i]),
            _ => None,
        }
    }

    pub fn push_null(&mut self) {
        let n = self.len();
        each!(&mut self.data, v => v.push(Default::default()));
        self.nulls.get_or_insert_with(|| vec![false; n]).push(true);
    }

    /// Appends a value, applying the lossless coercions INT→FLOAT and DATE→DATETIME.
    pub fn push(&mut self, value: Value) -> Result<(), String> {
        if value.is_null() {
            self.push_null();
            return Ok(());
        }
        match (&mut self.data, value) {
            (ColumnData::Bool(v), Value::Bool(x)) => v.push(x),
            (ColumnData::Int(v), Value::Int(x)) => v.push(x),
            (ColumnData::Float(v), Value::Float(x)) => v.push(x),
            (ColumnData::Float(v), Value::Int(x)) => v.push(x as f64),
            (ColumnData::Text(v), Value::Text(x)) => v.push(x),
            (ColumnData::Date(v), Value::Date(x)) => v.push(x),
            (ColumnData::DateTime(v), Value::DateTime(x)) => v.push(x),
            (ColumnData::DateTime(v), Value::Date(x)) => v.push(x as i64 * MICROS_PER_DAY),
            (ColumnData::Uuid(v), Value::Uuid(x)) => v.push(x),
            (d, v) => {
                return Err(format!("cannot store {:?} in a {} column", v.dtype(), d.dtype()));
            }
        }
        if let Some(n) = &mut self.nulls {
            n.push(false);
        }
        Ok(())
    }

    pub fn append(&mut self, other: &Column) {
        let n = self.len();
        match (&mut self.data, &other.data) {
            (ColumnData::Bool(a), ColumnData::Bool(b)) => a.extend_from_slice(b),
            (ColumnData::Int(a), ColumnData::Int(b)) => a.extend_from_slice(b),
            (ColumnData::Float(a), ColumnData::Float(b)) => a.extend_from_slice(b),
            (ColumnData::Text(a), ColumnData::Text(b)) => a.extend_from_slice(b),
            (ColumnData::Date(a), ColumnData::Date(b)) => a.extend_from_slice(b),
            (ColumnData::DateTime(a), ColumnData::DateTime(b)) => a.extend_from_slice(b),
            (ColumnData::Uuid(a), ColumnData::Uuid(b)) => a.extend_from_slice(b),
            _ => panic!("Column::append: type mismatch"),
        }
        match (&mut self.nulls, &other.nulls) {
            (None, None) => {}
            (Some(a), Some(b)) => a.extend_from_slice(b),
            (Some(a), None) => a.resize(n + other.len(), false),
            (a @ None, Some(b)) => {
                let mut m = vec![false; n];
                m.extend_from_slice(b);
                *a = Some(m);
            }
        }
    }

    pub fn concat(dtype: DataType, parts: &[&Column]) -> Column {
        let total = parts.iter().map(|c| c.len()).sum();
        let mut out = Column::with_capacity(dtype, total);
        for p in parts {
            out.append(p);
        }
        out
    }

    pub fn slice(&self, start: usize, len: usize) -> Column {
        let end = (start + len).min(self.len());
        Column::from_parts(
            map_data!(&self.data, v => v[start..end].to_vec()),
            self.nulls.as_ref().map(|n| n[start..end].to_vec()),
        )
    }

    pub fn take(&self, idx: &[usize]) -> Column {
        Column::from_parts(
            map_data!(&self.data, v => idx.iter().map(|&i| v[i].clone()).collect()),
            self.nulls.as_ref().map(|n| idx.iter().map(|&i| n[i]).collect()),
        )
    }

    /// Gather with holes: `None` produces NULL (outer joins).
    pub fn take_opt(&self, idx: &[Option<usize>]) -> Column {
        let data = map_data!(&self.data, v => idx
            .iter()
            .map(|i| match i {
                Some(i) => v[*i].clone(),
                None => Default::default(),
            })
            .collect());
        let nulls: Vec<bool> = idx
            .iter()
            .map(|i| match i {
                Some(i) => self.is_null(*i),
                None => true,
            })
            .collect();
        Column::from_parts(data, Some(nulls))
    }

    pub fn filter(&self, mask: &[bool]) -> Column {
        let idx: Vec<usize> = mask.iter().enumerate().filter(|(_, m)| **m).map(|(i, _)| i).collect();
        self.take(&idx)
    }

    /// For a BOOL column: `true` only where the value is TRUE (NULL counts as false).
    pub fn truth_mask(&self) -> Vec<bool> {
        match &self.data {
            ColumnData::Bool(v) => match &self.nulls {
                None => v.clone(),
                Some(n) => v.iter().zip(n).map(|(a, b)| *a && !*b).collect(),
            },
            _ => vec![false; self.len()],
        }
    }

    // ------------------------------------------------------------- row hashing
    /// Folds this column into per-row hashes (NULLs hash to a fixed token).
    pub fn hash_into(&self, hashes: &mut [u64]) {
        match &self.data {
            ColumnData::Bool(v) => v.iter().enumerate().for_each(|(i, x)| hashes[i] = mix(hashes[i], *x as u64)),
            ColumnData::Int(v) | ColumnData::DateTime(v) => {
                v.iter().enumerate().for_each(|(i, x)| hashes[i] = mix(hashes[i], *x as u64))
            }
            ColumnData::Date(v) => v.iter().enumerate().for_each(|(i, x)| hashes[i] = mix(hashes[i], *x as u64)),
            ColumnData::Float(v) => v.iter().enumerate().for_each(|(i, x)| hashes[i] = mix(hashes[i], float_bits(*x))),
            ColumnData::Uuid(v) => v.iter().enumerate().for_each(|(i, x)| {
                hashes[i] = mix(mix(hashes[i], *x as u64), (*x >> 64) as u64)
            }),
            ColumnData::Text(v) => v.iter().enumerate().for_each(|(i, x)| hashes[i] = mix(hashes[i], hash_bytes(x.as_bytes()))),
        }
        if let Some(n) = &self.nulls {
            for (i, isnull) in n.iter().enumerate() {
                if *isnull {
                    hashes[i] = mix(hashes[i], 0x9e37_79b9_7f4a_7c15);
                }
            }
        }
    }

    /// Grouping equality: NULL equals NULL. Both columns must have the same type.
    pub fn eq_at(&self, i: usize, other: &Column, j: usize) -> bool {
        match (self.is_null(i), other.is_null(j)) {
            (true, true) => return true,
            (false, false) => {}
            _ => return false,
        }
        match (&self.data, &other.data) {
            (ColumnData::Bool(a), ColumnData::Bool(b)) => a[i] == b[j],
            (ColumnData::Int(a), ColumnData::Int(b)) => a[i] == b[j],
            (ColumnData::Float(a), ColumnData::Float(b)) => float_bits(a[i]) == float_bits(b[j]),
            (ColumnData::Text(a), ColumnData::Text(b)) => a[i] == b[j],
            (ColumnData::Date(a), ColumnData::Date(b)) => a[i] == b[j],
            (ColumnData::DateTime(a), ColumnData::DateTime(b)) => a[i] == b[j],
            (ColumnData::Uuid(a), ColumnData::Uuid(b)) => a[i] == b[j],
            _ => false,
        }
    }

    /// Sort order of two rows of the same column; NULLs sort last.
    pub fn cmp_at(&self, i: usize, j: usize) -> Ordering {
        match (self.is_null(i), self.is_null(j)) {
            (true, true) => return Ordering::Equal,
            (true, false) => return Ordering::Greater,
            (false, true) => return Ordering::Less,
            _ => {}
        }
        match &self.data {
            ColumnData::Bool(v) => v[i].cmp(&v[j]),
            ColumnData::Int(v) | ColumnData::DateTime(v) => v[i].cmp(&v[j]),
            ColumnData::Float(v) => v[i].total_cmp(&v[j]),
            ColumnData::Text(v) => v[i].cmp(&v[j]),
            ColumnData::Date(v) => v[i].cmp(&v[j]),
            ColumnData::Uuid(v) => v[i].cmp(&v[j]),
        }
    }

    // --------------------------------------------------------------- zone map
    pub fn stats(&self) -> ColumnStats {
        let mut null_count = 0u32;
        let mut min: Option<usize> = None;
        let mut max: Option<usize> = None;
        let comparable = !matches!(self.data, ColumnData::Uuid(_));
        for i in 0..self.len() {
            if self.is_null(i) {
                null_count += 1;
                continue;
            }
            if let ColumnData::Float(v) = &self.data {
                if v[i].is_nan() {
                    // NaN has no place in a range; disable pruning for this column.
                    return ColumnStats { null_count: self.null_count() as u32, min: None, max: None };
                }
            }
            if !comparable {
                continue;
            }
            if min.map_or(true, |m| self.cmp_at(i, m) == Ordering::Less) {
                min = Some(i);
            }
            if max.map_or(true, |m| self.cmp_at(i, m) == Ordering::Greater) {
                max = Some(i);
            }
        }
        let long_text = |v: &Value| matches!(v, Value::Text(s) if s.len() > 64);
        let (min, max) = (min.map(|i| self.get(i)), max.map(|i| self.get(i)));
        if min.as_ref().map_or(false, long_text) || max.as_ref().map_or(false, long_text) {
            return ColumnStats { null_count, min: None, max: None };
        }
        ColumnStats { null_count, min, max }
    }
}

