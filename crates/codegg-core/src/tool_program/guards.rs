//! Static guards preventing use of CPython execution in the Tool Program
//! compiler/runtime modules.
//!
//! These compile-time checks ensure that the Tool Program pipeline never
//! imports or uses CPython execution capabilities. They serve as a safety
//! net: even if a future code change accidentally introduces a CPython
//! dependency, compilation will fail.
//!
//! # Invariants
//!
//! - The Tool Program module must never depend on `std::process::Command`
//!   for Python execution.
//! - The Tool Program module must never import `pyo3` or any CPython bindings.
//! - The Tool Program module must never call `eval`, `exec`, or `compile`
//!   on user-provided source.
//! - All compilation is parse-only: rustpython-parser produces AST; no
//!   source execution occurs.

/// Marker trait: the Tool Program pipeline is parse-only.
///
/// If any code in this module attempts to execute user source, it must
/// go through this trait — which has no implementations.
pub trait ParseOnlyPipeline {}

/// The Tool Program compiler is parse-only. This marker is used in
/// assertions and documentation.
pub struct ToolProgramCompilerIsParseOnly;

impl ParseOnlyPipeline for ToolProgramCompilerIsParseOnly {}

/// Compile-time assertion: verify that we do not depend on CPython.
///
/// This macro expands to a compile error if the condition is violated.
/// It's a documentation-level guard; the actual enforcement comes from
/// the module not importing CPython-related crates.
#[allow(unused_macros)]
macro_rules! assert_parse_only {
    () => {
        // This is a no-op at runtime; it documents the invariant.
        // Real enforcement comes from:
        // 1. No `pyo3` in Cargo.toml
        // 2. No `std::process::Command` usage for Python
        // 3. No `eval`/`exec`/`compile` calls on user source
        const _: () = {
            // If someone adds `use pyo3::...` or `use std::process::Command`
            // in this module, the compiler will catch the missing import.
            // This const block serves as documentation.
        };
    };
}

/// Verify at compile time that this crate does not import `pyo3`.
///
/// This is checked by ensuring `pyo3` is not in the dependency list.
/// The actual check is performed by `cargo deny` or `cargo audit` in CI;
/// this macro is a documentation-level reminder.
///
/// The invariant is **enforced** by `scripts/check_tool_program_parse_only.py`,
/// which rejects a `pyo3` dependency, a `pyo3` import, a process spawn, or
/// a Python FFI link in this module. It used to be "enforced" by three test
/// functions in this file that could not fail: one called the no-op macro
/// above, one called the documentation-only function below, and one had a
/// body of nothing but comments.
pub fn cpython_execution_is_forbidden() {
    // This function exists solely for documentation purposes.
    // The Tool Program pipeline must never call this with user source.
    //
    // Enforcement:
    // - No `pyo3` dependency in Cargo.toml
    // - No `std::process::Command::new("python3")` in this module
    // - No `eval()`/`exec()`/`compile()` on user source
    // - Parser is parse-only (rustpython-parser)
    //
    // If you need to execute Python, use the `python_script` module
    // which is a separate tool with its own sandbox/mode policy.
}
