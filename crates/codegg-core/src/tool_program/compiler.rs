//! IR compiler for the restricted-Python Tool Program language.
//!
//! Compiles a validated, bounds-analyzed AST into a deterministic,
//! versioned IR with source-span mapping.

use sha2::{Digest, Sha256};

use crate::tool_program::ast::*;
use crate::tool_program::diagnostics::{Diagnostic, DiagnosticCode, SourceSpan};
use crate::tool_program::ir::*;
use crate::tool_program::static_bounds::StaticBounds;
use crate::tool_program::ToolProgramError;

/// Compiler state during IR generation.
struct Compiler {
    instructions: Vec<IrInstruction>,
    spans: Vec<IrSpan>,
    strings: Vec<String>,
    integers: Vec<i64>,
    floats: Vec<f64>,
    locals: Vec<String>,
    source_hash: String,
    manifest_hash: String,
    limits_hash: String,
}

impl Compiler {
    fn new(
        _bounds: StaticBounds,
        source_hash: String,
        manifest_hash: String,
        limits_hash: String,
    ) -> Self {
        Self {
            instructions: Vec::new(),
            spans: Vec::new(),
            strings: Vec::new(),
            integers: Vec::new(),
            floats: Vec::new(),
            locals: Vec::new(),
            source_hash,
            manifest_hash,
            limits_hash,
        }
    }

    fn intern_string(&mut self, s: &str) -> u32 {
        if let Some(idx) = self.strings.iter().position(|x| x == s) {
            idx as u32
        } else {
            let idx = self.strings.len() as u32;
            self.strings.push(s.to_string());
            idx
        }
    }

    fn intern_int(&mut self, v: i64) -> u32 {
        if let Some(idx) = self.integers.iter().position(|x| *x == v) {
            idx as u32
        } else {
            let idx = self.integers.len() as u32;
            self.integers.push(v);
            idx
        }
    }

    fn intern_float(&mut self, v: f64) -> u32 {
        if let Some(idx) = self.floats.iter().position(|x| *x == v) {
            idx as u32
        } else {
            let idx = self.floats.len() as u32;
            self.floats.push(v);
            idx
        }
    }

    fn allocate_local(&mut self, name: &str) -> u32 {
        if let Some(idx) = self.locals.iter().position(|x| x == name) {
            idx as u32
        } else {
            let idx = self.locals.len() as u32;
            self.locals.push(name.to_string());
            idx
        }
    }

    fn lookup_local(&self, name: &str) -> Option<u32> {
        self.locals.iter().position(|x| x == name).map(|x| x as u32)
    }

    fn push(&mut self, op: IrOp, span: Span) {
        let span_idx = self.spans.len() as u32;
        self.spans.push(IrSpan::new(span.start, span.len()));
        self.instructions.push(IrInstruction { op, span_idx });
    }

    fn current_pc(&self) -> u32 {
        self.instructions.len() as u32
    }

    fn patch_jump(&mut self, jump_pc: u32, target: u32) {
        if let Some(inst) = self.instructions.get_mut(jump_pc as usize) {
            match &mut inst.op {
                IrOp::JumpIfFalse { target: t } => *t = target,
                IrOp::Jump { target: t } => *t = target,
                _ => {}
            }
        }
    }
}

/// Compile a validated AST into an IR program.
pub fn compile(program: &Program, bounds: &StaticBounds) -> Result<IrProgram, ToolProgramError> {
    compile_with_hashes(program, bounds, "", "", "")
}

/// Compile with explicit content hashes.
pub fn compile_with_hashes(
    program: &Program,
    bounds: &StaticBounds,
    source_hash: &str,
    manifest_hash: &str,
    limits_hash: &str,
) -> Result<IrProgram, ToolProgramError> {
    let mut compiler = Compiler::new(
        bounds.clone(),
        source_hash.to_string(),
        manifest_hash.to_string(),
        limits_hash.to_string(),
    );

    // Pre-allocate locals for all assignments
    preallocate_locals(program, &mut compiler);

    // Compile statements
    compile_stmts(&program.body, &mut compiler)?;

    // Implicit return at end
    compiler.push(IrOp::Return, program.span);

    let local_count = compiler.locals.len() as u32;
    let bounds_ir: IrBounds = bounds.into();

    let ir = IrProgram {
        version: IR_VERSION,
        language_version: LANGUAGE_VERSION,
        compiler_version: COMPILER_VERSION,
        parser_version: PARSER_VERSION,
        source_hash: compiler.source_hash,
        manifest_hash: compiler.manifest_hash,
        limits_hash: compiler.limits_hash,
        digest: String::new(), // computed below
        instructions: compiler.instructions,
        local_count,
        bounds: bounds_ir,
        spans: compiler.spans,
        strings: compiler.strings,
        integers: compiler.integers,
        floats: compiler.floats,
    };

    // Compute deterministic digest
    let digest = compute_digest(&ir);
    let mut ir = ir;
    ir.digest = digest;

    Ok(ir)
}

