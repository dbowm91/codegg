//! Parser for the restricted-Python Tool Program language.
//!
//! Uses `rustpython-parser` to parse source into a Python AST, then converts
//! into Codegg-owned normalized AST types. Unsupported constructs are rejected
//! during conversion. The parser never executes source.

use rustpython_parser::ast::{self as pyast, Expr as PyExpr, Mod, Stmt as PyStmt};
use rustpython_parser::{parse, Mode};

use crate::tool_program::ast::*;
use crate::tool_program::diagnostics::{Diagnostic, DiagnosticCode, SourceSpan};
use crate::tool_program::ToolProgramError;

/// Maximum source size in bytes.
const MAX_SOURCE_BYTES: usize = 1024 * 1024;
/// Maximum AST node count.
const MAX_AST_NODES: usize = 10_000;
/// Maximum nesting depth.
const MAX_NESTING_DEPTH: usize = 20;
/// Maximum string literal length.
const MAX_STRING_LENGTH: usize = 10_000;
/// Maximum collection elements.
const MAX_COLLECTION_ELEMENTS: usize = 1_000;

/// Parse restricted-Python source into a normalized AST.
pub fn parse_source(source: &str) -> Result<Program, ToolProgramError> {
    if source.len() > MAX_SOURCE_BYTES {
        return Err(ToolProgramError::Parse(Diagnostic::new(
            DiagnosticCode::SourceTooLarge,
            format!(
                "source too large: {} bytes (max {})",
                source.len(),
                MAX_SOURCE_BYTES
            ),
            SourceSpan::new(0, source.len().min(MAX_SOURCE_BYTES)),
        )));
    }

    let parsed = parse(source, Mode::Module, "<tool_program>").map_err(|e| {
        let msg = format!("{}", e);
        ToolProgramError::Parse(Diagnostic::new(
            DiagnosticCode::UnsupportedSyntax,
            format!("parse error: {}", truncate_message(&msg, 200)),
            SourceSpan::new(0, 0),
        ))
    })?;

    let module = match parsed {
        Mod::Module(m) => m,
        _ => {
            return Err(ToolProgramError::Parse(Diagnostic::new(
                DiagnosticCode::UnsupportedSyntax,
                "expected module".to_string(),
                SourceSpan::new(0, 0),
            )));
        }
    };

    convert_module(module, source)
}

/// Convert a Python AST module into our normalized AST.
fn convert_module(module: pyast::ModModule, source: &str) -> Result<Program, ToolProgramError> {
    let mut node_count = 0usize;
    let body = convert_stmts(&module.body, source, &mut node_count, 0)?;
    Ok(Program {
        body,
        span: Span::new(0, source.len()),
    })
}

fn convert_stmts(
    stmts: &[PyStmt],
    source: &str,
    node_count: &mut usize,
    depth: usize,
) -> Result<Vec<Stmt>, ToolProgramError> {
    let mut result = Vec::with_capacity(stmts.len());
    for stmt in stmts {
        result.push(convert_stmt(stmt, source, node_count, depth)?);
    }
    Ok(result)
}

fn check_depth(depth: usize) -> Result<(), ToolProgramError> {
    if depth > MAX_NESTING_DEPTH {
        return Err(ToolProgramError::Validate(Diagnostic::new(
            DiagnosticCode::MaxNestingDepth,
            format!(
                "nesting depth {} exceeds maximum {}",
                depth, MAX_NESTING_DEPTH
            ),
            SourceSpan::new(0, 0),
        )));
    }
    Ok(())
}

fn check_node_count(node_count: &mut usize) -> Result<(), ToolProgramError> {
    *node_count += 1;
    if *node_count > MAX_AST_NODES {
        return Err(ToolProgramError::Parse(Diagnostic::new(
            DiagnosticCode::MaxAstNodes,
            format!(
                "AST node count {} exceeds maximum {}",
                node_count, MAX_AST_NODES
            ),
            SourceSpan::new(0, 0),
        )));
    }
    Ok(())
}

