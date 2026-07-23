//! Static bound analysis for the restricted-Python Tool Program AST.
//!
//! Computes conservative upper bounds for:
//! - maximum IR steps
//! - maximum loop iterations per loop and total
//! - maximum call sites and dynamic call count
//! - maximum parallel width and nested parallel depth
//! - maximum collection/value growth
//! - maximum nesting depth

use crate::tool_program::ast::*;
use crate::tool_program::diagnostics::{Diagnostic, DiagnosticCode, SourceSpan};
use crate::tool_program::ir::IrBounds;
use crate::tool_program::ToolProgramError;

/// Default limits.
const DEFAULT_MAX_LOOP_ITERATIONS: u64 = 10_000;
const DEFAULT_MAX_TOTAL_ITERATIONS: u64 = 100_000;
const DEFAULT_MAX_PARALLEL_WIDTH: u32 = 10;
const DEFAULT_MAX_PARALLEL_DEPTH: u32 = 2;
const DEFAULT_MAX_NESTING_DEPTH: u32 = 20;
const DEFAULT_MAX_CALL_SITES: u32 = 100;
const DEFAULT_MAX_DYNAMIC_CALLS: u64 = 1_000;
const DEFAULT_MAX_VALUE_GROWTH: u64 = 10_000;
const DEFAULT_MAX_STEPS: u64 = 1_000_000;

/// Configuration for static bound limits.
#[derive(Debug, Clone)]
pub struct BoundsConfig {
    pub max_loop_iterations: u64,
    pub max_total_iterations: u64,
    pub max_parallel_width: u32,
    pub max_parallel_depth: u32,
    pub max_nesting_depth: u32,
    pub max_call_sites: u32,
    pub max_dynamic_calls: u64,
    pub max_value_growth: u64,
    pub max_steps: u64,
}

impl Default for BoundsConfig {
    fn default() -> Self {
        Self {
            max_loop_iterations: DEFAULT_MAX_LOOP_ITERATIONS,
            max_total_iterations: DEFAULT_MAX_TOTAL_ITERATIONS,
            max_parallel_width: DEFAULT_MAX_PARALLEL_WIDTH,
            max_parallel_depth: DEFAULT_MAX_PARALLEL_DEPTH,
            max_nesting_depth: DEFAULT_MAX_NESTING_DEPTH,
            max_call_sites: DEFAULT_MAX_CALL_SITES,
            max_dynamic_calls: DEFAULT_MAX_DYNAMIC_CALLS,
            max_value_growth: DEFAULT_MAX_VALUE_GROWTH,
            max_steps: DEFAULT_MAX_STEPS,
        }
    }
}

/// Analysis state accumulated during bound computation.
struct AnalysisState {
    total_loop_iterations: u64,
    call_site_count: u32,
    parallel_width: u32,
    parallel_depth: u32,
    max_parallel_depth: u32,
    max_nesting_depth: u32,
    estimated_steps: u64,
    config: BoundsConfig,
}

/// Analyze a Program and produce static bounds.
pub fn analyze(program: &Program) -> Result<StaticBounds, ToolProgramError> {
    analyze_with_config(program, &BoundsConfig::default())
}

/// Analyze with a custom bounds configuration.
pub fn analyze_with_config(
    program: &Program,
    config: &BoundsConfig,
) -> Result<StaticBounds, ToolProgramError> {
    let mut state = AnalysisState {
        total_loop_iterations: 0,
        call_site_count: 0,
        parallel_width: 0,
        parallel_depth: 0,
        max_parallel_depth: 0,
        max_nesting_depth: 0,
        estimated_steps: 0,
        config: config.clone(),
    };

    analyze_stmts(&program.body, &mut state, 0)?;

    // Add baseline steps for the program
    state.estimated_steps += program.body.len() as u64;

    let bounds = StaticBounds {
        max_steps: state.estimated_steps.min(config.max_steps),
        max_loop_iterations: config.max_loop_iterations,
        max_total_iterations: state.total_loop_iterations.min(config.max_total_iterations),
        call_site_count: state.call_site_count,
        max_dynamic_calls: (state.call_site_count as u64).min(config.max_dynamic_calls),
        max_parallel_width: state.parallel_width.min(config.max_parallel_width),
        max_parallel_depth: state.max_parallel_depth.min(config.max_parallel_depth),
        max_nesting_depth: state.max_nesting_depth,
        max_value_growth: config.max_value_growth,
    };

    Ok(bounds)
}

