# Repository Audit Corrective Milestone 001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/repository-audit-corrective/001-nested-runtime-panic-removal.md`

Source subsystem roadmap:

- `plans/subsystems/repository-audit-corrective-roadmap.md#milestone-c001--nested-runtime-panic-removal-in-the-tui-and-mcp-cli`

Repository baseline reviewed: `8590c17c5` (branch `fix/c001-audit-corrective`; workstream baseline `d5be7acd0`)

Implementation commits:

- `6c60b3921` — `fix(tui): remove the nested runtime entry on the LSP command path`

This record and the registry update land in the follow-up commit on the same
branch (`fix/c001-audit-corrective`), `docs(plans): close the repository audit
corrective C001 milestone`.

## 1. Executive finding

C001 is closed. Every one of the 21 identified call sites — the 17 `/lsp-*`
command arms and the `/worktree` arm in `src/tui/app/mod.rs`, and the three
sites in the `McpCommand::Debug` arm in `src/mcp/cli.rs` — now waits through
`tokio::task::block_in_place`, the pattern the repository already used at
three sites in the same file. The nested runtime entry is gone from the
production path: `Handle::block_on` inside an already-entered runtime context
panicked with *"Cannot start a runtime from within a runtime"*, and because
`App::on_key` is synchronous and the event loop's only `catch_unwind` wraps
rendering rather than key handling, that panic terminated a running TUI.

The user-visible result of every affected command is unchanged: the same
string, on the same surface. No dispatcher was made `async`, and the
`spawn_tui_task` / `finish(request_id)` / `fail(request_id, err)` completion
pattern was deliberately **not** adopted — the plan records it as deferred
work precisely because it changes completion semantics.

No `catch_unwind` was added around key handling; the reasoning is recorded in
§5 and the residual risk in §10.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| §13.1 — every `/lsp-*` command produces a result or a message, and the TUI survives | `tests/tui_render/nested_runtime_command_dispatch.inc` (`lsp_servers_command_completes_inside_the_event_loop_runtime`, `lsp_doctor_command_with_an_argument_completes_inside_the_event_loop_runtime`), driven through `App::on_key` inside a real `Runtime::block_on` | pass | The `/lsp-servers` toast is exactly `No active LSP servers`; the `/lsp-doctor` toast contains `LSP Doctor` — the same strings the arms produced before the change |
| §13.2 — `/worktree` produces a result or a message, and the TUI survives | `worktree_command_completes_inside_the_event_loop_runtime` | pass | The toast is exactly `No worktrees found` for a workspace outside a repository, and the app leaves command mode |
| §13.3 — `codegg mcp debug` completes inside an entered runtime context | `src/mcp/cli.rs::debug_runtime_entry_tests::mcp_debug_completes_inside_an_entered_runtime` | pass | The command returns `Err` from the MCP connection attempt and does not panic |
| §13.4 — each affected command returns the same value it returned before the change | §3 "same value" table; the assertions above | pass | Only the *waiting mechanism* changed; no arm's produced string, surface, or error text changed |
| §13.5 — `verify.sh quick` runs the new guard, and its `--self-test` passes | `scripts/verify.sh` (`run_guards()`), `.github/workflows/ci.yml`; §4 | pass | The guard is wired into both, and the self-test's 12 cases all pass |
| §13.6 — the regression test fails against the baseline and passes against the fix | §4 "Baseline failure" | pass | Captured verbatim in §4 |
| §3 inventory — every `Handle::block_on` in production code, with a disposition | §3 table | pass | No site remains outside `block_in_place` |
| §15 — the guard fails on an injected violating call | §4 "Guard non-vacuity" | pass | The guard is run against the reverted production files and reports all 21 sites |
| §15 — the inventory, the `catch_unwind` statement, the registry update, and the unblock audit | §3, §5, §11 | pass | |
| §11 — `verify.sh full` (clippy `-D warnings`) | **not run** | not run | The implementing session's instructions for this shared machine forbid `verify.sh full` and every broad workspace test sweep. §10 records this as an outstanding item, with the macOS lint asymmetry named by C005. |

## 3. Production implementation evidence

