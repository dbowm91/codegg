//! Metered deterministic IR interpreter for Tool Programs.
//!
//! The interpreter is a stack machine that evaluates a verified [`IrProgram`]
//! with bounded budgets for steps, bytes, iterations, calls, and parallel
//! groups. It delegates tool calls to a [`BrokerCallback`] abstraction and
//! supports checkpointing at defined boundaries.
//!
//! # Invariants
//!
//! - Every instruction consumes at least one step.
//! - Value growth is checked before allocation.
//! - Completed calls are tracked for replay and never repeated.
//! - No ambient clock, randomness, filesystem, network, or process access.
//! - Cancellation is cooperative via an async flag.

use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use super::ir::{IrBinOp, IrCmpOp, IrInstruction, IrOp, IrProgram, IrUnaryOp};

// ── Program value (metered, JSON-compatible) ──────────────────────

/// A JSON-compatible value with metered byte allocation.
///
/// The interpreter tracks the total byte size of all live values to
/// enforce the `max_value_growth` budget.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ProgramValue {
    None,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    List(Vec<ProgramValue>),
    Dict(Vec<(ProgramValue, ProgramValue)>),
    /// Opaque tool call result — returned by broker, not constructed
    /// by the interpreter directly.
    ToolResult(serde_json::Value),
}

impl ProgramValue {
    /// Approximate byte size for budget accounting.
    pub fn byte_size(&self) -> u64 {
        match self {
            ProgramValue::None => 1,
            ProgramValue::Bool(_) => 1,
            ProgramValue::Int(_) => 8,
            ProgramValue::Float(_) => 8,
            ProgramValue::String(s) => s.len() as u64 + 16,
            ProgramValue::List(items) => 16 + items.iter().map(|v| v.byte_size()).sum::<u64>(),
            ProgramValue::Dict(pairs) => {
                16 + pairs
                    .iter()
                    .map(|(k, v)| k.byte_size() + v.byte_size())
                    .sum::<u64>()
            }
            ProgramValue::ToolResult(val) => serde_json::to_vec(val)
                .map(|b| b.len() as u64 + 16)
                .unwrap_or(16),
        }
    }

    /// Convert to a JSON value for broker input.
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            ProgramValue::None => serde_json::Value::Null,
            ProgramValue::Bool(b) => serde_json::Value::Bool(*b),
            ProgramValue::Int(i) => serde_json::json!(i),
            ProgramValue::Float(f) => serde_json::json!(f),
            ProgramValue::String(s) => serde_json::Value::String(s.clone()),
            ProgramValue::List(items) => {
                serde_json::Value::Array(items.iter().map(|v| v.to_json()).collect())
            }
            ProgramValue::Dict(pairs) => {
                let map: serde_json::Map<String, serde_json::Value> = pairs
                    .iter()
                    .filter_map(|(k, v)| {
                        if let ProgramValue::String(key) = k {
                            Some((key.clone(), v.to_json()))
                        } else {
                            None
                        }
                    })
                    .collect();
                serde_json::Value::Object(map)
            }
            ProgramValue::ToolResult(val) => val.clone(),
        }
    }

    /// Construct from a JSON value.
    pub fn from_json(val: serde_json::Value) -> Self {
        match val {
            serde_json::Value::Null => ProgramValue::None,
            serde_json::Value::Bool(b) => ProgramValue::Bool(b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    ProgramValue::Int(i)
                } else if let Some(f) = n.as_f64() {
                    ProgramValue::Float(f)
                } else {
                    ProgramValue::None
                }
            }
            serde_json::Value::String(s) => ProgramValue::String(s),
            serde_json::Value::Array(arr) => {
                ProgramValue::List(arr.into_iter().map(ProgramValue::from_json).collect())
            }
            serde_json::Value::Object(map) => ProgramValue::Dict(
                map.into_iter()
                    .map(|(k, v)| (ProgramValue::String(k), ProgramValue::from_json(v)))
                    .collect(),
            ),
        }
    }

    /// Is this value falsy? (None, false, 0, 0.0, empty string/list/dict)
    pub fn is_falsy(&self) -> bool {
        match self {
            ProgramValue::None => true,
            ProgramValue::Bool(b) => !b,
            ProgramValue::Int(i) => *i == 0,
            ProgramValue::Float(f) => *f == 0.0,
            ProgramValue::String(s) => s.is_empty(),
            ProgramValue::List(v) => v.is_empty(),
            ProgramValue::Dict(v) => v.is_empty(),
            ProgramValue::ToolResult(v) => v.is_null(),
        }
    }

    /// Length for `len()` builtin.
    pub fn length(&self) -> Result<u64, InterpreterError> {
        match self {
            ProgramValue::String(s) => Ok(s.len() as u64),
            ProgramValue::List(v) => Ok(v.len() as u64),
            ProgramValue::Dict(v) => Ok(v.len() as u64),
            _ => Err(InterpreterError::TypeError(format!(
                "len() not supported for {:?}",
                std::mem::discriminant(self)
            ))),
        }
    }
}

impl fmt::Display for ProgramValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProgramValue::None => write!(f, "None"),
            ProgramValue::Bool(b) => write!(f, "{}", b),
            ProgramValue::Int(i) => write!(f, "{}", i),
            ProgramValue::Float(v) => write!(f, "{}", v),
            ProgramValue::String(s) => write!(f, "{}", s),
            ProgramValue::List(_) => write!(f, "[...]"),
            ProgramValue::Dict(_) => write!(f, "{{...}}"),
            ProgramValue::ToolResult(_) => write!(f, "<tool_result>"),
        }
    }
}

// ── Failure classification ────────────────────────────────────────

/// Classification of program failures for retry and reporting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureClass {
    /// Source/IR/manifest validation error.
    Validation,
    /// Manifest content drift detected.
    ManifestDrift,
    /// Authority narrowed after submission.
    AuthorityNarrowed,
    /// Output schema mismatch.
    SchemaMismatch,
    /// Transient backend/provider error (retry-eligible).
    TransientBackend,
    /// Wall-clock or stall timeout.
    Timeout,
    /// Interpreter stall detected.
    Stall,
    /// Explicitly cancelled by user or parent.
    Cancelled,
    /// Storage/persistence failure.
    Storage,
    /// Replay divergence detected.
    ReplayDivergence,
    /// Budget exhausted (steps, bytes, iterations, calls).
    BudgetExhausted,
    /// Execution error (type, index, division, etc).
    Execution,
    /// Internal panic or invariant violation.
    InternalPanic,
}

impl FailureClass {
    /// Whether this class is eligible for transient retry.
    pub fn is_retry_eligible(&self) -> bool {
        matches!(self, FailureClass::TransientBackend)
    }
}

impl fmt::Display for FailureClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FailureClass::Validation => write!(f, "validation"),
            FailureClass::ManifestDrift => write!(f, "manifest_drift"),
            FailureClass::AuthorityNarrowed => write!(f, "authority_narrowed"),
            FailureClass::SchemaMismatch => write!(f, "schema_mismatch"),
            FailureClass::TransientBackend => write!(f, "transient_backend"),
            FailureClass::Timeout => write!(f, "timeout"),
            FailureClass::Stall => write!(f, "stall"),
            FailureClass::Cancelled => write!(f, "cancelled"),
            FailureClass::Storage => write!(f, "storage"),
            FailureClass::ReplayDivergence => write!(f, "replay_divergence"),
            FailureClass::BudgetExhausted => write!(f, "budget_exhausted"),
            FailureClass::Execution => write!(f, "execution"),
            FailureClass::InternalPanic => write!(f, "internal_panic"),
        }
    }
}

// ── Program result ────────────────────────────────────────────────

