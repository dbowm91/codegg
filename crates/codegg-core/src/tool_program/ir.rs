//! Versioned IR types for Tool Programs.
//!
//! The IR is a flat instruction sequence with source-span mapping,
//! deterministic evaluation, and content-addressed integrity.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Current language version.
pub const LANGUAGE_VERSION: u32 = 1;
/// Current compiler version.
pub const COMPILER_VERSION: u32 = 1;
/// Current parser version (tracks rustpython-parser upgrade).
pub const PARSER_VERSION: u32 = 1;

/// IR format version.
pub type IrVersion = u32;

/// Current IR format version.
pub const IR_VERSION: IrVersion = 1;

/// A compiled Tool Program IR.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IrProgram {
    /// IR format version.
    pub version: IrVersion,
    /// Language version.
    pub language_version: u32,
    /// Compiler version.
    pub compiler_version: u32,
    /// Parser version.
    pub parser_version: u32,
    /// Source content hash (SHA-256).
    pub source_hash: String,
    /// Manifest content hash (SHA-256), empty if no manifest.
    pub manifest_hash: String,
    /// Limits content hash (SHA-256).
    pub limits_hash: String,
    /// Deterministic digest over the entire IR.
    pub digest: String,
    /// Instruction sequence.
    pub instructions: Vec<IrInstruction>,
    /// Local variable count (determined at compile time).
    pub local_count: u32,
    /// Static bounds computed during compilation.
    pub bounds: IrBounds,
    /// Source span mapping: instruction index → source span.
    pub spans: Vec<IrSpan>,
    /// String constants pool.
    pub strings: Vec<String>,
    /// Integer constants pool.
    pub integers: Vec<i64>,
    /// Float constants pool.
    pub floats: Vec<f64>,
}

/// Static bounds stored in the IR.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IrBounds {
    /// Maximum IR steps (instruction count).
    pub max_steps: u64,
    /// Maximum iterations per loop.
    pub max_loop_iterations: u64,
    /// Total maximum iterations across all loops.
    pub max_total_iterations: u64,
    /// Number of syntactic call sites.
    pub call_site_count: u32,
    /// Maximum dynamic call count upper bound.
    pub max_dynamic_calls: u64,
    /// Maximum parallel width.
    pub max_parallel_width: u32,
    /// Maximum nested parallel depth.
    pub max_parallel_depth: u32,
    /// Maximum nesting depth.
    pub max_nesting_depth: u32,
    /// Maximum collection/value growth upper bound.
    pub max_value_growth: u64,
}

/// An IR instruction with source span.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IrInstruction {
    pub op: IrOp,
    pub span_idx: u32,
}

/// IR operation codes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum IrOp {
    /// Load a constant integer from the pool.
    LoadInt { pool_idx: u32 },
    /// Load a constant float from the pool.
    LoadFloat { pool_idx: u32 },
    /// Load a constant string from the pool.
    LoadString { pool_idx: u32 },
    /// Load boolean true.
    LoadTrue,
    /// Load boolean false.
    LoadFalse,
    /// Load None.
    LoadNone,
    /// Load a local variable.
    LoadLocal { slot: u32 },
    /// Store to a local variable.
    StoreLocal { slot: u32 },
    /// Construct a list from N stack values.
    MakeList { count: u32 },
    /// Construct a tuple from N stack values.
    MakeTuple { count: u32 },
    /// Construct a dict from N key-value pairs on the stack.
    MakeDict { count: u32 },
    /// Binary operation.
    BinOp { kind: IrBinOp },
    /// Unary operation.
    UnaryOp { kind: IrUnaryOp },
    /// Comparison (pops 2 + comparator count).
    Compare { kind: IrCmpOp },
    /// Logical AND (short-circuit).
    BoolAnd,
    /// Logical OR (short-circuit).
    BoolOr,
    /// Logical NOT.
    BoolNot,
    /// Pop top of stack (discard).
    Pop,
    /// Duplicate top of stack.
    Dup,
    /// Index into a collection: `collection[key]`.
    Index,
    /// Slice: `collection[start:stop:step]` (all Option on stack).
    Slice,
    /// Get length of top-of-stack collection.
    Len,
    /// String conversion.
    Str,
    /// Integer conversion.
    Int,
    /// Boolean conversion.
    Bool,
    /// Conditional branch: jump to `target` if top-of-stack is falsy.
    JumpIfFalse { target: u32 },
    /// Unconditional jump.
    Jump { target: u32 },
    /// Begin a bounded for-loop iterator.
    /// `iterable` is on the stack; jumps to `body_start` or `loop_end`.
    ForLoopStart { body_start: u32, loop_end: u32 },
    /// End of loop body: jump back to iterator.
    ForLoopNext { loop_start: u32, loop_end: u32 },
    /// Push the current loop iterator value onto the stack.
    ForLoopIter,
    /// Construct a call request from a descriptor dict on the stack.
    /// Pops the descriptor and pushes a CallRequest.
    ConstructCall,
    /// Execute a sequential call: pops CallRequest, pushes result.
    ExecuteCall,
    /// Begin a parallel call group: N descriptors on the stack.
    ParallelStart { count: u32 },
    /// Execute parallel group: pops N descriptors, pushes list of results.
    ParallelExecute,
    /// Emit result: pops value from stack.
    Emit,
    /// Fail program with reason string on stack (or None).
    Fail,
    /// Checkpoint: record interpreter state.
    Checkpoint,
    /// Return from the program (implicit at end).
    Return,
}

