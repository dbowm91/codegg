//! Integration tests for the Tool Program restricted-Python frontend.
//!
//! These tests verify the full parse → validate → bounds → compile → verify pipeline.

use codegg_core::tool_program::{
    compile, compile_program, ir, parse_source, static_bounds, validate, verify_ir,
};

// ── Accepted source corpus ──────────────────────────────────────

#[test]
fn accept_simple_emit() {
    let result = compile_program(r#"emit({"result": "ok"})"#);
    assert!(result.is_ok(), "simple emit: {:?}", result.err());
}

#[test]
fn accept_assignment_and_emit() {
    let result = compile_program(
        r#"
x = 42
y = "hello"
emit({"x": x, "y": y})
"#,
    );
    assert!(result.is_ok(), "assignment+emit: {:?}", result.err());
}

#[test]
fn accept_for_range() {
    let result = compile_program(
        r#"
total = 0
for i in range(10):
    total = total + 1
emit({"total": total})
"#,
    );
    assert!(result.is_ok(), "for range: {:?}", result.err());
}

#[test]
fn accept_for_literal_list() {
    let result = compile_program(
        r#"
for name in ["alice", "bob", "charlie"]:
    emit({"name": name})
"#,
    );
    assert!(result.is_ok(), "for literal list: {:?}", result.err());
}

#[test]
fn accept_if_else() {
    let result = compile_program(
        r#"
x = 10
if x > 5:
    y = "big"
else:
    y = "small"
emit({"y": y})
"#,
    );
    assert!(result.is_ok(), "if else: {:?}", result.err());
}

#[test]
fn accept_elif_chain() {
    let result = compile_program(
        r#"
x = 0
if x > 0:
    y = "positive"
elif x < 0:
    y = "negative"
else:
    y = "zero"
emit({"y": y})
"#,
    );
    assert!(result.is_ok(), "elif chain: {:?}", result.err());
}

#[test]
fn accept_parallel_calls() {
    let result = compile_program(
        r#"
reads = parallel(
    {"tool": "read_file", "path": "a.py"},
    {"tool": "read_file", "path": "b.py"},
)
emit({"results": reads})
"#,
    );
    assert!(result.is_ok(), "parallel calls: {:?}", result.err());
}

#[test]
fn accept_tool_call() {
    let result = compile_program(
        r#"
result = call({"tool": "grep_search", "pattern": "TODO"})
emit(result)
"#,
    );
    assert!(result.is_ok(), "tool call: {:?}", result.err());
}

#[test]
fn accept_tool_call_with_kwargs() {
    let result = compile_program(
        r#"
result = call({"tool": "read_file"}, path="src/main.rs")
emit(result)
"#,
    );
    assert!(result.is_ok(), "tool call with kwargs: {:?}", result.err());
}

#[test]
fn accept_builtin_len() {
    let result = compile_program(
        r#"
items = [1, 2, 3]
n = len(items)
emit({"count": n})
"#,
    );
    assert!(result.is_ok(), "builtin len: {:?}", result.err());
}

#[test]
fn accept_builtin_str_int_bool() {
    let result = compile_program(
        r#"
x = 42
s = str(x)
i = int(s)
b = bool(i)
emit({"s": s, "i": i, "b": b})
"#,
    );
    assert!(result.is_ok(), "builtin conversions: {:?}", result.err());
}

#[test]
fn accept_subscript_index() {
    let result = compile_program(
        r#"
items = [10, 20, 30]
first = items[0]
emit({"first": first})
"#,
    );
    assert!(result.is_ok(), "subscript index: {:?}", result.err());
}

#[test]
fn accept_subscript_slice() {
    let result = compile_program(
        r#"
items = [10, 20, 30, 40]
subset = items[1:3]
emit({"subset": subset})
"#,
    );
    assert!(result.is_ok(), "subscript slice: {:?}", result.err());
}

#[test]
fn accept_dict_literal() {
    let result = compile_program(
        r#"
config = {"timeout": 30, "retries": 3}
emit(config)
"#,
    );
    assert!(result.is_ok(), "dict literal: {:?}", result.err());
}

#[test]
fn accept_tuple_literal() {
    let result = compile_program(
        r#"
pair = (1, "hello")
emit({"pair": pair})
"#,
    );
    assert!(result.is_ok(), "tuple literal: {:?}", result.err());
}

#[test]
fn accept_boolean_ops() {
    let result = compile_program(
        r#"
a = True
b = False
c = a and b
d = a or b
e = not c
emit({"c": c, "d": d, "e": e})
"#,
    );
    assert!(result.is_ok(), "boolean ops: {:?}", result.err());
}

#[test]
fn accept_comparisons() {
    let result = compile_program(
        r#"
x = 10
y = 20
r1 = x == y
r2 = x != y
r3 = x < y
r4 = x > y
r5 = x <= y
r6 = x >= y
emit({"r1": r1, "r2": r2, "r3": r3, "r4": r4, "r5": r5, "r6": r6})
"#,
    );
    assert!(result.is_ok(), "comparisons: {:?}", result.err());
}

#[test]
fn accept_arithmetic() {
    let result = compile_program(
        r#"
a = 10 + 3
b = 10 - 3
c = 10 * 3
d = 10 / 3
e = 10 % 3
f = 10 ** 2
g = 10 // 3
emit({"a": a, "b": b, "c": c, "d": d, "e": e, "f": f, "g": g})
"#,
    );
    assert!(result.is_ok(), "arithmetic: {:?}", result.err());
}

#[test]
fn accept_method_calls() {
    let result = compile_program(
        r#"
text = "hello world"
upper = text.upper()
stripped = text.strip()
emit({"upper": upper, "stripped": stripped})
"#,
    );
    assert!(result.is_ok(), "method calls: {:?}", result.err());
}

#[test]
fn accept_fail_with_reason() {
    let result = compile_program(r#"fail("something went wrong")"#);
    assert!(result.is_ok(), "fail with reason: {:?}", result.err());
}

#[test]
fn accept_fail_without_reason() {
    let result = compile_program(r#"fail()"#);
    assert!(result.is_ok(), "fail without reason: {:?}", result.err());
}

#[test]
fn accept_assert() {
    let result = compile_program(
        r#"
x = 10
assert x > 0, "x must be positive"
emit({"ok": True})
"#,
    );
    assert!(result.is_ok(), "assert: {:?}", result.err());
}

#[test]
fn accept_pass() {
    let result = compile_program(
        r#"
if True:
    pass
emit({"ok": True})
"#,
    );
    assert!(result.is_ok(), "pass: {:?}", result.err());
}

#[test]
fn accept_nested_loops() {
    let result = compile_program(
        r#"
total = 0
for i in range(3):
    for j in range(3):
        total = total + 1
emit({"total": total})
"#,
    );
    assert!(result.is_ok(), "nested loops: {:?}", result.err());
}

#[test]
fn accept_complex_program() {
    let result = compile_program(
        r#"
results = []
for file in ["a.py", "b.py"]:
    content = call({"tool": "read_file", "path": file})
    lines = len(content)
    if lines > 100:
        results = results + [file]
emit({"total": len(results)})
"#,
    );
    assert!(result.is_ok(), "complex program: {:?}", result.err());
}

#[test]
fn accept_none_and_booleans() {
    let result = compile_program(
        r#"
x = None
a = True
b = False
emit({"x": x, "a": a, "b": b})
"#,
    );
    assert!(result.is_ok(), "none and booleans: {:?}", result.err());
}

#[test]
fn accept_negative_numbers() {
    let result = compile_program(
        r#"
x = -42
y = +10
emit({"x": x, "y": y})
"#,
    );
    assert!(result.is_ok(), "negative numbers: {:?}", result.err());
}

#[test]
fn accept_in_operator() {
    let result = compile_program(
        r#"
items = [1, 2, 3]
check = 2 in items
emit({"check": check})
"#,
    );
    assert!(result.is_ok(), "in operator: {:?}", result.err());
}

// ── Rejected source corpus ──────────────────────────────────────

#[test]
fn reject_import() {
    let result = compile_program("import os\n");
    assert!(result.is_err(), "import should be rejected");
}

#[test]
fn reject_from_import() {
    let result = compile_program("from os import path\n");
    assert!(result.is_err(), "from import should be rejected");
}

#[test]
fn reject_while() {
    let result = compile_program("while True:\n    pass\n");
    assert!(result.is_err(), "while should be rejected");
}

#[test]
fn reject_function_def() {
    let result = compile_program("def foo():\n    pass\n");
    assert!(result.is_err(), "function def should be rejected");
}

#[test]
fn reject_class_def() {
    let result = compile_program("class Foo:\n    pass\n");
    assert!(result.is_err(), "class def should be rejected");
}

#[test]
fn reject_try_except() {
    let result = compile_program("try:\n    pass\nexcept:\n    pass\n");
    assert!(result.is_err(), "try/except should be rejected");
}

#[test]
fn reject_lambda() {
    let result = compile_program("f = lambda x: x + 1\n");
    assert!(result.is_err(), "lambda should be rejected");
}

#[test]
fn reject_list_comprehension() {
    let result = compile_program("x = [i for i in range(10)]\n");
    assert!(result.is_err(), "list comprehension should be rejected");
}

#[test]
fn reject_set_comprehension() {
    let result = compile_program("x = {i for i in range(10)}\n");
    assert!(result.is_err(), "set comprehension should be rejected");
}

#[test]
fn reject_dict_comprehension() {
    let result = compile_program("x = {i: i for i in range(10)}\n");
    assert!(result.is_err(), "dict comprehension should be rejected");
}

#[test]
fn reject_generator() {
    let result = compile_program("x = (i for i in range(10))\n");
    assert!(result.is_err(), "generator should be rejected");
}

#[test]
fn reject_augmented_assign() {
    let result = compile_program("x = 0\nx += 1\n");
    assert!(result.is_err(), "augmented assign should be rejected");
}

#[test]
fn reject_del() {
    let result = compile_program("x = 1\ndel x\n");
    assert!(result.is_err(), "del should be rejected");
}

#[test]
fn reject_global() {
    let result = compile_program("global x\n");
    assert!(result.is_err(), "global should be rejected");
}

#[test]
fn reject_return() {
    let result = compile_program("return 42\n");
    assert!(result.is_err(), "return should be rejected");
}

#[test]
fn reject_yield() {
    let result = compile_program("yield 42\n");
    assert!(result.is_err(), "yield should be rejected");
}

#[test]
fn reject_with_statement() {
    let result = compile_program("with open('f') as fh:\n    pass\n");
    assert!(result.is_err(), "with should be rejected");
}

#[test]
fn reject_match_statement() {
    let result = compile_program("match x:\n    case 1:\n        pass\n");
    assert!(result.is_err(), "match should be rejected");
}

#[test]
fn reject_raise() {
    let result = compile_program("raise ValueError('bad')\n");
    assert!(result.is_err(), "raise should be rejected");
}

#[test]
fn reject_fstring() {
    let result = compile_program("x = f\"hello {name}\"\n");
    assert!(result.is_err(), "f-string should be rejected");
}

#[test]
fn reject_walrus_operator() {
    let result = compile_program("if (n := 10) > 5:\n    pass\n");
    assert!(result.is_err(), "walrus operator should be rejected");
}

#[test]
fn reject_is_comparison() {
    let result = compile_program("x = a is b\n");
    assert!(result.is_err(), "is comparison should be rejected");
}

#[test]
fn reject_is_not_comparison() {
    let result = compile_program("x = a is not b\n");
    assert!(result.is_err(), "is not comparison should be rejected");
}

#[test]
fn reject_set_literal() {
    let result = compile_program("x = {1, 2, 3}\n");
    assert!(result.is_err(), "set literal should be rejected");
}

#[test]
fn reject_await() {
    let result = compile_program("x = await foo()\n");
    assert!(result.is_err(), "await should be rejected");
}

#[test]
fn reject_annotated_assignment() {
    let result = compile_program("x: int = 5\n");
    assert!(result.is_err(), "annotated assignment should be rejected");
}

#[test]
fn reject_disallowed_method() {
    let result = compile_program("x = a.os_system()\n");
    assert!(result.is_err(), "disallowed method should be rejected");
}

#[test]
fn reject_shadow_call() {
    let result = compile_program("call = 1\n");
    assert!(result.is_err(), "shadowing call should be rejected");
}

#[test]
fn reject_shadow_parallel() {
    let result = compile_program("parallel = 1\n");
    assert!(result.is_err(), "shadowing parallel should be rejected");
}

#[test]
fn reject_shadow_emit() {
    let result = compile_program("emit = 1\n");
    assert!(result.is_err(), "shadowing emit should be rejected");
}

#[test]
fn reject_shadow_fail() {
    let result = compile_program("fail = 1\n");
    assert!(result.is_err(), "shadowing fail should be rejected");
}

// ── Determinism tests ───────────────────────────────────────────

#[test]
fn deterministic_ir() {
    let src = r#"
results = []
for file in ["a.py", "b.py"]:
    content = call({"tool": "read_file", "path": file})
    lines = len(content)
    if lines > 100:
        results = results + [file]
emit({"total": len(results)})
"#;
    let r1 = compile_program(src).unwrap();
    let r2 = compile_program(src).unwrap();
    assert_eq!(r1.ir.digest, r2.ir.digest);
    assert_eq!(r1.ir.instructions, r2.ir.instructions);
}

#[test]
fn deterministic_ir_across_compilations() {
    let src = "x = 1\ny = x + 2\nemit(y)\n";
    let results: Vec<_> = (0..5).map(|_| compile_program(src).unwrap()).collect();
    let digest = &results[0].ir.digest;
    for r in &results[1..] {
        assert_eq!(&r.ir.digest, digest);
    }
}

// ── IR metadata tests ───────────────────────────────────────────

#[test]
fn ir_metadata_versions() {
    let src = "emit(1)\n";
    let result = compile_program(src).unwrap();
    assert_eq!(result.ir.version, ir::IR_VERSION);
    assert_eq!(result.ir.language_version, ir::LANGUAGE_VERSION);
    assert_eq!(result.ir.compiler_version, ir::COMPILER_VERSION);
    assert_eq!(result.ir.parser_version, ir::PARSER_VERSION);
}

#[test]
fn ir_bounds_recorded() {
    let src = "for i in range(10):\n    x = i\n";
    let result = compile_program(src).unwrap();
    assert!(result.ir.bounds.max_steps > 0);
    assert!(result.ir.bounds.max_loop_iterations > 0);
    assert!(result.ir.bounds.max_total_iterations > 0);
}

#[test]
fn ir_source_hash_recorded() {
    let src = "emit(1)\n";
    let result = compile_program(src).unwrap();
    assert!(!result.ir.source_hash.is_empty());
}

#[test]
fn ir_digest_nonempty() {
    let src = "emit(1)\n";
    let result = compile_program(src).unwrap();
    assert!(!result.ir.digest.is_empty());
    assert_eq!(result.ir.digest.len(), 64); // SHA-256 hex
}

// ── Static bounds tests ─────────────────────────────────────────

#[test]
fn bounds_simple_program() {
    let src = "x = 1\nemit(x)\n";
    let result = compile_program(src).unwrap();
    assert!(result.ir.bounds.max_steps >= 3);
    assert_eq!(result.ir.bounds.call_site_count, 0);
}

#[test]
fn bounds_with_calls() {
    let src = "a = call({\"tool\": \"read\"})\nb = call({\"tool\": \"grep\"})\n";
    let result = compile_program(src).unwrap();
    assert_eq!(result.ir.bounds.call_site_count, 2);
}

#[test]
fn bounds_parallel() {
    let src = "r = parallel({\"t\": 1}, {\"t\": 2}, {\"t\": 3})\n";
    let result = compile_program(src).unwrap();
    assert!(result.ir.bounds.max_parallel_width >= 3);
}

// ── Error code tests ────────────────────────────────────────────

#[test]
fn error_code_for_import() {
    let result = compile_program("import os\n");
    assert!(result.is_err());
    let code = result.unwrap_err().code();
    assert_eq!(code.code_str(), "TP001");
}

#[test]
fn error_code_for_while() {
    let result = compile_program("while True:\n    pass\n");
    assert!(result.is_err());
    let code = result.unwrap_err().code();
    assert_eq!(code.code_str(), "TP001");
}

#[test]
fn error_code_for_shadow() {
    let result = compile_program("call = 1\n");
    assert!(result.is_err());
    let code = result.unwrap_err().code();
    assert_eq!(code.code_str(), "TP005");
}

// ── Source too large ────────────────────────────────────────────

#[test]
fn reject_source_too_large() {
    let src = "x = 1\n".repeat(200_000);
    let result = compile_program(&src);
    assert!(result.is_err());
}

// ── Pipeline unit tests ─────────────────────────────────────────

#[test]
fn pipeline_parse_validate_bounds_compile_verify() {
    let src = r#"
results = []
for file in ["a.py", "b.py", "c.py"]:
    content = call({"tool": "read_file", "path": file})
    lines = len(content)
    if lines > 100:
        results = results + [file]
emit({"total": len(results)})
"#;
    // Step 1: Parse
    let ast = parse_source(src).unwrap();
    assert!(!ast.body.is_empty());

    // Step 2: Validate
    validate(&ast).unwrap();

    // Step 3: Static bounds
    let bounds = static_bounds::analyze(&ast).unwrap();
    assert!(bounds.max_steps > 0);

    // Step 4: Compile
    let ir = compile(&ast, &bounds).unwrap();
    assert_eq!(ir.version, ir::IR_VERSION);

    // Step 5: Verify
    verify_ir(&ir).unwrap();
}
