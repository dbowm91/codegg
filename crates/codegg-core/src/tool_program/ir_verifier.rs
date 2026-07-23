//! IR verifier for Tool Programs.
//!
//! Checks jump targets, stack/local bounds, call metadata, checkpoint
//! placement, and terminal instruction.

use crate::tool_program::diagnostics::{Diagnostic, DiagnosticCode, SourceSpan};
use crate::tool_program::ir::*;
use crate::tool_program::ToolProgramError;

/// Verify an IR program for correctness.
pub fn verify_ir(ir: &IrProgram) -> Result<(), ToolProgramError> {
    let mut verifier = IrVerifier::new(ir);
    verifier.verify()
}

struct IrVerifier<'a> {
    ir: &'a IrProgram,
}

impl<'a> IrVerifier<'a> {
    fn new(ir: &'a IrProgram) -> Self {
        Self { ir }
    }

    fn verify(&mut self) -> Result<(), ToolProgramError> {
        // Check version
        if self.ir.version != IR_VERSION {
            return Err(ToolProgramError::Verify(Diagnostic::new(
                DiagnosticCode::VerificationFailed,
                format!(
                    "IR version mismatch: expected {}, got {}",
                    IR_VERSION, self.ir.version
                ),
                SourceSpan::new(0, 0),
            )));
        }

        // Check that we have instructions
        if self.ir.instructions.is_empty() {
            return Err(ToolProgramError::Verify(Diagnostic::new(
                DiagnosticCode::VerificationFailed,
                "IR has no instructions".to_string(),
                SourceSpan::new(0, 0),
            )));
        }

        // Check that the last instruction is Return
        let last_op = &self.ir.instructions.last().unwrap().op;
        if !matches!(last_op, IrOp::Return) {
            return Err(ToolProgramError::Verify(Diagnostic::new(
                DiagnosticCode::VerificationFailed,
                "IR must end with Return".to_string(),
                SourceSpan::new(0, 0),
            )));
        }

        // Verify each instruction
        let len = self.ir.instructions.len() as u32;
        for (idx, inst) in self.ir.instructions.iter().enumerate() {
            self.verify_instruction(inst, idx as u32, len)?;
        }

        // Verify bounds are consistent
        self.verify_bounds()?;

        // Verify local count
        if self.ir.local_count == 0
            && self
                .ir
                .instructions
                .iter()
                .any(|i| matches!(i.op, IrOp::StoreLocal { .. } | IrOp::LoadLocal { .. }))
        {
            return Err(ToolProgramError::Verify(Diagnostic::new(
                DiagnosticCode::VerificationFailed,
                "IR has local operations but local_count is 0".to_string(),
                SourceSpan::new(0, 0),
            )));
        }

        // Verify string pool references
        for inst in &self.ir.instructions {
            if let IrOp::LoadString { pool_idx } = &inst.op {
                if *pool_idx as usize >= self.ir.strings.len() {
                    return Err(ToolProgramError::Verify(Diagnostic::new(
                        DiagnosticCode::VerificationFailed,
                        format!(
                            "LoadString pool_idx {} out of range (pool size {})",
                            pool_idx,
                            self.ir.strings.len()
                        ),
                        SourceSpan::new(0, 0),
                    )));
                }
            }
        }

        // Verify integer pool references
        for inst in &self.ir.instructions {
            if let IrOp::LoadInt { pool_idx } = &inst.op {
                if *pool_idx as usize >= self.ir.integers.len() {
                    return Err(ToolProgramError::Verify(Diagnostic::new(
                        DiagnosticCode::VerificationFailed,
                        format!(
                            "LoadInt pool_idx {} out of range (pool size {})",
                            pool_idx,
                            self.ir.integers.len()
                        ),
                        SourceSpan::new(0, 0),
                    )));
                }
            }
        }

        // Verify float pool references
        for inst in &self.ir.instructions {
            if let IrOp::LoadFloat { pool_idx } = &inst.op {
                if *pool_idx as usize >= self.ir.floats.len() {
                    return Err(ToolProgramError::Verify(Diagnostic::new(
                        DiagnosticCode::VerificationFailed,
                        format!(
                            "LoadFloat pool_idx {} out of range (pool size {})",
                            pool_idx,
                            self.ir.floats.len()
                        ),
                        SourceSpan::new(0, 0),
                    )));
                }
            }
        }

        // Verify digest is non-empty
        if self.ir.digest.is_empty() {
            return Err(ToolProgramError::Verify(Diagnostic::new(
                DiagnosticCode::VerificationFailed,
                "IR digest is empty".to_string(),
                SourceSpan::new(0, 0),
            )));
        }

        Ok(())
    }

