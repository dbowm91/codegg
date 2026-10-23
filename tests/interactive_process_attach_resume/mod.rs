//! Consolidated the interactive attach and resume protocol integration test
//! family (M004), and absorbed two further interactive families on
//! 2026-10-23.
//!
//! History: each member lived as its own `tests/<NAME>.rs` file and was
//! therefore its own Cargo test executable — 1 executable for 1 file.
//! The `.inc` extension is what stops Cargo's `tests/<dir>/*.rs`
//! auto-discovery from generating one executable per file, and
//! `tests/<dir>/mod.rs` is the single integration target. Cargo does not
//! auto-discover `tests/<dir>/mod.rs` either, so the target is declared
//! explicitly in the root `Cargo.toml`. Each member is included with
//! `#[path]`, the pattern the existing `tests/projection_replay/` and
//! `tests/session_family/` families already use.
//!
//! Consolidation replaces 1 separate Cargo test executable with 1,
//! preserving every test body and every assertion. The only visible change
//! is the test id, which becomes `<family>::<member>::<test>`.
//!
//! ## The two absorbed families
//!
//! `interactive_process_sessions` (the interactive PTY session engine) and
//! `interactive_terminal_tui` (the interactive terminal controller and its
//! TUI) were their own families, and their own Cargo test executables. They
//! are absorbed here rather than kept apart for two reasons, both measured:
//!
//! 1. **They were serial.** All three families sat in the `ci` nextest
//!    profile's whole-binary exclusion list
//!    (`threads-required = "num-cpus"`), so on the hosted 4-vCPU runner they
//!    ran **one at a time, alone**. Three exclusive executables cost three
//!    serial slots for 40 test functions; one costs one.
//! 2. **The link work was duplicated.** A family's cost is dominated by the
//!    code it links, not by the number of test functions in it, so three
//!    executables linking the same code cost three times one. Merging
//!    removes two link units.
//!
//! No test body changed and no assertion was removed: the members were
//! **moved**, and their libtest ids become
//! `interactive_process_attach_resume::<member>::<test>`.
//!
//! The merged target keeps the name `interactive_process_attach_resume`, so
//! the nextest `binary(...)` filter and the `Linux M004` lane's cargo target
//! both still resolve to the same executable.

#[path = "interactive_process_attach_resume.inc"]
pub mod interactive_process_attach_resume;

#[path = "interactive_process_sessions.inc"]
pub mod interactive_process_sessions;

#[path = "interactive_terminal_tui.inc"]
pub mod interactive_terminal_tui;
