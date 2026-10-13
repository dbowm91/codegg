# C047 — Authoritative Windows process/tool coverage audit (audit slice)

Baseline: `5520df1` line head (C041–C048 registration); audit of record
for the C047 audit-safe slice. Production restricted-execution
integration remains gated on a separately admitted backend (C048) and
is explicitly NOT claimed here.

Authority: ADR-0027 bounded test-only; ADR-0004 ceilings; ADR-0016
platform-owned launch. C042 typed contract (`src/security/sandbox/
guarantee.rs::negotiate`) is the negotiation gate any future backend
must pass before an untrusted target starts.

## Method

Inventory taken 2026-10-13 from `grep` over `src/` for
`std::process::Command`, `tokio::process::Command`,
`ManagedProcessService::run*`, plus `docs/execution-ownership.toml`
and `architecture/process-tool-execution-ownership.md`. Each surface is
classified per C047 work package A and given an audit disposition for
package B. Package C (real backend wiring) and package D (negative
subprocess-indirection matrix against a real backend) are deferred to
post-admission; their absence is the explicit uncovered item below.

## Inventory (untrusted native executables must all reach the canonical authority)

| Surface | File(s) | Current dispatch | Class | Inherited rights / env / cwd / identity | Audit disposition |
|---|---|---|---|---|---|
| Bash tool (model shell) | `src/tool/bash.rs`, `src/tool/bash/process.rs` | `ManagedProcessService::run` via scheduler executors; `SandboxRequest` from immutable policy snapshot | Controlled untrusted process | Filtered env, workspace cwd, per-run scratch TEMP, daemon identity; Windows resolves shells from absolute system dirs, never PATH | Controlled. C042 negotiation point is the future Windows gate; no shell regex substitute. |
| Scheduler executors (bash/python/test/task) | `src/scheduler/executors.rs` | `ManagedProcessService::run` with job/attempt provenance, cancellation, output bounds | Controlled untrusted process | Job-owned provenance env (`CODEGG_JOB_ID`/`ATTEMPT_ID`), scheduler permit, workspace lease root | Controlled. Provenance + permit + immutable snapshot enforced. |
| Python script tool/executor | `src/python_script/tool.rs`, `src/python_script/executor.rs`, `src/python_script/analyze.rs` | `ManagedProcessService::run` / `run_blocking` | Controlled untrusted process | Filtered env, workspace cwd, bounded output | Controlled. |
| Plugin process runtime | `src/plugin/runtime/process.rs` | `ManagedProcessService::run` | Controlled untrusted process | Plugin-scoped env, workspace cwd | Controlled. |
| Hooks | `src/hooks/mod.rs` | `ManagedProcessService::run` | Controlled untrusted process | Hook-scoped env | Controlled. |
| Formatter / terminal tools | `src/tool/formatter.rs`, `src/tool/terminal.rs` | `ManagedProcessService::run(_blocking)` | Controlled untrusted process | Tool-scoped env/cwd | Controlled. |
| Human shell runtime | `src/shell/runtime.rs`, `src/shell/rtk.rs`, `src/core/daemon_human_shell.rs` | `ManagedProcessService::run_streaming` / `run_blocking` after explicit promotion + daemon ack | Controlled untrusted process (explicitly promoted) | Daemon-host shell, canonical workspace cwd, filtered env, bounded staging; `!`/`!!`/`/shell-include` promotion model unchanged | Controlled. Human promotion stays out of model context per AGENTS.md. |
| IDE helper | `src/ide/mod.rs` | `ManagedProcessService::run_blocking` | Controlled untrusted process | IDE-scoped cwd/env | Controlled. |
| Test runner | `src/test_runner/runner.rs:372` (`Command::new(&resolved.argv[0])`) | Direct `std::process::Command` (Unix `CommandExt` pre-exec) | Controlled untrusted process — **exception recorded** | Test-resolved argv, workspace cwd | Explicitly uncovered for sandbox coverage: test execution is scheduler-owned heavy work (`JobKind::Test`) but spawns through its own runner, not `ManagedProcessService`. Any future advertised Windows sandbox coverage must either route it through the canonical launcher or explicitly exclude test execution with a testable denial. No change made in this slice. |
| Interactive processes (PTY) | `src/interactive_process.rs:1261` (`tokio::process::Command` + openpty/setsid) | Direct PTY spawn, scheduler permit (`ManagedProcess` resource class) | Interactive (user-controlled PTY), not model-untrusted batch | Master-fd scrollback, user input/resize/terminate, process-group cleanup | Explicitly excluded from advertised sandbox coverage by design (long-lived user session, not a containment boundary). No change. |
| MCP local servers | `src/mcp/local.rs:99` (`Command::new(&self.command)`) | Direct spawn | Controlled untrusted process — **exception recorded** | MCP-configured command/env/cwd | Explicitly uncovered: MCP server processes are untrusted native executables that bypass `ManagedProcessService` today. Must be routed or explicitly denied before any Windows sandbox coverage claim. No change in this slice. |
| LSP servers | `src/tool/lsp.rs`, `src/lsp/*` (server spawns via LSP client infra) | LSP client-managed subprocesses | Controlled untrusted process — **exception recorded** | Toolchain-resolved server binaries, workspace cwd | Explicitly uncovered: LSP server subprocesses (and any LSP-spawned helpers) are not yet under the single launcher contract. Negative subprocess-indirection tests deferred to post-admission. |
| Git operations | `src/git_service.rs`, `src/git_mutations.rs`, `src/git_network_policy.rs` (`egggit` read-only facts; mutations via guarded ops) | `egggit` facts + guarded mutation paths; test-only `StdCommand::new("git")` fixtures in `#[cfg(test)]` | Daemon-trusted native file operation (mutations) + test-only fixtures | Workspace-owned, mutation policy + network policy gates | Trusted mutations stay under existing authorization, never mislabeled sandboxed. Production git subprocess dispatch (where present) must be inventoried again at integration time. |
| Auth external / TTS / install / notifications | `src/auth/external.rs`, `src/tts/mod.rs`, `src/install.rs`, `src/core/notification.rs` | Direct spawns of configured helpers (TTS, openers, install probes) | Privileged setup / operator surface, not model-untrusted tool | Operator-configured binaries, user session env | Out of advertised sandbox coverage; `ExternalCommand` auth path is unsupported per AGENTS.md. No change. |
| Remote execution adapters | Eggwork fixed-target paths (`src/scheduler/*`, eggwork nodes) | Remote-node dispatch, never local spawn | Independent remote node | Remote identity/provenance, exact-source sealing | Out of local sandbox coverage by construction; refused locally per human-execution rules. |

