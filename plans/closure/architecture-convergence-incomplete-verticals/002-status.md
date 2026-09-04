# Architecture Convergence M002 — Process and Tool Execution Ownership Closure

Status: conditionally closed

Source implementation plan:

- `plans/implementation/architecture-convergence-incomplete-verticals/002-process-tool-execution-ownership-convergence.md`

Source subsystem roadmap:

- `plans/subsystems/architecture-convergence-incomplete-verticals-roadmap.md`

Repository baseline reviewed: `3c4890035513cd4d74430b6f64523c8be676024e`

Implementation commit:

- [`ffca847`](https://github.com/dbowm91/codegg/commit/ffca847) — converge finite process lifecycle on `ManagedProcessService`, migrate root finite callers, and add the ownership inventory and guard coverage.

The implementation plan moves from `closing` to `implemented` by the closure
commit that adds this record and updates the planning controls.

## 1. Executive finding

M002's production implementation is complete. Finite local process execution
now has one canonical lifecycle owner at `src/managed_process.rs`, with typed
argv requests/results, explicit cwd and environment policy, process-group
cleanup, timeout/cancellation, bounded capture and streaming, sandbox-helper
coordination, exit classification, cleanup diagnostics, and reaping.

Scheduler admission remains in `JobSubmissionService`; the managed process
service executes accepted attempts and is not a second scheduler. Shell
semantics, authorization, protocol framing, Git semantics, and domain result
mapping remain in their owning adapters.

The status is conditionally closed because this host cannot execute the root
CodeGG focused test binary: its x86_64 Rust target encounters incompatible
MacPorts arm64 native libraries during linking, and the explicit x86_64 retry
stalls in the host linker. Compilation, static guards, quick verification,
and targeted Clippy provide source/build evidence; the focused runtime suite
must be rerun on CI or a corrected host toolchain.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Every production spawn site has an ownership disposition | `docs/execution-ownership.toml`, strengthened imported-`Command` scan, inline test annotations, and `architecture/process-tool-execution-ownership.md` | pass |
| One canonical finite lifecycle owner | `ManagedProcessService::run`, `run_blocking`, and `run_streaming` in `src/managed_process.rs`; migrated shell, hook, plugin, terminal, formatter, IDE, RTK, and Python probe callers | pass |
| Explicit cwd/env and typed argv are preserved | `ManagedProcessRequest`, `EnvironmentPolicy::sanitized/inherited`, and migrated caller request construction | pass by source and compile evidence |
| Timeout/cancellation/process-tree cleanup/reaping remain centralized | Existing managed-process session cleanup plus timeout/cancellation tests and no duplicate finite lifecycle helpers in migrated callers | pass by source and compile evidence; focused runtime blocked by host linker |
| Bounded stdout/stderr capture and foreground streaming | Existing bounded capture plus `ManagedProcessOutputChunk`, `run_streaming`, and streaming regression test | pass by source and compile evidence; focused runtime blocked by host linker |
| Sandbox/resource policy remains fail-closed | Existing sandbox helper/status-pipe path retained under `ManagedProcessService`; `check_sandbox_contract.py` and quick verification pass | pass |
| Bash, shell sessions, Tool Programs, and ordinary one-shot callers converge | Bash and scheduler executors already consumed the service; shell runtime now uses streaming; other finite root callers use `run` or `run_blocking` adapters | pass |
| Scheduler authority is not duplicated | No new scheduler/registry; durable work still enters `JobSubmissionService`; ownership inventory distinguishes executor/adapters from protocol and domain exceptions | pass |
| Protocol children retain justified framing owners | LSP, MCP local, and standalone stdio are explicit inventory exceptions with concrete reasons in architecture docs | pass |
| Duplicate generic subprocess helpers are removed | Shell/RTK, plugin, hook, formatter, Python, IDE, and terminal-local lifecycle/capture wrappers were deleted or reduced to typed result mapping | pass |
| Compatibility remains intact | No storage or external protocol schema changes; shell IDs, run IDs, projection events, and user-facing result mapping are retained | pass by source and compile evidence |

## 3. Before/after ownership map

| Concern | Before | After |
|---|---|---|
| Finite process spawn | Several root callers and local wrappers | `ManagedProcessService` is the finite direct-spawn owner |
| Environment | Per-caller command env construction | Typed `EnvironmentPolicy`; sanitized by default, filtered inheritance only for human shell semantics |
| Timeout/cancellation | Shell, plugin, RTK, and other local wrappers | One managed lifecycle with typed termination reasons |
| Process tree | Repeated kill/abort paths | Managed Unix session/process-group termination and reaping |
| Output | Local `wait_with_output`, truncation, or streaming loops | Bounded head-plus-tail capture; optional bounded output chunks; final typed result remains authoritative |
| Scheduler | Admission and execution concerns adjacent in callers | `JobSubmissionService` admits durable jobs; schedulers invoke the managed service through executors |
| Protocol children | Direct child ownership without a single inventory | Explicit LSP/MCP/stdio exceptions, retaining framing and persistent connection state |
| Auditability | Static guard missed imported plain `Command::new` | Guard inventories fully-qualified and imported process commands and rejects unclassified sites |

The resulting production flow is:

```text
schema / authorization / command semantics
                  |
      JobSubmissionService for durable work
                  |
                  v
       ManagedProcessRequest (typed argv)
                  |
        ManagedProcessService
     /          |             \
 captured   blocking       streaming
                  |
                  v
       domain / projection result mapping
```

Protocol children and deferred typed Git/worktree probes are outside the
finite root service where framing, crate dependency direction, or domain
ownership makes that abstraction inappropriate. They are not hidden: each is
listed with an owner and reason in the checked-in manifest.

## 4. Production implementation

- Extended `ManagedProcessService` with filtered inherited environments,
  synchronous blocking adaptation, bounded output streaming, and preservation
  of the final typed result.
- Migrated `ShellRuntime` to typed managed streaming while retaining shell
  events, plugin environment hooks, session identity, and cancellation.
- Migrated plugin process execution, hooks, terminal tools, external
  formatters, IDE helpers, RTK projection/probes, and Python analysis/setup
  probes to managed requests.
- Preserved explicit working directories, stdin behavior, output limits,
  timeout categories, and domain-specific response mapping.
- Strengthened `scripts/check_execution_ownership.py` to inventory imported
  `tokio::process::Command` and `std::process::Command` uses without treating
  source-code strings in test fixtures as process sites.
- Added explicit manifest entries for protocol, standalone, helper, and
  deferred Git/worktree exceptions; annotated embedded test fixtures.
- Corrected two stale boolean `queue_message(...).is_err()` calls in the
  WebSocket compatibility path so the required strict Clippy command reaches
  the current API contract without changing protocol behavior.
- Added the canonical architecture/exception documentation and updated jobs,
  shell, LSP, and execution-ownership contracts.

## 5. Deleted and retained compatibility paths

Deleted or reduced to adapters:

- shell runtime-local `tokio::process::Child` timeout/abort/output loops;
- RTK-local threaded subprocess, timeout, kill, and output post-processing
  lifecycle;
- plugin-local `wait_with_output` and truncation helper;
- hook, formatter, terminal, IDE, and Python-local process construction and
  timeout/capture implementations.

Retained by justified exception:

- `crates/egglsp/src/launch.rs` for long-lived LSP Content-Length framing,
  restart state, and crate dependency direction;
- `src/mcp/local.rs` for persistent MCP JSON-RPC framing and connection state;
- `src/core/transport/stdio.rs` for deprecated standalone protocol framing;
- daemon bootstrap, external editor, speech, self-upgrade, and the
  installation-owned sandbox helper;
- typed Git/worktree/read probes pending M003 domain ownership convergence.

No public storage or protocol compatibility path was removed.

## 6. Complete spawn-site disposition

The complete machine-readable disposition is in
`docs/execution-ownership.toml`. The classified production groups are:

| Manifest group | Owner/disposition |
|---|---|
| `src/managed_process.rs` | Canonical finite-process direct-spawn owner |
| `src/tool/bash.rs`, `src/scheduler/`, `src/python_script/` | Scheduler-owned execution or canonical adapters |
| `src/shell/runtime.rs` | Interactive managed streaming adapter |
| `src/shell/rtk.rs`, `src/tool/formatter.rs`, `src/tool/terminal.rs`, `src/ide/`, `src/hooks/` | Managed blocking/finite adapters |
| `src/plugin/runtime/process.rs` | Managed finite child, deferred plugin admission/lifecycle domain |
| `crates/egglsp/src/launch.rs`, `src/mcp/local.rs` | Protocol-specialized long-lived children |
| `src/core/transport/stdio.rs` | Standalone compatibility transport |
| `src/core/instance.rs`, `src/tui/app/`, `src/tts/`, `src/core/notification.rs`, `src/upgrade/` | Explicit standalone or interactive exceptions |
| `src/bin/codegg-sandbox-helper.rs` | Helper beneath the canonical service |
| `src/git_mutations.rs`, `src/git_network_ops.rs`, `src/git_recovery.rs`, `src/git_service.rs`, `crates/egggit/`, `crates/codegg-core/src/worktree.rs`, `worktree_service.rs`, `repository_lineage.rs` | Deferred typed Git/worktree/domain execution for M003 |
| `src/security/workflow/report.rs`, embedded fixture lines in worktree/LSP tests | Test-only |

The guard passes with no `forbidden_bypass` entries and no unclassified
process/dispatch sites.

## 7. Verification executed

Successful commands:

```text
rtk cargo fmt --all -- --check
rtk cargo check -p codegg --all-targets
rtk cargo clippy -p codegg --lib -- -D warnings
rtk python3 scripts/check_execution_ownership.py --self-test
rtk python3 scripts/check_execution_ownership.py
rtk scripts/verify.sh quick
rtk git diff --check
```

Results:

- CodeGG all-target check passed.
- CodeGG library Clippy passed with `-D warnings`.
- Quick verification passed generated-agent freshness, core boundary,
  sandbox contract, execution ownership, formatting, and locked workspace
  all-target compilation.
- The ownership guard and its negative self-test passed.
- The focused managed-process/shell/plugin/Python test command was attempted,
  but root test-binary linking failed on the host's x86_64/arm64 native
  library mismatch. A retry with `/usr/local` x86_64 `libiconv`/`liblzma`
  paths stalled in the host linker before tests ran.
- The required strict all-features workspace Clippy command was attempted;
  its first pass exposed two pre-existing `queue_message` boolean API uses,
  which were corrected in the implementation commit. The subsequent
  feature-heavy all-target run became silent after compilation and was
  interrupted after no active compiler/linker process remained. No changed
  execution-path diagnostic was emitted; package Clippy and all-target check
  passed.

## 8. Security, failure, and recovery review

- Sanitized environments remain the default; inherited environments are only
  used for explicitly human shell commands and still strip unsafe Git
  variables.
- All finite managed processes retain bounded stdout/stderr drains, typed
  timeout/cancel/output-limit outcomes, Unix process-group termination, and
  direct-child reaping.
- Sandbox helper identity, owner-only temporary launch specs, bounded status
  channels, and fail-closed setup/exec handling remain under the canonical
  service.
- Shell kill now cancels the managed request while preserving shell-session
  event and store semantics.
- Protocol exceptions remain explicit and retain their own framing boundary;
  no shell parsing or unbounded captured-output abstraction was introduced.
- No credentials, raw authenticated remotes, or sensitive command output were
  added to durable state or closure artifacts.

## 9. Unresolved findings by severity

| Severity | Finding | Disposition |
|---|---|---|
| critical/high/medium | None in the changed execution ownership or lifecycle paths | closed |
| low | Focused root runtime tests cannot execute on this host because the x86_64 Rust/macOS link environment selects incompatible arm64 native libraries; the explicit compatible-path retry stalled in the linker | named condition for this conditional closure; rerun on CI or a corrected host toolchain |
| low | Feature-heavy all-target Clippy rerun did not complete after its pre-existing stale boolean calls were corrected | package Clippy, all-target check, and quick verification passed; rerun strict command in CI/corrected environment |

## 10. Migration and compatibility

No storage schema, external protocol, shell-session identity, Tool Program
run-ID, or projection-event migration was required. Existing callers retain
domain-specific result mapping and protocol framing. Deferred Git/worktree
sites remain explicitly tracked for M003; this closure does not claim their
domain ownership convergence.

## 11. Downstream dependency audit

M002 closes the interface dependency for M007. The registry and roadmap were
audited after implementation:

- M003 remains dependency-ready and unchanged.
- M008 remains independently dependency-ready and unchanged.
- M007 is now dependency-ready because its canonical process/edit integration
  boundary is stable; it is moved from blocked to the dependency-ready table.
- M004 remains blocked on M002 and M003; M002 is now satisfied but M003 is not.
- M005 remains blocked on M003.
- M006 remains blocked on M004.
- M001 remains conditionally closed on its separate named host-toolchain
  evidence condition and does not block M007.
- No new plan or ADR is required, and no other blocked plan becomes ready.

## 12. Registry updates

The closure commit:

- marks this implementation plan `implemented`;
- marks M002 closed in the source roadmap and retains its closure evidence;
- removes M002 from active implementation work;
- adds M007 to the dependency-ready implementation table;
- removes M007 from blocked work while preserving M004/M005/M006 blockers;
- records this closure in the recently completed control points; and
- records that M007, and only M007, was unblocked by this closure.

Final disposition: conditionally closed pending only the named host-toolchain
focused-runtime rerun and strict feature-heavy Clippy completion; no
corrective implementation pass is required.