/// Pre-allocate local variable slots for all assignments in the program.
fn preallocate_locals(program: &Program, compiler: &mut Compiler) {
    for stmt in &program.body {
        preallocate_stmt(stmt, compiler);
    }
}

fn preallocate_stmt(stmt: &Stmt, compiler: &mut Compiler) {
    match stmt {
        Stmt::Assign { targets, .. } => {
            for t in targets {
                compiler.allocate_local(&t.name);
            }
        }
        Stmt::If {
            body,
            elif_clauses,
            else_body,
            ..
        } => {
            preallocate_stmts(body, compiler);
            for clause in elif_clauses {
                preallocate_stmts(&clause.body, compiler);
            }
            if let Some(e) = else_body {
                preallocate_stmts(e, compiler);
            }
        }
        Stmt::For { target, body, .. } => {
            compiler.allocate_local(&target.name);
            preallocate_stmts(body, compiler);
        }
        Stmt::ToolCall { target, .. } => {
            compiler.allocate_local(&target.name);
        }
        Stmt::Parallel { target, .. } => {
            compiler.allocate_local(&target.name);
        }
        _ => {}
    }
}

fn preallocate_stmts(stmts: &[Stmt], compiler: &mut Compiler) {
    for stmt in stmts {
        preallocate_stmt(stmt, compiler);
    }
}

fn compile_stmts(stmts: &[Stmt], compiler: &mut Compiler) -> Result<(), ToolProgramError> {
    for stmt in stmts {
        compile_stmt(stmt, compiler)?;
    }
    Ok(())
}

fn compile_stmt(stmt: &Stmt, compiler: &mut Compiler) -> Result<(), ToolProgramError> {
    match stmt {
        Stmt::Assign {
            targets,
            value,
            span,
        } => {
            compile_expr(value, compiler)?;
            for target in targets {
                let slot = compiler.lookup_local(&target.name).ok_or_else(|| {
                    ToolProgramError::Compile(Diagnostic::new(
                        DiagnosticCode::InternalError,
                        format!("unallocated local '{}'", target.name),
                        SourceSpan::new(target.span.start, target.span.len()),
                    ))
                })?;
                if targets.len() > 1 {
                    compiler.push(IrOp::Dup, *span);
                }
                compiler.push(IrOp::StoreLocal { slot }, *span);
            }
        }
        Stmt::If {
            test,
            body,
            elif_clauses,
            else_body,
            span,
        } => {
            compile_if(
                test,
                body,
                elif_clauses,
                else_body.as_deref(),
                *span,
                compiler,
            )?;
        }
        Stmt::For {
            target,
            iter,
            body,
            span,
        } => {
            compile_for(target, iter, body, *span, compiler)?;
        }
        Stmt::Assert { test, msg, span } => {
            compile_expr(test, compiler)?;
            // If assertion fails, emit fail with message
            if let Some(m) = msg {
                compile_expr(m, compiler)?;
            } else {
                let pool_idx = compiler.intern_string("assertion failed");
                compiler.push(IrOp::LoadString { pool_idx }, *span);
            }
            // The assert doesn't have a clean IR representation yet;
            // for now, just compile the test and message without branching
            compiler.push(IrOp::Pop, *span);
        }
        Stmt::ToolCall {
            target,
            descriptor,
            kwargs,
            span,
        } => {
            // Build call request
            compile_tool_call_descriptor(descriptor, kwargs, compiler)?;
            compiler.push(IrOp::ConstructCall, *span);
            compiler.push(IrOp::ExecuteCall, *span);
            let slot = compiler.lookup_local(&target.name).ok_or_else(|| {
                ToolProgramError::Compile(Diagnostic::new(
                    DiagnosticCode::InternalError,
                    format!("unallocated local '{}'", target.name),
                    SourceSpan::new(span.start, span.len()),
                ))
            })?;
            compiler.push(IrOp::StoreLocal { slot }, *span);
        }
        Stmt::Parallel {
            target,
            descriptors,
            span,
        } => {
            let count = descriptors.len() as u32;
            for desc in descriptors {
                compile_expr(desc, compiler)?;
            }
            compiler.push(IrOp::ParallelStart { count }, *span);
            compiler.push(IrOp::ParallelExecute, *span);
            let slot = compiler.lookup_local(&target.name).ok_or_else(|| {
                ToolProgramError::Compile(Diagnostic::new(
                    DiagnosticCode::InternalError,
                    format!("unallocated local '{}'", target.name),
                    SourceSpan::new(span.start, span.len()),
                ))
            })?;
            compiler.push(IrOp::StoreLocal { slot }, *span);
        }
        Stmt::Emit { value, span } => {
            compile_expr(value, compiler)?;
            compiler.push(IrOp::Emit, *span);
        }
        Stmt::Fail { reason, span } => {
            if let Some(r) = reason {
                compile_expr(r, compiler)?;
            } else {
                compiler.push(IrOp::LoadNone, *span);
            }
            compiler.push(IrOp::Fail, *span);
        }
        Stmt::Pass { span } => {
            // No-op
            let _ = span;
        }
    }
    Ok(())
}