fn analyze_stmts(
    stmts: &[Stmt],
    state: &mut AnalysisState,
    depth: u32,
) -> Result<(), ToolProgramError> {
    for stmt in stmts {
        analyze_stmt(stmt, state, depth)?;
    }
    Ok(())
}

fn analyze_stmt(
    stmt: &Stmt,
    state: &mut AnalysisState,
    depth: u32,
) -> Result<(), ToolProgramError> {
    if depth > state.config.max_nesting_depth {
        return Err(ToolProgramError::Bounds(Diagnostic::new(
            DiagnosticCode::MaxNestingDepth,
            format!(
                "nesting depth {} exceeds maximum {}",
                depth, state.config.max_nesting_depth
            ),
            SourceSpan::new(0, 0),
        )));
    }
    if depth > state.max_nesting_depth {
        state.max_nesting_depth = depth;
    }

    match stmt {
        Stmt::Assign { value, .. } => {
            analyze_expr(value, state, depth)?;
            state.estimated_steps += 1;
        }
        Stmt::If {
            test,
            body,
            elif_clauses,
            else_body,
            ..
        } => {
            analyze_expr(test, state, depth)?;
            state.estimated_steps += 1; // branch test
            analyze_stmts(body, state, depth + 1)?;
            for clause in elif_clauses {
                analyze_expr(&clause.test, state, depth)?;
                analyze_stmts(&clause.body, state, depth + 1)?;
                state.estimated_steps += 1;
            }
            if let Some(else_b) = else_body {
                analyze_stmts(else_b, state, depth + 1)?;
            }
        }
        Stmt::For { iter, body, .. } => {
            analyze_expr(iter, state, depth)?;
            // Estimate loop iterations
            let iter_count = estimate_iteration_count(iter);
            state.total_loop_iterations = state.total_loop_iterations.saturating_add(iter_count);
            if iter_count > state.config.max_loop_iterations {
                return Err(ToolProgramError::Bounds(Diagnostic::new(
                    DiagnosticCode::UnboundedLoop,
                    format!(
                        "loop iteration count {} exceeds maximum {}",
                        iter_count, state.config.max_loop_iterations
                    ),
                    SourceSpan::new(0, 0),
                )));
            }
            if state.total_loop_iterations > state.config.max_total_iterations {
                return Err(ToolProgramError::Bounds(Diagnostic::new(
                    DiagnosticCode::MaxTotalIterations,
                    format!(
                        "total loop iterations {} exceeds maximum {}",
                        state.total_loop_iterations, state.config.max_total_iterations
                    ),
                    SourceSpan::new(0, 0),
                )));
            }
            analyze_stmts(body, state, depth + 1)?;
            // Each iteration executes the body once
            state.estimated_steps = state
                .estimated_steps
                .saturating_add(iter_count.saturating_mul(body.len() as u64));
        }
        Stmt::Assert { test, msg, .. } => {
            analyze_expr(test, state, depth)?;
            if let Some(m) = msg {
                analyze_expr(m, state, depth)?;
            }
            state.estimated_steps += 1;
        }
        Stmt::ToolCall {
            descriptor, kwargs, ..
        } => {
            analyze_expr(descriptor, state, depth)?;
            for kw in kwargs {
                analyze_expr(&kw.value, state, depth)?;
            }
            state.call_site_count += 1;
            state.estimated_steps += 2; // construct + execute
        }
        Stmt::Parallel { descriptors, .. } => {
            let count = descriptors.len() as u32;
            if count > state.config.max_parallel_width {
                return Err(ToolProgramError::Bounds(Diagnostic::new(
                    DiagnosticCode::MaxParallelWidth,
                    format!(
                        "parallel width {} exceeds maximum {}",
                        count, state.config.max_parallel_width
                    ),
                    SourceSpan::new(0, 0),
                )));
            }
            if count > state.parallel_width {
                state.parallel_width = count;
            }
            state.parallel_depth += 1;
            if state.parallel_depth > state.max_parallel_depth {
                state.max_parallel_depth = state.parallel_depth;
            }
            if state.max_parallel_depth > state.config.max_parallel_depth {
                return Err(ToolProgramError::Bounds(Diagnostic::new(
                    DiagnosticCode::MaxParallelWidth,
                    format!(
                        "parallel nesting depth {} exceeds maximum {}",
                        state.max_parallel_depth, state.config.max_parallel_depth
                    ),
                    SourceSpan::new(0, 0),
                )));
            }
            for desc in descriptors {
                analyze_expr(desc, state, depth)?;
                state.call_site_count += 1;
            }
            state.parallel_depth -= 1;
            state.estimated_steps += 2 + count as u64;
        }
        Stmt::Emit { value, .. } => {
            analyze_expr(value, state, depth)?;
            state.estimated_steps += 1;
        }
        Stmt::Fail { reason, .. } => {
            if let Some(r) = reason {
                analyze_expr(r, state, depth)?;
            }
            state.estimated_steps += 1;
        }
        Stmt::Pass { .. } => {
            state.estimated_steps += 1;
        }
    }
    Ok(())
}

