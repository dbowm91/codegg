//! Restricted-Python frontend for Tool Programs.
//!
//! This module provides parsing, validation, static-bound analysis, and
//! IR compilation for the Tool Program restricted-Python language subset.
//! The pipeline is parse-only: it never executes source or loads modules.
//!
//! # Pipeline
//!
//! ```text
//! source bytes → parse → normalized AST → validate → static bounds → compile IR → verify IR
//! ```
//!
//! # Invariants
//!
//! - Parsing never executes source or spawns subprocesses.
//! - Every accepted program has conservative finite bounds.
//! - IR is deterministic, versioned, verified, and content-addressed.
//! - Unsupported syntax fails closed with bounded diagnostics.

pub mod ast;
pub mod compiler;
pub mod diagnostics;
pub mod guards;
pub mod interpreter;
pub mod ir;
pub mod ir_verifier;
pub mod parser;
pub mod static_bounds;
pub mod store;
pub mod validator;

pub use ast::{Expr, Program, Slice, Stmt};
pub use compiler::compile;
pub use diagnostics::{Diagnostic, DiagnosticCode, SourceSpan};
pub use guards::{cpython_execution_is_forbidden, ToolProgramCompilerIsParseOnly};
pub use interpreter::{
    BrokerCallback, BudgetSnapshot, CallRequest, CallResult, CompletedCall, FailureClass,
    InterpreterCheckpoint, InterpreterError, MeteredInterpreter, ProgramResult, ProgramStatus,
    ProgramValue, RunConfig, RuntimeLimits,
};
pub use ir::{IrProgram, IrVersion, COMPILER_VERSION, LANGUAGE_VERSION, PARSER_VERSION};
pub use ir_verifier::verify_ir;
pub use parser::parse_source;
pub use static_bounds::StaticBounds;
pub use store::{deserialize_ir, serialize_ir, verify_ir_integrity, ProgramStore};
pub use validator::validate;

use std::fmt;

/// Top-level error for the Tool Program frontend pipeline.
#[derive(Debug, Clone)]
pub enum ToolProgramError {
    Parse(diagnostics::Diagnostic),
    Validate(diagnostics::Diagnostic),
    Bounds(diagnostics::Diagnostic),
    Compile(diagnostics::Diagnostic),
    Verify(diagnostics::Diagnostic),
}

impl fmt::Display for ToolProgramError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(d)
            | Self::Validate(d)
            | Self::Bounds(d)
            | Self::Compile(d)
            | Self::Verify(d) => {
                write!(f, "{}", d)
            }
        }
    }
}

impl std::error::Error for ToolProgramError {}

impl ToolProgramError {
    /// Return the diagnostic code for this error.
    pub fn code(&self) -> DiagnosticCode {
        match self {
            Self::Parse(d)
            | Self::Validate(d)
            | Self::Bounds(d)
            | Self::Compile(d)
            | Self::Verify(d) => d.code,
        }
    }

    /// Return the diagnostic for this error.
    pub fn diagnostic(&self) -> &diagnostics::Diagnostic {
        match self {
            Self::Parse(d)
            | Self::Validate(d)
            | Self::Bounds(d)
            | Self::Compile(d)
            | Self::Verify(d) => d,
        }
    }
}

/// Full compilation result: the IR plus all diagnostics produced during the pipeline.
#[derive(Debug, Clone)]
pub struct CompilationResult {
    pub ir: ir::IrProgram,
    pub warnings: Vec<diagnostics::Diagnostic>,
}

/// Convenience: run the full parse → validate → bounds → compile → verify pipeline.
///
/// Returns `Ok(CompilationResult)` on success or the first error encountered.
pub fn compile_program(source: &str) -> Result<CompilationResult, ToolProgramError> {
    use sha2::{Digest, Sha256};
    let ast = parse_source(source)?;
    validate(&ast)?;
    let bounds = static_bounds::analyze(&ast)?;
    let source_hash = {
        let mut hasher = Sha256::new();
        hasher.update(source.as_bytes());
        format!("{:x}", hasher.finalize())
    };
    let ir = compiler::compile_with_hashes(&ast, &bounds, &source_hash, "", "")?;
    ir_verifier::verify_ir(&ir)?;
    Ok(CompilationResult {
        ir,
        warnings: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile_simple_emit() {
        let src = r#"
emit({"result": "ok"})
"#;
        let result = compile_program(src);
        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
    }

    #[test]
    fn compile_for_loop() {
        let src = r#"
total = 0
for i in range(5):
    total = total + 1
emit({"total": total})
"#;
        let result = compile_program(src);
        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
    }

    #[test]
    fn compile_parallel_calls() {
        let src = r#"
reads = parallel(
    {"tool": "read_file", "path": "a.py"},
    {"tool": "read_file", "path": "b.py"},
)
emit({"results": reads})
"#;
        let result = compile_program(src);
        assert!(result.is_ok(), "expected Ok, got {:?}", result.err());
    }

    #[test]
    fn reject_import() {
        let src = "import os\n";
        let result = compile_program(src);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code(), DiagnosticCode::UnsupportedSyntax);
    }

    #[test]
    fn reject_while() {
        let src = "while True:\n    pass\n";
        let result = compile_program(src);
        assert!(result.is_err());
    }

    #[test]
    fn reject_function_def() {
        let src = "def foo():\n    pass\n";
        let result = compile_program(src);
        assert!(result.is_err());
    }

    #[test]
    fn reject_comprehension() {
        let src = "x = [i for i in range(10)]\n";
        let result = compile_program(src);
        assert!(result.is_err());
    }

    #[test]
    fn reject_lambda() {
        let src = "f = lambda x: x + 1\n";
        let result = compile_program(src);
        assert!(result.is_err());
    }

    #[test]
    fn reject_class() {
        let src = "class Foo:\n    pass\n";
        let result = compile_program(src);
        assert!(result.is_err());
    }

    #[test]
    fn reject_try() {
        let src = "try:\n    pass\nexcept:\n    pass\n";
        let result = compile_program(src);
        assert!(result.is_err());
    }

    #[test]
    fn reject_global() {
        let src = "global x\n";
        let result = compile_program(src);
        assert!(result.is_err());
    }

    #[test]
    fn reject_del() {
        let src = "del x\n";
        let result = compile_program(src);
        assert!(result.is_err());
    }
}