fn compile_if(
    test: &Expr,
    body: &[Stmt],
    elif_clauses: &[ElifClause],
    else_body: Option<&[Stmt]>,
    span: Span,
    compiler: &mut Compiler,
) -> Result<(), ToolProgramError> {
    compile_expr(test, compiler)?;

    if elif_clauses.is_empty() && else_body.is_none() {
        // Simple if: test → jump_if_false past body
        let jump_pc = compiler.current_pc();
        compiler.push(IrOp::JumpIfFalse { target: 0 }, span);
        compile_stmts(body, compiler)?;
        let end_pc = compiler.current_pc();
        compiler.patch_jump(jump_pc, end_pc);
        return Ok(());
    }

    // If with elif/else: chain
    let mut jump_pcs = Vec::new();

    let jump_pc = compiler.current_pc();
    compiler.push(IrOp::JumpIfFalse { target: 0 }, span);
    compile_stmts(body, compiler)?;

    for clause in elif_clauses {
        let jump_to_next = compiler.current_pc();
        compiler.push(IrOp::Jump { target: 0 }, span);
        let else_start = compiler.current_pc();
        compiler.patch_jump(jump_pc, else_start);

        compile_expr(&clause.test, compiler)?;
        let next_jump_pc = compiler.current_pc();
        compiler.push(IrOp::JumpIfFalse { target: 0 }, clause.span);
        compile_stmts(&clause.body, compiler)?;

        jump_pcs.push((next_jump_pc, jump_to_next));
    }

    if let Some(eb) = else_body {
        let jump_to_end = compiler.current_pc();
        compiler.push(IrOp::Jump { target: 0 }, span);
        let else_start = compiler.current_pc();

        // Patch the last jump_if_false to here
        if let Some((last_jump_pc, _)) = jump_pcs.last_mut() {
            compiler.patch_jump(*last_jump_pc, else_start);
            *last_jump_pc = jump_to_end; // repurpose for end jump
        } else {
            compiler.patch_jump(jump_pc, else_start);
        }

        compile_stmts(eb, compiler)?;
        let end_pc = compiler.current_pc();

        // Patch all pending jumps to end
        if let Some((last_jump_pc, _)) = jump_pcs.last() {
            compiler.patch_jump(*last_jump_pc, end_pc);
        }
        // Patch the unconditional jump we added
        let unconditional_pc = jump_to_end;
        compiler.patch_jump(unconditional_pc, end_pc);
    } else {
        // No else: patch the last jump_if_false to current_pc
        if let Some((last_jump_pc, _)) = jump_pcs.last() {
            let end_pc = compiler.current_pc();
            compiler.patch_jump(*last_jump_pc, end_pc);
        } else {
            let end_pc = compiler.current_pc();
            compiler.patch_jump(jump_pc, end_pc);
        }
    }

    Ok(())
}

fn compile_for(
    target: &Ident,
    iter: &Expr,
    body: &[Stmt],
    span: Span,
    compiler: &mut Compiler,
) -> Result<(), ToolProgramError> {
    let slot = compiler.lookup_local(&target.name).ok_or_else(|| {
        ToolProgramError::Compile(Diagnostic::new(
            DiagnosticCode::InternalError,
            format!("unallocated local '{}'", target.name),
            SourceSpan::new(target.span.start, target.span.len()),
        ))
    })?;

    // Compile the iterable
    compile_expr(iter, compiler)?;

    let loop_start = compiler.current_pc();
    let body_start = loop_start + 1; // after ForLoopStart
    let loop_end = compiler.current_pc() + 2; // placeholder

    compiler.push(
        IrOp::ForLoopStart {
            body_start,
            loop_end,
        },
        span,
    );

    // Load iterator value
    compiler.push(IrOp::ForLoopIter, target.span);
    compiler.push(IrOp::StoreLocal { slot }, target.span);

    // Compile body
    compile_stmts(body, compiler)?;

    // Jump back to loop start
    compiler.push(
        IrOp::ForLoopNext {
            loop_start,
            loop_end,
        },
        span,
    );

    // Patch loop_end in the ForLoopStart instruction
    let end_pc = compiler.current_pc();
    if let Some(inst) = compiler.instructions.get_mut(loop_start as usize) {
        if let IrOp::ForLoopStart {
            loop_end: ref mut le,
            ..
        } = &mut inst.op
        {
            *le = end_pc;
        }
    }

    Ok(())
}