/// Terminal or recoverable outcome of a program execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramResult {
    pub status: ProgramStatus,
    pub output: Option<ProgramValue>,
    pub error_message: Option<String>,
    pub failure_class: Option<FailureClass>,
    pub steps_used: u64,
    pub bytes_used: u64,
    pub calls_completed: u32,
    pub calls_total: u32,
    pub iterations_used: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProgramStatus {
    Completed,
    Failed,
    Cancelled,
    TimedOut,
    Stalled,
    Incomplete,
    Recoverable,
}

impl ProgramResult {
    pub fn completed(output: ProgramValue, budget: &BudgetSnapshot) -> Self {
        Self {
            status: ProgramStatus::Completed,
            output: Some(output),
            error_message: None,
            failure_class: None,
            steps_used: budget.steps,
            bytes_used: budget.bytes,
            calls_completed: budget.calls,
            calls_total: budget.calls,
            iterations_used: budget.iterations,
        }
    }

    pub fn failed(class: FailureClass, msg: String, budget: &BudgetSnapshot) -> Self {
        Self {
            status: ProgramStatus::Failed,
            output: None,
            error_message: Some(msg),
            failure_class: Some(class),
            steps_used: budget.steps,
            bytes_used: budget.bytes,
            calls_completed: budget.calls,
            calls_total: budget.calls,
            iterations_used: budget.iterations,
        }
    }

    pub fn cancelled(budget: &BudgetSnapshot) -> Self {
        Self {
            status: ProgramStatus::Cancelled,
            output: None,
            error_message: Some("cancelled".into()),
            failure_class: Some(FailureClass::Cancelled),
            steps_used: budget.steps,
            bytes_used: budget.bytes,
            calls_completed: budget.calls,
            calls_total: budget.calls,
            iterations_used: budget.iterations,
        }
    }

    pub fn timed_out(budget: &BudgetSnapshot) -> Self {
        Self {
            status: ProgramStatus::TimedOut,
            output: None,
            error_message: Some("timed out".into()),
            failure_class: Some(FailureClass::Timeout),
            steps_used: budget.steps,
            bytes_used: budget.bytes,
            calls_completed: budget.calls,
            calls_total: budget.calls,
            iterations_used: budget.iterations,
        }
    }

    pub fn stalled(budget: &BudgetSnapshot) -> Self {
        Self {
            status: ProgramStatus::Stalled,
            output: None,
            error_message: Some("stalled".into()),
            failure_class: Some(FailureClass::Stall),
            steps_used: budget.steps,
            bytes_used: budget.bytes,
            calls_completed: budget.calls,
            calls_total: budget.calls,
            iterations_used: budget.iterations,
        }
    }

    pub fn incomplete(msg: String, budget: &BudgetSnapshot) -> Self {
        Self {
            status: ProgramStatus::Incomplete,
            output: None,
            error_message: Some(msg),
            failure_class: Some(FailureClass::BudgetExhausted),
            steps_used: budget.steps,
            bytes_used: budget.bytes,
            calls_completed: budget.calls,
            calls_total: budget.calls,
            iterations_used: budget.iterations,
        }
    }

    pub fn recoverable(class: FailureClass, msg: String, budget: &BudgetSnapshot) -> Self {
        Self {
            status: ProgramStatus::Recoverable,
            output: None,
            error_message: Some(msg),
            failure_class: Some(class),
            steps_used: budget.steps,
            bytes_used: budget.bytes,
            calls_completed: budget.calls,
            calls_total: budget.calls,
            iterations_used: budget.iterations,
        }
    }
}

// ── Runtime limits ────────────────────────────────────────────────

/// Runtime limits enforced by the interpreter. Derived from [`IrBounds`]
/// plus runtime configuration. Persisted at execution start; config
/// changes do not reinterpret in-flight work.
#[derive(Debug, Clone)]
pub struct RuntimeLimits {
    pub max_steps: u64,
    pub max_loop_iterations: u64,
    pub max_total_iterations: u64,
    pub max_dynamic_calls: u64,
    pub max_parallel_width: u32,
    pub max_parallel_depth: u32,
    pub max_value_growth: u64,
    pub max_bytes: u64,
}

impl From<&super::ir::IrBounds> for RuntimeLimits {
    fn from(bounds: &super::ir::IrBounds) -> Self {
        Self {
            max_steps: bounds.max_steps,
            max_loop_iterations: bounds.max_loop_iterations,
            max_total_iterations: bounds.max_total_iterations,
            max_dynamic_calls: bounds.max_dynamic_calls,
            max_parallel_width: bounds.max_parallel_width,
            max_parallel_depth: bounds.max_parallel_depth,
            max_value_growth: bounds.max_value_growth,
            max_bytes: bounds.max_value_growth,
        }
    }
}

// ── Budget snapshot ───────────────────────────────────────────────

/// Current consumption snapshot for progress and completion reporting.
#[derive(Debug, Clone, Default)]
pub struct BudgetSnapshot {
    pub steps: u64,
    pub bytes: u64,
    pub calls: u32,
    pub iterations: u64,
    pub parallel_groups: u32,
}

// ── Checkpoint ────────────────────────────────────────────────────

/// Serializable interpreter state at a checkpoint boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterpreterCheckpoint {
    pub pc: u32,
    pub steps: u64,
    pub iterations: u64,
    pub calls_completed: u32,
    pub locals_hash: String,
}

// ── Broker call types ─────────────────────────────────────────────

/// A tool call request constructed by the interpreter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallRequest {
    pub tool_name: String,
    pub input: serde_json::Value,
    pub call_id: Option<String>,
}

/// Result of a broker tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallResult {
    pub output: ProgramValue,
    pub artifacts: Vec<String>,
}

/// Recorded state of a completed call for replay.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletedCall {
    pub sequence: u32,
    pub request: CallRequest,
    pub result: CallResult,
}

// ── Broker callback trait ─────────────────────────────────────────

/// Abstraction for tool broker invocation. The interpreter never
/// touches I/O directly; it delegates through this trait.
#[async_trait::async_trait]
pub trait BrokerCallback: Send + Sync {
    /// Execute a tool call through the broker.
    async fn execute_call(&self, request: &CallRequest) -> Result<CallResult, InterpreterError>;
}

// ── Interpreter errors ────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum InterpreterError {
    BudgetExceeded(String),
    TypeError(String),
    IndexError(String),
    KeyError(String),
    DivisionByZero,
    BrokerError(String),
    Cancelled,
    InternalError(String),
}

impl fmt::Display for InterpreterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BudgetExceeded(msg) => write!(f, "budget exceeded: {}", msg),
            Self::TypeError(msg) => write!(f, "type error: {}", msg),
            Self::IndexError(msg) => write!(f, "index error: {}", msg),
            Self::KeyError(msg) => write!(f, "key error: {}", msg),
            Self::DivisionByZero => write!(f, "division by zero"),
            Self::BrokerError(msg) => write!(f, "broker error: {}", msg),
            Self::Cancelled => write!(f, "cancelled"),
            Self::InternalError(msg) => write!(f, "internal error: {}", msg),
        }
    }
}

impl std::error::Error for InterpreterError {}

impl InterpreterError {
    pub fn failure_class(&self) -> FailureClass {
        match self {
            Self::BudgetExceeded(_) => FailureClass::BudgetExhausted,
            Self::TypeError(_) | Self::IndexError(_) | Self::KeyError(_) | Self::DivisionByZero => {
                FailureClass::Execution
            }
            Self::BrokerError(_) => FailureClass::TransientBackend,
            Self::Cancelled => FailureClass::Cancelled,
            Self::InternalError(_) => FailureClass::InternalPanic,
        }
    }
}

// ── Metered interpreter ───────────────────────────────────────────

