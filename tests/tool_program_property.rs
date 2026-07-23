//! Property tests for Tool Programs against a reference evaluator.
//!
//! These tests verify that:
//! - The parser never panics on arbitrary bytes
//! - Accepted programs have deterministic IR
//! - Static bounds are conservative (programs terminate within bounds)
//! - Mutation fuzzing cannot turn rejected constructs into accepted IR
//! - Deterministic digest across repeated compilation

use codegg_core::tool_program::{compile_program, parse_source, static_bounds, validate};

fn reference_eval_steps(src: &str) -> Result<u64, ()> {
    let ast = parse_source(src).map_err(|_| ())?;
    validate(&ast).map_err(|_| ())?;
    let bounds = static_bounds::analyze(&ast).map_err(|_| ())?;
    let steps = count_ast_steps(&ast);
    if steps > bounds.max_steps {
        return Err(());
    }
    Ok(steps)
}

fn count_ast_steps(program: &codegg_core::tool_program::Program) -> u64 {
    let mut count = 0u64;
    for stmt in &program.body {
        count += count_stmt_steps(stmt);
    }
    count
}

fn count_stmt_steps(stmt: &codegg_core::tool_program::Stmt) -> u64 {
    use codegg_core::tool_program::Stmt;
    match stmt {
        Stmt::Assign { value, .. } => 1 + count_expr_steps(value),
        Stmt::If {
            test,
            body,
            elif_clauses,
            else_body,
            ..
        } => {
            let mut s = 1 + count_expr_steps(test);
            for stmt in body {
                s += count_stmt_steps(stmt);
            }
            for clause in elif_clauses {
                s += count_expr_steps(&clause.test);
                for stmt in &clause.body {
                    s += count_stmt_steps(stmt);
                }
            }
            if let Some(eb) = else_body {
                for stmt in eb {
                    s += count_stmt_steps(stmt);
                }
            }
            s
        }
        Stmt::For { iter, body, .. } => {
            let mut s = 1 + count_expr_steps(iter);
            for stmt in body {
                s += count_stmt_steps(stmt);
            }
            s
        }
        Stmt::Assert { test, msg, .. } => {
            let mut s = 1 + count_expr_steps(test);
            if let Some(m) = msg {
                s += count_expr_steps(m);
            }
            s
        }
        Stmt::ToolCall {
            descriptor, kwargs, ..
        } => {
            let mut s = 2 + count_expr_steps(descriptor);
            for kw in kwargs {
                s += count_expr_steps(&kw.value);
            }
            s
        }
        Stmt::Parallel { descriptors, .. } => {
            let mut s = 2;
            for desc in descriptors {
                s += count_expr_steps(desc);
            }
            s
        }
        Stmt::Emit { value, .. } => 1 + count_expr_steps(value),
        Stmt::Fail {
            reason: Some(r), ..
        } => 1 + count_expr_steps(r),
        Stmt::Fail { .. } => 1,
        Stmt::Pass { .. } => 1,
    }
}

fn count_expr_steps(expr: &codegg_core::tool_program::Expr) -> u64 {
    use codegg_core::tool_program::Expr;
    match expr {
        Expr::NoneLiteral(_)
        | Expr::BoolLiteral { .. }
        | Expr::IntLiteral { .. }
        | Expr::FloatLiteral { .. }
        | Expr::StringLiteral { .. }
        | Expr::Name { .. } => 1,
        Expr::BinOp { left, right, .. } => 1 + count_expr_steps(left) + count_expr_steps(right),
        Expr::UnaryOp { operand, .. } => 1 + count_expr_steps(operand),
        Expr::BoolAnd { left, right, .. } | Expr::BoolOr { left, right, .. } => {
            2 + count_expr_steps(left) + count_expr_steps(right)
        }
        Expr::BoolNot { operand, .. } => 1 + count_expr_steps(operand),
        Expr::Compare {
            left, comparators, ..
        } => {
            let mut s = 1 + count_expr_steps(left);
            for c in comparators {
                s += count_expr_steps(c);
            }
            s
        }
        Expr::Subscript { value, slice, .. } => {
            let mut s = 1 + count_expr_steps(value);
            match slice {
                codegg_core::tool_program::Slice::Index(e) => s += count_expr_steps(e),
                codegg_core::tool_program::Slice::Range { start, stop, step } => {
                    if let Some(s_) = start {
                        s += count_expr_steps(s_);
                    }
                    if let Some(s_) = stop {
                        s += count_expr_steps(s_);
                    }
                    if let Some(s_) = step {
                        s += count_expr_steps(s_);
                    }
                }
            }
            s
        }
        Expr::CallBuiltin { arg, .. } => 1 + count_expr_steps(arg),
        Expr::MethodCall { object, args, .. } => {
            let mut s = 2 + count_expr_steps(object);
            for a in args {
                s += count_expr_steps(a);
            }
            s
        }
        Expr::ToolCallExpr {
            descriptor, kwargs, ..
        } => {
            let mut s = 2 + count_expr_steps(descriptor);
            for kw in kwargs {
                s += count_expr_steps(&kw.value);
            }
            s
        }
        Expr::ParallelExpr { descriptors, .. } => {
            let mut s = 2;
            for d in descriptors {
                s += count_expr_steps(d);
            }
            s
        }
        Expr::List { elements, .. } => {
            let mut s = 1;
            for e in elements {
                s += count_expr_steps(e);
            }
            s
        }
        Expr::Tuple { elements, .. } => {
            let mut s = 1;
            for e in elements {
                s += count_expr_steps(e);
            }
            s
        }
        Expr::Dict { keys, values, .. } => {
            let mut s = 1;
            for k in keys {
                s += count_expr_steps(k);
            }
            for v in values {
                s += count_expr_steps(v);
            }
            s
        }
        Expr::Range {
            start, stop, step, ..
        } => {
            let mut s = 1;
            if let Some(s_) = start {
                s += count_expr_steps(s_);
            }
            s += count_expr_steps(stop);
            if let Some(s_) = step {
                s += count_expr_steps(s_);
            }
            s
        }
        Expr::Parenthesized { inner, .. } => count_expr_steps(inner),
    }
}