fn convert_stmt(
    stmt: &PyStmt,
    source: &str,
    node_count: &mut usize,
    depth: usize,
) -> Result<Stmt, ToolProgramError> {
    check_depth(depth)?;
    check_node_count(node_count)?;

    match stmt {
        PyStmt::Assign(a) => {
            let value = convert_expr(&a.value, source, node_count, depth)?;
            let mut targets = Vec::new();
            for target in &a.targets {
                match target {
                    PyExpr::Name(n) => {
                        targets.push(Ident {
                            name: n.id.to_string(),
                            span: Span::new(0, 0),
                        });
                    }
                    _ => {
                        return Err(ToolProgramError::Parse(Diagnostic::new(
                            DiagnosticCode::UnsupportedSyntax,
                            "unsupported assignment target".to_string(),
                            SourceSpan::new(0, 0),
                        )));
                    }
                }
            }
            Ok(Stmt::Assign {
                targets,
                value,
                span: Span::new(0, 0),
            })
        }
        PyStmt::AnnAssign(_) => Err(ToolProgramError::Parse(Diagnostic::new(
            DiagnosticCode::UnsupportedSyntax,
            "annotated assignment not supported".to_string(),
            SourceSpan::new(0, 0),
        ))),
        PyStmt::If(i) => {
            let test = convert_expr(&i.test, source, node_count, depth + 1)?;
            let body = convert_stmts(&i.body, source, node_count, depth + 1)?;
            let mut elif_clauses = Vec::new();
            let mut current_else = i.orelse.as_slice();
            while !current_else.is_empty() {
                if let Some(PyStmt::If(elif_if)) = current_else.first() {
                    let elif_test = convert_expr(&elif_if.test, source, node_count, depth + 1)?;
                    let elif_body = convert_stmts(&elif_if.body, source, node_count, depth + 1)?;
                    elif_clauses.push(ElifClause {
                        test: elif_test,
                        body: elif_body,
                        span: Span::new(0, 0),
                    });
                    current_else = &elif_if.orelse;
                    continue;
                }
                break;
            }
            let else_body = if current_else.is_empty() {
                None
            } else {
                Some(convert_stmts(current_else, source, node_count, depth + 1)?)
            };
            Ok(Stmt::If {
                test,
                body,
                elif_clauses,
                else_body,
                span: Span::new(0, 0),
            })
        }
        PyStmt::For(f) => {
            let target = match f.target.as_ref() {
                PyExpr::Name(n) => Ident {
                    name: n.id.to_string(),
                    span: Span::new(0, 0),
                },
                _ => {
                    return Err(ToolProgramError::Parse(Diagnostic::new(
                        DiagnosticCode::UnsupportedSyntax,
                        "for-loop target must be a simple name".to_string(),
                        SourceSpan::new(0, 0),
                    )));
                }
            };
            let iter = convert_expr(&f.iter, source, node_count, depth + 1)?;
            let body = convert_stmts(&f.body, source, node_count, depth + 1)?;
            Ok(Stmt::For {
                target,
                iter,
                body,
                span: Span::new(0, 0),
            })
        }
        PyStmt::While(_) => Err(ToolProgramError::Parse(Diagnostic::new(
            DiagnosticCode::UnsupportedSyntax,
            "while loops are not supported".to_string(),
            SourceSpan::new(0, 0),
        ))),
        PyStmt::Assert(a) => {
            let test = convert_expr(&a.test, source, node_count, depth)?;
            let msg = a
                .msg
                .as_ref()
                .map(|m| convert_expr(m, source, node_count, depth))
                .transpose()?;
            Ok(Stmt::Assert {
                test,
                msg,
                span: Span::new(0, 0),
            })
        }
        PyStmt::Expr(e) => convert_expr_stmt(&e.value, source, node_count, depth),
        PyStmt::Pass(_) => Ok(Stmt::Pass {
            span: Span::new(0, 0),
        }),
        PyStmt::Return(_) => Err(ToolProgramError::Parse(Diagnostic::new(
            DiagnosticCode::UnsupportedSyntax,
            "return statements are not supported".to_string(),
            SourceSpan::new(0, 0),
        ))),
        PyStmt::Import(_) | PyStmt::ImportFrom(_) => Err(ToolProgramError::Parse(Diagnostic::new(
            DiagnosticCode::UnsupportedSyntax,
            "import statements are not supported".to_string(),
            SourceSpan::new(0, 0),
        ))),
        PyStmt::FunctionDef(_) | PyStmt::AsyncFunctionDef(_) => {
            Err(ToolProgramError::Parse(Diagnostic::new(
                DiagnosticCode::UnsupportedSyntax,
                "function definitions are not supported".to_string(),
                SourceSpan::new(0, 0),
            )))
        }
        PyStmt::ClassDef(_) => Err(ToolProgramError::Parse(Diagnostic::new(
            DiagnosticCode::UnsupportedSyntax,
            "class definitions are not supported".to_string(),
            SourceSpan::new(0, 0),
        ))),
        PyStmt::Try(_) | PyStmt::TryStar(_) => Err(ToolProgramError::Parse(Diagnostic::new(
            DiagnosticCode::UnsupportedSyntax,
            "try/except is not supported".to_string(),
            SourceSpan::new(0, 0),
        ))),
        PyStmt::Raise(_) => Err(ToolProgramError::Parse(Diagnostic::new(
            DiagnosticCode::UnsupportedSyntax,
            "raise statements are not supported".to_string(),
            SourceSpan::new(0, 0),
        ))),
        PyStmt::Delete(_) => Err(ToolProgramError::Parse(Diagnostic::new(
            DiagnosticCode::UnsupportedSyntax,
            "del statements are not supported".to_string(),
            SourceSpan::new(0, 0),
        ))),
        PyStmt::Global(_) | PyStmt::Nonlocal(_) => Err(ToolProgramError::Parse(Diagnostic::new(
            DiagnosticCode::UnsupportedSyntax,
            "global/nonlocal statements are not supported".to_string(),
            SourceSpan::new(0, 0),
        ))),
        PyStmt::With(_) | PyStmt::AsyncWith(_) => Err(ToolProgramError::Parse(Diagnostic::new(
            DiagnosticCode::UnsupportedSyntax,
            "with statements are not supported".to_string(),
            SourceSpan::new(0, 0),
        ))),
        PyStmt::Match(_) => Err(ToolProgramError::Parse(Diagnostic::new(
            DiagnosticCode::UnsupportedSyntax,
            "match statements are not supported".to_string(),
            SourceSpan::new(0, 0),
        ))),
        PyStmt::Break(_) | PyStmt::Continue(_) => Err(ToolProgramError::Parse(Diagnostic::new(
            DiagnosticCode::UnsupportedSyntax,
            "break/continue are not supported".to_string(),
            SourceSpan::new(0, 0),
        ))),
        PyStmt::AugAssign(_) => Err(ToolProgramError::Parse(Diagnostic::new(
            DiagnosticCode::UnsupportedSyntax,
            "augmented assignment (+=, -=, etc.) is not supported".to_string(),
            SourceSpan::new(0, 0),
        ))),
        PyStmt::TypeAlias(_) => Err(ToolProgramError::Parse(Diagnostic::new(
            DiagnosticCode::UnsupportedSyntax,
            "type alias statements are not supported".to_string(),
            SourceSpan::new(0, 0),
        ))),
        PyStmt::AsyncFor(_) => Err(ToolProgramError::Parse(Diagnostic::new(
            DiagnosticCode::UnsupportedSyntax,
            "async for is not supported".to_string(),
            SourceSpan::new(0, 0),
        ))),
    }
}