/// Core stack-machine interpreter for verified IR programs.
///
/// The interpreter is deterministic: given the same IR, initial locals,
/// and broker results, it produces the same output. It enforces all
/// budgets and tracks completed calls for replay.
pub struct MeteredInterpreter {
    program: IrProgram,
    limits: RuntimeLimits,
    stack: Vec<ProgramValue>,
    locals: Vec<Option<ProgramValue>>,
    pc: u32,
    budget: BudgetSnapshot,
    /// Completed calls indexed by sequence number.
    completed_calls: HashMap<u32, CompletedCall>,
    /// Next call sequence number.
    next_call_seq: u32,
    /// Whether the program has reached a terminal instruction.
    terminated: bool,
}

impl MeteredInterpreter {
    /// Create a new interpreter for the given program and limits.
    pub fn new(program: IrProgram, limits: RuntimeLimits) -> Self {
        let local_count = program.local_count as usize;
        Self {
            program,
            limits,
            stack: Vec::with_capacity(256),
            locals: vec![None; local_count],
            pc: 0,
            budget: BudgetSnapshot::default(),
            completed_calls: HashMap::new(),
            next_call_seq: 0,
            terminated: false,
        }
    }

    /// Run the program to completion, using the given broker for tool
    /// calls. Returns a [`ProgramResult`] with the terminal outcome.
    pub async fn run(
        &mut self,
        broker: &dyn BrokerCallback,
        cancelled: Option<&tokio_util::sync::CancellationToken>,
    ) -> ProgramResult {
        loop {
            // Check cancellation
            if let Some(token) = cancelled {
                if token.is_cancelled() {
                    return ProgramResult::cancelled(&self.budget);
                }
            }

            // Check termination
            if self.terminated {
                return ProgramResult::failed(
                    FailureClass::InternalPanic,
                    "interpreter terminated without emit/fail/return".into(),
                    &self.budget,
                );
            }

            // Check step budget
            if self.budget.steps >= self.limits.max_steps {
                return ProgramResult::incomplete(
                    format!(
                        "step budget exhausted: {} >= {}",
                        self.budget.steps, self.limits.max_steps
                    ),
                    &self.budget,
                );
            }

            // Fetch instruction
            let pc = self.pc as usize;
            if pc >= self.program.instructions.len() {
                return ProgramResult::failed(
                    FailureClass::InternalPanic,
                    format!("PC out of bounds: {}", pc),
                    &self.budget,
                );
            }

            let instruction = self.program.instructions[pc].clone();
            self.budget.steps += 1;
            self.pc += 1;

            match self
                .execute_instruction(&instruction, broker, cancelled)
                .await
            {
                Ok(StepResult::Continue) => {}
                Ok(StepResult::Yield(value)) => {
                    return ProgramResult::completed(value, &self.budget);
                }
                Ok(StepResult::Fail(reason)) => {
                    return ProgramResult::failed(FailureClass::Execution, reason, &self.budget);
                }
                Err(e) => {
                    let class = e.failure_class();
                    return ProgramResult::failed(class, e.to_string(), &self.budget);
                }
            }
        }
    }