### The 21 sites, and what each became

Every site below keeps its result binding, its produced string, and its
surface; only the wait mechanism changed.

| Site (before the change) | After | Result binding |
|---|---|---|
| `src/tui/app/mod.rs` — 17 `/lsp-*` arms: `/lsp-servers`, `/lsp-capabilities`, `/lsp-errors`, `/lsp-doctor`, `/lsp-context-diagnostics`, `/lsp-repair-local`, `/lsp-repair-hunk`, `/lsp-review-file`, `/lsp-review-diff`, `/lsp-security-review`, `/lsp-impact`, `/lsp-test-repair`, `/lsp-interface`, `/lsp-cross-repair`, `/lsp-call-neighbors`, `/lsp-restart`, `/lsp-stop` | `tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(..))` | unchanged (`detail`, `text`, `result`, `outcome`) |
| `src/tui/app/mod.rs` — the `/worktree` arm | the same | `trees` |
| `src/mcp/cli.rs` — the `McpCommand::Debug` arm, three sites (a configured remote server, `--url` only, and `--name` plus `--url`) | the same | `result` |

The `let handle = tokio::runtime::Handle::current();` bindings were removed
where they existed only to feed the `block_on`, and the now-unused
`use tokio::runtime::Handle;` in the MCP arm was removed with them.

### The full `Handle::block_on` inventory, with a disposition

"No remaining site" is an assertion backed by this list, not an absence.

| File | Site | Disposition |
|---|---|---|
| `src/tui/app/mod.rs` | `read_persisted_theme_id` | Pre-existing `block_in_place`; unchanged |
| `src/tui/app/mod.rs` | `read_persisted_preferences` | Pre-existing `block_in_place`; unchanged |
| `src/tui/app/mod.rs` (a `#[cfg(test)]` module) | `apply_persisted_preferences` | Pre-existing `block_in_place`; unchanged |
| `src/tui/app/mod.rs` | 17 `/lsp-*` arms | **Fixed by this milestone** — now `block_in_place` |
| `src/tui/app/mod.rs` | `/worktree` arm | **Fixed by this milestone** — now `block_in_place` |
| `src/mcp/cli.rs` | `McpCommand::Debug`, three sites | **Fixed by this milestone** — now `block_in_place` |

`Runtime::block_on` is a **different operation** and is not this milestone's
subject: blocking on a `Runtime` value is not a `Handle` entry. The
repository's uses of it were read and each is legal — none of them nests into
an entered context:

| File | Site | Why it is not a nested entry |
|---|---|---|
| `src/upgrade/managed.rs` | `CodeggTransport::block_on` | Builds a **fresh** current-thread runtime; reached from `tokio::task::spawn_blocking` (`src/upgrade/mod.rs:110`), a thread with no entered runtime context |
| `src/managed_process.rs` | `run_blocking` | Builds a fresh current-thread runtime on a dedicated `std::thread` |
| `src/plugin/runtime/wasm.rs`, `src/tool/work_order.rs`, `src/error.rs`, `src/shell/runtime.rs`, `src/tui/mod.rs`, `src/tool/job.rs` | test-module `Runtime::block_on` calls | Test-local runtimes, at the top level of a `#[test]` body — no entered context |

### The "same value" reference

`architecture/lsp.md` §"Slash commands" and `architecture/command.md` §the
command table are the reference for what each `/lsp-*` command reports. The
assertions in the new member pin the exact strings the arms produce:

| Command | Asserted string |
|---|---|
| `/lsp-servers` | `No active LSP servers` |
| `/lsp-doctor <path>` | a toast containing `LSP Doctor` |
| `/worktree` | `No worktrees found` |

### The guard

`scripts/check_no_nested_runtime_block_on.py` scans every `*.rs` file under
`src/` and `crates/` and fails the build when a `Handle`'s `block_on` appears
outside a `block_in_place(...)` argument.

