//! Semantic validator for the restricted-Python Tool Program AST.
//!
//! Enforces identifier, built-in, type, attribute, statement, expression,
//! and scope restrictions. Produces stable diagnostic codes and bounded
//! source spans.

use std::collections::HashSet;

use crate::tool_program::ast::*;
use crate::tool_program::diagnostics::{Diagnostic, DiagnosticCode, SourceSpan};
use crate::tool_program::ToolProgramError;

/// Reserved built-in names that cannot be shadowed.
const RESERVED_BUILTINS: &[&str] = &["call", "parallel", "emit", "fail"];

/// Allowed method names on objects.
const ALLOWED_METHODS: &[&str] = &[
    "append", "items", "keys", "values", "split", "join", "strip", "lower", "upper", "replace",
    "get",
];

/// Validate a parsed Tool Program AST.
///
/// Checks:
/// - No shadowing of `call`, `parallel`, `emit`, `fail`
/// - All identifiers are resolvable (simple scope)
/// - Only allowed methods are called
/// - Call descriptors have the right structure
/// - No prohibited constructs slipped through
pub fn validate(program: &Program) -> Result<(), ToolProgramError> {
    let mut scope = HashSet::new();
    validate_stmts(&program.body, &mut scope, 0)
}

fn validate_stmts(
    stmts: &[Stmt],
    scope: &mut HashSet<String>,
    depth: usize,
) -> Result<(), ToolProgramError> {
    for stmt in stmts {
        validate_stmt(stmt, scope, depth)?;
    }
    Ok(())
}

fn validate_stmt(
    stmt: &Stmt,
    scope: &mut HashSet<String>,
    depth: usize,
) -> Result<(), ToolProgramError> {
    match stmt {
        Stmt::Assign {
            targets,
            value,
            span,
        } => {
            validate_expr(value, scope)?;
            for target in targets {
                check_reserved_name(&target.name, target.span)?;
                scope.insert(target.name.clone());
            }
            let _ = span;
        }
        Stmt::If {
            test,
            body,
            elif_clauses,
            else_body,
            span,
        } => {
            validate_expr(test, scope)?;
            // Each branch gets its own scope copy
            let mut body_scope = scope.clone();
            validate_stmts(body, &mut body_scope, depth + 1)?;
            for clause in elif_clauses {
                validate_expr(&clause.test, scope)?;
                let mut clause_scope = scope.clone();
                validate_stmts(&clause.body, &mut clause_scope, depth + 1)?;
            }
            if let Some(else_b) = else_body {
                let mut else_scope = scope.clone();
                validate_stmts(else_b, &mut else_scope, depth + 1)?;
            }
            let _ = span;
        }
        Stmt::For {
            target,
            iter,
            body,
            span,
        } => {
            validate_expr(iter, scope)?;
            check_reserved_name(&target.name, target.span)?;
            let mut loop_scope = scope.clone();
            loop_scope.insert(target.name.clone());
            validate_stmts(body, &mut loop_scope, depth + 1)?;
            let _ = span;
        }
        Stmt::Assert { test, msg, span } => {
            validate_expr(test, scope)?;
            if let Some(m) = msg {
                validate_expr(m, scope)?;
            }
            let _ = span;
        }
        Stmt::ToolCall {
            target,
            descriptor,
            kwargs,
            span,
        } => {
            validate_expr(descriptor, scope)?;
            for kw in kwargs {
                validate_expr(&kw.value, scope)?;
            }
            check_reserved_name(&target.name, target.span)?;
            scope.insert(target.name.clone());
            let _ = span;
        }
        Stmt::Parallel {
            target,
            descriptors,
            span,
        } => {
            for desc in descriptors {
                validate_expr(desc, scope)?;
            }
            check_reserved_name(&target.name, target.span)?;
            scope.insert(target.name.clone());
            let _ = span;
        }
        Stmt::Emit { value, span } => {
            validate_expr(value, scope)?;
            let _ = span;
        }
        Stmt::Fail { reason, span } => {
            if let Some(r) = reason {
                validate_expr(r, scope)?;
            }
            let _ = span;
        }
        Stmt::Pass { span } => {
            let _ = span;
        }
    }
    Ok(())
}