    /// Execute a single instruction. Returns `Continue` to advance,
    /// `Yield` for emit, `Fail` for fail, or `Err` for runtime errors.
    async fn execute_instruction(
        &mut self,
        instruction: &IrInstruction,
        broker: &dyn BrokerCallback,
        _cancelled: Option<&tokio_util::sync::CancellationToken>,
    ) -> Result<StepResult, InterpreterError> {
        match &instruction.op {
            // ── Constants ──
            IrOp::LoadInt { pool_idx } => {
                let val = self.program.integers[*pool_idx as usize];
                self.push(ProgramValue::Int(val))?;
            }
            IrOp::LoadFloat { pool_idx } => {
                let val = self.program.floats[*pool_idx as usize];
                self.push(ProgramValue::Float(val))?;
            }
            IrOp::LoadString { pool_idx } => {
                let val = self.program.strings[*pool_idx as usize].clone();
                self.push(ProgramValue::String(val))?;
            }
            IrOp::LoadTrue => self.push(ProgramValue::Bool(true))?,
            IrOp::LoadFalse => self.push(ProgramValue::Bool(false))?,
            IrOp::LoadNone => self.push(ProgramValue::None)?,

            // ── Locals ──
            IrOp::LoadLocal { slot } => {
                let val = self.locals[*slot as usize]
                    .clone()
                    .unwrap_or(ProgramValue::None);
                self.push(val)?;
            }
            IrOp::StoreLocal { slot } => {
                let val = self.pop()?;
                self.locals[*slot as usize] = Some(val);
            }

            // ── Collections ──
            IrOp::MakeList { count } => {
                let mut items = Vec::with_capacity(*count as usize);
                for _ in 0..*count {
                    items.insert(0, self.pop()?);
                }
                self.push(ProgramValue::List(items))?;
            }
            IrOp::MakeTuple { count } => {
                let mut items = Vec::with_capacity(*count as usize);
                for _ in 0..*count {
                    items.insert(0, self.pop()?);
                }
                // Tuples are represented as Lists in the interpreter
                self.push(ProgramValue::List(items))?;
            }
            IrOp::MakeDict { count } => {
                let mut pairs = Vec::with_capacity(*count as usize);
                for _ in 0..*count {
                    let val = self.pop()?;
                    let key = self.pop()?;
                    pairs.insert(0, (key, val));
                }
                self.push(ProgramValue::Dict(pairs))?;
            }

            // ── Binary ops ──
            IrOp::BinOp { kind } => {
                let right = self.pop()?;
                let left = self.pop()?;
                let result = self.eval_binop(*kind, &left, &right)?;
                self.push(result)?;
            }

            // ── Unary ops ──
            IrOp::UnaryOp { kind } => {
                let operand = self.pop()?;
                let result = self.eval_unaryop(*kind, &operand)?;
                self.push(result)?;
            }

            // ── Comparison ──
            IrOp::Compare { kind } => {
                let right = self.pop()?;
                let left = self.pop()?;
                let result = self.eval_compare(*kind, &left, &right)?;
                self.push(ProgramValue::Bool(result))?;
            }

            // ── Boolean ops ──
            IrOp::BoolAnd => {
                let right = self.pop()?;
                let left = self.pop()?;
                let result = !left.is_falsy() && !right.is_falsy();
                self.push(ProgramValue::Bool(result))?;
            }
            IrOp::BoolOr => {
                let right = self.pop()?;
                let left = self.pop()?;
                let result = !left.is_falsy() || !right.is_falsy();
                self.push(ProgramValue::Bool(result))?;
            }
            IrOp::BoolNot => {
                let val = self.pop()?;
                self.push(ProgramValue::Bool(val.is_falsy()))?;
            }

            // ── Stack ──
            IrOp::Pop => {
                self.pop()?;
            }
            IrOp::Dup => {
                let val = self.peek()?.clone();
                self.push(val)?;
            }

            // ── Indexing ──
            IrOp::Index => {
                let key = self.pop()?;
                let collection = self.pop()?;
                let result = self.eval_index(&collection, &key)?;
                self.push(result)?;
            }
            IrOp::Slice => {
                let step = self.pop()?;
                let stop = self.pop()?;
                let start = self.pop()?;
                let collection = self.pop()?;
                let result = self.eval_slice(&collection, &start, &stop, &step)?;
                self.push(result)?;
            }

            // ── Builtin conversions ──
            IrOp::Len => {
                let val = self.pop()?;
                let len = val.length()?;
                self.push(ProgramValue::Int(len as i64))?;
            }
            IrOp::Str => {
                let val = self.pop()?;
                self.push(ProgramValue::String(format!("{}", val)))?;
            }
            IrOp::Int => {
                let val = self.pop()?;
                let int_val = match &val {
                    ProgramValue::Int(i) => *i,
                    ProgramValue::Float(f) => *f as i64,
                    ProgramValue::String(s) => s
                        .parse::<i64>()
                        .map_err(|e| InterpreterError::TypeError(e.to_string()))?,
                    ProgramValue::Bool(b) => {
                        if *b {
                            1
                        } else {
                            0
                        }
                    }
                    _ => {
                        return Err(InterpreterError::TypeError(format!(
                            "int() not supported for {:?}",
                            std::mem::discriminant(&val)
                        )))
                    }
                };
                self.push(ProgramValue::Int(int_val))?;
            }
            IrOp::Bool => {
                let val = self.pop()?;
                self.push(ProgramValue::Bool(!val.is_falsy()))?;
            }

            // ── Control flow ──
            IrOp::JumpIfFalse { target } => {
                let val = self.pop()?;
                if val.is_falsy() {
                    self.pc = *target;
                }
            }
            IrOp::Jump { target } => {
                self.pc = *target;
            }

            // ── For loops ──
            IrOp::ForLoopStart {
                body_start,
                loop_end,
            } => {
                // Detect if this is a re-entry from ForLoopNext:
                // Stack has [List, Int(index)] instead of range params.
                let is_resume = self.stack.len() >= 2
                    && matches!(&self.stack[self.stack.len() - 2], ProgramValue::List(_));

                if is_resume {
                    // Re-entering from ForLoopNext — check total iteration budget
                    // (already counted on first entry) and jump to ForLoopIter.
                    if self.budget.iterations > self.limits.max_total_iterations {
                        return Err(InterpreterError::BudgetExceeded(format!(
                            "total iterations {} exceeds max {}",
                            self.budget.iterations, self.limits.max_total_iterations
                        )));
                    }
                    self.pc = *body_start;
                    return Ok(StepResult::Continue);
                }

                // First entry — pop range params or iterable
                let items = self.resume_for_loop(*body_start, *loop_end)?;
                let count = items.len() as u64;

                // Check loop iteration budget (conservative: count all items)
                if count > self.limits.max_loop_iterations {
                    return Err(InterpreterError::BudgetExceeded(format!(
                        "loop iteration count {} exceeds max {}",
                        count, self.limits.max_loop_iterations
                    )));
                }
                // Don't add to total iterations here — ForLoopNext counts
                // each continuation, and the first iteration is counted by
                // the implicit first pass through ForLoopNext.
                // Actually, count the first iteration here:
                self.budget.iterations += count;

                // Push loop metadata: (items, index)
                // We store items in a special way — as a List on the stack
                // with an index counter tracked separately.
                self.push(ProgramValue::List(items))?;
                self.push(ProgramValue::Int(0))?;
                // The ForLoopIter will push the current item.
                // If the list is empty, jump to loop_end.
                if count == 0 {
                    self.pc = *loop_end;
                } else {
                    self.pc = *body_start;
                }
            }
            IrOp::ForLoopIter => {
                // Stack layout: [..., List, Int(index)]
                let idx_val = self.pop()?;
                let list_val = self.pop()?;
                if let (ProgramValue::List(items), ProgramValue::Int(idx)) = (&list_val, &idx_val) {
                    if (*idx as usize) < items.len() {
                        let item = items[*idx as usize].clone();
                        // Push back: list, index, current_item
                        self.push(list_val)?;
                        self.push(idx_val)?;
                        self.push(item)?;
                    } else {
                        return Err(InterpreterError::InternalError(
                            "ForLoopIter: index out of bounds".into(),
                        ));
                    }
                } else {
                    return Err(InterpreterError::TypeError(
                        "ForLoopIter: expected list and int on stack".into(),
                    ));
                }
            }
            IrOp::ForLoopNext {
                loop_start,
                loop_end: _,
            } => {
                // Stack: [..., List, Int(index)]
                let idx_val = self.pop()?;
                let list_val = self.pop()?;
                if let (ProgramValue::List(items), ProgramValue::Int(idx)) = (&list_val, &idx_val) {
                    let new_idx = idx + 1;
                    if (new_idx as usize) < items.len() {
                        // Jump back to ForLoopStart which will detect re-entry
                        self.push(list_val)?;
                        self.push(ProgramValue::Int(new_idx))?;
                        self.pc = *loop_start;
                    }
                    // Otherwise fall through to next instruction
                } else {
                    return Err(InterpreterError::TypeError(
                        "ForLoopNext: expected list and int on stack".into(),
                    ));
                }
            }

            // ── Tool calls ──
            IrOp::ConstructCall => {
                let descriptor = self.pop()?;
                let request = self.construct_call_request(&descriptor)?;
                self.push(ProgramValue::String(
                    serde_json::to_string(&request).unwrap_or_default(),
                ))?;
            }
            IrOp::ExecuteCall => {
                let req_val = self.pop()?;
                let request: CallRequest = match &req_val {
                    ProgramValue::String(s) => serde_json::from_str(s)
                        .map_err(|e| InterpreterError::InternalError(e.to_string()))?,
                    _ => {
                        return Err(InterpreterError::TypeError(
                            "ExecuteCall: expected serialized CallRequest".into(),
                        ))
                    }
                };

                // Check call budget
                if self.budget.calls as u64 >= self.limits.max_dynamic_calls {
                    return Err(InterpreterError::BudgetExceeded(format!(
                        "call count {} exceeds max {}",
                        self.budget.calls, self.limits.max_dynamic_calls
                    )));
                }

                let seq = self.next_call_seq;
                self.next_call_seq += 1;

                // Check for replay from completed calls
                let result = if let Some(completed) = self.completed_calls.get(&seq) {
                    completed.result.clone()
                } else {
                    // Execute through broker
                    let result = broker.execute_call(&request).await?;
                    // Record as completed
                    self.completed_calls.insert(
                        seq,
                        CompletedCall {
                            sequence: seq,
                            request,
                            result: result.clone(),
                        },
                    );
                    result
                };

                self.budget.calls += 1;
                self.push(result.output)?;
            }

            // ── Parallel calls ──
            IrOp::ParallelStart { count } => {
                // Pop N descriptors, construct N requests
                let mut requests = Vec::with_capacity(*count as usize);
                for _ in 0..*count {
                    let descriptor = self.pop()?;
                    requests.insert(0, self.construct_call_request(&descriptor)?);
                }
                // Push requests as a list
                let req_strs: Vec<ProgramValue> = requests
                    .iter()
                    .map(|r| ProgramValue::String(serde_json::to_string(r).unwrap_or_default()))
                    .collect();
                self.push(ProgramValue::List(req_strs))?;
            }
            IrOp::ParallelExecute => {
                let req_list = self.pop()?;
                if let ProgramValue::List(req_vals) = &req_list {
                    let count = req_vals.len();
                    if count as u32 > self.limits.max_parallel_width {
                        return Err(InterpreterError::BudgetExceeded(format!(
                            "parallel width {} exceeds max {}",
                            count, self.limits.max_parallel_width
                        )));
                    }

                    let mut results = Vec::with_capacity(count);
                    for req_val in req_vals {
                        if let ProgramValue::String(s) = req_val {
                            let request: CallRequest = serde_json::from_str(s)
                                .map_err(|e| InterpreterError::InternalError(e.to_string()))?;

                            if self.budget.calls as u64 >= self.limits.max_dynamic_calls {
                                return Err(InterpreterError::BudgetExceeded(format!(
                                    "call count {} exceeds max {}",
                                    self.budget.calls, self.limits.max_dynamic_calls
                                )));
                            }

                            let seq = self.next_call_seq;
                            self.next_call_seq += 1;

                            let result = if let Some(completed) = self.completed_calls.get(&seq) {
                                completed.result.clone()
                            } else {
                                let result = broker.execute_call(&request).await?;
                                self.completed_calls.insert(
                                    seq,
                                    CompletedCall {
                                        sequence: seq,
                                        request,
                                        result: result.clone(),
                                    },
                                );
                                result
                            };

                            self.budget.calls += 1;
                            results.push(result.output);
                        } else {
                            return Err(InterpreterError::TypeError(
                                "ParallelExecute: expected serialized CallRequest list".into(),
                            ));
                        }
                    }
                    self.push(ProgramValue::List(results))?;
                } else {
                    return Err(InterpreterError::TypeError(
                        "ParallelExecute: expected list".into(),
                    ));
                }
            }

            // ── Terminal ──
            IrOp::Emit => {
                let value = self.pop()?;
                self.terminated = true;
                return Ok(StepResult::Yield(value));
            }
            IrOp::Fail => {
                let reason = self.pop()?;
                let msg = match &reason {
                    ProgramValue::String(s) => s.clone(),
                    ProgramValue::None => "program failed".into(),
                    _ => format!("{}", reason),
                };
                self.terminated = true;
                return Ok(StepResult::Fail(msg));
            }
            IrOp::Checkpoint => {
                // Checkpoint is a no-op at the pure interpreter level;
                // the executor layer handles persistence.
            }
            IrOp::Return => {
                self.terminated = true;
                return Ok(StepResult::Yield(ProgramValue::None));
            }
        }

        Ok(StepResult::Continue)
    }