/// Convert an expression statement, recognizing call(), emit(), fail().
fn convert_expr_stmt(
    expr: &PyExpr,
    source: &str,
    node_count: &mut usize,
    depth: usize,
) -> Result<Stmt, ToolProgramError> {
    match expr {
        PyExpr::Call(c) => {
            if let PyExpr::Name(n) = c.func.as_ref() {
                if n.id.as_str() == "emit" {
                    if c.args.len() != 1 {
                        return Err(ToolProgramError::Parse(Diagnostic::new(
                            DiagnosticCode::UnsupportedSyntax,
                            "emit() requires exactly one argument".to_string(),
                            SourceSpan::new(0, 0),
                        )));
                    }
                    let value = convert_expr(&c.args[0], source, node_count, depth)?;
                    return Ok(Stmt::Emit {
                        value,
                        span: Span::new(0, 0),
                    });
                }
                if n.id.as_str() == "fail" {
                    let reason = if c.args.is_empty() {
                        None
                    } else if c.args.len() == 1 {
                        Some(convert_expr(&c.args[0], source, node_count, depth)?)
                    } else {
                        return Err(ToolProgramError::Parse(Diagnostic::new(
                            DiagnosticCode::UnsupportedSyntax,
                            "fail() accepts at most one argument".to_string(),
                            SourceSpan::new(0, 0),
                        )));
                    };
                    return Ok(Stmt::Fail {
                        reason,
                        span: Span::new(0, 0),
                    });
                }
            }
            Err(ToolProgramError::Parse(Diagnostic::new(
                DiagnosticCode::UnsupportedSyntax,
                "expression statements must be assignment, call(), emit(), or fail()".to_string(),
                SourceSpan::new(0, 0),
            )))
        }
        _ => Err(ToolProgramError::Parse(Diagnostic::new(
            DiagnosticCode::UnsupportedSyntax,
            "expression statements must be assignment, call(), emit(), or fail()".to_string(),
            SourceSpan::new(0, 0),
        ))),
    }
}