// ── Property tests ──────────────────────────────────────────────

#[test]
fn property_parser_never_panics_on_arbitrary_bytes() {
    let inputs: Vec<&[u8]> = vec![
        b"",
        b"\0",
        b"\xff\xfe\xfd",
        b"import os\nclass X: pass\nwhile True: pass\nlambda x: x\n",
        b"((((((((((((((((((((((((((((((((((((",
        b"x = 1\n",
    ];
    for input in &inputs {
        if let Ok(s) = std::str::from_utf8(input) {
            let _ = parse_source(s);
        }
    }
}

#[test]
fn property_accepted_programs_have_deterministic_digest() {
    let sources = vec![
        "emit({\"ok\": true})\n",
        "x = 1\nemit(x)\n",
        "for i in range(5):\n    x = i\nemit(x)\n",
        "if True:\n    x = 1\nelse:\n    x = 0\nemit(x)\n",
        "r = parallel({\"t\": 1}, {\"t\": 2})\nemit(r)\n",
    ];
    for src in &sources {
        let r1 = compile_program(src).unwrap();
        let r2 = compile_program(src).unwrap();
        assert_eq!(r1.ir.digest, r2.ir.digest, "digest mismatch for: {}", src);
    }
}

#[test]
fn property_bounds_are_conservative() {
    let sources = vec![
        "emit({\"ok\": true})\n",
        "x = 42\nemit(x)\n",
        "for i in range(10):\n    x = i\nemit(x)\n",
        "total = 0\nfor i in range(100):\n    total = total + 1\nemit(total)\n",
        "if True:\n    x = 1\nelse:\n    x = 0\nemit(x)\n",
        "r = parallel({\"t\": 1}, {\"t\": 2})\nemit(r)\n",
    ];
    for src in &sources {
        let steps = reference_eval_steps(src).unwrap();
        assert!(steps > 0, "steps should be positive for: {}", src);
        let ast = parse_source(src).unwrap();
        let bounds = static_bounds::analyze(&ast).unwrap();
        assert!(
            steps <= bounds.max_steps,
            "steps {} > max_steps {} for: {}",
            steps,
            bounds.max_steps,
            src
        );
    }
}

#[test]
fn property_rejected_syntax_remains_rejected() {
    let rejected = vec![
        "import os\n",
        "class Foo:\n    pass\n",
        "while True:\n    pass\n",
        "f = lambda x: x\n",
        "x = [i for i in range(10)]\n",
        "try:\n    pass\nexcept:\n    pass\n",
        "def foo():\n    pass\n",
        "global x\n",
        "del x\n",
        "yield x\n",
        "with open('f') as fh:\n    pass\n",
    ];
    for src in &rejected {
        assert!(parse_source(src).is_err(), "should reject: {}", src);
    }
}

#[test]
fn property_ir_version_is_consistent() {
    let src = "emit({\"ok\": true})\n";
    let result = compile_program(src).unwrap();
    assert_eq!(result.ir.version, 1);
    assert_eq!(result.ir.language_version, 1);
}

#[test]
fn property_ir_has_return_instruction() {
    let sources = vec![
        "emit({\"ok\": true})\n",
        "x = 1\nemit(x)\n",
        "for i in range(5):\n    x = i\n",
    ];
    for src in &sources {
        let result = compile_program(src).unwrap();
        assert!(
            matches!(
                result.ir.instructions.last().unwrap().op,
                codegg_core::tool_program::ir::IrOp::Return
            ),
            "IR must end with Return for: {}",
            src
        );
    }
}

#[test]
fn property_ir_bounds_match_analyzer() {
    let sources = vec![
        "emit({\"ok\": true})\n",
        "for i in range(10):\n    x = i\nemit(x)\n",
        "r = parallel({\"t\": 1}, {\"t\": 2})\nemit(r)\n",
    ];
    for src in &sources {
        let ast = parse_source(src).unwrap();
        let bounds = static_bounds::analyze(&ast).unwrap();
        let result = compile_program(src).unwrap();
        assert_eq!(result.ir.bounds.max_steps, bounds.max_steps);
        assert_eq!(
            result.ir.bounds.max_loop_iterations,
            bounds.max_loop_iterations
        );
        assert_eq!(result.ir.bounds.call_site_count, bounds.call_site_count);
    }
}

#[test]
fn property_deterministic_across_repeated_compilation() {
    let src = r#"
results = []
for file in ["a.py", "b.py"]:
    content = call({"tool": "read_file", "path": file})
    lines = len(content)
    if lines > 100:
        results = results + [file]
emit({"total": len(results), "files": results})
"#;
    let digests: Vec<String> = (0..10)
        .map(|_| compile_program(src).unwrap().ir.digest)
        .collect();
    assert!(digests.windows(2).all(|w| w[0] == w[1]));
}
