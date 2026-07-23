//! Integration tests for Tool Program IR compilation and verification.

use codegg_core::tool_program::{compile_program, ir, verify_ir};

#[test]
fn ir_emit_produces_emit_instruction() {
    let src = "emit({\"ok\": true})\n";
    let result = compile_program(src).unwrap();
    assert!(result
        .ir
        .instructions
        .iter()
        .any(|i| matches!(i.op, ir::IrOp::Emit)));
}

#[test]
fn ir_fail_produces_fail_instruction() {
    let src = "fail(\"error\")\n";
    let result = compile_program(src).unwrap();
    assert!(result
        .ir
        .instructions
        .iter()
        .any(|i| matches!(i.op, ir::IrOp::Fail)));
}

#[test]
fn ir_tool_call_produces_construct_and_execute() {
    let src = "result = call({\"tool\": \"grep\"})\n";
    let result = compile_program(src).unwrap();
    assert!(result
        .ir
        .instructions
        .iter()
        .any(|i| matches!(i.op, ir::IrOp::ConstructCall)));
    assert!(result
        .ir
        .instructions
        .iter()
        .any(|i| matches!(i.op, ir::IrOp::ExecuteCall)));
}

#[test]
fn ir_parallel_produces_parallel_instructions() {
    let src = "r = parallel({\"t\": 1}, {\"t\": 2})\n";
    let result = compile_program(src).unwrap();
    assert!(result
        .ir
        .instructions
        .iter()
        .any(|i| matches!(i.op, ir::IrOp::ParallelStart { count: 2 })));
    assert!(result
        .ir
        .instructions
        .iter()
        .any(|i| matches!(i.op, ir::IrOp::ParallelExecute)));
}

#[test]
fn ir_for_loop_produces_loop_instructions() {
    let src = "for i in [1, 2, 3]:\n    x = i\n";
    let result = compile_program(src).unwrap();
    assert!(result
        .ir
        .instructions
        .iter()
        .any(|i| matches!(i.op, ir::IrOp::ForLoopStart { .. })));
    assert!(result
        .ir
        .instructions
        .iter()
        .any(|i| matches!(i.op, ir::IrOp::ForLoopNext { .. })));
    assert!(result
        .ir
        .instructions
        .iter()
        .any(|i| matches!(i.op, ir::IrOp::ForLoopIter)));
}

#[test]
fn ir_if_else_produces_jump_instructions() {
    let src = "if True:\n    x = 1\nelse:\n    x = 0\n";
    let result = compile_program(src).unwrap();
    assert!(result
        .ir
        .instructions
        .iter()
        .any(|i| matches!(i.op, ir::IrOp::JumpIfFalse { .. })));
    assert!(result
        .ir
        .instructions
        .iter()
        .any(|i| matches!(i.op, ir::IrOp::Jump { .. })));
}

#[test]
fn ir_binop_produces_binop_instruction() {
    let src = "x = 1 + 2\n";
    let result = compile_program(src).unwrap();
    assert!(result.ir.instructions.iter().any(|i| matches!(
        i.op,
        ir::IrOp::BinOp {
            kind: ir::IrBinOp::Add
        }
    )));
}

#[test]
fn ir_compare_produces_compare_instruction() {
    let src = "x = 1 == 2\n";
    let result = compile_program(src).unwrap();
    assert!(result.ir.instructions.iter().any(|i| matches!(
        i.op,
        ir::IrOp::Compare {
            kind: ir::IrCmpOp::Eq
        }
    )));
}

#[test]
fn ir_bool_ops() {
    let src = "x = True and False\ny = True or False\n";
    let result = compile_program(src).unwrap();
    assert!(result
        .ir
        .instructions
        .iter()
        .any(|i| matches!(i.op, ir::IrOp::BoolAnd)));
    assert!(result
        .ir
        .instructions
        .iter()
        .any(|i| matches!(i.op, ir::IrOp::BoolOr)));
}

#[test]
fn ir_not_operator() {
    let src = "x = not True\n";
    let result = compile_program(src).unwrap();
    assert!(result
        .ir
        .instructions
        .iter()
        .any(|i| matches!(i.op, ir::IrOp::BoolNot)));
}

#[test]
fn ir_subscript_produces_index() {
    let src = "x = [1, 2, 3]\ny = x[0]\n";
    let result = compile_program(src).unwrap();
    assert!(result
        .ir
        .instructions
        .iter()
        .any(|i| matches!(i.op, ir::IrOp::Index)));
}

#[test]
fn ir_slice_produces_slice() {
    let src = "x = [1, 2, 3, 4]\ny = x[1:3]\n";
    let result = compile_program(src).unwrap();
    assert!(result
        .ir
        .instructions
        .iter()
        .any(|i| matches!(i.op, ir::IrOp::Slice)));
}

#[test]
fn ir_builtin_len() {
    let src = "x = [1, 2]\ny = len(x)\n";
    let result = compile_program(src).unwrap();
    assert!(result
        .ir
        .instructions
        .iter()
        .any(|i| matches!(i.op, ir::IrOp::Len)));
}

