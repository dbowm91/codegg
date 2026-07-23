//! Integration tests for Tool Program static bound analysis.

use codegg_core::tool_program::{
    parse_source, static_bounds,
    static_bounds::{analyze_with_config, BoundsConfig},
    validate,
};

#[test]
fn bounds_simple_emit() {
    let src = "emit({\"ok\": true})\n";
    let ast = parse_source(src).unwrap();
    validate(&ast).unwrap();
    let bounds = static_bounds::analyze(&ast).unwrap();
    assert!(bounds.max_steps > 0);
    assert_eq!(bounds.call_site_count, 0);
    assert_eq!(bounds.max_parallel_width, 0);
}

#[test]
fn bounds_for_loop_literal_list() {
    let src = "for x in [1, 2, 3]:\n    emit(x)\n";
    let ast = parse_source(src).unwrap();
    validate(&ast).unwrap();
    let bounds = static_bounds::analyze(&ast).unwrap();
    assert!(bounds.max_total_iterations >= 3);
}

#[test]
fn bounds_for_loop_variable() {
    let src = "items = [1, 2]\nfor x in items:\n    emit(x)\n";
    let ast = parse_source(src).unwrap();
    validate(&ast).unwrap();
    let bounds = static_bounds::analyze(&ast).unwrap();
    assert!(bounds.max_steps > 0);
}

#[test]
fn bounds_nested_loops() {
    let src = "for i in [1, 2]:\n    for j in [3, 4]:\n        emit(i + j)\n";
    let ast = parse_source(src).unwrap();
    validate(&ast).unwrap();
    let bounds = static_bounds::analyze(&ast).unwrap();
    assert!(bounds.max_nesting_depth >= 2);
}

#[test]
fn bounds_tool_call_counted() {
    let src =
        "a = call({\"tool\": \"x\"})\nb = call({\"tool\": \"y\"})\nc = call({\"tool\": \"z\"})\n";
    let ast = parse_source(src).unwrap();
    validate(&ast).unwrap();
    let bounds = static_bounds::analyze(&ast).unwrap();
    assert_eq!(bounds.call_site_count, 3);
}

#[test]
fn bounds_parallel_width() {
    let src = "r = parallel({\"t\": 1}, {\"t\": 2}, {\"t\": 3}, {\"t\": 4})\n";
    let ast = parse_source(src).unwrap();
    validate(&ast).unwrap();
    let bounds = static_bounds::analyze(&ast).unwrap();
    assert!(bounds.max_parallel_width >= 4);
}

#[test]
fn bounds_reject_excessive_loop() {
    let src = "for i in range(1000):\n    x = i\n";
    let ast = parse_source(src).unwrap();
    validate(&ast).unwrap();
    let config = BoundsConfig {
        max_loop_iterations: 10,
        ..Default::default()
    };
    let result = analyze_with_config(&ast, &config);
    assert!(result.is_err());
}

#[test]
fn bounds_reject_excessive_parallel() {
    // 11 parallel calls exceeds default max of 10
    let src = "r = parallel({\"t\":1},{\"t\":2},{\"t\":3},{\"t\":4},{\"t\":5},{\"t\":6},{\"t\":7},{\"t\":8},{\"t\":9},{\"t\":10},{\"t\":11})\n";
    let ast = parse_source(src).unwrap();
    validate(&ast).unwrap();
    let result = static_bounds::analyze(&ast);
    assert!(result.is_err());
}

#[test]
fn bounds_reject_total_iterations_exceeded() {
    // Two loops each 6 iterations = 12 total, exceeds max of 10
    let src = "for i in [1,2,3,4,5,6]:\n    for j in [1,2,3,4,5,6]:\n        x = i + j\n";
    let ast = parse_source(src).unwrap();
    validate(&ast).unwrap();
    let config = BoundsConfig {
        max_total_iterations: 10,
        max_loop_iterations: 100,
        ..Default::default()
    };
    let result = analyze_with_config(&ast, &config);
    assert!(result.is_err());
}

#[test]
fn bounds_complex_program() {
    let src = r#"
results = []
for file in ["a.py", "b.py", "c.py"]:
    content = call({"tool": "read_file", "path": file})
    lines = len(content)
    if lines > 100:
        results = results + [file]
emit({"total": len(results)})
"#;
    let ast = parse_source(src).unwrap();
    validate(&ast).unwrap();
    let bounds = static_bounds::analyze(&ast).unwrap();
    assert!(bounds.max_steps > 0);
    assert_eq!(bounds.call_site_count, 1);
}

#[test]
fn bounds_recorded_in_ir() {
    use codegg_core::tool_program::{compile, ir};
    let src = "for i in range(5):\n    x = i\n";
    let ast = parse_source(src).unwrap();
    validate(&ast).unwrap();
    let bounds = static_bounds::analyze(&ast).unwrap();
    let ir_prog = compile(&ast, &bounds).unwrap();
    assert!(ir_prog.bounds.max_steps > 0);
    assert!(ir_prog.bounds.max_loop_iterations > 0);
    assert!(ir_prog.bounds.max_total_iterations > 0);
    assert_eq!(ir_prog.bounds.call_site_count, 0);
}

#[test]
fn bounds_method_calls_not_counted_as_tool_calls() {
    let src = "x = [1, 2]\nx = x + [3]\n";
    let ast = parse_source(src).unwrap();
    validate(&ast).unwrap();
    let bounds = static_bounds::analyze(&ast).unwrap();
    // append is not a tool call
    assert_eq!(bounds.call_site_count, 0);
}
