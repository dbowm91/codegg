# Repository Audit Corrective Milestone C001 — Nested-Runtime Panic Removal in the TUI and MCP CLI

Status: closed — closure record at `plans/closure/repository-audit-corrective/001-status.md`

Repository baseline: `3b29c5b88` (`origin/main`)
Audit evidence was gathered at `04536eeeb`; every file this workstream cites is
byte-identical at that commit and at the baseline above, so the findings carry
forward unchanged.

Source roadmap: `plans/subsystems/repository-audit-corrective-roadmap.md#milestone-c001--nested-runtime-panic-removal-in-the-tui-and-mcp-cli`

Long-term requirements: `plans/000-long-term-specification.md`, `plans/002-long-term-roadmap.md`, `architecture/tui.md`, `architecture/core.md`

Applicable ADRs: none required. This milestone does not change an identity, a boundary, a protocol, or an authorization model.

Primary class: **invariant**

## 1. Objective

No user-facing command may terminate the process by panicking on a nested runtime entry. Every command that is reachable from a keypress in the TUI, or from the `codegg mcp debug` subcommand, either completes or fails closed with a message.

## 2. Why this milestone is ready

It has no hard or interface dependency. It is the first milestone of a new workstream registered at a single baseline, and it owns one defect class in one ownership boundary: the blocking-wait call sites on the command-dispatch path.

It is sequenced first because it is the only finding in the audit that can terminate a running process, and because a second milestone re-enables the verification surface that should have caught it.

## 3. Current implementation evidence

The defect is a nested runtime entry, and its mechanism was confirmed against the tokio 1.52.3 source in the vendored registry rather than inferred.

The path that enters the runtime:

- `src/main.rs` declares `#[tokio::main(flavor = "multi_thread")]`, so `Runtime::block_on` runs the whole of `main`'s future.
- `Runtime::block_on_inner` delegates to `Scheduler::MultiThread(exec).block_on(..)`, which calls `context::enter_runtime(handle, true, ..)`. That sets the thread-local to `EnterRuntime::Entered`.
- `src/main.rs` awaits `tui::run_event_loop(&mut app)` inside that future, so the event loop's future is polled with the runtime context entered.

The path that panics:

- `tokio::runtime::Handle::block_on` → `Handle::block_on_inner` → `context::enter_runtime(&self.inner, true, ..)`.
- `context::enter_runtime` computes `maybe_guard` as `None` when `c.runtime.get().is_entered()` is true, and then panics with *"Cannot start a runtime from within a runtime"*.
- `Handle::current()` succeeds in that state — it only reads the current handle — so the call site looks valid.

The call sites, all inside a context that is already entered:

| Surface | Sites |
|---|---|
| `src/tui/app/mod.rs` — 17 `/lsp-*` slash-command arms | 7736, 7759, 7780, 7816, 7835, 7862, 7890, 7916, 7935, 7965, 7994, 8024, 8054, 8088, 8128, 8149, 8166 |
| `src/tui/app/mod.rs` — `/worktree` arm | 6720 |
| `src/mcp/cli.rs` — the `McpCommand::Debug` arm | 330, 394, 410 |

`src/mcp/cli.rs:312` obtains `Handle::current()` and the arm is reached from `src/main.rs:1847`, which is inside the same `#[tokio::main]` future, so the CLI sites are in the same state as the TUI sites.

The blast radius reaches the user because:

- `App::on_key` (`src/tui/app/mod.rs`) is synchronous.
- The only `catch_unwind` in the TUI event loop wraps rendering, not key handling.
- `app.lsp_tool` is populated with `Some(..)` whenever the process is not in socket mode (`src/main.rs`), which is the default local TUI mode, so the `/lsp-*` commands are live in the shipped binary.

Why the existing test suite is green:

- `src/tui/app/mod.rs` has three `lsp_doctor` tests, one of which reaches the `handle.block_on(..)` call.
- Those tests enter the runtime with `tokio::runtime::Runtime::new().unwrap()` followed by `rt.enter()`.
- `Runtime::enter` → `Handle::enter` → `context::try_set_current` → `Context::set_current` sets the current **handle** and increments the enter depth. It does **not** set `EnterRuntime::Entered`, which is only ever set by `context::enter_runtime`.
- So in the test the nested `block_on` finds `maybe_guard = Some(..)` and succeeds. The test exercises a different runtime-entry path than production.