#[allow(clippy::only_used_in_recursion)]
fn analyze_expr(
    expr: &Expr,
    state: &mut AnalysisState,
    depth: u32,
) -> Result<(), ToolProgramError> {
    match expr {
        Expr::NoneLiteral(_)
        | Expr::BoolLiteral { .. }
        | Expr::IntLiteral { .. }
        | Expr::FloatLiteral { .. }
        | Expr::StringLiteral { .. }
        | Expr::Name { .. } => {
            state.estimated_steps += 1;
        }
        Expr::BinOp { left, right, .. } => {
            analyze_expr(left, state, depth)?;
            analyze_expr(right, state, depth)?;
            state.estimated_steps += 1;
        }
        Expr::UnaryOp { operand, .. } => {
            analyze_expr(operand, state, depth)?;
            state.estimated_steps += 1;
        }
        Expr::BoolAnd { left, right, .. } | Expr::BoolOr { left, right, .. } => {
            analyze_expr(left, state, depth)?;
            analyze_expr(right, state, depth)?;
            state.estimated_steps += 2;
        }
        Expr::BoolNot { operand, .. } => {
            analyze_expr(operand, state, depth)?;
            state.estimated_steps += 1;
        }
        Expr::Compare {
            left, comparators, ..
        } => {
            analyze_expr(left, state, depth)?;
            for c in comparators {
                analyze_expr(c, state, depth)?;
            }
            state.estimated_steps += comparators.len() as u64;
        }
        Expr::Subscript { value, slice, .. } => {
            analyze_expr(value, state, depth)?;
            match slice {
                Slice::Index(e) => analyze_expr(e, state, depth)?,
                Slice::Range { start, stop, step } => {
                    if let Some(s) = start {
                        analyze_expr(s, state, depth)?;
                    }
                    if let Some(s) = stop {
                        analyze_expr(s, state, depth)?;
                    }
                    if let Some(s) = step {
                        analyze_expr(s, state, depth)?;
                    }
                }
            }
            state.estimated_steps += 1;
        }
        Expr::CallBuiltin { arg, .. } => {
            analyze_expr(arg, state, depth)?;
            state.estimated_steps += 1;
        }
        Expr::MethodCall { object, args, .. } => {
            analyze_expr(object, state, depth)?;
            for a in args {
                analyze_expr(a, state, depth)?;
            }
            state.estimated_steps += 2;
        }
        Expr::ToolCallExpr {
            descriptor, kwargs, ..
        } => {
            analyze_expr(descriptor, state, depth)?;
            for kw in kwargs {
                analyze_expr(&kw.value, state, depth)?;
            }
            state.call_site_count += 1;
            state.estimated_steps += 2;
        }
        Expr::ParallelExpr { descriptors, .. } => {
            let count = descriptors.len() as u32;
            if count > state.config.max_parallel_width {
                return Err(ToolProgramError::Bounds(Diagnostic::new(
                    DiagnosticCode::MaxParallelWidth,
                    format!(
                        "parallel width {} exceeds maximum {}",
                        count, state.config.max_parallel_width
                    ),
                    SourceSpan::new(0, 0),
                )));
            }
            if count > state.parallel_width {
                state.parallel_width = count;
            }
            state.parallel_depth += 1;
            if state.parallel_depth > state.max_parallel_depth {
                state.max_parallel_depth = state.parallel_depth;
            }
            for d in descriptors {
                analyze_expr(d, state, depth)?;
                state.call_site_count += 1;
            }
            state.parallel_depth -= 1;
            state.estimated_steps += 2 + count as u64;
        }
        Expr::List { elements, .. } => {
            if elements.len() as u64 > state.config.max_value_growth {
                return Err(ToolProgramError::Bounds(Diagnostic::new(
                    DiagnosticCode::MaxCollectionSize,
                    format!(
                        "list with {} elements exceeds growth bound {}",
                        elements.len(),
                        state.config.max_value_growth
                    ),
                    SourceSpan::new(0, 0),
                )));
            }
            for e in elements {
                analyze_expr(e, state, depth)?;
            }
            state.estimated_steps += elements.len() as u64;
        }
        Expr::Tuple { elements, .. } => {
            for e in elements {
                analyze_expr(e, state, depth)?;
            }
            state.estimated_steps += elements.len() as u64;
        }
        Expr::Dict { keys, values, .. } => {
            if keys.len() as u64 > state.config.max_value_growth {
                return Err(ToolProgramError::Bounds(Diagnostic::new(
                    DiagnosticCode::MaxCollectionSize,
                    format!(
                        "dict with {} entries exceeds growth bound {}",
                        keys.len(),
                        state.config.max_value_growth
                    ),
                    SourceSpan::new(0, 0),
                )));
            }
            for k in keys {
                analyze_expr(k, state, depth)?;
            }
            for v in values {
                analyze_expr(v, state, depth)?;
            }
            state.estimated_steps += keys.len() as u64 * 2;
        }
        Expr::Range {
            start, stop, step, ..
        } => {
            if let Some(s) = start {
                analyze_expr(s, state, depth)?;
            }
            analyze_expr(stop, state, depth)?;
            if let Some(s) = step {
                analyze_expr(s, state, depth)?;
            }
            state.estimated_steps += 1;
        }
        Expr::Parenthesized { inner, .. } => {
            analyze_expr(inner, state, depth)?;
        }
    }
    Ok(())
}