**The rule for test modules, decided explicitly.** The guard applies to
`#[cfg(test)]` modules inside the production trees exactly as it applies to
production code: there is **no file exemption and no line-pattern
allowlist**. A `Handle::block_on` in a test module is the same defect, because
a `#[tokio::test]` body runs with the runtime context entered and would
panic there too. The `lsp_doctor` tests in `src/tui/app/mod.rs` reach the
production handler and therefore travel the fixed path; after the fix they
contain no `Handle::block_on` of their own, so they satisfy the rule as they
stand and needed no change.

The root `tests/` tree is **not** scanned. It is the integration-test
harness, not shipped code, and this milestone's regression test must be able
to drive the production handler from inside a real `Runtime::block_on`. It
does so by calling the production handler, so it needs no exemption of its
own.

The scan is Rust-aware: source is masked with `scripts/rust_test_scan.py`
before it is searched, so a lifetime is never misread as a character literal
and a comment that merely names `Handle::block_on` is not a violation.

## 4. Verification executed

### Commands run

```bash
# The new guard, including its non-vacuity proof.
python3 scripts/check_no_nested_runtime_block_on.py --self-test
python3 scripts/check_no_nested_runtime_block_on.py

# The affected family (narrow: one member).
CARGO_BUILD_JOBS=3 cargo test --test tui_render -- nested_runtime_command_dispatch

# The MCP CLI focused test.
CARGO_BUILD_JOBS=3 cargo test -p codegg --lib mcp::cli::debug_runtime_entry_tests

# Rust-aware test inventory (ground truth; never regex-scan Rust).
python3 scripts/rust_test_scan.py --validate

# The test-surface guards the new tests touch.
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
python3 scripts/check_test_ownership.py
python3 scripts/check_test_target_references.py

# The canonical sanity sweep.
scripts/verify.sh quick
```

### Results

| Command | Result |
|---|---|
| `python3 scripts/check_no_nested_runtime_block_on.py --self-test` | **pass** — 12/12 cases |
| `python3 scripts/check_no_nested_runtime_block_on.py` | **pass** — "nested runtime check passed — no Handle::block_on outside a block_in_place" |
| `cargo test --test tui_render -- nested_runtime_command_dispatch` | **pass** — 4 passed, 0 failed |
| `cargo test -p codegg --lib mcp::cli::debug_runtime_entry_tests` | **pass** — 1 passed, 0 failed |
| `python3 scripts/rust_test_scan.py --validate` | **pass** — all checks pass (tests 5147, src 7551, crates 3774; residual 0) |
| `python3 scripts/check_test_assertion_quality.py` | **pass** |
| `python3 scripts/check_test_duplication.py` | **pass** |
| `python3 scripts/check_test_ownership.py` | **pass** |
| `python3 scripts/check_test_target_references.py` | **pass** |
| `scripts/verify.sh quick` | **pass** — see §4.1 |
| `scripts/verify.sh full` | **not run** — forbidden by this session's shared-machine instructions; see §10 |

### 4.1 The guard's `--self-test` output

```
  ok   a bare Handle::current().block_on is caught
  ok   a Handle binding's .block_on is caught
  ok   the same call inside block_in_place is accepted
  ok   a Handle binding inside block_in_place is accepted
  ok   one legal and one illegal call in the same file: only the illegal one is reported
  ok   a Runtime's block_on is not this rule's subject
  ok   a comment that names Handle::block_on is not a violation
  ok   a string literal that names block_in_place does not protect a call
  ok   a lifetime is not read as a character literal
  ok   a nested block_in_place around a nested block_in_place is accepted
  ok   a call after a completed block_in_place is still reported
  ok   a test module's Handle::block_on is caught too

self-test passed: 12 case(s).
```

### 4.2 Baseline failure (the regression test fails on the baseline)

The production files were reverted with `git stash push -- src/tui/app/mod.rs
src/mcp/cli.rs` (the new test files stayed in place, and the MCP CLI's test
module was re-attached to the reverted file so the focused test existed
against the baseline production code). Both test targets were then run
against that baseline and both failed, with tokio's nested-entry panic. The
panic locations are the **baseline production lines**, which is the proof
that the tests drive the production path rather than a copy of it.