The correct pattern is already present three times in the same file — `tokio::task::block_in_place(|| { ... Handle::current().block_on(..) .. })` at `src/tui/app/mod.rs:3035`, `:3053` and `:17333` — and `block_in_place` is what makes the nested entry legal, because it leaves the runtime context before the inner `block_on` re-enters it.

## 4. Invariants that must not regress

- The TUI must not lose the user's typed input, the current session, or the visible transcript when a command fails.
- A command that cannot complete must fail closed with a message on the surface that asked for it, and must not leave a pending indicator behind.
- The MCP debug subcommand must not print a partial result and must not leave a connection open.
- The one-frame-table / one-`ActivityPhase` cadence must not gain a second spinner or a second frame source.
- A slow command must not block the event loop's ability to service input; the fix must not make the UI unresponsive while the command runs.
- `scripts/verify.sh quick` and CI must stay green.

## 5. Scope

**In scope**

- The 21 call sites listed in §3.
- A static guard that fails when a `Handle::block_on` appears outside a `block_in_place` in production code.
- A regression test that reproduces the production runtime-entry path and fails on the baseline.
- The `catch_unwind` boundary, if the analysis in §8 shows the key-handling path needs one.

**Out of scope**

- Converting the command dispatcher to `async`. `dispatch_builtin_command` is synchronous and 161 arms wide; converting it is C003's neighbourhood and is not required to remove the panic.
- Converting these arms to the `spawn_tui_task` / `finish(request_id)` pattern. That is the architecturally preferred shape and is the right long-term answer, but it changes the command's completion semantics, which §14 makes a stop condition rather than a silent part of this milestone. It is recorded as deferred work below.
- Any change to what the `/lsp-*` commands return.
- The 615-line disabled function body (C005) and the argument-extraction defect (C003).

## 6. Required production changes

### Core

None. This milestone changes no domain type, no durable value, and no protocol message.

### Storage

None.

### Protocol

None.

### Runtime

- The 21 call sites must not call `Handle::block_on` from a thread whose runtime context is entered.
- The bound form is `tokio::task::block_in_place(|| { .. Handle::current().block_on(..) .. })`, matching the three sites already in `src/tui/app/mod.rs`. `block_in_place` requires a multi-threaded runtime; the TUI's `#[tokio::main(flavor = "multi_thread")]` satisfies this, and the implementer must confirm the MCP CLI path also runs on a multi-threaded runtime before using it there.
- If `block_in_place` is not viable at a site — for example if the enclosing task is not on a runtime worker — the site's fix is to make the wait async rather than to suppress the panic.

### Frontend

- The user-visible result of each affected command must be unchanged. A command that used to return a string must still return the same string on the same surface.

### Security

- No change. A panic is an uncontrolled termination, and removing it is a safety improvement, but no authorization boundary moves.

### Docs

- `architecture/tui.md` must record the rule that a command handler may not block on a runtime handle from inside the runtime, and the reason the existing `lsp_doctor` test could not catch it.

## 7. Ordered work packages

### WP1 — Fix the TUI call sites

Intent: remove the 18 nested entries in `src/tui/app/mod.rs`.

Required changes: wrap each `handle.block_on(..)` in `tokio::task::block_in_place`, matching the existing sites in the same file. Keep the result binding identical.

Acceptance evidence: the 18 sites no longer call `Handle::block_on` outside a `block_in_place`; the affected commands return the same values as before, asserted by the tests in §10.

### WP2 — Fix the MCP CLI call sites

Intent: remove the 3 nested entries in `src/mcp/cli.rs`.

Required changes: the `McpCommand::Debug` arm must not block on a runtime handle from inside the runtime. Confirm the CLI's runtime flavour before choosing the form; if the CLI is entered from a current-thread runtime, `block_in_place` is not legal and the arm must be made async.

Acceptance evidence: `codegg mcp debug` completes and prints its result in a process whose runtime context is entered, asserted by the test in §10.