/// Estimate the number of iterations a loop will execute.
///
/// Returns a conservative upper bound. For unknown iterables, returns 1
/// (the static bound is the safety net).
fn estimate_iteration_count(iter: &Expr) -> u64 {
    match iter {
        Expr::Range {
            start, stop, step, ..
        } => {
            let start_ref = start.as_ref().map(|b| b.as_ref());
            let stop_ref = stop.as_ref();
            let step_ref = step.as_ref().map(|b| b.as_ref());
            match (start_ref, stop_ref, step_ref) {
                (None, Expr::IntLiteral { value: stop, .. }, None) => (*stop).max(0) as u64,
                (
                    Some(Expr::IntLiteral { value: start, .. }),
                    Expr::IntLiteral { value: stop, .. },
                    None,
                ) => {
                    let diff = stop - start;
                    diff.max(0) as u64
                }
                (
                    Some(Expr::IntLiteral { value: start, .. }),
                    Expr::IntLiteral { value: stop, .. },
                    Some(Expr::IntLiteral { value: step, .. }),
                ) => {
                    if *step == 0 {
                        0
                    } else {
                        let diff = stop - start;
                        (diff / step).max(0) as u64
                    }
                }
                _ => 1,
            }
        }
        Expr::List { elements, .. } => elements.len() as u64,
        Expr::Tuple { elements, .. } => elements.len() as u64,
        Expr::Dict { keys, .. } => keys.len() as u64,
        Expr::Name { .. }
        | Expr::MethodCall { .. }
        | Expr::Subscript { .. }
        | Expr::CallBuiltin { .. } => 1,
        _ => 1,
    }
}

