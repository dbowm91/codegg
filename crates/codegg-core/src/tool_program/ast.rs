//! Normalized AST types for the restricted-Python Tool Program language.
//!
//! These are Codegg-owned types. The parser converts upstream AST nodes
//! immediately into these types; third-party AST structures are not persisted.

use std::fmt;

/// A complete Tool Program source file.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub body: Vec<Stmt>,
    pub span: Span,
}

/// Source byte span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub fn len(&self) -> usize {
        self.end.saturating_sub(self.start)
    }

    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }
}

/// Statement nodes.
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// `target = expr` or `a, b = expr`
    Assign {
        targets: Vec<Ident>,
        value: Expr,
        span: Span,
    },
    /// `if expr: ... [elif expr: ...] [else: ...]`
    If {
        test: Expr,
        body: Vec<Stmt>,
        elif_clauses: Vec<ElifClause>,
        else_body: Option<Vec<Stmt>>,
        span: Span,
    },
    /// `for target in iterable: ...`
    For {
        target: Ident,
        iter: Expr,
        body: Vec<Stmt>,
        span: Span,
    },
    /// `assert expr [, msg]`
    Assert {
        test: Expr,
        msg: Option<Expr>,
        span: Span,
    },
    /// `target = call({...})`
    ToolCall {
        target: Ident,
        descriptor: Expr,
        kwargs: Vec<KwArg>,
        span: Span,
    },
    /// `target = parallel({...}, {...}, ...)`
    Parallel {
        target: Ident,
        descriptors: Vec<Expr>,
        span: Span,
    },
    /// `emit(expr)`
    Emit { value: Expr, span: Span },
    /// `fail([expr])`
    Fail { reason: Option<Expr>, span: Span },
    /// `pass`
    Pass { span: Span },
}

/// An `elif` clause.
#[derive(Debug, Clone, PartialEq)]
pub struct ElifClause {
    pub test: Expr,
    pub body: Vec<Stmt>,
    pub span: Span,
}

/// Expression nodes.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// `None`
    NoneLiteral(Span),
    /// `True` / `False`
    BoolLiteral { value: bool, span: Span },
    /// Integer literal (decimal)
    IntLiteral { value: i64, span: Span },
    /// Float literal
    FloatLiteral { value: f64, span: Span },
    /// String literal (single or double quoted)
    StringLiteral { value: String, span: Span },
    /// Identifier / variable reference
    Name { id: String, span: Span },
    /// `a + b`, `a - b`, `a * b`, `a / b`, `a // b`, `a % b`, `a ** b`, `a @ b`
    BinOp {
        left: Box<Expr>,
        op: BinOpKind,
        right: Box<Expr>,
        span: Span,
    },
    /// `-a`, `+a`, `~a`
    UnaryOp {
        op: UnaryOpKind,
        operand: Box<Expr>,
        span: Span,
    },
    /// `a == b`, `a != b`, `a < b`, etc.
    Compare {
        left: Box<Expr>,
        ops: Vec<CmpOp>,
        comparators: Vec<Expr>,
        span: Span,
    },
    /// `a and b`
    BoolAnd {
        left: Box<Expr>,
        right: Box<Expr>,
        span: Span,
    },
    /// `a or b`
    BoolOr {
        left: Box<Expr>,
        right: Box<Expr>,
        span: Span,
    },
    /// `not a`
    BoolNot { operand: Box<Expr>, span: Span },
    /// `a[b]` or `a[b:c]` or `a[:c]` or `a[b:]` or `a[:]`
    Subscript {
        value: Box<Expr>,
        slice: Slice,
        span: Span,
    },
    /// `len(expr)`, `str(expr)`, `int(expr)`, `bool(expr)`
    CallBuiltin {
        func: BuiltinFunc,
        arg: Box<Expr>,
        span: Span,
    },
    /// `obj.method(args)` — only allowed methods
    MethodCall {
        object: Box<Expr>,
        method: String,
        args: Vec<Expr>,
        span: Span,
    },
    /// `call({...}, key=val, ...)`
    ToolCallExpr {
        descriptor: Box<Expr>,
        kwargs: Vec<KwArg>,
        span: Span,
    },
    /// `parallel({...}, {...}, ...)`
    ParallelExpr { descriptors: Vec<Expr>, span: Span },
    /// `[a, b, c]`
    List { elements: Vec<Expr>, span: Span },
    /// `(a, b, c)`
    Tuple { elements: Vec<Expr>, span: Span },
    /// `{k: v, ...}`
    Dict {
        keys: Vec<Expr>,
        values: Vec<Expr>,
        span: Span,
    },
    /// Parenthesized expression
    Parenthesized { inner: Box<Expr>, span: Span },
    /// `range(stop)`, `range(start, stop)`, `range(start, stop, step)`
    Range {
        start: Option<Box<Expr>>,
        stop: Box<Expr>,
        step: Option<Box<Expr>>,
        span: Span,
    },
}

impl Expr {
    /// Return the source span for this expression.
    pub fn span(&self) -> Span {
        match self {
            Expr::NoneLiteral(s)
            | Expr::BoolLiteral { span: s, .. }
            | Expr::IntLiteral { span: s, .. }
            | Expr::FloatLiteral { span: s, .. }
            | Expr::StringLiteral { span: s, .. }
            | Expr::Name { span: s, .. }
            | Expr::BinOp { span: s, .. }
            | Expr::UnaryOp { span: s, .. }
            | Expr::Compare { span: s, .. }
            | Expr::BoolAnd { span: s, .. }
            | Expr::BoolOr { span: s, .. }
            | Expr::BoolNot { span: s, .. }
            | Expr::Subscript { span: s, .. }
            | Expr::CallBuiltin { span: s, .. }
            | Expr::MethodCall { span: s, .. }
            | Expr::ToolCallExpr { span: s, .. }
            | Expr::ParallelExpr { span: s, .. }
            | Expr::List { span: s, .. }
            | Expr::Tuple { span: s, .. }
            | Expr::Dict { span: s, .. }
            | Expr::Parenthesized { span: s, .. }
            | Expr::Range { span: s, .. } => *s,
        }
    }
}

/// Identifier with source span.
#[derive(Debug, Clone, PartialEq)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

/// Subscript slice.
#[derive(Debug, Clone, PartialEq)]
pub enum Slice {
    /// `expr`
    Index(Box<Expr>),
    /// `expr:expr`, `expr:`, `:expr`, `:`
    Range {
        start: Option<Box<Expr>>,
        stop: Option<Box<Expr>>,
        step: Option<Box<Expr>>,
    },
}

/// Keyword argument.
#[derive(Debug, Clone, PartialEq)]
pub struct KwArg {
    pub name: String,
    pub value: Expr,
    pub span: Span,
}

/// Binary operator kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOpKind {
    Add,
    Sub,
    Mul,
    Div,
    FloorDiv,
    Mod,
    Pow,
    MatMul,
    BitOr,
    BitXor,
    BitAnd,
    LShift,
    RShift,
}

/// Unary operator kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOpKind {
    Neg,
    Pos,
    Invert,
}

/// Comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    NotEq,
    Lt,
    LtE,
    Gt,
    GtE,
    In,
    NotIn,
}

/// Allowed built-in function names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuiltinFunc {
    Len,
    Str,
    Int,
    Bool,
}

impl fmt::Display for BuiltinFunc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Len => write!(f, "len"),
            Self::Str => write!(f, "str"),
            Self::Int => write!(f, "int"),
            Self::Bool => write!(f, "bool"),
        }
    }
}