#[test]
fn ir_builtin_str() {
    let src = "x = 42\ny = str(x)\n";
    let result = compile_program(src).unwrap();
    assert!(result
        .ir
        .instructions
        .iter()
        .any(|i| matches!(i.op, ir::IrOp::Str)));
}

#[test]
fn ir_builtin_int() {
    let src = "x = \"42\"\ny = int(x)\n";
    let result = compile_program(src).unwrap();
    assert!(result
        .ir
        .instructions
        .iter()
        .any(|i| matches!(i.op, ir::IrOp::Int)));
}

#[test]
fn ir_builtin_bool() {
    let src = "x = 1\ny = bool(x)\n";
    let result = compile_program(src).unwrap();
    assert!(result
        .ir
        .instructions
        .iter()
        .any(|i| matches!(i.op, ir::IrOp::Bool)));
}

#[test]
fn ir_local_variables() {
    let src = "x = 1\ny = 2\nz = x + y\n";
    let result = compile_program(src).unwrap();
    assert!(result.ir.local_count >= 3);
    assert!(result
        .ir
        .instructions
        .iter()
        .any(|i| matches!(i.op, ir::IrOp::StoreLocal { .. })));
    assert!(result
        .ir
        .instructions
        .iter()
        .any(|i| matches!(i.op, ir::IrOp::LoadLocal { .. })));
}

#[test]
fn ir_string_constants_pool() {
    let src = "a = \"hello\"\nb = \"world\"\n";
    let result = compile_program(src).unwrap();
    assert!(result.ir.strings.len() >= 2);
}

#[test]
fn ir_integer_constants_pool() {
    let src = "a = 42\nb = 100\n";
    let result = compile_program(src).unwrap();
    assert!(result.ir.integers.len() >= 2);
}

#[test]
fn ir_spans_recorded() {
    let src = "x = 1\nemit(x)\n";
    let result = compile_program(src).unwrap();
    assert!(!result.ir.spans.is_empty());
    // Each instruction should have a valid span index
    for inst in &result.ir.instructions {
        assert!((inst.span_idx as usize) < result.ir.spans.len());
    }
}

#[test]
fn ir_ends_with_return() {
    let src = "emit(1)\n";
    let result = compile_program(src).unwrap();
    let last = result.ir.instructions.last().unwrap();
    assert!(matches!(last.op, ir::IrOp::Return));
}

#[test]
fn ir_verification_passes() {
    let src = r#"
results = []
for file in ["a.py", "b.py"]:
    content = call({"tool": "read_file", "path": file})
    lines = len(content)
    if lines > 100:
        results = results + [file]
emit({"total": len(results)})
"#;
    let result = compile_program(src).unwrap();
    assert!(verify_ir(&result.ir).is_ok());
}

#[test]
fn ir_digest_is_sha256() {
    let src = "x = 1\nemit(x)\n";
    let result = compile_program(src).unwrap();
    assert_eq!(result.ir.digest.len(), 64);
    // Should be valid hex
    assert!(u64::from_str_radix(&result.ir.digest[..8], 16).is_ok());
}

#[test]
fn ir_deterministic_digest() {
    let src = "for i in range(10):\n    x = call({\"tool\": \"grep\", \"pattern\": str(i)})\n";
    let r1 = compile_program(src).unwrap();
    let r2 = compile_program(src).unwrap();
    assert_eq!(r1.ir.digest, r2.ir.digest);
}

#[test]
fn ir_different_source_different_digest() {
    let r1 = compile_program("x = 1\n").unwrap();
    let r2 = compile_program("x = 2\n").unwrap();
    assert_ne!(r1.ir.digest, r2.ir.digest);
}

#[test]
fn ir_complex_program_structure() {
    let src = r#"
results = []
for file in ["a.py", "b.py", "c.py"]:
    content = call({"tool": "read_file", "path": file})
    lines = len(content)
    if lines > 100:
        results = results + [file]
emit({"total": len(results)})
"#;
    let result = compile_program(src).unwrap();
    let ir = &result.ir;
    // Should have multiple instruction types
    let has_for = ir
        .instructions
        .iter()
        .any(|i| matches!(i.op, ir::IrOp::ForLoopStart { .. }));
    let has_call = ir
        .instructions
        .iter()
        .any(|i| matches!(i.op, ir::IrOp::ConstructCall));
    let has_if = ir
        .instructions
        .iter()
        .any(|i| matches!(i.op, ir::IrOp::JumpIfFalse { .. }));
    let has_emit = ir
        .instructions
        .iter()
        .any(|i| matches!(i.op, ir::IrOp::Emit));
    let has_return = ir
        .instructions
        .iter()
        .any(|i| matches!(i.op, ir::IrOp::Return));
    assert!(has_for, "should have for loop");
    assert!(has_call, "should have tool call");
    assert!(has_if, "should have if branch");
    assert!(has_emit, "should have emit");
    assert!(has_return, "should have return");
}