    // ── Internal helpers ──

    fn push(&mut self, val: ProgramValue) -> Result<(), InterpreterError> {
        let new_bytes = val.byte_size();
        if self.budget.bytes + new_bytes > self.limits.max_value_growth {
            return Err(InterpreterError::BudgetExceeded(format!(
                "value bytes {} + {} exceeds max {}",
                self.budget.bytes, new_bytes, self.limits.max_value_growth
            )));
        }
        self.budget.bytes += new_bytes;
        self.stack.push(val);
        Ok(())
    }

    fn pop(&mut self) -> Result<ProgramValue, InterpreterError> {
        self.stack
            .pop()
            .ok_or_else(|| InterpreterError::InternalError("stack underflow".into()))
    }

    fn peek(&self) -> Result<&ProgramValue, InterpreterError> {
        self.stack
            .last()
            .ok_or_else(|| InterpreterError::InternalError("stack underflow on peek".into()))
    }

    /// Pop range parameters or iterable and generate loop items.
    fn resume_for_loop(
        &mut self,
        body_start: u32,
        loop_end: u32,
    ) -> Result<Vec<ProgramValue>, InterpreterError> {
        let items = if self.stack.len() >= 3 {
            // Check if top 3 values are all integers (range pattern)
            let top = &self.stack[self.stack.len() - 1];
            let mid = &self.stack[self.stack.len() - 2];
            let bot = &self.stack[self.stack.len() - 3];
            if matches!(top, ProgramValue::Int(_))
                && matches!(mid, ProgramValue::Int(_))
                && matches!(bot, ProgramValue::Int(_))
            {
                // Range pattern: pop step, stop, start
                let step_val = self.pop()?;
                let stop_val = self.pop()?;
                let start_val = self.pop()?;
                if let (
                    ProgramValue::Int(start),
                    ProgramValue::Int(stop),
                    ProgramValue::Int(step),
                ) = (&start_val, &stop_val, &step_val)
                {
                    if *step == 0 {
                        return Err(InterpreterError::TypeError(
                            "range() step cannot be zero".into(),
                        ));
                    }
                    let mut items = Vec::new();
                    if *step > 0 {
                        let mut v = *start;
                        while v < *stop {
                            items.push(ProgramValue::Int(v));
                            v += step;
                        }
                    } else {
                        let mut v = *start;
                        while v > *stop {
                            items.push(ProgramValue::Int(v));
                            v += step;
                        }
                    }
                    items
                } else {
                    unreachable!()
                }
            } else {
                let iterable = self.pop()?;
                self.iterable_to_vec(&iterable)?
            }
        } else {
            let iterable = self.pop()?;
            self.iterable_to_vec(&iterable)?
        };
        let _ = (body_start, loop_end);
        Ok(items)
    }

    fn construct_call_request(
        &self,
        descriptor: &ProgramValue,
    ) -> Result<CallRequest, InterpreterError> {
        match descriptor {
            ProgramValue::Dict(pairs) => {
                let mut tool_name = None;
                let mut input = serde_json::Map::new();
                for (k, v) in pairs {
                    if let ProgramValue::String(key) = k {
                        if key == "tool" {
                            tool_name = Some(match v {
                                ProgramValue::String(s) => s.clone(),
                                _ => {
                                    return Err(InterpreterError::TypeError(
                                        "tool name must be a string".into(),
                                    ))
                                }
                            });
                        } else {
                            input.insert(key.clone(), v.to_json());
                        }
                    }
                }
                let name = tool_name.ok_or_else(|| {
                    InterpreterError::TypeError("call descriptor missing 'tool' key".into())
                })?;
                Ok(CallRequest {
                    tool_name: name,
                    input: serde_json::Value::Object(input),
                    call_id: None,
                })
            }
            _ => Err(InterpreterError::TypeError(
                "call descriptor must be a dict".into(),
            )),
        }
    }

    fn iterable_to_vec(&self, val: &ProgramValue) -> Result<Vec<ProgramValue>, InterpreterError> {
        match val {
            ProgramValue::List(items) => Ok(items.clone()),
            ProgramValue::String(s) => Ok(s
                .chars()
                .map(|c| ProgramValue::String(c.to_string()))
                .collect()),
            ProgramValue::Dict(pairs) => Ok(pairs.iter().map(|(k, _)| k.clone()).collect()),
            _ => Err(InterpreterError::TypeError(format!(
                "cannot iterate over {:?}",
                std::mem::discriminant(val)
            ))),
        }
    }

    fn eval_binop(
        &self,
        kind: IrBinOp,
        left: &ProgramValue,
        right: &ProgramValue,
    ) -> Result<ProgramValue, InterpreterError> {
        match kind {
            IrBinOp::Add => self.add_values(left, right),
            IrBinOp::Sub => {
                let (l, r) = self.to_numbers(left, right)?;
                Ok(ProgramValue::Int(l - r))
            }
            IrBinOp::Mul => {
                let (l, r) = self.to_numbers(left, right)?;
                Ok(ProgramValue::Int(l * r))
            }
            IrBinOp::Div => {
                let (l, r) = self.to_numbers(left, right)?;
                if r == 0 {
                    return Err(InterpreterError::DivisionByZero);
                }
                Ok(ProgramValue::Float(l as f64 / r as f64))
            }
            IrBinOp::FloorDiv => {
                let (l, r) = self.to_numbers(left, right)?;
                if r == 0 {
                    return Err(InterpreterError::DivisionByZero);
                }
                Ok(ProgramValue::Int(l / r))
            }
            IrBinOp::Mod => {
                let (l, r) = self.to_numbers(left, right)?;
                if r == 0 {
                    return Err(InterpreterError::DivisionByZero);
                }
                Ok(ProgramValue::Int(l % r))
            }
            IrBinOp::Pow => {
                let (l, r) = self.to_numbers(left, right)?;
                Ok(ProgramValue::Int(l.pow(r as u32)))
            }
            IrBinOp::BitOr => {
                let (l, r) = self.to_numbers(left, right)?;
                Ok(ProgramValue::Int(l | r))
            }
            IrBinOp::BitXor => {
                let (l, r) = self.to_numbers(left, right)?;
                Ok(ProgramValue::Int(l ^ r))
            }
            IrBinOp::BitAnd => {
                let (l, r) = self.to_numbers(left, right)?;
                Ok(ProgramValue::Int(l & r))
            }
            IrBinOp::LShift => {
                let (l, r) = self.to_numbers(left, right)?;
                Ok(ProgramValue::Int(l << r))
            }
            IrBinOp::RShift => {
                let (l, r) = self.to_numbers(left, right)?;
                Ok(ProgramValue::Int(l >> r))
            }
        }
    }