**Process note, recorded because it was a real hazard.** This capture ran
*before* the implementation was committed, which is the ordering that lost a
sibling milestone's work when its turn died mid-capture. The implementation
was protected by mirroring both changed production files to
`/tmp/c001-baseline/` **before** the stash, and by verifying byte-identity
against the restored files afterwards
(`diff /tmp/c001-baseline/app-mod-fixed.rs src/tui/app/mod.rs` and the same
for `src/mcp/cli.rs`, both empty). The lesson stands regardless: the next
capture on this branch commits first and reverts with
`git checkout HEAD -- <production files>`.

`cargo test --test tui_render -- nested_runtime_command_dispatch`:

```
running 4 tests
test nested_runtime_command_dispatch::worktree_command_completes_inside_the_event_loop_runtime ... FAILED
test nested_runtime_command_dispatch::lsp_servers_command_completes_inside_the_event_loop_runtime ... FAILED
test nested_runtime_command_dispatch::a_command_that_waits_on_the_runtime_leaves_the_session_usable ... FAILED
test nested_runtime_command_dispatch::lsp_doctor_command_with_an_argument_completes_inside_the_event_loop_runtime ... FAILED

---- nested_runtime_command_dispatch::lsp_servers_command_completes_inside_the_event_loop_runtime stdout ----

thread 'nested_runtime_command_dispatch::lsp_servers_command_completes_inside_the_event_loop_runtime' panicked at src/tui/app/mod.rs:7736:41:
Cannot start a runtime from within a runtime. This happens because a function (like `block_on`) attempted to block the current thread while the thread is being used to drive asynchronous tasks.

---- nested_runtime_command_dispatch::lsp_doctor_command_with_an_argument_completes_inside_the_event_loop_runtime stdout ----

thread 'nested_runtime_command_dispatch::lsp_doctor_command_with_an_argument_completes_inside_the_event_loop_runtime' panicked at src/tui/app/mod.rs:7816:39:
Cannot start a runtime from within a runtime. This happens because a function (like `block_on`) attempted to block the current thread while the thread is being used to drive asynchronous tasks.

---- nested_runtime_command_dispatch::worktree_command_completes_inside_the_event_loop_runtime stdout ----

thread 'nested_runtime_command_dispatch::worktree_command_completes_inside_the_event_loop_runtime' panicked at src/tui/app/mod.rs:6720:67:
Cannot start a runtime from within a runtime. This happens because a function (like `block_on`) attempted to block the current thread while the thread is being used to drive asynchronous tasks.

---- nested_runtime_command_dispatch::a_command_that_waits_on_the_runtime_leaves_the_session_usable stdout ----

thread 'nested_runtime_command_dispatch::a_command_that_waits_on_the_runtime_leaves_the_session_usable' panicked at src/tui/app/mod.rs:7736:41:
Cannot start a runtime from within a runtime. This happens because a function (like `block_on`) attempted to block the current thread while the thread is being used to drive asynchronous tasks.

test result: FAILED. 0 passed; 4 failed; 0 ignored; 0 measured; 1052 filtered out; finished in 0.06s
```

`cargo test -p codegg --lib mcp::cli::debug_runtime_entry_tests`:

```
running 1 test
test mcp::cli::debug_runtime_entry_tests::mcp_debug_completes_inside_an_entered_runtime ... FAILED

---- mcp::cli::debug_runtime_entry_tests::mcp_debug_completes_inside_an_entered_runtime stdout ----
Testing MCP server connection...

thread 'mcp::cli::debug_runtime_entry_tests::mcp_debug_completes_inside_an_entered_runtime' panicked at src/mcp/cli.rs:394:41:
Cannot start a runtime from within a runtime. This happens because a function (like `block_on`) attempted to block the current thread while the thread is being used to drive asynchronous tasks.

thread 'mcp::cli::debug_runtime_entry_tests::mcp_debug_completes_inside_an_entered_runtime' panicked at src/mcp/cli.rs:464:30:
the MCP debug command must not panic when the runtime context is already entered: Any { .. }

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 7418 filtered out; finished in 0.06s
```

The panic text is tokio 1.52.3's own (`runtime/context/runtime.rs`:
*"Cannot start a runtime from within a runtime. This happens because a
function (like `block_on`) attempted to block the current thread while the
thread is being used to drive asynchronous tasks."*), not the shorter
paraphrase the plan quoted.