/// Convert a Python expression into our normalized AST.
fn convert_expr(
    expr: &PyExpr,
    source: &str,
    node_count: &mut usize,
    depth: usize,
) -> Result<Expr, ToolProgramError> {
    check_node_count(node_count)?;

    match expr {
        PyExpr::Constant(c) => match &c.value {
            pyast::Constant::None => Ok(Expr::NoneLiteral(Span::new(0, 0))),
            pyast::Constant::Bool(b) => Ok(Expr::BoolLiteral {
                value: *b,
                span: Span::new(0, 0),
            }),
            pyast::Constant::Int(i) => {
                let val: i64 = i.try_into().map_err(|_| {
                    ToolProgramError::Parse(Diagnostic::new(
                        DiagnosticCode::UnsupportedSyntax,
                        "integer literal out of range".to_string(),
                        SourceSpan::new(0, 0),
                    ))
                })?;
                Ok(Expr::IntLiteral {
                    value: val,
                    span: Span::new(0, 0),
                })
            }
            pyast::Constant::Float(f) => Ok(Expr::FloatLiteral {
                value: *f,
                span: Span::new(0, 0),
            }),
            pyast::Constant::Str(s) => {
                if s.len() > MAX_STRING_LENGTH {
                    return Err(ToolProgramError::Parse(Diagnostic::new(
                        DiagnosticCode::MaxCollectionSize,
                        format!(
                            "string literal length {} exceeds maximum {}",
                            s.len(),
                            MAX_STRING_LENGTH
                        ),
                        SourceSpan::new(0, 0),
                    )));
                }
                Ok(Expr::StringLiteral {
                    value: s.clone(),
                    span: Span::new(0, 0),
                })
            }
            pyast::Constant::Bytes(_) => Err(ToolProgramError::Parse(Diagnostic::new(
                DiagnosticCode::UnsupportedSyntax,
                "bytes literals are not supported".to_string(),
                SourceSpan::new(0, 0),
            ))),
            pyast::Constant::Tuple(_) => Err(ToolProgramError::Parse(Diagnostic::new(
                DiagnosticCode::UnsupportedSyntax,
                "constant tuples not supported as literals".to_string(),
                SourceSpan::new(0, 0),
            ))),
            pyast::Constant::Complex { .. } => Err(ToolProgramError::Parse(Diagnostic::new(
                DiagnosticCode::UnsupportedSyntax,
                "complex/imaginary literals not supported".to_string(),
                SourceSpan::new(0, 0),
            ))),
            pyast::Constant::Ellipsis => Err(ToolProgramError::Parse(Diagnostic::new(
                DiagnosticCode::UnsupportedSyntax,
                "ellipsis literal not supported".to_string(),
                SourceSpan::new(0, 0),
            ))),
        },
        PyExpr::Name(n) => Ok(Expr::Name {
            id: n.id.to_string(),
            span: Span::new(0, 0),
        }),
        PyExpr::BinOp(b) => {
            let left = convert_expr(&b.left, source, node_count, depth)?;
            let right = convert_expr(&b.right, source, node_count, depth)?;
            let op = convert_binop(b.op);
            Ok(Expr::BinOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
                span: Span::new(0, 0),
            })
        }
        PyExpr::UnaryOp(u) => {
            let operand = convert_expr(&u.operand, source, node_count, depth)?;
            let op = match u.op {
                pyast::UnaryOp::USub => UnaryOpKind::Neg,
                pyast::UnaryOp::UAdd => UnaryOpKind::Pos,
                pyast::UnaryOp::Invert => UnaryOpKind::Invert,
                pyast::UnaryOp::Not => {
                    // 'not' as unary operator — compile as boolean not
                    // This is allowed for `not expr` patterns
                    let operand = convert_expr(&u.operand, source, node_count, depth)?;
                    return Ok(Expr::BoolNot {
                        operand: Box::new(operand),
                        span: Span::new(0, 0),
                    });
                }
            };
            Ok(Expr::UnaryOp {
                op,
                operand: Box::new(operand),
                span: Span::new(0, 0),
            })
        }
        PyExpr::BoolOp(b) => {
            if b.values.len() < 2 {
                return Err(ToolProgramError::Parse(Diagnostic::new(
                    DiagnosticCode::UnsupportedSyntax,
                    "boolean operator requires at least two operands".to_string(),
                    SourceSpan::new(0, 0),
                )));
            }
            let mut result = convert_expr(&b.values[0], source, node_count, depth)?;
            for val in &b.values[1..] {
                let right = convert_expr(val, source, node_count, depth)?;
                result = match b.op {
                    pyast::BoolOp::And => Expr::BoolAnd {
                        left: Box::new(result),
                        right: Box::new(right),
                        span: Span::new(0, 0),
                    },
                    pyast::BoolOp::Or => Expr::BoolOr {
                        left: Box::new(result),
                        right: Box::new(right),
                        span: Span::new(0, 0),
                    },
                };
            }
            Ok(result)
        }
        PyExpr::Compare(c) => {
            let left = convert_expr(&c.left, source, node_count, depth)?;
            let mut ops = Vec::new();
            let mut comparators = Vec::new();
            for (op, comp) in c.ops.iter().zip(c.comparators.iter()) {
                ops.push(convert_cmpop(op)?);
                comparators.push(convert_expr(comp, source, node_count, depth)?);
            }
            Ok(Expr::Compare {
                left: Box::new(left),
                ops,
                comparators,
                span: Span::new(0, 0),
            })
        }
        PyExpr::Call(c) => {
            if let PyExpr::Name(n) = c.func.as_ref() {
                match n.id.as_str() {
                    "len" | "str" | "int" | "bool" => {
                        if c.args.len() != 1 {
                            return Err(ToolProgramError::Parse(Diagnostic::new(
                                DiagnosticCode::UnsupportedSyntax,
                                format!("{}() requires exactly one argument", n.id),
                                SourceSpan::new(0, 0),
                            )));
                        }
                        let func = match n.id.as_str() {
                            "len" => BuiltinFunc::Len,
                            "str" => BuiltinFunc::Str,
                            "int" => BuiltinFunc::Int,
                            "bool" => BuiltinFunc::Bool,
                            _ => unreachable!(),
                        };
                        let arg = convert_expr(&c.args[0], source, node_count, depth)?;
                        return Ok(Expr::CallBuiltin {
                            func,
                            arg: Box::new(arg),
                            span: Span::new(0, 0),
                        });
                    }
                    "call" => {
                        if c.args.is_empty() {
                            return Err(ToolProgramError::Parse(Diagnostic::new(
                                DiagnosticCode::InvalidCallDescriptor,
                                "call() requires a descriptor dict argument".to_string(),
                                SourceSpan::new(0, 0),
                            )));
                        }
                        let descriptor = convert_expr(&c.args[0], source, node_count, depth)?;
                        let mut kwargs = Vec::new();
                        for kw in &c.keywords {
                            if let Some(name) = &kw.arg {
                                kwargs.push(KwArg {
                                    name: name.to_string(),
                                    value: convert_expr(&kw.value, source, node_count, depth)?,
                                    span: Span::new(0, 0),
                                });
                            } else {
                                return Err(ToolProgramError::Parse(Diagnostic::new(
                                    DiagnosticCode::UnsupportedSyntax,
                                    "**kwargs unpacking not supported".to_string(),
                                    SourceSpan::new(0, 0),
                                )));
                            }
                        }
                        return Ok(Expr::ToolCallExpr {
                            descriptor: Box::new(descriptor),
                            kwargs,
                            span: Span::new(0, 0),
                        });
                    }
                    "parallel" => {
                        let mut descriptors = Vec::new();
                        for arg in &c.args {
                            descriptors.push(convert_expr(arg, source, node_count, depth)?);
                        }
                        return Ok(Expr::ParallelExpr {
                            descriptors,
                            span: Span::new(0, 0),
                        });
                    }
                    "range" => {
                        let (start, stop, step) = match c.args.len() {
                            1 => (
                                None,
                                convert_expr(&c.args[0], source, node_count, depth)?,
                                None,
                            ),
                            2 => (
                                Some(convert_expr(&c.args[0], source, node_count, depth)?),
                                convert_expr(&c.args[1], source, node_count, depth)?,
                                None,
                            ),
                            3 => (
                                Some(convert_expr(&c.args[0], source, node_count, depth)?),
                                convert_expr(&c.args[1], source, node_count, depth)?,
                                Some(convert_expr(&c.args[2], source, node_count, depth)?),
                            ),
                            _ => {
                                return Err(ToolProgramError::Parse(Diagnostic::new(
                                    DiagnosticCode::UnsupportedSyntax,
                                    "range() accepts 1-3 arguments".to_string(),
                                    SourceSpan::new(0, 0),
                                )));
                            }
                        };
                        return Ok(Expr::Range {
                            start: start.map(Box::new),
                            stop: Box::new(stop),
                            step: step.map(Box::new),
                            span: Span::new(0, 0),
                        });
                    }
                    _ => {}
                }
            }
            // Method calls: obj.method(args)
            if let PyExpr::Attribute(a) = c.func.as_ref() {
                let object = convert_expr(&a.value, source, node_count, depth)?;
                let method = a.attr.to_string();
                let mut args = Vec::new();
                for arg in &c.args {
                    args.push(convert_expr(arg, source, node_count, depth)?);
                }
                return Ok(Expr::MethodCall {
                    object: Box::new(object),
                    method,
                    args,
                    span: Span::new(0, 0),
                });
            }
            Err(ToolProgramError::Parse(Diagnostic::new(
                DiagnosticCode::UnsupportedSyntax,
                "unsupported function call".to_string(),
                SourceSpan::new(0, 0),
            )))
        }
        PyExpr::Attribute(a) => {
            let object = convert_expr(&a.value, source, node_count, depth)?;
            match a.attr.as_str() {
                "append" | "items" | "keys" | "values" | "split" | "join" | "strip" | "lower"
                | "upper" | "replace" | "get" => Ok(Expr::MethodCall {
                    object: Box::new(object),
                    method: a.attr.to_string(),
                    args: vec![],
                    span: Span::new(0, 0),
                }),
                _ => Err(ToolProgramError::Parse(Diagnostic::new(
                    DiagnosticCode::IllegalAttributeAccess,
                    format!("attribute '{}' is not allowed", a.attr),
                    SourceSpan::new(0, 0),
                ))),
            }
        }
        PyExpr::Subscript(s) => {
            let value = convert_expr(&s.value, source, node_count, depth)?;
            let slice = convert_slice(&s.slice, source, node_count, depth)?;
            Ok(Expr::Subscript {
                value: Box::new(value),
                slice,
                span: Span::new(0, 0),
            })
        }
        PyExpr::Slice(sl) => {
            let start = sl
                .lower
                .as_ref()
                .map(|e| convert_expr(e, source, node_count, depth))
                .transpose()?;
            let stop = sl
                .upper
                .as_ref()
                .map(|e| convert_expr(e, source, node_count, depth))
                .transpose()?;
            let step = sl
                .step
                .as_ref()
                .map(|e| convert_expr(e, source, node_count, depth))
                .transpose()?;
            Ok(Expr::Subscript {
                value: Box::new(Expr::NoneLiteral(Span::new(0, 0))),
                slice: Slice::Range {
                    start: start.map(Box::new),
                    stop: stop.map(Box::new),
                    step: step.map(Box::new),
                },
                span: Span::new(0, 0),
            })
        }
        PyExpr::List(l) => {
            if l.elts.len() > MAX_COLLECTION_ELEMENTS {
                return Err(ToolProgramError::Parse(Diagnostic::new(
                    DiagnosticCode::MaxCollectionSize,
                    format!(
                        "list with {} elements exceeds maximum {}",
                        l.elts.len(),
                        MAX_COLLECTION_ELEMENTS
                    ),
                    SourceSpan::new(0, 0),
                )));
            }
            let elements = l
                .elts
                .iter()
                .map(|e| convert_expr(e, source, node_count, depth))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Expr::List {
                elements,
                span: Span::new(0, 0),
            })
        }
        PyExpr::Tuple(t) => {
            if t.elts.len() > MAX_COLLECTION_ELEMENTS {
                return Err(ToolProgramError::Parse(Diagnostic::new(
                    DiagnosticCode::MaxCollectionSize,
                    format!(
                        "tuple with {} elements exceeds maximum {}",
                        t.elts.len(),
                        MAX_COLLECTION_ELEMENTS
                    ),
                    SourceSpan::new(0, 0),
                )));
            }
            let elements = t
                .elts
                .iter()
                .map(|e| convert_expr(e, source, node_count, depth))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Expr::Tuple {
                elements,
                span: Span::new(0, 0),
            })
        }
        PyExpr::Dict(d) => {
            if d.keys.len() > MAX_COLLECTION_ELEMENTS {
                return Err(ToolProgramError::Parse(Diagnostic::new(
                    DiagnosticCode::MaxCollectionSize,
                    format!(
                        "dict with {} entries exceeds maximum {}",
                        d.keys.len(),
                        MAX_COLLECTION_ELEMENTS
                    ),
                    SourceSpan::new(0, 0),
                )));
            }
            let mut keys = Vec::new();
            let mut values = Vec::new();
            for (k, v) in d.keys.iter().zip(d.values.iter()) {
                let key = k
                    .as_ref()
                    .map(|k| convert_expr(k, source, node_count, depth))
                    .transpose()?;
                let value = convert_expr(v, source, node_count, depth)?;
                keys.push(key.unwrap_or(Expr::NoneLiteral(Span::new(0, 0))));
                values.push(value);
            }
            Ok(Expr::Dict {
                keys,
                values,
                span: Span::new(0, 0),
            })
        }
        PyExpr::Set(_) => Err(ToolProgramError::Parse(Diagnostic::new(
            DiagnosticCode::UnsupportedSyntax,
            "set literals are not supported".to_string(),
            SourceSpan::new(0, 0),
        ))),
        PyExpr::NamedExpr(_) => Err(ToolProgramError::Parse(Diagnostic::new(
            DiagnosticCode::UnsupportedSyntax,
            "walrus operator (:=) is not supported".to_string(),
            SourceSpan::new(0, 0),
        ))),
        PyExpr::Lambda(_) => Err(ToolProgramError::Parse(Diagnostic::new(
            DiagnosticCode::UnsupportedSyntax,
            "lambda expressions are not supported".to_string(),
            SourceSpan::new(0, 0),
        ))),
        PyExpr::IfExp(_) => Err(ToolProgramError::Parse(Diagnostic::new(
            DiagnosticCode::UnsupportedSyntax,
            "conditional expressions (ternary) are not supported in version 1".to_string(),
            SourceSpan::new(0, 0),
        ))),
        PyExpr::ListComp(_)
        | PyExpr::SetComp(_)
        | PyExpr::DictComp(_)
        | PyExpr::GeneratorExp(_) => Err(ToolProgramError::Parse(Diagnostic::new(
            DiagnosticCode::UnsupportedSyntax,
            "comprehensions/generators are not supported".to_string(),
            SourceSpan::new(0, 0),
        ))),
        PyExpr::Await(_) => Err(ToolProgramError::Parse(Diagnostic::new(
            DiagnosticCode::UnsupportedSyntax,
            "await expressions are not supported".to_string(),
            SourceSpan::new(0, 0),
        ))),
        PyExpr::Yield(_) | PyExpr::YieldFrom(_) => Err(ToolProgramError::Parse(Diagnostic::new(
            DiagnosticCode::UnsupportedSyntax,
            "yield expressions are not supported".to_string(),
            SourceSpan::new(0, 0),
        ))),
        PyExpr::FormattedValue(_) | PyExpr::JoinedStr(_) => {
            Err(ToolProgramError::Parse(Diagnostic::new(
                DiagnosticCode::UnsupportedSyntax,
                "f-string expressions are not supported".to_string(),
                SourceSpan::new(0, 0),
            )))
        }
        PyExpr::Starred(_) => Err(ToolProgramError::Parse(Diagnostic::new(
            DiagnosticCode::UnsupportedSyntax,
            "starred expressions are not supported".to_string(),
            SourceSpan::new(0, 0),
        ))),
    }
}

