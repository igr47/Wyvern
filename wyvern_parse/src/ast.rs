//! Borrowed AST. Every name is a `&str` into the source text; only string
//! literals containing `''` escapes allocate (`Cow`).

use std::borrow::Cow;
pub use chumsky::span::SimpleSpan as Span;

#[derive(Debug, Clone, PartialEq)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}

/// `name` or `db.name`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObjectName<'a> {
    pub schema: Option<&'a str>,
    pub name: &'a str,
}

// ------------------------------------------------------------------ statements
#[derive(Debug, Clone, PartialEq)]
pub enum Statement<'a> {
    // databases
    CreateDatabase { name: &'a str, if_not_exists: bool },
    UseDatabase { name: &'a str },
    DropDatabase { name: &'a str, if_exists: bool },
    // tables
    CreateTable(CreateTable<'a>),
    DropTable { names: Vec<ObjectName<'a>>, if_exists: bool },
    AlterTable(AlterTable<'a>),
    // data
    Insert(Insert<'a>),
    Update(Update<'a>),
    Delete(Delete<'a>),
    Load(Load<'a>),
    // queries & introspection
    Select(Box<Select<'a>>),
    Show(ShowTarget),
    Describe { table: ObjectName<'a> },
    Summarize(Relation<'a>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShowTarget {
    Databases,
    Tables,
}

/// Something you can run analysis on: a stored table or an ad-hoc query.
#[derive(Debug, Clone, PartialEq)]
pub enum Relation<'a> {
    Table(ObjectName<'a>),
    Query(Box<Select<'a>>),
}

// ------------------------------------------------------------------------ DDL
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableKind {
    Persistent,
    InMemory,
    Temporary,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CreateTable<'a> {
    pub name: ObjectName<'a>,
    pub kind: TableKind,
    pub or_replace: bool,
    pub if_not_exists: bool,
    pub body: TableBody<'a>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TableBody<'a> {
    /// `(col type constraints, ...)`
    Columns(Vec<ColumnDef<'a>>),
    /// `FROM 'file.csv'` — schema inferred from the file
    File(FileSpec<'a>),
    /// `AS SELECT ...`
    Query(Box<Select<'a>>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ColumnDef<'a> {
    pub name: &'a str,
    pub ty: DataType,
    pub constraints: Vec<ColumnConstraint<'a>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ColumnConstraint<'a> {
    PrimaryKey,
    NotNull,
    Null,
    Unique,
    AutoIncrement,
    Default(Expr<'a>),
    Check(Expr<'a>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataType {
    Bool,
    Int,
    Float,
    Decimal { precision: Option<u8>, scale: Option<u8> },
    Text,
    Date,
    DateTime,
    Uuid,
}

impl DataType {
    /// Maps a (case-insensitive) type name and its `(n, m)` arguments to a type.
    /// Add synonyms here; the grammar never changes.
    pub fn from_name(name: &str, args: &[u32]) -> Option<DataType> {
        let n = name.to_ascii_lowercase();
        Some(match n.as_str() {
            "bool" | "boolean" => DataType::Bool,
            "int" | "integer" | "bigint" | "smallint" | "tinyint" | "int64" | "long" => DataType::Int,
            "float" | "double" | "real" | "float64" => DataType::Float,
            "decimal" | "numeric" => DataType::Decimal {
                precision: args.first().map(|&p| p as u8),
                scale: args.get(1).map(|&s| s as u8),
            },
            "text" | "string" | "varchar" | "char" => DataType::Text,
            "date" => DataType::Date,
            "datetime" | "timestamp" => DataType::DateTime,
            "uuid" => DataType::Uuid,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlterTable<'a> {
    pub table: ObjectName<'a>,
    /// `ALTER TABLE t ADD a INT, DROP b, RENAME c TO d` — many actions, one statement
    pub actions: Vec<AlterAction<'a>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AlterAction<'a> {
    AddColumn(ColumnDef<'a>),
    DropColumn { name: &'a str, if_exists: bool },
    RenameColumn { from: &'a str, to: &'a str },
    RenameTable(&'a str),
    ModifyColumn { name: &'a str, ty: DataType },
}

// ------------------------------------------------------------------------ DML
#[derive(Debug, Clone, PartialEq)]
pub struct Insert<'a> {
    pub table: ObjectName<'a>,
    /// empty = all columns, in table order
    pub columns: Vec<&'a str>,
    pub source: InsertSource<'a>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InsertSource<'a> {
    Values(Vec<Vec<Expr<'a>>>),
    Query(Box<Select<'a>>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Update<'a> {
    pub table: ObjectName<'a>,
    pub assignments: Vec<(&'a str, Expr<'a>)>,
    pub filter: Option<Expr<'a>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Delete<'a> {
    pub table: ObjectName<'a>,
    pub filter: Option<Expr<'a>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Load<'a> {
    pub table: ObjectName<'a>,
    pub file: FileSpec<'a>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct FileSpec<'a> {
    pub path: Cow<'a, str>,
    /// `None` = auto-detect
    pub header: Option<bool>,
    pub delimiter: Option<Cow<'a, str>>,
    /// `csv`, `json`, `parquet`... `None` = infer from extension
    pub format: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FileOpt<'a> {
    Header(bool),
    Delimiter(Cow<'a, str>),
    Format(&'a str),
}

impl<'a> FileSpec<'a> {
    pub fn new(path: Cow<'a, str>, opts: Vec<FileOpt<'a>>) -> Self {
        let mut spec = FileSpec { path, ..Default::default() };
        for opt in opts {
            match opt {
                FileOpt::Header(h) => spec.header = Some(h),
                FileOpt::Delimiter(d) => spec.delimiter = Some(d),
                FileOpt::Format(f) => spec.format = Some(f),
            }
        }
        spec
    }
}

// --------------------------------------------------------------------- SELECT
#[derive(Debug, Clone, PartialEq)]
pub struct Select<'a> {
    pub distinct: bool,
    pub projection: Vec<SelectItem<'a>>,
    pub from: Option<TableRef<'a>>,
    pub joins: Vec<Join<'a>>,
    pub filter: Option<Expr<'a>>,
    pub group_by: Option<GroupBy<'a>>,
    pub having: Option<Expr<'a>>,
    pub order_by: Vec<OrderItem<'a>>,
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SelectItem<'a> {
    /// `*` or `* EXCLUDE (a, b)`
    Wildcard { exclude: Vec<&'a str> },
    Expr { expr: Expr<'a>, alias: Option<&'a str> },
}

#[derive(Debug, Clone, PartialEq)]
pub enum GroupBy<'a> {
    /// `GROUP BY ALL`: group by every non-aggregate projection item
    All,
    Exprs(Vec<Expr<'a>>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct OrderItem<'a> {
    pub expr: Expr<'a>,
    pub descending: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableRef<'a> {
    pub factor: TableFactor<'a>,
    pub alias: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TableFactor<'a> {
    Table(ObjectName<'a>),
    Subquery(Box<Select<'a>>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinKind {
    Inner,
    Left,
    Right,
    Full,
    Cross,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Join<'a> {
    pub kind: JoinKind,
    pub table: TableRef<'a>,
    pub constraint: Option<JoinConstraint<'a>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum JoinConstraint<'a> {
    On(Expr<'a>),
    Using(Vec<&'a str>),
}

// ---------------------------------------------------------------- expressions
#[derive(Debug, Clone, PartialEq)]
pub enum Expr<'a> {
    Literal(Literal<'a>),
    Column { table: Option<&'a str>, name: &'a str },
    Unary { op: UnaryOp, expr: Box<Expr<'a>> },
    Binary { op: BinaryOp, left: Box<Expr<'a>>, right: Box<Expr<'a>> },
    IsNull { expr: Box<Expr<'a>>, negated: bool },
    InList { expr: Box<Expr<'a>>, list: Vec<Expr<'a>>, negated: bool },
    InSubquery { expr: Box<Expr<'a>>, query: Box<Select<'a>>, negated: bool },
    Between { expr: Box<Expr<'a>>, low: Box<Expr<'a>>, high: Box<Expr<'a>>, negated: bool },
    Like { expr: Box<Expr<'a>>, pattern: Box<Expr<'a>>, negated: bool, case_insensitive: bool },
    Case { operand: Option<Box<Expr<'a>>>, branches: Vec<(Expr<'a>, Expr<'a>)>, else_branch: Option<Box<Expr<'a>>> },
    Cast { expr: Box<Expr<'a>>, ty: DataType },
    Function(FunctionCall<'a>),
    Subquery(Box<Select<'a>>),
    Exists(Box<Select<'a>>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal<'a> {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(Cow<'a, str>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add, Sub, Mul, Div, Mod, Concat,
    Eq, Ne, Lt, Le, Gt, Ge,
    And, Or,
}

/// Every function — scalar, aggregate, window, statistical — is one node.
/// New functions need *no* parser or lexer change; the executor resolves `name`.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionCall<'a> {
    pub name: &'a str,
    pub args: Vec<Expr<'a>>,
    pub distinct: bool,
    /// `count(*)`
    pub star: bool,
    pub over: Option<WindowSpec<'a>>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct WindowSpec<'a> {
    pub partition_by: Vec<Expr<'a>>,
    pub order_by: Vec<OrderItem<'a>>,
}