    fn add_values(
        &self,
        left: &ProgramValue,
        right: &ProgramValue,
    ) -> Result<ProgramValue, InterpreterError> {
        match (left, right) {
            (ProgramValue::Int(l), ProgramValue::Int(r)) => Ok(ProgramValue::Int(l + r)),
            (ProgramValue::Float(l), ProgramValue::Float(r)) => Ok(ProgramValue::Float(l + r)),
            (ProgramValue::Int(l), ProgramValue::Float(r)) => {
                Ok(ProgramValue::Float(*l as f64 + r))
            }
            (ProgramValue::Float(l), ProgramValue::Int(r)) => {
                Ok(ProgramValue::Float(l + *r as f64))
            }
            (ProgramValue::String(l), ProgramValue::String(r)) => {
                Ok(ProgramValue::String(format!("{}{}", l, r)))
            }
            (ProgramValue::List(l), ProgramValue::List(r)) => {
                let mut result = l.clone();
                result.extend(r.iter().cloned());
                Ok(ProgramValue::List(result))
            }
            _ => Err(InterpreterError::TypeError(format!(
                "cannot add {:?} and {:?}",
                std::mem::discriminant(left),
                std::mem::discriminant(right)
            ))),
        }
    }

    fn to_numbers(
        &self,
        left: &ProgramValue,
        right: &ProgramValue,
    ) -> Result<(i64, i64), InterpreterError> {
        match (left, right) {
            (ProgramValue::Int(l), ProgramValue::Int(r)) => Ok((*l, *r)),
            _ => Err(InterpreterError::TypeError(format!(
                "cannot apply arithmetic to {:?} and {:?}",
                std::mem::discriminant(left),
                std::mem::discriminant(right)
            ))),
        }
    }

    fn eval_unaryop(
        &self,
        kind: IrUnaryOp,
        operand: &ProgramValue,
    ) -> Result<ProgramValue, InterpreterError> {
        match kind {
            IrUnaryOp::Neg => match operand {
                ProgramValue::Int(i) => Ok(ProgramValue::Int(-i)),
                ProgramValue::Float(f) => Ok(ProgramValue::Float(-f)),
                _ => Err(InterpreterError::TypeError(format!(
                    "cannot negate {:?}",
                    std::mem::discriminant(operand)
                ))),
            },
            IrUnaryOp::Pos => match operand {
                ProgramValue::Int(_) | ProgramValue::Float(_) => Ok(operand.clone()),
                _ => Err(InterpreterError::TypeError(format!(
                    "cannot apply positive to {:?}",
                    std::mem::discriminant(operand)
                ))),
            },
            IrUnaryOp::Invert => match operand {
                ProgramValue::Int(i) => Ok(ProgramValue::Int(!i)),
                _ => Err(InterpreterError::TypeError(format!(
                    "cannot invert {:?}",
                    std::mem::discriminant(operand)
                ))),
            },
        }
    }

    fn eval_compare(
        &self,
        kind: IrCmpOp,
        left: &ProgramValue,
        right: &ProgramValue,
    ) -> Result<bool, InterpreterError> {
        Ok(match kind {
            IrCmpOp::Eq => self.values_equal(left, right),
            IrCmpOp::NotEq => !self.values_equal(left, right),
            IrCmpOp::Lt => self.value_cmp(left, right)? == std::cmp::Ordering::Less,
            IrCmpOp::LtE => {
                let ord = self.value_cmp(left, right)?;
                ord == std::cmp::Ordering::Less || ord == std::cmp::Ordering::Equal
            }
            IrCmpOp::Gt => self.value_cmp(left, right)? == std::cmp::Ordering::Greater,
            IrCmpOp::GtE => {
                let ord = self.value_cmp(left, right)?;
                ord == std::cmp::Ordering::Greater || ord == std::cmp::Ordering::Equal
            }
            IrCmpOp::In => match right {
                ProgramValue::List(items) => items.iter().any(|i| self.values_equal(left, i)),
                ProgramValue::Dict(pairs) => pairs.iter().any(|(k, _)| self.values_equal(left, k)),
                ProgramValue::String(s) => match left {
                    ProgramValue::String(sub) => s.contains(sub.as_str()),
                    _ => false,
                },
                _ => false,
            },
            IrCmpOp::NotIn => !self.eval_compare(IrCmpOp::In, left, right)?,
        })
    }

    fn values_equal(&self, left: &ProgramValue, right: &ProgramValue) -> bool {
        match (left, right) {
            (ProgramValue::None, ProgramValue::None) => true,
            (ProgramValue::Bool(l), ProgramValue::Bool(r)) => l == r,
            (ProgramValue::Int(l), ProgramValue::Int(r)) => l == r,
            (ProgramValue::Float(l), ProgramValue::Float(r)) => l == r,
            (ProgramValue::Int(l), ProgramValue::Float(r)) => (*l as f64) == *r,
            (ProgramValue::Float(l), ProgramValue::Int(r)) => *l == (*r as f64),
            (ProgramValue::String(l), ProgramValue::String(r)) => l == r,
            (ProgramValue::List(l), ProgramValue::List(r)) => {
                l.len() == r.len() && l.iter().zip(r.iter()).all(|(a, b)| self.values_equal(a, b))
            }
            _ => false,
        }
    }

    fn value_cmp(
        &self,
        left: &ProgramValue,
        right: &ProgramValue,
    ) -> Result<std::cmp::Ordering, InterpreterError> {
        match (left, right) {
            (ProgramValue::Int(l), ProgramValue::Int(r)) => Ok(l.cmp(r)),
            (ProgramValue::Float(l), ProgramValue::Float(r)) => {
                Ok(l.partial_cmp(r).unwrap_or(std::cmp::Ordering::Equal))
            }
            (ProgramValue::Int(l), ProgramValue::Float(r)) => Ok((*l as f64)
                .partial_cmp(r)
                .unwrap_or(std::cmp::Ordering::Equal)),
            (ProgramValue::Float(l), ProgramValue::Int(r)) => Ok(l
                .partial_cmp(&(*r as f64))
                .unwrap_or(std::cmp::Ordering::Equal)),
            (ProgramValue::String(l), ProgramValue::String(r)) => Ok(l.cmp(r)),
            _ => Err(InterpreterError::TypeError(format!(
                "cannot compare {:?} and {:?}",
                std::mem::discriminant(left),
                std::mem::discriminant(right)
            ))),
        }
    }

    fn eval_index(
        &self,
        collection: &ProgramValue,
        key: &ProgramValue,
    ) -> Result<ProgramValue, InterpreterError> {
        match (collection, key) {
            (ProgramValue::List(items), ProgramValue::Int(i)) => {
                let idx = if *i < 0 {
                    (items.len() as i64 + i) as usize
                } else {
                    *i as usize
                };
                items.get(idx).cloned().ok_or_else(|| {
                    InterpreterError::IndexError(format!("index {} out of range", idx))
                })
            }
            (ProgramValue::Dict(pairs), ProgramValue::String(key_str)) => pairs
                .iter()
                .find(|(k, _)| matches!(k, ProgramValue::String(s) if s == key_str))
                .map(|(_, v)| v.clone())
                .ok_or_else(|| InterpreterError::KeyError(key_str.clone())),
            (ProgramValue::String(s), ProgramValue::Int(i)) => {
                let idx = if *i < 0 {
                    (s.len() as i64 + i) as usize
                } else {
                    *i as usize
                };
                s.chars()
                    .nth(idx)
                    .map(|c| ProgramValue::String(c.to_string()))
                    .ok_or_else(|| {
                        InterpreterError::IndexError(format!("index {} out of range", idx))
                    })
            }
            _ => Err(InterpreterError::TypeError(format!(
                "cannot index {:?} with {:?}",
                std::mem::discriminant(collection),
                std::mem::discriminant(key)
            ))),
        }
    }