### WP3 — Add the static guard

Intent: make the defect class unrepeatable.

Required changes: a new `scripts/check_no_nested_runtime_block_on.py` that scans production Rust sources for `Handle::block_on` / `handle.block_on` occurrences that are not inside a `block_in_place` closure, and fails with the file and line. It must:

- use `scripts/rust_test_scan.py` for anything that requires parsing Rust, so a lifetime is never misread as a character literal;
- exclude test modules, or apply a separate, recorded rule to them — the existing `lsp_doctor` tests call the same code path and their status must be decided explicitly rather than by accident;
- provide `--self-test` that injects a violating call and proves the guard rejects it, and a negative case it must accept, so it cannot pass vacuously;
- be wired into `scripts/verify.sh`'s guard set and into CI, with the wiring proven by the guard actually running in `verify.sh quick`.

Acceptance evidence: `verify.sh quick` runs the new guard; the `--self-test` passes; the guard fails when a violating call is injected into a production file.

### WP4 — Add the regression test

Intent: a test that reproduces the production entry path, so the defect cannot return unnoticed.

Required changes: a test member in the `tui_render` family that drives an affected command from inside a runtime whose context is **entered** — that is, the equivalent of `Runtime::block_on`, or `rt.enter()` **plus** an actual `block_on` of the driving future — and asserts that the command completes.

The test must be written so that it would **fail on the baseline**. A test that enters with `rt.enter()` alone cannot serve as evidence, and must not be accepted as such.

Acceptance evidence: the test fails when run against the baseline implementation and passes against the fix.

### WP5 — Record the rule

Intent: prevent the shape from being reintroduced by someone who does not know the mechanism.

Required changes: `architecture/tui.md` gains a short subsection stating the rule and the reason the unit test could not catch it. The subsection links to the guard; it does not restate the guard's implementation.

Acceptance evidence: the subsection exists, is linked from the guard's docstring, and does not duplicate the guard's logic.

## 8. Failure, cancellation, restart, contention semantics

- **Failure.** A command whose work fails must fail closed with a message. The current behaviour — process termination — is what this milestone replaces, so a test must assert the message path, not merely the absence of a panic.
- **Cancellation.** The task that carries the blocking wait is cancelled when the session is torn down. Under `block_in_place` the worker thread is moved off the runtime while it waits; the implementer must confirm that cancellation of the enclosing task cannot leave the worker thread parked indefinitely.
- **Restart.** Nothing durable changes, so a restart is unaffected.
- **Contention.** `block_in_place` moves the current task off its worker thread and lets the runtime schedule another task there. The TUI's single event-loop task must not deadlock against the work it is waiting for — in particular, the LSP work must not require the event loop to make progress in order to complete. If it does, the `spawn_tui_task` form is required and §14 applies.
- **Panic containment.** Whether the key-handling path needs a `catch_unwind` is an open question this milestone must answer. Adding one is a behaviour change — it turns a crash into a degraded session — and it must be decided deliberately, recorded, and tested, not added as a side effect. If it is added, the recovered state must be one the user can continue from.

## 9. Compatibility and migration

No wire, storage, or configuration compatibility impact. The user-visible outcome of every affected command is unchanged. The only observable difference is that the process no longer terminates.

## 10. Required tests

**Focused**

- A test that enters the runtime the way production does and drives one `/lsp-*` command to completion.
- A test that the same command returns the same value as before the change.
- A test that `codegg mcp debug` completes inside an entered runtime context.
- A test asserting the failure path produces a message when the underlying call fails.

**Integration**

- A `tui_render` family member that exercises the command-dispatch path end to end through the event loop's key handling, not by calling the handler directly — the defect is in the *composition* of the entry path and the handler, and a direct handler call does not reproduce it.

**Security**

- None required. This milestone does not move an authorization boundary.

**Migration**

- None.

**Placement.** The root package's integration tests are families, one executable per subject area, with members included by `#[path]`. A new member is `tests/tui_render/<name>.inc` and is bound in `tests/tui_render/mod.rs` with `#[path = "<name>.inc"] pub mod <name>;`. The family target is already declared in the root `Cargo.toml`; a **new** family would need that declaration, and is not expected here.