### 4.3 Guard non-vacuity against the baseline

With the production files reverted, the guard was run against the tree and
reported **every one of the 21 sites** — so "no remaining site" is a list,
not an absence:

```
  src/mcp/cli.rs:330: let result = handle.block_on(async {
  src/mcp/cli.rs:394: let result = handle.block_on(async {
  src/mcp/cli.rs:410: let result = handle.block_on(async {
  src/tui/app/mod.rs:6720: let trees = tokio::runtime::Handle::current().block_on(async {
  src/tui/app/mod.rs:7736: let detail = handle.block_on(lsp_tool.lsp_servers_detail());
  src/tui/app/mod.rs:7759: match handle.block_on(lsp_tool.lsp_capabilities_for_key(key)) {
  src/tui/app/mod.rs:7780: match handle.block_on(lsp_tool.lsp_errors_for_key(key)) {
  src/tui/app/mod.rs:7816: let text = handle.block_on(lsp_tool.lsp_doctor(path_arg));
  src/tui/app/mod.rs:7835: let text = handle.block_on(lsp_tool.lsp_context_diagnostics(path_arg));
  src/tui/app/mod.rs:7862: match handle.block_on(lsp_tool.run_lsp_workflow(&invocation)) {
  src/tui/app/mod.rs:7890: match handle.block_on(lsp_tool.run_lsp_workflow(&invocation)) {
  src/tui/app/mod.rs:7916: match handle.block_on(lsp_tool.run_lsp_workflow(&invocation)) {
  src/tui/app/mod.rs:7935: match handle.block_on(lsp_tool.run_lsp_workflow(&invocation)) {
  src/tui/app/mod.rs:7965: match handle.block_on(lsp_tool.run_lsp_workflow(&invocation)) {
  src/tui/app/mod.rs:7994: match handle.block_on(lsp_tool.run_lsp_workflow(&invocation)) {
  src/tui/app/mod.rs:8024: match handle.block_on(lsp_tool.run_lsp_workflow(&invocation)) {
  src/tui/app/mod.rs:8054: match handle.block_on(lsp_tool.run_lsp_workflow(&invocation)) {
  src/tui/app/mod.rs:8088: match handle.block_on(lsp_tool.run_lsp_workflow(&invocation)) {
  src/tui/app/mod.rs:8128: match handle.block_on(lsp_tool.run_lsp_workflow(&invocation)) {
  src/tui/app/mod.rs:8149: let result = handle.block_on(lsp_tool.lsp_restart_server(key));
  src/tui/app/mod.rs:8166: let result = handle.block_on(lsp_tool.lsp_stop_server(key_opt));

21 violation(s) found.
```

### 4.4 `scripts/verify.sh quick`

**pass** — `EXIT=0`, ending in `==> Quick verification passed.`

The run covers rustfmt, the two generated-asset checks, the guard suite
(including `check_no_nested_runtime_block_on.py` in both its `--self-test`
and tree-scan forms, at lines 93–109 of the run log), and the workspace
`cargo check`. The only warnings are the pre-existing ones in
`src/lsp/m004.rs`, `src/managed_process.rs`, `src/security/sandbox.rs`, and
`src/bin/codegg-sandbox-helper.rs`; this change adds none.

## 5. Invariant review