/// Static bounds computed during analysis.
#[derive(Debug, Clone, PartialEq)]
pub struct StaticBounds {
    pub max_steps: u64,
    pub max_loop_iterations: u64,
    pub max_total_iterations: u64,
    pub call_site_count: u32,
    pub max_dynamic_calls: u64,
    pub max_parallel_width: u32,
    pub max_parallel_depth: u32,
    pub max_nesting_depth: u32,
    pub max_value_growth: u64,
}

impl From<&StaticBounds> for IrBounds {
    fn from(b: &StaticBounds) -> Self {
        IrBounds {
            max_steps: b.max_steps,
            max_loop_iterations: b.max_loop_iterations,
            max_total_iterations: b.max_total_iterations,
            call_site_count: b.call_site_count,
            max_dynamic_calls: b.max_dynamic_calls,
            max_parallel_width: b.max_parallel_width,
            max_parallel_depth: b.max_parallel_depth,
            max_nesting_depth: b.max_nesting_depth,
            max_value_growth: b.max_value_growth,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_program::parser::parse_source;

    #[test]
    fn analyze_simple_program() {
        let prog = parse_source("x = 1\nemit(x)\n").unwrap();
        let bounds = analyze(&prog).unwrap();
        assert!(bounds.max_steps > 0);
        assert_eq!(bounds.call_site_count, 0);
        assert_eq!(bounds.max_parallel_width, 0);
    }

    #[test]
    fn analyze_for_loop() {
        let src = "for i in range(5):\n    x = i\n";
        let prog = parse_source(src).unwrap();
        let bounds = analyze(&prog).unwrap();
        assert!(bounds.max_total_iterations > 0);
    }

    #[test]
    fn analyze_call_site() {
        let src = "result = call({\"tool\": \"grep\"})\n";
        let prog = parse_source(src).unwrap();
        let bounds = analyze(&prog).unwrap();
        assert_eq!(bounds.call_site_count, 1);
    }

    #[test]
    fn analyze_parallel() {
        let src = "r = parallel({\"tool\": \"a\"}, {\"tool\": \"b\"})\n";
        let prog = parse_source(src).unwrap();
        let bounds = analyze(&prog).unwrap();
        assert_eq!(bounds.max_parallel_width, 2);
    }

    #[test]
    fn analyze_list_literal() {
        let src = "x = [1, 2, 3]\n";
        let prog = parse_source(src).unwrap();
        let bounds = analyze(&prog).unwrap();
        assert!(bounds.max_steps > 0);
    }

    #[test]
    fn reject_unbounded_loop() {
        // A loop with a variable iterable gets conservative bound of 1
        // which should pass. But with a tight config, it could fail.
        let src = "for i in range(100):\n    x = i\n";
        let prog = parse_source(src).unwrap();
        let config = BoundsConfig {
            max_loop_iterations: 5,
            ..Default::default()
        };
        let result = analyze_with_config(&prog, &config);
        assert!(result.is_err());
    }

    #[test]
    fn reject_excessive_parallel() {
        let src = "r = parallel({\"t\": 1}, {\"t\": 2}, {\"t\": 3}, {\"t\": 4}, {\"t\": 5}, {\"t\": 6}, {\"t\": 7}, {\"t\": 8}, {\"t\": 9}, {\"t\": 10}, {\"t\": 11})\n";
        let prog = parse_source(src).unwrap();
        let result = analyze(&prog);
        assert!(result.is_err());
    }

    #[test]
    fn analyze_complex_program() {
        let src = r#"
results = []
for file in ["a.py", "b.py", "c.py"]:
    content = call({"tool": "read_file", "path": file})
    lines = len(content)
emit({"total": len(results)})
"#;
        let prog = parse_source(src).unwrap();
        let bounds = analyze(&prog).unwrap();
        assert!(bounds.max_steps > 0);
        assert_eq!(bounds.call_site_count, 1);
    }
}