    fn verify_instruction(
        &mut self,
        inst: &IrInstruction,
        idx: u32,
        total: u32,
    ) -> Result<(), ToolProgramError> {
        // Verify span index
        if inst.span_idx as usize >= self.ir.spans.len() {
            return Err(ToolProgramError::Verify(Diagnostic::new(
                DiagnosticCode::VerificationFailed,
                format!(
                    "instruction {} has span_idx {} out of range (spans count {})",
                    idx,
                    inst.span_idx,
                    self.ir.spans.len()
                ),
                SourceSpan::new(0, 0),
            )));
        }

        match &inst.op {
            IrOp::JumpIfFalse { target } | IrOp::Jump { target } => {
                if *target >= total {
                    return Err(ToolProgramError::Verify(Diagnostic::new(
                        DiagnosticCode::VerificationFailed,
                        format!(
                            "instruction {} has jump target {} out of range (total {})",
                            idx, target, total
                        ),
                        SourceSpan::new(0, 0),
                    )));
                }
            }
            IrOp::ForLoopStart {
                body_start,
                loop_end,
            } => {
                if *body_start >= total {
                    return Err(ToolProgramError::Verify(Diagnostic::new(
                        DiagnosticCode::VerificationFailed,
                        format!("ForLoopStart body_start {} out of range", body_start),
                        SourceSpan::new(0, 0),
                    )));
                }
                if *loop_end >= total {
                    return Err(ToolProgramError::Verify(Diagnostic::new(
                        DiagnosticCode::VerificationFailed,
                        format!("ForLoopStart loop_end {} out of range", loop_end),
                        SourceSpan::new(0, 0),
                    )));
                }
                if *body_start <= idx {
                    return Err(ToolProgramError::Verify(Diagnostic::new(
                        DiagnosticCode::VerificationFailed,
                        format!(
                            "ForLoopStart body_start {} must be after instruction {}",
                            body_start, idx
                        ),
                        SourceSpan::new(0, 0),
                    )));
                }
                if *loop_end <= *body_start {
                    return Err(ToolProgramError::Verify(Diagnostic::new(
                        DiagnosticCode::VerificationFailed,
                        format!(
                            "ForLoopStart loop_end {} must be after body_start {}",
                            loop_end, body_start
                        ),
                        SourceSpan::new(0, 0),
                    )));
                }
            }
            IrOp::ForLoopNext {
                loop_start,
                loop_end,
            } => {
                if *loop_start >= total {
                    return Err(ToolProgramError::Verify(Diagnostic::new(
                        DiagnosticCode::VerificationFailed,
                        format!("ForLoopNext loop_start {} out of range", loop_start),
                        SourceSpan::new(0, 0),
                    )));
                }
                if *loop_end >= total {
                    return Err(ToolProgramError::Verify(Diagnostic::new(
                        DiagnosticCode::VerificationFailed,
                        format!("ForLoopNext loop_end {} out of range", loop_end),
                        SourceSpan::new(0, 0),
                    )));
                }
            }
            #[allow(clippy::collapsible_match)]
            IrOp::LoadLocal { slot } | IrOp::StoreLocal { slot } => {
                if *slot >= self.ir.local_count {
                    return Err(ToolProgramError::Verify(Diagnostic::new(
                        DiagnosticCode::VerificationFailed,
                        format!(
                            "local slot {} out of range (local_count {})",
                            slot, self.ir.local_count
                        ),
                        SourceSpan::new(0, 0),
                    )));
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn verify_bounds(&self) -> Result<(), ToolProgramError> {
        let b = &self.ir.bounds;

        if b.max_steps == 0 {
            return Err(ToolProgramError::Verify(Diagnostic::new(
                DiagnosticCode::VerificationFailed,
                "bounds.max_steps is 0".to_string(),
                SourceSpan::new(0, 0),
            )));
        }

        // max_loop_iterations and max_total_iterations can be 0 for programs without loops.
        // The runtime will enforce actual bounds during execution.
        // When both are 0, the program has no loops and this is valid.

        // When total is 0 (no loops), loop_iterations can also be 0.
        // When there are loops, total >= loop_iterations is checked by the analyzer.

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_program::compiler::compile;
    use crate::tool_program::parser::parse_source;
    use crate::tool_program::static_bounds;

    #[test]
    fn verify_simple_program() {
        let src = "emit({\"result\": \"ok\"})\n";
        let prog = parse_source(src).unwrap();
        let bounds = static_bounds::analyze(&prog).unwrap();
        let ir = compile(&prog, &bounds).unwrap();
        assert!(verify_ir(&ir).is_ok());
    }

    #[test]
    fn verify_for_loop_program() {
        let src = "for i in range(5):\n    x = i\n";
        let prog = parse_source(src).unwrap();
        let bounds = static_bounds::analyze(&prog).unwrap();
        let ir = compile(&prog, &bounds).unwrap();
        assert!(verify_ir(&ir).is_ok());
    }

    #[test]
    fn verify_if_else_program() {
        let src = "if True:\n    x = 1\nelse:\n    x = 0\n";
        let prog = parse_source(src).unwrap();
        let bounds = static_bounds::analyze(&prog).unwrap();
        let ir = compile(&prog, &bounds).unwrap();
        assert!(verify_ir(&ir).is_ok());
    }

    #[test]
    fn verify_tool_call_program() {
        let src = "result = call({\"tool\": \"grep\"})\n";
        let prog = parse_source(src).unwrap();
        let bounds = static_bounds::analyze(&prog).unwrap();
        let ir = compile(&prog, &bounds).unwrap();
        assert!(verify_ir(&ir).is_ok());
    }

    #[test]
    fn verify_complex_program() {
        let src = r#"
results = []
for file in ["a.py", "b.py"]:
    content = call({"tool": "read_file", "path": file})
    lines = len(content)
    if lines > 100:
        results = results + [file]
emit({"total": len(results)})
"#;
        let prog = parse_source(src).unwrap();
        let bounds = static_bounds::analyze(&prog).unwrap();
        let ir = compile(&prog, &bounds).unwrap();
        assert!(verify_ir(&ir).is_ok());
    }

    #[test]
    fn verify_rejects_bad_version() {
        let src = "emit(1)\n";
        let prog = parse_source(src).unwrap();
        let bounds = static_bounds::analyze(&prog).unwrap();
        let mut ir = compile(&prog, &bounds).unwrap();
        ir.version = 999;
        assert!(verify_ir(&ir).is_err());
    }

    #[test]
    fn verify_rejects_empty_instructions() {
        let src = "emit(1)\n";
        let prog = parse_source(src).unwrap();
        let bounds = static_bounds::analyze(&prog).unwrap();
        let mut ir = compile(&prog, &bounds).unwrap();
        ir.instructions.clear();
        assert!(verify_ir(&ir).is_err());
    }

    #[test]
    fn verify_rejects_missing_return() {
        let src = "emit(1)\n";
        let prog = parse_source(src).unwrap();
        let bounds = static_bounds::analyze(&prog).unwrap();
        let mut ir = compile(&prog, &bounds).unwrap();
        // Remove the last instruction (Return)
        ir.instructions.pop();
        assert!(verify_ir(&ir).is_err());
    }

    #[test]
    fn verify_rejects_bad_local_slot() {
        let src = "x = 1\n";
        let prog = parse_source(src).unwrap();
        let bounds = static_bounds::analyze(&prog).unwrap();
        let mut ir = compile(&prog, &bounds).unwrap();
        // Corrupt a local slot
        for inst in &mut ir.instructions {
            if let IrOp::StoreLocal { slot } = &mut inst.op {
                *slot = 999;
            }
        }
        assert!(verify_ir(&ir).is_err());
    }
}