fn convert_slice(
    slice: &PyExpr,
    source: &str,
    node_count: &mut usize,
    depth: usize,
) -> Result<Slice, ToolProgramError> {
    match slice {
        PyExpr::Slice(sl) => {
            let start = sl
                .lower
                .as_ref()
                .map(|e| convert_expr(e, source, node_count, depth))
                .transpose()?;
            let stop = sl
                .upper
                .as_ref()
                .map(|e| convert_expr(e, source, node_count, depth))
                .transpose()?;
            let step = sl
                .step
                .as_ref()
                .map(|e| convert_expr(e, source, node_count, depth))
                .transpose()?;
            Ok(Slice::Range {
                start: start.map(Box::new),
                stop: stop.map(Box::new),
                step: step.map(Box::new),
            })
        }
        _ => Ok(Slice::Index(Box::new(convert_expr(
            slice, source, node_count, depth,
        )?))),
    }
}

fn convert_binop(op: pyast::Operator) -> BinOpKind {
    match op {
        pyast::Operator::Add => BinOpKind::Add,
        pyast::Operator::Sub => BinOpKind::Sub,
        pyast::Operator::Mult => BinOpKind::Mul,
        pyast::Operator::Div => BinOpKind::Div,
        pyast::Operator::FloorDiv => BinOpKind::FloorDiv,
        pyast::Operator::Mod => BinOpKind::Mod,
        pyast::Operator::Pow => BinOpKind::Pow,
        pyast::Operator::MatMult => BinOpKind::MatMul,
        pyast::Operator::BitOr => BinOpKind::BitOr,
        pyast::Operator::BitXor => BinOpKind::BitXor,
        pyast::Operator::BitAnd => BinOpKind::BitAnd,
        pyast::Operator::LShift => BinOpKind::LShift,
        pyast::Operator::RShift => BinOpKind::RShift,
    }
}