**Vacuity.** A test that cannot fail on the baseline is not evidence. The implementer must show the test failing against the baseline implementation, and record that result.

## 11. Required verification commands

```bash
# Scoped: quick's guards plus only the tests this change can affect.
scripts/verify.sh scoped HEAD~1

# The new guard, including its non-vacuity proof.
python3 scripts/check_no_nested_runtime_block_on.py --self-test

# The affected families.
cargo test --test tui_render
cargo test --test security_permission

# Rust-aware test inventory (ground truth; never regex-scan Rust).
python3 scripts/rust_test_scan.py --validate

# Full workspace, before the closure record is written.
bash scripts/verify.sh full
```

`scripts/verify.sh full` runs `cargo clippy --workspace --all-targets --locked -- -D warnings`. On macOS this currently fails for reasons unrelated to this milestone (C005 owns that asymmetry). The implementer must record the exact failure set before and after, and state that the change neither added nor removed a lint finding. On Linux the gate is green and must stay green.

## 12. Documentation updates

- `architecture/tui.md`: the rule that a command handler may not block on a runtime handle from inside the runtime, and why the existing unit test could not catch it.
- `plans/registry.md`: this milestone's status, per the registry maintenance rules.
- `docs/` user-facing documentation: none. The user-visible behaviour is unchanged.

## 13. Acceptance criteria

Externally observable:

1. Running any `/lsp-*` command in the TUI produces a result or a message. The TUI process remains alive.
2. Running `/worktree` in the TUI produces a result or a message. The TUI process remains alive.
3. Running `codegg mcp debug` completes and prints its result. The process exits with success.
4. Each affected command returns the same value it returned before the change.
5. `verify.sh quick` runs the new guard, and the new guard's `--self-test` passes.
6. The regression test fails against the baseline and passes against the fix.

## 14. Stop conditions

The implementer must report rather than improvise if any of the following holds.

- The fix would change what a command returns or displays. The user-visible result is out of scope.
- The fix would require converting the command dispatcher to `async`, or would require the `spawn_tui_task` completion-semantics change. Those are separate decisions with their own closure obligations; §5 records them as deferred.
- `block_in_place` turns out to be illegal on the runtime the MCP CLI uses. The arm must then be made async, and if that is a larger change than this milestone's boundary allows, report it.
- The deadlock risk in §8 turns out to be real — that is, the LSP work cannot complete while the event loop is blocked waiting for it.
- A `catch_unwind` would be needed around key handling, and the degraded state it produces cannot be made one the user can continue from.
- Any new call site of this shape is discovered outside the 21 sites listed in §3. The inventory must then be completed before the fix, not after.

## 15. Closure evidence required

- The guard's `--self-test` output, and a demonstration that the guard fails on an injected violating call.
- The regression test's failure against the baseline, captured.
- The exact `verify.sh scoped` and `verify.sh full` results, labelled local versus CI, with the macOS lint set stated before and after.
- The inventory of every `Handle::block_on` in production code and its disposition, so that "no remaining site" is an assertion backed by a list rather than an absence.
- A statement of whether a `catch_unwind` was added around key handling, and if so, the recovered-state contract.
- The registry update, and the unblock audit of the registry's Blocked work section.

## 16. Handoff notes

- Read `architecture/tui.md` before starting. The `spawn_tui_task` / `finish(request_id)` / `fail(request_id, err)` pattern with its stale-completion test is the architecturally preferred shape; this milestone deliberately does not adopt it, and says so.
- The three `block_in_place` sites already in `src/tui/app/mod.rs` are the model for the fix. Read them before writing new ones.
- The single most important detail in this plan is in §3: **the existing test does not reproduce the defect, because `rt.enter()` does not set the entered flag.** A regression test written the same way will pass on the baseline and prove nothing. The implementer must show the test failing against the baseline.
- The affected `/lsp-*` commands are documented in `architecture/tui.md`; their expected results are the reference for the "same value" assertion.
- This milestone closes no other milestone. C005 depends on it, because C005 re-enables the verification surface that should have caught this class of defect.
