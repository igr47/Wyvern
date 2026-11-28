//! Type conversion between values and columns.

use crate::bail;
use crate::error::Result;
use wyvern_storage::temporal::{self, MICROS_PER_DAY};
use wyvern_storage::types::{format_uuid, parse_uuid};
use wyvern_storage::{Column, ColumnData, DataType, Value};

fn round_to(x: f64, scale: u8) -> f64 {
    let m = 10f64.powi(scale as i32);
    (x * m).round() / m
}

pub fn cast_value(v: Value, to: DataType) -> Result<Value> {
    if v.is_null() {
        return Ok(Value::Null);
    }
    let fail = |v: &Value| -> Result<Value> { bail!("cannot cast {v:?} to {to}") };
    Ok(match (to.canonical(), &v) {
        (DataType::Bool, Value::Bool(_)) => v,
        (DataType::Bool, Value::Int(i)) => Value::Bool(*i != 0),
        (DataType::Bool, Value::Text(s)) => match s.trim().to_ascii_lowercase().as_str() {
            "true" | "t" | "yes" | "y" | "1" => Value::Bool(true),
            "false" | "f" | "no" | "n" | "0" => Value::Bool(false),
            _ => bail!("cannot cast '{s}' to BOOL"),
        },
        (DataType::Int, Value::Int(_)) => v,
        (DataType::Int, Value::Bool(b)) => Value::Int(*b as i64),
        (DataType::Int, Value::Float(f)) => {
            if !f.is_finite() || f.abs() >= 9.2e18 {
                bail!("cannot cast {f} to INT: out of range");
            }
            Value::Int(f.round() as i64)
        }
        (DataType::Int, Value::Text(s)) => match s.trim().parse::<i64>() {
            Ok(i) => Value::Int(i),
            Err(_) => match s.trim().parse::<f64>() {
                Ok(f) if f.is_finite() && f.fract() == 0.0 => Value::Int(f as i64),
                _ => bail!("cannot cast '{s}' to INT"),
            },
        },
        (DataType::Float, Value::Float(f)) => match to {
            DataType::Decimal { scale: Some(s), .. } => Value::Float(round_to(*f, s)),
            _ => v,
        },
        (DataType::Float, Value::Int(i)) => Value::Float(match to {
            DataType::Decimal { scale: Some(s), .. } => round_to(*i as f64, s),
            _ => *i as f64,
        }),
        (DataType::Float, Value::Bool(b)) => Value::Float(*b as i64 as f64),
        (DataType::Float, Value::Text(s)) => match s.trim().parse::<f64>() {
            Ok(f) => match to {
                DataType::Decimal { scale: Some(sc), .. } => Value::Float(round_to(f, sc)),
                _ => Value::Float(f),
            },
            Err(_) => bail!("cannot cast '{s}' to {to}"),
        },
        (DataType::Text, Value::Text(_)) => v,
        (DataType::Text, other) => Value::Text(other.to_string()),
        (DataType::Date, Value::Date(_)) => v,
        (DataType::Date, Value::DateTime(t)) => Value::Date(t.div_euclid(MICROS_PER_DAY) as i32),
        (DataType::Date, Value::Text(s)) => match temporal::parse_date(s) {
            Some(d) => Value::Date(d),
            None => match temporal::parse_datetime(s) {
                Some(t) => Value::Date(t.div_euclid(MICROS_PER_DAY) as i32),
                None => bail!("cannot cast '{s}' to DATE (expected YYYY-MM-DD)"),
            },
        },
        (DataType::DateTime, Value::DateTime(_)) => v,
        (DataType::DateTime, Value::Date(d)) => Value::DateTime(*d as i64 * MICROS_PER_DAY),
        (DataType::DateTime, Value::Text(s)) => match temporal::parse_datetime(s) {
            Some(t) => Value::DateTime(t),
            None => bail!("cannot cast '{s}' to DATETIME (expected YYYY-MM-DD HH:MM:SS)"),
        },
        (DataType::Uuid, Value::Uuid(_)) => v,
        (DataType::Uuid, Value::Text(s)) => match parse_uuid(s) {
            Some(u) => Value::Uuid(u),
            None => bail!("cannot cast '{s}' to UUID"),
        },
        _ => return fail(&v),
    })
}

/// Column-wide cast with fast paths for the common numeric conversions.
pub fn cast_column(c: &Column, to: DataType) -> Result<Column> {
    if c.dtype() == to.canonical() && !matches!(to, DataType::Decimal { scale: Some(_), .. }) {
        return Ok(c.clone());
    }
    match (&c.data, to.canonical()) {
        (ColumnData::Int(v), DataType::Float) if !matches!(to, DataType::Decimal { .. }) => {
            return Ok(Column::from_parts(ColumnData::Float(v.iter().map(|x| *x as f64).collect()), c.nulls.clone()));
        }
        (ColumnData::Date(v), DataType::DateTime) => {
            return Ok(Column::from_parts(
                ColumnData::DateTime(v.iter().map(|d| *d as i64 * MICROS_PER_DAY).collect()),
                c.nulls.clone(),
            ));
        }
        _ => {}
    }
    let mut out = Column::with_capacity(to, c.len());
    for i in 0..c.len() {
        out.push(cast_value(c.get(i), to)?).map_err(crate::error::EngineError::msg)?;
    }
    Ok(out)
}

pub fn uuid_text(u: u128) -> String {
    format_uuid(u)
}

/// Parser type → storage type.
pub fn to_storage(t: wyvern_parse::ast::DataType) -> DataType {
    use wyvern_parse::ast::DataType as P;
    match t {
        P::Bool => DataType::Bool,
        P::Int => DataType::Int,
        P::Float => DataType::Float,
        P::Decimal { precision, scale } => DataType::Decimal { precision, scale },
        P::Text => DataType::Text,
        P::Date => DataType::Date,
        P::DateTime => DataType::DateTime,
        P::Uuid => DataType::Uuid,
    }
}