fn compile_tool_call_descriptor(
    descriptor: &Expr,
    kwargs: &[KwArg],
    compiler: &mut Compiler,
) -> Result<(), ToolProgramError> {
    // Build a dict from the descriptor expression + kwargs
    compile_expr(descriptor, compiler)?;

    // Add kwargs as additional dict entries
    for kw in kwargs {
        let key_idx = compiler.intern_string(&kw.name);
        compiler.push(IrOp::LoadString { pool_idx: key_idx }, kw.span);
        compile_expr(&kw.value, compiler)?;
    }

    // If we have kwargs, merge them into the descriptor at compile time
    if !kwargs.is_empty() {
        compiler.push(
            IrOp::MakeDict {
                count: kwargs.len() as u32,
            },
            descriptor.span(),
        );
    }

    Ok(())
}

fn compile_expr(expr: &Expr, compiler: &mut Compiler) -> Result<(), ToolProgramError> {
    match expr {
        Expr::NoneLiteral(span) => {
            compiler.push(IrOp::LoadNone, *span);
        }
        Expr::BoolLiteral { value, span } => {
            if *value {
                compiler.push(IrOp::LoadTrue, *span);
            } else {
                compiler.push(IrOp::LoadFalse, *span);
            }
        }
        Expr::IntLiteral { value, span } => {
            let pool_idx = compiler.intern_int(*value);
            compiler.push(IrOp::LoadInt { pool_idx }, *span);
        }
        Expr::FloatLiteral { value, span } => {
            let pool_idx = compiler.intern_float(*value);
            compiler.push(IrOp::LoadFloat { pool_idx }, *span);
        }
        Expr::StringLiteral { value, span } => {
            let pool_idx = compiler.intern_string(value);
            compiler.push(IrOp::LoadString { pool_idx }, *span);
        }
        Expr::Name { id, span } => {
            if let Some(slot) = compiler.lookup_local(id) {
                compiler.push(IrOp::LoadLocal { slot }, *span);
            } else {
                // Unresolved name — push None as placeholder
                // (validator should have caught this)
                compiler.push(IrOp::LoadNone, *span);
            }
        }
        Expr::BinOp {
            left,
            op,
            right,
            span,
        } => {
            compile_expr(left, compiler)?;
            compile_expr(right, compiler)?;
            let kind = match op {
                BinOpKind::Add => IrBinOp::Add,
                BinOpKind::Sub => IrBinOp::Sub,
                BinOpKind::Mul => IrBinOp::Mul,
                BinOpKind::Div => IrBinOp::Div,
                BinOpKind::FloorDiv => IrBinOp::FloorDiv,
                BinOpKind::Mod => IrBinOp::Mod,
                BinOpKind::Pow => IrBinOp::Pow,
                BinOpKind::MatMul => IrBinOp::Mul, // fallback
                BinOpKind::BitOr => IrBinOp::BitOr,
                BinOpKind::BitXor => IrBinOp::BitXor,
                BinOpKind::BitAnd => IrBinOp::BitAnd,
                BinOpKind::LShift => IrBinOp::LShift,
                BinOpKind::RShift => IrBinOp::RShift,
            };
            compiler.push(IrOp::BinOp { kind }, *span);
        }
        Expr::UnaryOp { op, operand, span } => {
            compile_expr(operand, compiler)?;
            let kind = match op {
                UnaryOpKind::Neg => IrUnaryOp::Neg,
                UnaryOpKind::Pos => IrUnaryOp::Pos,
                UnaryOpKind::Invert => IrUnaryOp::Invert,
            };
            compiler.push(IrOp::UnaryOp { kind }, *span);
        }
        Expr::Compare {
            left,
            ops,
            comparators,
            span,
        } => {
            compile_expr(left, compiler)?;
            for (op, comp) in ops.iter().zip(comparators.iter()) {
                compile_expr(comp, compiler)?;
                let kind = match op {
                    CmpOp::Eq => IrCmpOp::Eq,
                    CmpOp::NotEq => IrCmpOp::NotEq,
                    CmpOp::Lt => IrCmpOp::Lt,
                    CmpOp::LtE => IrCmpOp::LtE,
                    CmpOp::Gt => IrCmpOp::Gt,
                    CmpOp::GtE => IrCmpOp::GtE,
                    CmpOp::In => IrCmpOp::In,
                    CmpOp::NotIn => IrCmpOp::NotIn,
                };
                compiler.push(IrOp::Compare { kind }, *span);
            }
        }
        Expr::BoolAnd { left, right, span } => {
            compile_expr(left, compiler)?;
            compile_expr(right, compiler)?;
            compiler.push(IrOp::BoolAnd, *span);
        }
        Expr::BoolOr { left, right, span } => {
            compile_expr(left, compiler)?;
            compile_expr(right, compiler)?;
            compiler.push(IrOp::BoolOr, *span);
        }
        Expr::BoolNot { operand, span } => {
            compile_expr(operand, compiler)?;
            compiler.push(IrOp::BoolNot, *span);
        }
        Expr::Subscript { value, slice, span } => {
            compile_expr(value, compiler)?;
            match slice {
                Slice::Index(idx) => {
                    compile_expr(idx, compiler)?;
                    compiler.push(IrOp::Index, *span);
                }
                Slice::Range { start, stop, step } => {
                    // Push start, stop, step (or None for missing)
                    if let Some(s) = start {
                        compile_expr(s, compiler)?;
                    } else {
                        compiler.push(IrOp::LoadNone, *span);
                    }
                    if let Some(s) = stop {
                        compile_expr(s, compiler)?;
                    } else {
                        compiler.push(IrOp::LoadNone, *span);
                    }
                    if let Some(s) = step {
                        compile_expr(s, compiler)?;
                    } else {
                        compiler.push(IrOp::LoadNone, *span);
                    }
                    compiler.push(IrOp::Slice, *span);
                }
            }
        }
        Expr::CallBuiltin { func, arg, span } => {
            compile_expr(arg, compiler)?;
            let op = match func {
                BuiltinFunc::Len => IrOp::Len,
                BuiltinFunc::Str => IrOp::Str,
                BuiltinFunc::Int => IrOp::Int,
                BuiltinFunc::Bool => IrOp::Bool,
            };
            compiler.push(op, *span);
        }
        Expr::MethodCall {
            object,
            method,
            args,
            span,
        } => {
            compile_expr(object, compiler)?;
            for a in args {
                compile_expr(a, compiler)?;
            }
            // Method calls are compiled as builtin-like operations
            // The runtime will dispatch based on the method name
            match method.as_str() {
                "append" => {
                    // list.append(value) — this is a mutation, compile as a special op
                    // For now, just compile as a no-op (the value is consumed)
                    compiler.push(IrOp::Pop, *span);
                }
                "items" | "keys" | "values" => {
                    // These are read-only — compile as method call
                    // The runtime will handle the actual call
                    let method_idx = compiler.intern_string(method);
                    compiler.push(
                        IrOp::LoadString {
                            pool_idx: method_idx,
                        },
                        *span,
                    );
                    compiler.push(IrOp::ExecuteCall, *span);
                }
                "split" | "join" | "strip" | "lower" | "upper" | "replace" | "get" => {
                    let method_idx = compiler.intern_string(method);
                    compiler.push(
                        IrOp::LoadString {
                            pool_idx: method_idx,
                        },
                        *span,
                    );
                    compiler.push(IrOp::ExecuteCall, *span);
                }
                _ => {
                    return Err(ToolProgramError::Compile(Diagnostic::new(
                        DiagnosticCode::IllegalAttributeAccess,
                        format!("unsupported method '{}'", method),
                        SourceSpan::new(span.start, span.len()),
                    )));
                }
            }
        }
        Expr::ToolCallExpr {
            descriptor,
            kwargs,
            span,
        } => {
            compile_tool_call_descriptor(descriptor, kwargs, compiler)?;
            compiler.push(IrOp::ConstructCall, *span);
            compiler.push(IrOp::ExecuteCall, *span);
        }
        Expr::ParallelExpr { descriptors, span } => {
            let count = descriptors.len() as u32;
            for d in descriptors {
                compile_expr(d, compiler)?;
            }
            compiler.push(IrOp::ParallelStart { count }, *span);
            compiler.push(IrOp::ParallelExecute, *span);
        }
        Expr::List { elements, span } => {
            for e in elements {
                compile_expr(e, compiler)?;
            }
            compiler.push(
                IrOp::MakeList {
                    count: elements.len() as u32,
                },
                *span,
            );
        }
        Expr::Tuple { elements, span } => {
            for e in elements {
                compile_expr(e, compiler)?;
            }
            compiler.push(
                IrOp::MakeTuple {
                    count: elements.len() as u32,
                },
                *span,
            );
        }
        Expr::Dict { keys, values, span } => {
            for (k, v) in keys.iter().zip(values.iter()) {
                compile_expr(k, compiler)?;
                compile_expr(v, compiler)?;
            }
            compiler.push(
                IrOp::MakeDict {
                    count: keys.len() as u32,
                },
                *span,
            );
        }
        Expr::Range {
            start,
            stop,
            step,
            span,
        } => {
            // Compile range() as a construction: push start/stop/step
            if let Some(s) = start {
                compile_expr(s, compiler)?;
            } else {
                let pool_idx = compiler.intern_int(0);
                compiler.push(IrOp::LoadInt { pool_idx }, *span);
            }
            compile_expr(stop, compiler)?;
            if let Some(s) = step {
                compile_expr(s, compiler)?;
            } else {
                let pool_idx = compiler.intern_int(1);
                compiler.push(IrOp::LoadInt { pool_idx }, *span);
            }
            // For now, compile range as a list construction
            // The runtime will handle the actual range iteration
        }
        Expr::Parenthesized { inner, .. } => {
            compile_expr(inner, compiler)?;
        }
    }
    Ok(())
}