| Invariant | Evidence | Result |
|---|---|---|
| The user-visible result of each affected command is unchanged | §3's "same value" table; the assertions in the new member | pass |
| The command dispatcher is **not** converted to `async` | `dispatch_builtin_command` and `dispatch_tui_command` are unchanged in shape; the diff touches only the waits | pass |
| The `spawn_tui_task` / `finish(request_id)` / `fail(request_id, err)` pattern is **not** adopted for these arms | No `spawn_tui_task` call was added; the plan records it as deferred | pass |
| The MCP CLI's runtime flavour is multi-thread, so `block_in_place` is legal there | `src/main.rs:1677` is `#[tokio::main(flavor = "multi_thread")]`; `mcp::cli::exec_mcp_command(command.clone())?` is called at `src/main.rs:1847`, inside `async fn main` — the whole of `main`, and therefore the MCP debug arm, runs on that multi-thread runtime. Tokio's multi-thread scheduler enters the runtime with `allow_block_in_place = true` (`runtime/scheduler/multi_thread/mod.rs`), and `Runtime::block_on` on a current-thread runtime would not | pass |
| No nested runtime entry remains on a production path reachable from a runtime context | §3's inventory; the guard's tree scan | pass |
| The panic is not relocated to a different site | The wait mechanism is the only change; no other `block_on` was introduced | pass |
| The LSP work does not require the event loop to make progress in order to complete | The LSP service's work runs on the runtime's worker threads; `block_in_place` leaves the runtime context around the inner `block_on` and, on a worker thread, hands the core to another worker. The `/lsp-servers` and `/lsp-doctor` arms complete in the regression test, which runs the real handler | pass |
| The command's result is produced on the same surface | Every arm still writes to `self.messages_state.toasts` | pass |
| Storage layout, protocol, scheduler, permission, and Git surfaces are untouched | The diff touches `src/tui/app/mod.rs` and `src/mcp/cli.rs` only | pass |

### Was a `catch_unwind` added around key handling?

**No.** The decision, recorded deliberately:

1. The defect was a *nested runtime entry*, not a recoverable panic. A
   `catch_unwind` would have converted a process-terminating defect into a
   silently degraded session — exactly the class of hidden failure this
   milestone exists to remove.
2. `App::on_key` mutates `App` in place. A panic mid-mutation leaves the app
   in an unknown state (the command palette's query set but the prompt not
   cleared, or the prompt mutated but the completion overlay stale), and
   there is no cheap, auditable way to derive a coherent state to recover
   into from a partially applied keypress. The plan's stop condition — "a
   `catch_unwind` would be needed around key handling, and the degraded state
   it produces cannot be made one the user can continue from" — is therefore
   not reached, because the `catch_unwind` is not needed at all.
3. The event loop's existing `catch_unwind`, which wraps rendering, was not
   extended. It exists for a different reason, and the plan does not extend
   it.

The residual risk — that a *different* panic in a command handler would still
kill the TUI — is the structural issue the plan defers, and is recorded in
§10.

## 6. Failure and recovery review

- **Malformed or unauthorised input**: the MCP CLI probe is given a URL that
  cannot be parsed; the failure surfaces as `AppError::Mcp(McpError::Connection("invalid URL: …"))`
  and the process reports the error rather than panicking. No network I/O is
  reached, because the URL is rejected before any request.