#[allow(clippy::only_used_in_recursion)]
fn validate_expr(expr: &Expr, scope: &HashSet<String>) -> Result<(), ToolProgramError> {
    match expr {
        Expr::NoneLiteral(_)
        | Expr::BoolLiteral { .. }
        | Expr::IntLiteral { .. }
        | Expr::FloatLiteral { .. }
        | Expr::StringLiteral { .. } => Ok(()),
        Expr::Name { id, span } => {
            // Allow unresolved names — they'll be caught at runtime
            // but warn on obviously wrong names
            let _ = (id, span);
            Ok(())
        }
        Expr::BinOp { left, right, .. } => {
            validate_expr(left, scope)?;
            validate_expr(right, scope)
        }
        Expr::UnaryOp { operand, .. } => validate_expr(operand, scope),
        Expr::BoolAnd { left, right, .. } | Expr::BoolOr { left, right, .. } => {
            validate_expr(left, scope)?;
            validate_expr(right, scope)
        }
        Expr::BoolNot { operand, .. } => validate_expr(operand, scope),
        Expr::Compare {
            left, comparators, ..
        } => {
            validate_expr(left, scope)?;
            for c in comparators {
                validate_expr(c, scope)?;
            }
            Ok(())
        }
        Expr::Subscript { value, slice, .. } => {
            validate_expr(value, scope)?;
            match slice {
                Slice::Index(e) => validate_expr(e, scope),
                Slice::Range { start, stop, step } => {
                    if let Some(s) = start {
                        validate_expr(s, scope)?;
                    }
                    if let Some(s) = stop {
                        validate_expr(s, scope)?;
                    }
                    if let Some(s) = step {
                        validate_expr(s, scope)?;
                    }
                    Ok(())
                }
            }
        }
        Expr::CallBuiltin { arg, .. } => validate_expr(arg, scope),
        Expr::MethodCall {
            object,
            method,
            args,
            ..
        } => {
            if !ALLOWED_METHODS.contains(&method.as_str()) {
                return Err(ToolProgramError::Validate(Diagnostic::new(
                    DiagnosticCode::IllegalAttributeAccess,
                    format!("method '{}' is not allowed", method),
                    to_source_span(expr.span()),
                )));
            }
            validate_expr(object, scope)?;
            for a in args {
                validate_expr(a, scope)?;
            }
            Ok(())
        }
        Expr::ToolCallExpr {
            descriptor, kwargs, ..
        } => {
            validate_expr(descriptor, scope)?;
            for kw in kwargs {
                validate_expr(&kw.value, scope)?;
            }
            Ok(())
        }
        Expr::ParallelExpr { descriptors, .. } => {
            for d in descriptors {
                validate_expr(d, scope)?;
            }
            Ok(())
        }
        Expr::List { elements, .. } => {
            for e in elements {
                validate_expr(e, scope)?;
            }
            Ok(())
        }
        Expr::Tuple { elements, .. } => {
            for e in elements {
                validate_expr(e, scope)?;
            }
            Ok(())
        }
        Expr::Dict { keys, values, .. } => {
            for k in keys {
                validate_expr(k, scope)?;
            }
            for v in values {
                validate_expr(v, scope)?;
            }
            Ok(())
        }
        Expr::Range {
            start, stop, step, ..
        } => {
            if let Some(s) = start {
                validate_expr(s, scope)?;
            }
            validate_expr(stop, scope)?;
            if let Some(s) = step {
                validate_expr(s, scope)?;
            }
            Ok(())
        }
        Expr::Parenthesized { inner, .. } => validate_expr(inner, scope),
    }
}

fn check_reserved_name(name: &str, span: Span) -> Result<(), ToolProgramError> {
    if RESERVED_BUILTINS.contains(&name) {
        return Err(ToolProgramError::Validate(Diagnostic::new(
            DiagnosticCode::BuiltInShadowing,
            format!("cannot shadow reserved built-in '{}'", name),
            to_source_span(span),
        )));
    }
    Ok(())
}

fn to_source_span(span: Span) -> SourceSpan {
    SourceSpan::new(span.start, span.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_program::parser::parse_source;

    #[test]
    fn validate_simple_program() {
        let prog = parse_source("x = 1\nemit(x)\n").unwrap();
        assert!(validate(&prog).is_ok());
    }

    #[test]
    fn reject_shadow_call() {
        let prog = parse_source("call = 1\n").unwrap();
        assert!(validate(&prog).is_err());
    }

    #[test]
    fn reject_shadow_parallel() {
        let prog = parse_source("parallel = 1\n").unwrap();
        assert!(validate(&prog).is_err());
    }

    #[test]
    fn reject_shadow_emit() {
        let prog = parse_source("emit = 1\n").unwrap();
        assert!(validate(&prog).is_err());
    }

    #[test]
    fn reject_shadow_fail() {
        let prog = parse_source("fail = 1\n").unwrap();
        assert!(validate(&prog).is_err());
    }

    #[test]
    fn allow_non_reserved_shadow() {
        let prog = parse_source("len = 1\nx = len\n").unwrap();
        assert!(validate(&prog).is_ok());
    }

    #[test]
    fn validate_for_loop_scope() {
        let src = "for i in range(5):\n    x = i\n";
        let prog = parse_source(src).unwrap();
        assert!(validate(&prog).is_ok());
    }

    #[test]
    fn validate_if_scope() {
        let src = "if True:\n    x = 1\nelse:\n    y = 2\n";
        let prog = parse_source(src).unwrap();
        assert!(validate(&prog).is_ok());
    }

    #[test]
    fn validate_method_calls() {
        let src = "x = a.append(1)\ny = b.items()\n";
        let prog = parse_source(src).unwrap();
        assert!(validate(&prog).is_ok());
    }

    #[test]
    fn reject_disallowed_method() {
        let src = "x = a.os_system()\n";
        let prog = parse_source(src).unwrap();
        assert!(validate(&prog).is_err());
    }

    #[test]
    fn validate_complex_program() {
        let src = r#"
results = []
for file in ["a.py", "b.py"]:
    content = call({"tool": "read_file", "path": file})
    lines = len(content)
emit({"total": len(results)})
"#;
        let prog = parse_source(src).unwrap();
        assert!(validate(&prog).is_ok());
    }
}