/// Compute a deterministic SHA-256 digest over the IR (public for verifier).
pub fn compute_digest_public(ir: &IrProgram) -> String {
    compute_digest(ir)
}

/// Compute a deterministic SHA-256 digest over the IR.
fn compute_digest(ir: &IrProgram) -> String {
    let mut hasher = Sha256::new();

    // Version fields
    hasher.update(ir.version.to_le_bytes());
    hasher.update(ir.language_version.to_le_bytes());
    hasher.update(ir.compiler_version.to_le_bytes());
    hasher.update(ir.parser_version.to_le_bytes());

    // Content hashes
    hasher.update(ir.source_hash.as_bytes());
    hasher.update(ir.manifest_hash.as_bytes());
    hasher.update(ir.limits_hash.as_bytes());

    // Instructions
    for inst in &ir.instructions {
        hasher.update((inst.span_idx).to_le_bytes());
        digest_op(&mut hasher, &inst.op);
    }

    // Pools
    for s in &ir.strings {
        hasher.update(s.as_bytes());
        hasher.update([0u8]); // separator
    }
    for i in &ir.integers {
        hasher.update(i.to_le_bytes());
    }
    for f in &ir.floats {
        hasher.update(f.to_le_bytes());
    }

    // Bounds
    hasher.update(ir.bounds.max_steps.to_le_bytes());
    hasher.update(ir.bounds.max_loop_iterations.to_le_bytes());
    hasher.update(ir.bounds.max_total_iterations.to_le_bytes());
    hasher.update(ir.bounds.call_site_count.to_le_bytes());
    hasher.update(ir.bounds.max_dynamic_calls.to_le_bytes());
    hasher.update(ir.bounds.max_parallel_width.to_le_bytes());
    hasher.update(ir.bounds.max_parallel_depth.to_le_bytes());
    hasher.update(ir.bounds.max_nesting_depth.to_le_bytes());
    hasher.update(ir.bounds.max_value_growth.to_le_bytes());

    format!("{:x}", hasher.finalize())
}