## Package B disposition (single launcher contract)

`ManagedProcessService` in `src/managed_process.rs` remains the single
production tool-call boundary for model-untrusted batch execution
(bash, python, plugins, hooks, formatters, human-shell finite
children, IDE helpers). Scheduler executors own provenance, immutable
policy snapshots, audit/status receipts, cancellation, and process-tree
lifecycle. No shell regex or tool-name allowlist is a substitute; none
was added.

Three surfaces are explicitly NOT under that contract today and are
therefore excluded from any future advertised Windows sandbox coverage
until routed or denied with tests: the test runner's direct spawn, MCP
local-server spawn, and LSP server subprocesses. Interactive PTY
sessions and operator setup helpers are excluded by design. Trusted
in-process file mutations (daemon file tools, git projectors) remain
approval-controlled and are never labeled sandboxed.

## Package C/D (deferred, not claimed)

No backend is wired: `negotiate()` + private launch-status verification
exist as the gate, but no caller passes a Windows backend, no child
starts under a new policy, no `FullHost` fallback was added, and no
agent-controlled privileged helper path exists. Negative
subprocess-indirection tests (shell→python→git, MCP→process,
LSP→subprocess, plugin→process, symlinked cwd, cancelled jobs,
malicious quoting, env-provided tooling) against a real backend await
admission and a disposable Windows host.

## Uncovered items (explicit)

1. Test runner, MCP local servers, LSP subprocesses bypass the
   canonical launcher (table above). Production integration must
   close or explicitly deny each with tests.
2. No Windows native execution ran on this host (darwin); all
   Windows claims remain `not-run`, never `pass`.
3. No schema migration was made; none is claimed needed.
4. Linux/macOS containment, remote-node ownership, human-shell
   promotion, and all approval gates are preserved unchanged.