/// Binary operation kinds for IR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IrBinOp {
    Add,
    Sub,
    Mul,
    Div,
    FloorDiv,
    Mod,
    Pow,
    BitOr,
    BitXor,
    BitAnd,
    LShift,
    RShift,
}

/// Unary operation kinds for IR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IrUnaryOp {
    Neg,
    Pos,
    Invert,
}

/// Comparison operation kinds for IR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IrCmpOp {
    Eq,
    NotEq,
    Lt,
    LtE,
    Gt,
    GtE,
    In,
    NotIn,
}

/// Source span mapping for an IR instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct IrSpan {
    pub start: u32,
    pub length: u32,
}

impl IrSpan {
    pub fn new(start: usize, length: usize) -> Self {
        Self {
            start: start as u32,
            length: length as u32,
        }
    }
}

impl fmt::Display for IrOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IrOp::LoadInt { pool_idx } => write!(f, "LoadInt({})", pool_idx),
            IrOp::LoadFloat { pool_idx } => write!(f, "LoadFloat({})", pool_idx),
            IrOp::LoadString { pool_idx } => write!(f, "LoadString({})", pool_idx),
            IrOp::LoadTrue => write!(f, "LoadTrue"),
            IrOp::LoadFalse => write!(f, "LoadFalse"),
            IrOp::LoadNone => write!(f, "LoadNone"),
            IrOp::LoadLocal { slot } => write!(f, "LoadLocal({})", slot),
            IrOp::StoreLocal { slot } => write!(f, "StoreLocal({})", slot),
            IrOp::MakeList { count } => write!(f, "MakeList({})", count),
            IrOp::MakeTuple { count } => write!(f, "MakeTuple({})", count),
            IrOp::MakeDict { count } => write!(f, "MakeDict({})", count),
            IrOp::BinOp { kind } => write!(f, "BinOp({:?})", kind),
            IrOp::UnaryOp { kind } => write!(f, "UnaryOp({:?})", kind),
            IrOp::Compare { kind } => write!(f, "Compare({:?})", kind),
            IrOp::BoolAnd => write!(f, "BoolAnd"),
            IrOp::BoolOr => write!(f, "BoolOr"),
            IrOp::BoolNot => write!(f, "BoolNot"),
            IrOp::Pop => write!(f, "Pop"),
            IrOp::Dup => write!(f, "Dup"),
            IrOp::Index => write!(f, "Index"),
            IrOp::Slice => write!(f, "Slice"),
            IrOp::Len => write!(f, "Len"),
            IrOp::Str => write!(f, "Str"),
            IrOp::Int => write!(f, "Int"),
            IrOp::Bool => write!(f, "Bool"),
            IrOp::JumpIfFalse { target } => write!(f, "JumpIfFalse({})", target),
            IrOp::Jump { target } => write!(f, "Jump({})", target),
            IrOp::ForLoopStart {
                body_start,
                loop_end,
            } => {
                write!(f, "ForLoopStart({}, {})", body_start, loop_end)
            }
            IrOp::ForLoopNext {
                loop_start,
                loop_end,
            } => {
                write!(f, "ForLoopNext({}, {})", loop_start, loop_end)
            }
            IrOp::ForLoopIter => write!(f, "ForLoopIter"),
            IrOp::ConstructCall => write!(f, "ConstructCall"),
            IrOp::ExecuteCall => write!(f, "ExecuteCall"),
            IrOp::ParallelStart { count } => write!(f, "ParallelStart({})", count),
            IrOp::ParallelExecute => write!(f, "ParallelExecute"),
            IrOp::Emit => write!(f, "Emit"),
            IrOp::Fail => write!(f, "Fail"),
            IrOp::Checkpoint => write!(f, "Checkpoint"),
            IrOp::Return => write!(f, "Return"),
        }
    }
}