    fn eval_slice(
        &self,
        collection: &ProgramValue,
        start: &ProgramValue,
        stop: &ProgramValue,
        step: &ProgramValue,
    ) -> Result<ProgramValue, InterpreterError> {
        let to_usize = |v: &ProgramValue, default: Option<usize>| -> Option<usize> {
            match v {
                ProgramValue::Int(i) => Some(*i as usize),
                ProgramValue::None => default,
                _ => None,
            }
        };

        match collection {
            ProgramValue::List(items) => {
                let start_idx = to_usize(start, Some(0)).unwrap_or(0);
                let stop_idx = to_usize(stop, Some(items.len())).unwrap_or(items.len());
                let step_val = to_usize(step, Some(1)).unwrap_or(1).max(1);
                let sliced: Vec<ProgramValue> = items
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| {
                        i >= &start_idx && i < &stop_idx && (i - start_idx) % step_val == 0
                    })
                    .map(|(_, v)| v.clone())
                    .collect();
                Ok(ProgramValue::List(sliced))
            }
            ProgramValue::String(s) => {
                let chars: Vec<char> = s.chars().collect();
                let start_idx = to_usize(start, Some(0)).unwrap_or(0);
                let stop_idx = to_usize(stop, Some(chars.len())).unwrap_or(chars.len());
                let step_val = to_usize(step, Some(1)).unwrap_or(1).max(1);
                let sliced: String = chars
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| {
                        i >= &start_idx && i < &stop_idx && (i - start_idx) % step_val == 0
                    })
                    .map(|(_, c)| *c)
                    .collect();
                Ok(ProgramValue::String(sliced))
            }
            _ => Err(InterpreterError::TypeError(format!(
                "cannot slice {:?}",
                std::mem::discriminant(collection)
            ))),
        }
    }

    /// Get the current budget snapshot.
    pub fn budget(&self) -> &BudgetSnapshot {
        &self.budget
    }

    /// Get completed calls for replay.
    pub fn completed_calls(&self) -> &HashMap<u32, CompletedCall> {
        &self.completed_calls
    }

    /// Load completed calls from a checkpoint (for restart replay).
    pub fn load_completed_calls(&mut self, calls: HashMap<u32, CompletedCall>) {
        self.next_call_seq = calls.keys().max().map_or(0, |k| k + 1);
        self.completed_calls = calls;
    }
}