fn digest_op(hasher: &mut Sha256, op: &IrOp) {
    // Discriminant + relevant data
    match op {
        IrOp::LoadInt { pool_idx } => {
            hasher.update([1]);
            hasher.update(pool_idx.to_le_bytes());
        }
        IrOp::LoadFloat { pool_idx } => {
            hasher.update([2]);
            hasher.update(pool_idx.to_le_bytes());
        }
        IrOp::LoadString { pool_idx } => {
            hasher.update([3]);
            hasher.update(pool_idx.to_le_bytes());
        }
        IrOp::LoadTrue => hasher.update([4]),
        IrOp::LoadFalse => hasher.update([5]),
        IrOp::LoadNone => hasher.update([6]),
        IrOp::LoadLocal { slot } => {
            hasher.update([7]);
            hasher.update(slot.to_le_bytes());
        }
        IrOp::StoreLocal { slot } => {
            hasher.update([8]);
            hasher.update(slot.to_le_bytes());
        }
        IrOp::MakeList { count } => {
            hasher.update([9]);
            hasher.update(count.to_le_bytes());
        }
        IrOp::MakeTuple { count } => {
            hasher.update([10]);
            hasher.update(count.to_le_bytes());
        }
        IrOp::MakeDict { count } => {
            hasher.update([11]);
            hasher.update(count.to_le_bytes());
        }
        IrOp::BinOp { kind } => {
            hasher.update([12]);
            hasher.update((*kind as u8).to_le_bytes());
        }
        IrOp::UnaryOp { kind } => {
            hasher.update([13]);
            hasher.update((*kind as u8).to_le_bytes());
        }
        IrOp::Compare { kind } => {
            hasher.update([14]);
            hasher.update((*kind as u8).to_le_bytes());
        }
        IrOp::BoolAnd => hasher.update([15]),
        IrOp::BoolOr => hasher.update([16]),
        IrOp::BoolNot => hasher.update([17]),
        IrOp::Pop => hasher.update([18]),
        IrOp::Dup => hasher.update([19]),
        IrOp::Index => hasher.update([20]),
        IrOp::Slice => hasher.update([21]),
        IrOp::Len => hasher.update([22]),
        IrOp::Str => hasher.update([23]),
        IrOp::Int => hasher.update([24]),
        IrOp::Bool => hasher.update([25]),
        IrOp::JumpIfFalse { target } => {
            hasher.update([26]);
            hasher.update(target.to_le_bytes());
        }
        IrOp::Jump { target } => {
            hasher.update([27]);
            hasher.update(target.to_le_bytes());
        }
        IrOp::ForLoopStart {
            body_start,
            loop_end,
        } => {
            hasher.update([28]);
            hasher.update(body_start.to_le_bytes());
            hasher.update(loop_end.to_le_bytes());
        }
        IrOp::ForLoopNext {
            loop_start,
            loop_end,
        } => {
            hasher.update([29]);
            hasher.update(loop_start.to_le_bytes());
            hasher.update(loop_end.to_le_bytes());
        }
        IrOp::ForLoopIter => hasher.update([30]),
        IrOp::ConstructCall => hasher.update([31]),
        IrOp::ExecuteCall => hasher.update([32]),
        IrOp::ParallelStart { count } => {
            hasher.update([33]);
            hasher.update(count.to_le_bytes());
        }
        IrOp::ParallelExecute => hasher.update([34]),
        IrOp::Emit => hasher.update([35]),
        IrOp::Fail => hasher.update([36]),
        IrOp::Checkpoint => hasher.update([37]),
        IrOp::Return => hasher.update([38]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_program::parser::parse_source;
    use crate::tool_program::static_bounds;

    #[test]
    fn compile_simple_emit() {
        let src = "emit({\"result\": \"ok\"})\n";
        let prog = parse_source(src).unwrap();
        let bounds = static_bounds::analyze(&prog).unwrap();
        let ir = compile(&prog, &bounds).unwrap();
        assert_eq!(ir.version, IR_VERSION);
        assert_eq!(ir.language_version, LANGUAGE_VERSION);
        assert!(!ir.instructions.is_empty());
        assert!(!ir.digest.is_empty());
    }

    #[test]
    fn compile_for_loop() {
        let src = "total = 0\nfor i in range(5):\n    total = total + 1\nemit(total)\n";
        let prog = parse_source(src).unwrap();
        let bounds = static_bounds::analyze(&prog).unwrap();
        let ir = compile(&prog, &bounds).unwrap();
        assert!(ir.local_count >= 2); // total + i
        assert!(ir
            .instructions
            .iter()
            .any(|i| matches!(i.op, IrOp::ForLoopStart { .. })));
    }

    #[test]
    fn compile_if_else() {
        let src = "if True:\n    x = 1\nelse:\n    x = 0\n";
        let prog = parse_source(src).unwrap();
        let bounds = static_bounds::analyze(&prog).unwrap();
        let ir = compile(&prog, &bounds).unwrap();
        assert!(ir
            .instructions
            .iter()
            .any(|i| matches!(i.op, IrOp::JumpIfFalse { .. })));
    }

    #[test]
    fn compile_tool_call() {
        let src = "result = call({\"tool\": \"grep\"})\n";
        let prog = parse_source(src).unwrap();
        let bounds = static_bounds::analyze(&prog).unwrap();
        let ir = compile(&prog, &bounds).unwrap();
        assert!(ir
            .instructions
            .iter()
            .any(|i| matches!(i.op, IrOp::ConstructCall)));
        assert!(ir
            .instructions
            .iter()
            .any(|i| matches!(i.op, IrOp::ExecuteCall)));
    }

    #[test]
    fn compile_parallel() {
        let src = "reads = parallel({\"tool\": \"a\"}, {\"tool\": \"b\"})\n";
        let prog = parse_source(src).unwrap();
        let bounds = static_bounds::analyze(&prog).unwrap();
        let ir = compile(&prog, &bounds).unwrap();
        assert!(ir
            .instructions
            .iter()
            .any(|i| matches!(i.op, IrOp::ParallelStart { count: 2 })));
        assert!(ir
            .instructions
            .iter()
            .any(|i| matches!(i.op, IrOp::ParallelExecute)));
    }

    #[test]
    fn compile_deterministic() {
        let src = "x = 1\nemit(x)\n";
        let prog = parse_source(src).unwrap();
        let bounds = static_bounds::analyze(&prog).unwrap();
        let ir1 = compile(&prog, &bounds).unwrap();
        let ir2 = compile(&prog, &bounds).unwrap();
        assert_eq!(ir1.digest, ir2.digest);
        assert_eq!(ir1.instructions, ir2.instructions);
    }

    #[test]
    fn compile_list_literal() {
        let src = "x = [1, 2, 3]\n";
        let prog = parse_source(src).unwrap();
        let bounds = static_bounds::analyze(&prog).unwrap();
        let ir = compile(&prog, &bounds).unwrap();
        assert!(ir
            .instructions
            .iter()
            .any(|i| matches!(i.op, IrOp::MakeList { count: 3 })));
    }

    #[test]
    fn compile_dict_literal() {
        let src = "x = {\"a\": 1, \"b\": 2}\n";
        let prog = parse_source(src).unwrap();
        let bounds = static_bounds::analyze(&prog).unwrap();
        let ir = compile(&prog, &bounds).unwrap();
        assert!(ir
            .instructions
            .iter()
            .any(|i| matches!(i.op, IrOp::MakeDict { count: 2 })));
    }

    #[test]
    fn compile_comparisons() {
        let src = "x = a == b\ny = c > d\n";
        let prog = parse_source(src).unwrap();
        let bounds = static_bounds::analyze(&prog).unwrap();
        let ir = compile(&prog, &bounds).unwrap();
        assert!(ir
            .instructions
            .iter()
            .any(|i| matches!(i.op, IrOp::Compare { kind: IrCmpOp::Eq })));
        assert!(ir
            .instructions
            .iter()
            .any(|i| matches!(i.op, IrOp::Compare { kind: IrCmpOp::Gt })));
    }

    #[test]
    fn compile_bool_ops() {
        let src = "x = a and b\ny = c or d\n";
        let prog = parse_source(src).unwrap();
        let bounds = static_bounds::analyze(&prog).unwrap();
        let ir = compile(&prog, &bounds).unwrap();
        assert!(ir
            .instructions
            .iter()
            .any(|i| matches!(i.op, IrOp::BoolAnd)));
        assert!(ir.instructions.iter().any(|i| matches!(i.op, IrOp::BoolOr)));
    }

    #[test]
    fn compile_fail() {
        let src = "fail(\"error\")\n";
        let prog = parse_source(src).unwrap();
        let bounds = static_bounds::analyze(&prog).unwrap();
        let ir = compile(&prog, &bounds).unwrap();
        assert!(ir.instructions.iter().any(|i| matches!(i.op, IrOp::Fail)));
    }

    #[test]
    fn compile_subscript() {
        let src = "x = a[0]\n";
        let prog = parse_source(src).unwrap();
        let bounds = static_bounds::analyze(&prog).unwrap();
        let ir = compile(&prog, &bounds).unwrap();
        assert!(ir.instructions.iter().any(|i| matches!(i.op, IrOp::Index)));
    }

    #[test]
    fn compile_slice() {
        let src = "x = a[1:3]\n";
        let prog = parse_source(src).unwrap();
        let bounds = static_bounds::analyze(&prog).unwrap();
        let ir = compile(&prog, &bounds).unwrap();
        assert!(ir.instructions.iter().any(|i| matches!(i.op, IrOp::Slice)));
    }

    #[test]
    fn compile_builtin_len() {
        let src = "x = len(a)\n";
        let prog = parse_source(src).unwrap();
        let bounds = static_bounds::analyze(&prog).unwrap();
        let ir = compile(&prog, &bounds).unwrap();
        assert!(ir.instructions.iter().any(|i| matches!(i.op, IrOp::Len)));
    }

    #[test]
    fn compile_complex_program() {
        let src = r#"
results = []
for file in ["a.py", "b.py"]:
    content = call({"tool": "read_file", "path": file})
    lines = len(content)
    if lines > 100:
        results = results + [file]
emit({"total": len(results), "files": results})
"#;
        let prog = parse_source(src).unwrap();
        let bounds = static_bounds::analyze(&prog).unwrap();
        let ir = compile(&prog, &bounds).unwrap();
        assert!(!ir.digest.is_empty());
        assert!(ir.local_count >= 3); // results, file, content, lines
    }
}
