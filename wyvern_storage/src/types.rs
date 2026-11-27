use crate::temporal;
use std::cmp::Ordering;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DataType {
    Bool,
    Int,
    Float,
    /// Stored as `Float`; precision/scale are advisory (values are rounded to `scale`).
    Decimal { precision: Option<u8>, scale: Option<u8> },
    Text,
    Date,
    DateTime,
    Uuid,
}

impl DataType {
    /// The physical type a column of this type is stored as.
    pub fn canonical(self) -> DataType {
        match self {
            DataType::Decimal { .. } => DataType::Float,
            t => t,
        }
    }
    pub fn is_numeric(self) -> bool {
        matches!(self.canonical(), DataType::Int | DataType::Float)
    }
    pub fn name(self) -> &'static str {
        match self {
            DataType::Bool => "BOOL",
            DataType::Int => "INT",
            DataType::Float => "FLOAT",
            DataType::Decimal { .. } => "DECIMAL",
            DataType::Text => "TEXT",
            DataType::Date => "DATE",
            DataType::DateTime => "DATETIME",
            DataType::Uuid => "UUID",
        }
    }
}

impl fmt::Display for DataType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DataType::Decimal { precision: Some(p), scale: Some(s) } => write!(f, "DECIMAL({p},{s})"),
            DataType::Decimal { precision: Some(p), scale: None } => write!(f, "DECIMAL({p})"),
            t => f.write_str(t.name()),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Date(i32),
    DateTime(i64),
    Uuid(u128),
}

/// Hashable, equality-comparable image of a `Value` (integral floats collapse
/// to ints so `1 = 1.0` in hash joins / IN sets).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    Null,
    Bool(bool),
    Int(i64),
    Float(u64),
    Text(String),
    Date(i32),
    DateTime(i64),
    Uuid(u128),
}

impl Value {
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    pub fn dtype(&self) -> Option<DataType> {
        Some(match self {
            Value::Null => return None,
            Value::Bool(_) => DataType::Bool,
            Value::Int(_) => DataType::Int,
            Value::Float(_) => DataType::Float,
            Value::Text(_) => DataType::Text,
            Value::Date(_) => DataType::Date,
            Value::DateTime(_) => DataType::DateTime,
            Value::Uuid(_) => DataType::Uuid,
        })
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Int(i) => Some(*i as f64),
            Value::Float(f) => Some(*f),
            _ => None,
        }
    }

    pub fn key(&self) -> Key {
        match self {
            Value::Null => Key::Null,
            Value::Bool(b) => Key::Bool(*b),
            Value::Int(i) => Key::Int(*i),
            Value::Float(f) => {
                if f.fract() == 0.0 && f.abs() < 9.0e18 {
                    Key::Int(*f as i64)
                } else if f.is_nan() {
                    Key::Float(f64::NAN.to_bits())
                } else {
                    Key::Float(f.to_bits())
                }
            }
            Value::Text(s) => Key::Text(s.clone()),
            Value::Date(d) => Key::Date(*d),
            Value::DateTime(d) => Key::DateTime(*d),
            Value::Uuid(u) => Key::Uuid(*u),
        }
    }

    /// SQL comparison: `None` when either side is NULL or the types are not comparable.
    pub fn sql_cmp(&self, other: &Value) -> Option<Ordering> {
        use Value::*;
        match (self, other) {
            (Null, _) | (_, Null) => None,
            (Bool(a), Bool(b)) => Some(a.cmp(b)),
            (Int(a), Int(b)) => Some(a.cmp(b)),
            (Int(a), Float(b)) => (*a as f64).partial_cmp(b),
            (Float(a), Int(b)) => a.partial_cmp(&(*b as f64)),
            (Float(a), Float(b)) => a.partial_cmp(b),
            (Text(a), Text(b)) => Some(a.cmp(b)),
            (Date(a), Date(b)) => Some(a.cmp(b)),
            (DateTime(a), DateTime(b)) => Some(a.cmp(b)),
            (Date(a), DateTime(b)) => Some((*a as i64 * temporal::MICROS_PER_DAY).cmp(b)),
            (DateTime(a), Date(b)) => Some(a.cmp(&(*b as i64 * temporal::MICROS_PER_DAY))),
            (Uuid(a), Uuid(b)) => Some(a.cmp(b)),
            _ => None,
        }
    }

    /// Total order used by ORDER BY / sorting: NULL sorts after everything.
    pub fn total_cmp(&self, other: &Value) -> Ordering {
        match (self.is_null(), other.is_null()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            _ => self.sql_cmp(other).unwrap_or_else(|| match (self, other) {
                (Value::Float(a), Value::Float(b)) => a.total_cmp(b),
                _ => self.tag().cmp(&other.tag()),
            }),
        }
    }

    fn tag(&self) -> u8 {
        match self {
            Value::Null => 0,
            Value::Bool(_) => 1,
            Value::Int(_) | Value::Float(_) => 2,
            Value::Text(_) => 3,
            Value::Date(_) => 4,
            Value::DateTime(_) => 5,
            Value::Uuid(_) => 6,
        }
    }
}

pub fn format_uuid(u: u128) -> String {
    let h = format!("{u:032x}");
    format!("{}-{}-{}-{}-{}", &h[0..8], &h[8..12], &h[12..16], &h[16..20], &h[20..32])
}

pub fn parse_uuid(s: &str) -> Option<u128> {
    let hex: String = s.trim().chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 {
        return None;
    }
    u128::from_str_radix(&hex, 16).ok()
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => f.write_str("NULL"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Int(i) => write!(f, "{i}"),
            Value::Float(x) => {
                if x.is_finite() && x.fract() == 0.0 && x.abs() < 1e15 {
                    write!(f, "{x:.1}")
                } else {
                    write!(f, "{x}")
                }
            }
            Value::Text(s) => f.write_str(s),
            Value::Date(d) => f.write_str(&temporal::format_date(*d)),
            Value::DateTime(d) => f.write_str(&temporal::format_datetime(*d)),
            Value::Uuid(u) => f.write_str(&format_uuid(*u)),
        }
    }
}

// ------------------------------------------------------------------- schema
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub name: String,
    pub dtype: DataType,
    pub nullable: bool,
    pub primary_key: bool,
    pub unique: bool,
    pub auto_increment: bool,
    /// Constant default, evaluated when the table was created.
    pub default: Option<Value>,
}

impl Field {
    pub fn new(name: impl Into<String>, dtype: DataType) -> Self {
        Field {
            name: name.into(),
            dtype,
            nullable: true,
            primary_key: false,
            unique: false,
            auto_increment: false,
            default: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Schema {
    pub fields: Vec<Field>,
}

impl Schema {
    pub fn new(fields: Vec<Field>) -> Self {
        Schema { fields }
    }
    pub fn len(&self) -> usize {
        self.fields.len()
    }
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }
    /// Case-insensitive column lookup.
    pub fn index_of(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|f| f.name.eq_ignore_ascii_case(name))
    }
}