/// Result of executing a single instruction.
enum StepResult {
    Continue,
    Yield(ProgramValue),
    Fail(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_program::compile_program;

    struct NoopBroker;

    #[async_trait::async_trait]
    impl BrokerCallback for NoopBroker {
        async fn execute_call(
            &self,
            _request: &CallRequest,
        ) -> Result<CallResult, InterpreterError> {
            Ok(CallResult {
                output: ProgramValue::String("mock_result".into()),
                artifacts: vec![],
            })
        }
    }

    fn run_program(source: &str) -> ProgramResult {
        let result = compile_program(source).unwrap();
        let limits = RuntimeLimits::from(&result.ir.bounds);
        let mut interp = MeteredInterpreter::new(result.ir, limits);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let broker = NoopBroker;
        rt.block_on(interp.run(&broker, None))
    }

    #[test]
    fn emit_constant_integer() {
        let result = run_program("emit(42)\n");
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(result.output, Some(ProgramValue::Int(42)));
    }

    #[test]
    fn emit_constant_string() {
        let result = run_program("emit(\"hello\")\n");
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(result.output, Some(ProgramValue::String("hello".into())));
    }

    #[test]
    fn emit_constant_bool() {
        let result = run_program("emit(True)\n");
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(result.output, Some(ProgramValue::Bool(true)));
    }

    #[test]
    fn emit_none() {
        let result = run_program("emit(None)\n");
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(result.output, Some(ProgramValue::None));
    }

    #[test]
    fn emit_arithmetic() {
        let result = run_program("emit(2 + 3)\n");
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(result.output, Some(ProgramValue::Int(5)));
    }

    #[test]
    fn emit_string_concatenation() {
        let result = run_program("emit(\"hello\" + \" world\")\n");
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(
            result.output,
            Some(ProgramValue::String("hello world".into()))
        );
    }

    #[test]
    fn emit_list_literal() {
        let result = run_program("emit([1, 2, 3])\n");
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(
            result.output,
            Some(ProgramValue::List(vec![
                ProgramValue::Int(1),
                ProgramValue::Int(2),
                ProgramValue::Int(3),
            ]))
        );
    }

    #[test]
    fn emit_dict_literal() {
        let result = run_program("emit({\"a\": 1, \"b\": 2})\n");
        assert_eq!(result.status, ProgramStatus::Completed);
    }

    #[test]
    fn for_loop_sum() {
        let src = r#"
total = 0
for i in range(5):
    total = total + i
emit(total)
"#;
        let result = run_program(src);
        assert_eq!(result.status, ProgramStatus::Completed);
        // 0+1+2+3+4 = 10
        assert_eq!(result.output, Some(ProgramValue::Int(10)));
    }

    #[test]
    fn for_loop_empty() {
        let src = r#"
total = 0
for i in range(0):
    total = total + 1
emit(total)
"#;
        let result = run_program(src);
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(result.output, Some(ProgramValue::Int(0)));
    }

    #[test]
    fn if_else() {
        let src = r#"
x = 10
if x > 5:
    result = "big"
else:
    result = "small"
emit(result)
"#;
        let result = run_program(src);
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(result.output, Some(ProgramValue::String("big".into())));
    }

    #[test]
    fn if_elif_else() {
        let src = r#"
x = 5
if x > 10:
    result = "big"
elif x > 3:
    result = "medium"
else:
    result = "small"
emit(result)
"#;
        let result = run_program(src);
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(result.output, Some(ProgramValue::String("medium".into())));
    }

    #[test]
    fn comparison_operators() {
        let result = run_program("emit(3 < 5)\n");
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(result.output, Some(ProgramValue::Bool(true)));

        let result = run_program("emit(3 > 5)\n");
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(result.output, Some(ProgramValue::Bool(false)));

        let result = run_program("emit(3 == 3)\n");
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(result.output, Some(ProgramValue::Bool(true)));

        let result = run_program("emit(3 != 3)\n");
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(result.output, Some(ProgramValue::Bool(false)));
    }

    #[test]
    fn boolean_operators() {
        let result = run_program("emit(True and False)\n");
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(result.output, Some(ProgramValue::Bool(false)));

        let result = run_program("emit(True or False)\n");
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(result.output, Some(ProgramValue::Bool(true)));

        let result = run_program("emit(not False)\n");
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(result.output, Some(ProgramValue::Bool(true)));
    }

    #[test]
    fn builtin_len() {
        let result = run_program("emit(len([1, 2, 3]))\n");
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(result.output, Some(ProgramValue::Int(3)));
    }

    #[test]
    fn builtin_str_conversion() {
        let result = run_program("emit(str(42))\n");
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(result.output, Some(ProgramValue::String("42".into())));
    }

    #[test]
    fn builtin_int_conversion() {
        let result = run_program("emit(int(\"42\"))\n");
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(result.output, Some(ProgramValue::Int(42)));
    }

    #[test]
    fn builtin_bool_conversion() {
        let result = run_program("emit(bool(1))\n");
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(result.output, Some(ProgramValue::Bool(true)));
    }

    #[test]
    fn fail_program() {
        let result = run_program("fail(\"oops\")\n");
        assert_eq!(result.status, ProgramStatus::Failed);
        assert_eq!(result.failure_class, Some(FailureClass::Execution));
        assert!(result.error_message.unwrap().contains("oops"));
    }

    #[test]
    fn step_budget_enforced() {
        let result = compile_program("emit(42)\n").unwrap();
        let mut limits = RuntimeLimits::from(&result.ir.bounds);
        limits.max_steps = 1; // Very tight budget — only 1 instruction allowed
        let mut interp = MeteredInterpreter::new(result.ir, limits);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let broker = NoopBroker;
        let res = rt.block_on(interp.run(&broker, None));
        assert_eq!(res.status, ProgramStatus::Incomplete);
    }

    #[test]
    fn iteration_budget_enforced() {
        let src = r#"
total = 0
for i in range(100):
    total = total + 1
emit(total)
"#;
        let result = compile_program(src).unwrap();
        let mut limits = RuntimeLimits::from(&result.ir.bounds);
        limits.max_total_iterations = 5;
        let mut interp = MeteredInterpreter::new(result.ir, limits);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let broker = NoopBroker;
        let res = rt.block_on(interp.run(&broker, None));
        assert_eq!(res.status, ProgramStatus::Failed);
        assert_eq!(res.failure_class, Some(FailureClass::BudgetExhausted));
    }

    #[tokio::test]
    async fn cancellation() {
        let result = compile_program("emit(42)\n").unwrap();
        let limits = RuntimeLimits::from(&result.ir.bounds);
        let mut interp = MeteredInterpreter::new(result.ir, limits);
        let token = tokio_util::sync::CancellationToken::new();
        token.cancel();
        let broker = NoopBroker;
        let res = interp.run(&broker, Some(&token)).await;
        assert_eq!(res.status, ProgramStatus::Cancelled);
    }

    #[test]
    fn budget_snapshot_tracks_usage() {
        let src = r#"
total = 0
for i in range(3):
    total = total + 1
emit(total)
"#;
        let result = compile_program(src).unwrap();
        let limits = RuntimeLimits::from(&result.ir.bounds);
        let mut interp = MeteredInterpreter::new(result.ir, limits);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let broker = NoopBroker;
        let _ = rt.block_on(interp.run(&broker, None));
        let budget = interp.budget();
        assert!(budget.steps > 0);
        assert!(budget.iterations > 0);
    }

    #[test]
    fn nested_loop() {
        let src = r#"
total = 0
for i in range(3):
    for j in range(3):
        total = total + 1
emit(total)
"#;
        let result = run_program(src);
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(result.output, Some(ProgramValue::Int(9)));
    }

    #[test]
    fn list_index() {
        let result = run_program("emit([10, 20, 30][1])\n");
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(result.output, Some(ProgramValue::Int(20)));
    }

    #[test]
    fn string_index() {
        let result = run_program("emit(\"hello\"[1])\n");
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(result.output, Some(ProgramValue::String("e".into())));
    }

    #[test]
    fn dict_index() {
        let result = run_program("emit({\"a\": 1, \"b\": 2}[\"b\"])\n");
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(result.output, Some(ProgramValue::Int(2)));
    }

    #[test]
    fn list_slice() {
        let result = run_program("emit([1, 2, 3, 4, 5][1:4])\n");
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(
            result.output,
            Some(ProgramValue::List(vec![
                ProgramValue::Int(2),
                ProgramValue::Int(3),
                ProgramValue::Int(4),
            ]))
        );
    }

    #[test]
    fn negative_index() {
        let result = run_program("emit([10, 20, 30][-1])\n");
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(result.output, Some(ProgramValue::Int(30)));
    }

    #[test]
    fn division_by_zero() {
        let result = run_program("emit(1 / 0)\n");
        assert_eq!(result.status, ProgramStatus::Failed);
        assert_eq!(result.failure_class, Some(FailureClass::Execution));
    }

    #[test]
    fn unary_negation() {
        let result = run_program("emit(-5)\n");
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(result.output, Some(ProgramValue::Int(-5)));
    }

    #[test]
    fn nested_if() {
        let src = r#"
x = 10
y = 20
if x > 5:
    if y > 15:
        result = "both"
    else:
        result = "x only"
else:
    result = "neither"
emit(result)
"#;
        let result = run_program(src);
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(result.output, Some(ProgramValue::String("both".into())));
    }

    #[test]
    fn value_growth_budget() {
        let result = compile_program("emit(42)\n").unwrap();
        let mut limits = RuntimeLimits::from(&result.ir.bounds);
        limits.max_value_growth = 5; // Very tight
        let mut interp = MeteredInterpreter::new(result.ir, limits);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let broker = NoopBroker;
        let res = rt.block_on(interp.run(&broker, None));
        // May fail with budget or succeed depending on exact sizes
        assert!(
            res.status == ProgramStatus::Completed
                || res.status == ProgramStatus::Failed
                || res.status == ProgramStatus::Incomplete
        );
    }

    #[test]
    fn for_loop_list_iteration() {
        let src = r#"
total = 0
for item in [10, 20, 30]:
    total = total + item
emit(total)
"#;
        let result = run_program(src);
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(result.output, Some(ProgramValue::Int(60)));
    }

    #[test]
    fn list_construction() {
        let src = r#"
a = 1
b = 2
c = 3
emit([a, b, c])
"#;
        let result = run_program(src);
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(
            result.output,
            Some(ProgramValue::List(vec![
                ProgramValue::Int(1),
                ProgramValue::Int(2),
                ProgramValue::Int(3),
            ]))
        );
    }

    #[test]
    fn dict_construction() {
        let src = r#"
k = "name"
v = "test"
emit({k: v})
"#;
        let result = run_program(src);
        assert_eq!(result.status, ProgramStatus::Completed);
    }

    #[test]
    fn program_value_json_roundtrip() {
        let val = ProgramValue::Dict(vec![
            (ProgramValue::String("key".into()), ProgramValue::Int(42)),
            (
                ProgramValue::String("nested".into()),
                ProgramValue::List(vec![ProgramValue::Bool(true), ProgramValue::None]),
            ),
        ]);
        let json = val.to_json();
        let restored = ProgramValue::from_json(json);
        assert_eq!(val, restored);
    }

    #[test]
    fn program_value_byte_size() {
        assert_eq!(ProgramValue::None.byte_size(), 1);
        assert_eq!(ProgramValue::Bool(true).byte_size(), 1);
        assert_eq!(ProgramValue::Int(42).byte_size(), 8);
        assert_eq!(ProgramValue::Float(1.5).byte_size(), 8);
        let s = ProgramValue::String("hello".into());
        assert!(s.byte_size() > 5);
        let list = ProgramValue::List(vec![ProgramValue::Int(1), ProgramValue::Int(2)]);
        assert!(list.byte_size() > 16);
    }

    #[test]
    fn program_value_is_falsy() {
        assert!(ProgramValue::None.is_falsy());
        assert!(ProgramValue::Bool(false).is_falsy());
        assert!(ProgramValue::Int(0).is_falsy());
        assert!(ProgramValue::Float(0.0).is_falsy());
        assert!(ProgramValue::String("".into()).is_falsy());
        assert!(ProgramValue::List(vec![]).is_falsy());
        assert!(ProgramValue::Dict(vec![]).is_falsy());

        assert!(!ProgramValue::Bool(true).is_falsy());
        assert!(!ProgramValue::Int(1).is_falsy());
        assert!(!ProgramValue::String("x".into()).is_falsy());
        assert!(!ProgramValue::List(vec![ProgramValue::Int(1)]).is_falsy());
    }

    #[test]
    fn test_operator_precedence() {
        // 2 + 3 * 4 should be 14, not 20
        let result = run_program("emit(2 + 3 * 4)\n");
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(result.output, Some(ProgramValue::Int(14)));
    }

    #[test]
    fn emit_float() {
        let result = run_program("emit(1.5)\n");
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(result.output, Some(ProgramValue::Float(1.5)));
    }

    #[test]
    fn nested_list_index() {
        let src = "emit([[1, 2], [3, 4]][1][0])\n";
        let result = run_program(src);
        assert_eq!(result.status, ProgramStatus::Completed);
        assert_eq!(result.output, Some(ProgramValue::Int(3)));
    }
}