fn convert_cmpop(op: &pyast::CmpOp) -> Result<CmpOp, ToolProgramError> {
    match op {
        pyast::CmpOp::Eq => Ok(CmpOp::Eq),
        pyast::CmpOp::NotEq => Ok(CmpOp::NotEq),
        pyast::CmpOp::Lt => Ok(CmpOp::Lt),
        pyast::CmpOp::LtE => Ok(CmpOp::LtE),
        pyast::CmpOp::Gt => Ok(CmpOp::Gt),
        pyast::CmpOp::GtE => Ok(CmpOp::GtE),
        pyast::CmpOp::In => Ok(CmpOp::In),
        pyast::CmpOp::NotIn => Ok(CmpOp::NotIn),
        pyast::CmpOp::Is | pyast::CmpOp::IsNot => Err(ToolProgramError::Parse(Diagnostic::new(
            DiagnosticCode::UnsupportedSyntax,
            "identity comparison (is/is not) is not supported".to_string(),
            SourceSpan::new(0, 0),
        ))),
    }
}

/// Truncate a message to a maximum length.
fn truncate_message(msg: &str, max_len: usize) -> String {
    if msg.len() <= max_len {
        msg.to_string()
    } else {
        format!("{}...", &msg[..max_len])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_assignment() {
        let prog = parse_source("x = 1\n").unwrap();
        assert_eq!(prog.body.len(), 1);
        assert!(matches!(&prog.body[0], Stmt::Assign { .. }));
    }

    #[test]
    fn parse_emit() {
        let prog = parse_source("emit({\"result\": \"ok\"})\n").unwrap();
        assert_eq!(prog.body.len(), 1);
        assert!(matches!(&prog.body[0], Stmt::Emit { .. }));
    }

    #[test]
    fn parse_for_range() {
        let prog = parse_source("for i in range(5):\n    x = i\n").unwrap();
        assert_eq!(prog.body.len(), 1);
        assert!(matches!(&prog.body[0], Stmt::For { .. }));
    }

    #[test]
    fn parse_if_else() {
        let src = "if x > 0:\n    y = 1\nelse:\n    y = 0\n";
        let prog = parse_source(src).unwrap();
        assert_eq!(prog.body.len(), 1);
        match &prog.body[0] {
            Stmt::If { else_body, .. } => assert!(else_body.is_some()),
            _ => panic!("expected If"),
        }
    }

    #[test]
    fn parse_elif() {
        let src = "if x > 0:\n    y = 1\nelif x < 0:\n    y = -1\nelse:\n    y = 0\n";
        let prog = parse_source(src).unwrap();
        match &prog.body[0] {
            Stmt::If {
                elif_clauses,
                else_body,
                ..
            } => {
                assert_eq!(elif_clauses.len(), 1);
                assert!(else_body.is_some());
            }
            _ => panic!("expected If"),
        }
    }

    #[test]
    fn reject_import() {
        assert!(parse_source("import os\n").is_err());
    }

    #[test]
    fn reject_while() {
        assert!(parse_source("while True:\n    pass\n").is_err());
    }

    #[test]
    fn reject_function_def() {
        assert!(parse_source("def foo():\n    pass\n").is_err());
    }

    #[test]
    fn reject_class() {
        assert!(parse_source("class Foo:\n    pass\n").is_err());
    }

    #[test]
    fn reject_try() {
        assert!(parse_source("try:\n    pass\nexcept:\n    pass\n").is_err());
    }

    #[test]
    fn reject_lambda() {
        assert!(parse_source("f = lambda x: x + 1\n").is_err());
    }

    #[test]
    fn reject_comprehension() {
        assert!(parse_source("x = [i for i in range(10)]\n").is_err());
    }

    #[test]
    fn reject_fstring() {
        assert!(parse_source("x = f\"hello {name}\"\n").is_err());
    }

    #[test]
    fn reject_augmented_assign() {
        assert!(parse_source("x += 1\n").is_err());
    }

    #[test]
    fn reject_del() {
        assert!(parse_source("del x\n").is_err());
    }

    #[test]
    fn reject_global() {
        assert!(parse_source("global x\n").is_err());
    }

    #[test]
    fn reject_return() {
        assert!(parse_source("return x\n").is_err());
    }

    #[test]
    fn reject_with() {
        assert!(parse_source("with open('f') as fh:\n    pass\n").is_err());
    }

    #[test]
    fn reject_yield() {
        assert!(parse_source("yield x\n").is_err());
    }

    #[test]
    fn parse_builtin_len() {
        let prog = parse_source("x = len(a)\n").unwrap();
        match &prog.body[0] {
            Stmt::Assign { value, .. } => {
                assert!(matches!(
                    value,
                    Expr::CallBuiltin {
                        func: BuiltinFunc::Len,
                        ..
                    }
                ));
            }
            _ => panic!("expected Assign"),
        }
    }

    #[test]
    fn parse_method_call() {
        let prog = parse_source("x = a.append(1)\n").unwrap();
        match &prog.body[0] {
            Stmt::Assign { value, .. } => {
                assert!(matches!(value, Expr::MethodCall { .. }));
            }
            _ => panic!("expected Assign"),
        }
    }

    #[test]
    fn parse_subscript() {
        let prog = parse_source("x = a[0]\n").unwrap();
        match &prog.body[0] {
            Stmt::Assign { value, .. } => {
                assert!(matches!(value, Expr::Subscript { .. }));
            }
            _ => panic!("expected Assign"),
        }
    }

    #[test]
    fn parse_slice() {
        let prog = parse_source("x = a[1:3]\n").unwrap();
        match &prog.body[0] {
            Stmt::Assign { value, .. } => match value {
                Expr::Subscript { slice, .. } => {
                    assert!(matches!(slice, Slice::Range { .. }));
                }
                _ => panic!("expected Subscript"),
            },
            _ => panic!("expected Assign"),
        }
    }

    #[test]
    fn parse_dict_literal() {
        let prog = parse_source("x = {\"a\": 1, \"b\": 2}\n").unwrap();
        match &prog.body[0] {
            Stmt::Assign { value, .. } => {
                assert!(matches!(value, Expr::Dict { .. }));
            }
            _ => panic!("expected Assign"),
        }
    }

    #[test]
    fn parse_parallel() {
        let src = "reads = parallel({\"tool\": \"a\"}, {\"tool\": \"b\"})\n";
        let prog = parse_source(src).unwrap();
        match &prog.body[0] {
            Stmt::Assign { value, .. } => {
                assert!(matches!(value, Expr::ParallelExpr { .. }));
            }
            _ => panic!("expected Assign"),
        }
    }

    #[test]
    fn parse_tool_call() {
        let src = "result = call({\"tool\": \"grep_search\", \"pattern\": \"TODO\"})\n";
        let prog = parse_source(src).unwrap();
        match &prog.body[0] {
            Stmt::Assign { value, .. } => {
                assert!(matches!(value, Expr::ToolCallExpr { .. }));
            }
            _ => panic!("expected Assign"),
        }
    }

    #[test]
    fn reject_set_literal() {
        assert!(parse_source("x = {1, 2, 3}\n").is_err());
    }

    #[test]
    fn reject_is_comparison() {
        assert!(parse_source("x = a is b\n").is_err());
    }

    #[test]
    fn source_too_large() {
        let src = "x = 1\n".repeat(200_000);
        assert!(parse_source(&src).is_err());
    }
}
