# Async Background Jobs M002 — Background Bash, Build, Lint and Format

Status: blocked — M001 must close
Baseline: `e6a5e85de632ea5057d59aee585942d10f9d950b` (revalidate before implementation)
Roadmap: `plans/subsystems/async-background-jobs-and-ci-watch-roadmap.md#m002--background-bash-and-routed-buildlintformat`
Class: capability. Hard dependency: M001 notification/JobHandle/control contract. Soft dependency: M003 safe idle continuation (does not gate execution).

## Objective and non-goals

Add explicit `execution_mode: foreground|background` to the **existing** model-facing `bash` tool and reuse its native/routed Test/Build/Lint/Format/ManagedProcess/Shell scheduling for long-running work. A `cargo build` or `cargo test` started in background must return a `JobHandle` promptly and notify when complete. Keep all shell policy, sandbox, approval, actual-executor audit and RunStore provenance. Do not introduce detached `&` processes, a second process service, shell-regex security rules, speculative command classification, automatic backgrounding, or network access escalation.

## Live code to inspect

`src/tool/bash.rs` schema and policy/execute sequence; `src/tool/bash/{policy,process,output}.rs` command intent preparation and routed shell/test/build/lint/format/managed dispatch; `src/command_intent/{plan,pipeline,shell_shape}.rs`; `src/managed_process.rs`; `src/scheduler/{submission,scheduler,executors}.rs`; `src/tool/test.rs` M001 adapter; `src/agent/tool_batch.rs` invocation identity and ToolBroker; `docs/execution-ownership.toml`, `architecture/command_intent.md`, `architecture/process-tool-execution-ownership.md`.

Important observed behavior: `bash` schema currently has command/workdir/timeout only; command dispatch awaits `wait_for_completion` for scheduler routes, with direct raw-shell compatibility for other routes. Routed Bash Test `NewJob` currently sets `session_id: None` and `turn_id: None`; background mode **must** supply trusted owner lineage, not a spoofable model field. Shell native/direct paths have distinct sandbox semantics. The default shell is `sh -c` on Unix, configurable Windows shells on Windows.

## Non-regression contract

- Foreground/default output, error shape, call audit/provenance, timeout precedence, approved command routing, subprocess cleanup and sandbox behavior remain equivalent.
- Background request is never executed by returning a detached PID or delegating to unsupervised host shell. If a planned command cannot run safely under scheduler-managed execution with its effective policy, fail before execution and explain why; do not silently foreground, fall back to direct raw-shell, change command grammar, or downgrade sandbox.
- Safety decisions occur before submission and are frozen to the admitted operation; canonical host policy is authoritative. Both argv-native and explicit shell commands use the existing `ManagedProcessService` and correct platform launcher, preserving Windows raw-command semantics.
- The background process retains job kind/resource profile, immutable cwd/workspace identity, credential references, tool permission grant, execution timeout and command audit. A timeout on status/wait does not terminate it; actual execution timeout does.
- M009 call identity deduplicates retry of the same accepted invocation but does **not** collapse two independent `cargo build` invocations sharing argv. Never generate submission identity from only command text or source hash.
- Treat output and RunStore artifacts as bounded references. Job terminal summary must distinguish exit code/signal, failure, cancellation, sandbox refusal, process timeout and recoverable interruption; do not fabricate exit code zero/one when the durable executor has richer status.
- Cancellation terminates descendant process trees and frees scheduler permits. Parent cancellation policy must be explicit and consistent with detached job ownership; no global kill by executable name. Platform-specific `Required` sandbox remains fail-closed where unsupported.

## Ordered implementation packages

A. **Characterize routes with fixtures.** Build table of `bash` command-intent families and live dispatch owners, including compound shell syntax, `cargo build`, `cargo test`, `cargo fmt`, `cargo clippy`, python, Git, direct raw-shell, Windows PowerShell/Cmd/Git Bash/WSL. Prove exact foreground behavior before introducing background fields. Confirm hosted approval path and authoritative sandbox profile, not regex denial.

B. **Factor submission from wait.** For scheduler-backed Bash routes separate plan/authorize/submit from wait/result formatting. Return the same canonical `JobHandle` and subscribe to M001 notifications before reporting acceptance, including session/turn/job/run provenance. Reuse M001 status/inspect/cancel tool; no new private task queue. Native argv and shell execution must be supervised by scheduler+ManagedProcessService using the already accepted routing plan; preserving command meaning is an acceptance gate.

C. **Constrain direct compatibility.** Explicitly enumerate direct/shell-only routes. Where a safe scheduler-backed `JobKind::Shell` translation already exists, adopt that same executor/approval/sandbox route. Otherwise reject `execution_mode=background` without spawning. No time-of-check/time-of-use reparse that could broaden command scope; no fallback after a failed submit or rejected sandbox. Include nested shells and shell metacharacters in negative fixtures.

D. **Receipt and return shape.** Extend model `bash` parameters only with the mode and, if required, a bounded future continuation policy reserved for M003. Foreground remains default; background returns handle/status and a concise message telling the agent that polling is unnecessary. The tool result must not claim process success until terminal status is authoritative. Job wait/inspect returns head/tail bounded logs or artifact handle, not unbounded capture. Ensure timing of tool-call completion and audit represents *submission* rather than process completion.

E. **Cross-tool consistency / docs.** Ensure `test` direct and `bash` translated tests produce consistent canonical kinds and notices without double notification. Keep `ToolProgram` and `AgentRun` background domains distinct. Document execution-mode usage in agent prompt/tool discovery only as a small contextual hint; no broad mandatory extra prompt. Update architecture, protocol if needed, config, closure, registry.

## Focused verification / acceptance

1. Foreground Bash and Test tool contracts unchanged; default command timeout and optional explicit longer timeout still work.
2. Fake slow Cargo build/test launched in background yields one durable handle promptly, permits parent to do a second tool operation, then produces one actionable terminal notice. No model-authored sleep/poll script used.
3. Accepted `bash` with pipes/compound syntax retains exact actual shell semantics, or explicitly fails before spawn; no fallback/double execution.
4. Security regression: approval Ask, sandbox-required unavailable, unauthorized workdir, shell policy denial, changing session/turn, Windows constrained mode, command timeout, cancel/restart and process grandchildren all preserve enforcement and provenance.
5. Separate identical commands receive distinct IDs, repeated same accepted model invocation gets the same ID, and no status/wait timeout is recorded as job failure.
6. Cross-platform compile and platform-relevant fixtures. Record Windows/macOS unavailable hosts honestly; no pretend sandbox guarantee.

Run existing `bash`/process/test/command-intent focused suites, `tests/managed_process_descendants.rs`, `tests/scheduler_authority_matrix.rs`, M001 tests, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `scripts/verify.sh quick`, `python3 scripts/check_scheduler_bypass.py`, `python3 scripts/check_execution_ownership.py`, `python3 scripts/check_git_forbidden_patterns.py` if Git routing changes.

Stop and write corrective if it cannot be made scheduler-owned without a policy downgrade or process semantic drift. Closure: `plans/closure/async-background-jobs/002-status.md`.