- **Partial persistence failure**: not applicable — no state is written.
- **Contention and resource release**: `block_in_place` borrows a runtime
  worker core for the duration of the wait on a worker thread and returns it
  when the closure ends, including on panic (`Reset`'s `Drop`). The waits
  here are short, bounded reads of the LSP service's own state.
- **Stale generation or lease**: not applicable — no lease is taken.
- **Cancellation races**: not applicable — the waits are not cancellable and
  were not cancellable before.
- **Bounded behaviour**: the LSP arms were already bounded by the LSP
  service's own timeouts; the fix does not change any bound.

## 7. Migration and compatibility review

- No schema migration, no storage layout change, no protocol change, no
  configuration change, no wire-format change.
- Backward compatibility: the user-visible behaviour is identical, so an
  existing session, script, or muscle memory is unaffected.
- Rollback limitation: the guard is a build-time check, so reverting the
  production change without reverting the guard turns the build red — which
  is the intended coupling.

## 8. Security review

- No authorization boundary moved. The plan records that this milestone does
  not move one, and the diff touches no permission, policy, sandbox, or
  secret-handling code.
- The MCP CLI probe fails closed: an unparsable URL is rejected before any
  socket is opened, and the SSRF guard (`validate_url_target`) still runs for
  any URL that does parse.
- No secret is logged. The `tracing::debug!` lines in the MCP arm are
  unchanged.
- Denial-of-service bounds: unchanged. The fix does not add a wait that
  outlives the LSP service's own timeouts.

## 9. Documentation and operations

| Document | Change |
|---|---|
| `architecture/tui.md` | New subsection **"Blocking on a runtime handle"**: the rule, the panic it prevents, the `block_in_place` form, why the existing unit test could not catch it, and a link to the guard. A dated `## Source verification` note records what was checked and when |
| `architecture/overview.md` | The `## Verified counts` table's three moved rows re-measured (integration test files 327 → 328, guard scripts 80 → 81, CI guard scripts 40 → 41), with a dated re-measurement paragraph |
| `architecture/count_audit_history.md` | A third amendment recording the three moved values and confirming that no row's *measure* changed |
| `plans/registry.md` | The C001 milestone row's status moves to **closed**, with the closure pointer and the unblock audit |
| `docs/` | None. The user-visible behaviour is unchanged, so there is no user-facing documentation to update |

Static guards added: `scripts/check_no_nested_runtime_block_on.py`, wired into
`scripts/verify.sh`'s `run_guards()` and into the hosted CI guard step.

Recovery instructions: none — the defect was a crash, and the fix is a
mechanism change with no operational state.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | `scripts/verify.sh full` was **not run** in this session, so the clippy `-D warnings` set was not re-measured before and after | The plan's §11 requires the macOS lint set to be stated before and after. This session's instructions forbid `verify.sh full` and every broad workspace sweep, because the machine is shared with three other parallel milestones | Run `bash scripts/verify.sh full` on an idle machine, and record the clippy set. The change adds no new lint pattern — the only new constructs are `tokio::task::block_in_place` closures and a `#[cfg(test)]` module — but the gate was not observed here. C005 owns the macOS/Linux lint asymmetry itself |
| medium | The whole-workspace test sweep was not run | Only the `tui_render` family member, the MCP CLI unit test, and the Python guards were run. A defect elsewhere in the workspace caused by this change would not have been observed | Run `cargo nextest run --workspace --locked --profile ci` on an idle machine |
| low | A command handler that panics for a reason *other* than a nested runtime entry still terminates the TUI, because there is no `catch_unwind` around key handling | The class of defect is narrowed to "the handler itself is wrong"; the process still dies | Deliberately deferred. The architecturally preferred shape is the `spawn_tui_task` / `finish(request_id)` / `fail(request_id, err)` pattern with a stale-completion test, which the plan records as deferred work because it changes completion semantics. The structural cause — a 161-arm dispatcher in a 20k-line module — is recorded in the roadmap's §4 and is not restructured here |
| low | The guard recognises a `Handle` receiver as `Handle::current()`, a `Handle`-qualified path, or an identifier whose name ends in `handle` | A `Handle` bound to a name that does not end in `handle` (say `let h = …`) would not be detected | The naming convention is the repository's own; a violation would be caught by review rather than by the guard. Recorded rather than left implicit |

## 11. Roadmap disposition

**Milestone closed.** The C001 invariant — no user-facing command can
terminate the process by panicking on a nested runtime entry — holds on the
production paths in scope, is backed by a guard that fails the build on a
regression, and is backed by a regression test that was shown failing on the
baseline.

**Unblock audit.** C001's closure unblocks **nothing** on its own:

- **C005** (verification surface and dead code) needs C001–C004 closed. C002,
  C003 and C004 are still open, so C005 remains blocked.
- **C006** (TUI render-path and transcript cost) needs C001 and C003. C003 is
  still open, so C006 remains blocked.
- **C007** (comment corpus reduction and drift guard) needs C001–C006 closed.
  It remains blocked.

No other registered plan names C001 as a dependency.

## 12. Registry updates

- `plans/registry.md`, the `Repository audit corrective` workstream row: the
  C001 milestone row's status moves from **ready for handoff** to **closed**,
  with the closure pointer
  `plans/closure/repository-audit-corrective/001-status.md` and this
  milestone's summary.
- `plans/subsystems/repository-audit-corrective-roadmap.md`: the C001
  milestone's exit conditions are met. Its dependency graph rows for C005,
  C006 and C007 are unchanged, because none of them is unblocked by C001
  alone.
- No other registry row changes: C001 closes no other milestone, and the
  workstream row's own status is unchanged because three of its seven
  milestones are still open.
