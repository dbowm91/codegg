# W001 — Workspace-Root Execution Authority — Closure Record

Plan: `plans/implementation/workspace-authority-and-session-navigation/001-workspace-root-execution-authority.md`
Roadmap: `plans/subsystems/workspace-authority-and-session-navigation-corrective-addendum.md`
Baseline: `b77c8156b594494b7f59e0348d93a73ca2c1fd1c`
Closed: 2026-10-17
Class: invariant

## 1. Requirement → evidence matrix

| # | Requirement | Evidence | Status |
|---|---|---|---|
| R1 | A delegated subagent job resolves a real workspace root from daemon-owned job state | `src/scheduler/executors.rs` binds `ctx.workspace_root`; `tests/workspace_root_execution_authority.rs` | met |
| R2 | Absence of a resolved root is a typed, fail-closed outcome | `ToolError::MissingWorkspaceRoot`; `util::validate_path`/`validate_target_path` guard | met |
| R3 | No daemon-owned execution reads the process cwd | 28 sites removed; `check_daemon_cwd_usage.py` exits 0 with 0 findings | met |
| R4 | A registry built without a root cannot produce cwd-rooted tools | 14 `None` arms → `UnrootedTool` | met |
| R5 | Dead code is deleted, not migrated | `build_session_tool_registry_legacy` removed (46 lines) | met |
| R6 | The guard actually detects violations | `--self-test` 8/8 **and** live injection test (exit 1) | met |
| R7 | Typed error, not an overloaded `Execution` | distinct variant; `broker` class `missing_workspace_root`; `progress_recovery` arm | met |

## 2. The premise was corrected during execution

The plan's §18.3 claimed the reported symptom's live path was
`SubAgentRequest.workspace_root: None` reaching the registry's `None` arm and
silently executing tools in the daemon's cwd. Executing W2 falsified both
halves. Recorded in plan §19; the substance:

1. **`src/job_dispatcher.rs` is entirely unreferenced.** `JobDispatcher`,
   `SubAgentJobDispatcher`, and `NullJobDispatcher` have zero references outside
   the file declaring them. `docs/execution-ownership.toml` already recorded the
   dispatcher as a legacy bridge superseded by `SubagentJobExecutor`. Only
   `src/scheduler/executors.rs:717` is live.

2. **`workspace_root: None` hard-fails; it does not silently retarget.**
   `src/agent/worker.rs:1494` refuses before `AgentLoop::new`:

   ```rust
   let workspace_root = request.workspace_root.clone()
       .ok_or_else(|| "subagent execution requires an explicit workspace root".to_string())?;
   ```

   The cwd-defaulted tools are constructed and then never invoked.

3. **The live defect is a liveness defect.** Empty `allowed_paths` is a real
   production state (`daemon.rs` chat-collaboration dispatch, the schedule
   materializer), so a legitimate delegated job was refused while the daemon
   held the correct root.

Consequence, stated plainly: **W5–W7 do not fix an observed breach.** They make
the invariant structural so the next caller cannot trip a landmine whose trigger
is a one-line `ToolRegistry::default()`. W3 is the fix for the real defect. This
record does not claim a cross-workspace authority breach was closed, because none
was demonstrated.

## 3. W2 — failing evidence (baseline `b77c8156`)

`tests/workspace_root_execution_authority.rs` drives the real
`SubagentJobExecutor` with a scripted child whose only tool call is a `read` of
a **relative** path, and asserts on the tool result the child received — not on
the child's final text, so it cannot pass for the wrong reason.

```
---- a_subagent_job_without_allowed_paths_resolves_the_jobs_workspace_root stdout
assertion `left == right` failed: child run failed before the read was attempted:
  Subagent task failed: subagent execution requires an explicit workspace root
  left: Failed
 right: Completed
```

After W3:

```
test a_subagent_job_without_allowed_paths_resolves_the_jobs_workspace_root ... ok
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

The test also asserts a premise guard: the marker file must **not** be visible
from the process cwd, or the two roots would be indistinguishable.

## 4. Production changes

| Area | Change |
|---|---|
| `src/scheduler/executors.rs` | Root resolves to `allowed_paths.first()` else `ctx.workspace_root`; empty root fails the attempt. The dead `let Some(repository_root) = … else` branch is gone. |
| `src/tool/mod.rs` | 14 `None` arms → `unrooted(inner)`. Added `ToolRegistry::register_boxed`, `impl Tool for Box<dyn Tool>`, `unrooted`, `MISSING_WORKSPACE_ROOT_CONTEXT`. `build_research_tool` takes a non-`Option` root. |
| `src/tool/disabled.rs` | New `UnrootedTool`: delegates name/description/schema/category/contract/causal contract; refuses `execute` and `execute_structured`; `expose_in_definitions() == false`. |
| `crates/codegg-core/src/error.rs` | `ToolError::MissingWorkspaceRoot { context }`. |
| `src/tool/util.rs` | `unresolved_workspace_root()`, `is_unresolved_workspace_root`, and the guard in both validators. |
| 18 tool constructors | Seed the sentinel instead of the process cwd. |
| `src/tool/bash/process.rs` ×6, `bash/output.rs`, `terminal.rs`, `formatter.rs`, `tool_program.rs` | cwd fallbacks removed; typed refusal. |
| `src/tool/bash.rs` | With no explicit `workdir`, default to the session root (set after `CommandIntentContext` so classification is unchanged). |
| `src/tool/skill.rs` | Carries an explicit `workspace_root`; refuses without one. |
| `src/tool/research.rs` | `with_default_service` deleted; it is now a test-only local helper. |
| `src/tool/factory.rs` | `build_session_tool_registry_legacy` deleted (zero callers). |
| `src/tool/broker.rs`, `src/agent/progress_recovery.rs` | New error variant classified, not ignored. |
| `scripts/check_daemon_cwd_usage.py` | Rewritten (§6). |

### Why `UnrootedTool` wraps rather than re-describes

The first implementation was a parallel stub with its own name/description. That
silently broke `ToolProgramExecutor`: `validate_runtime` resolves manifests from
`self.catalog()`, and the stub's conservative default contract made every
workspace tool look non-program-callable — so `validate_accepts_valid_job`
failed. In production `injected` is `None`, so **every legitimate Tool Program
would have been rejected.** Caught by the test suite, not by inspection.

Contracts are a property of the tool *type*, not of a constructed instance, so
`UnrootedTool` now delegates them. Constructing the wrapped tool is free of
process state because W6 already replaced its cwd seed with the sentinel.

## 5. Test fallout, and the 3 pre-existing failures

W5/W6 broke 47 lib tests. All were relying on process cwd — the exact defect
being removed — and each was fixed by naming a root:

- `src/tool/bash.rs` — added `rooted_bash()`; 34 call sites repointed at
  `CARGO_MANIFEST_DIR` (the repo, because some tests run real `git`).
- `src/tool/lsp.rs` — 18 constructions plus the shared `live_security_tool()`
  helper repointed at the repo, since those tests pass repo-relative `file_path`s.
- `src/scheduler/tool_program_executor.rs`, `src/tool_advisor/causal_frontier.rs`
  — contract-snapshot registries now built `with_options { workspace_root: Some(..) }`.

**Pre-existing, not caused by W001.** Verified in a separate worktree at clean
HEAD `b77c8156`, with none of these changes present:

```
$ cd /tmp/w001-baseline && cargo test -p codegg --lib -- <3 tests>
test result: FAILED. 0 passed; 3 failed; 0 ignored; 0 measured; 6349 filtered out
    core::daemon::tests::session_create_through_daemon
    core::daemon::tests::turn_submit_rejects_when_active
    tui::commands::interactive_terminal::tests::pressing_i_on_the_open_box_enters_focused_input
```

`InvalidRootPath("/tmp")` is environment-dependent (macOS `/tmp` is a symlink);
the terminal one asserts focus state and belongs to the uncommitted TUI panel
work in this worktree, not to W001.

## 6. The guard (`scripts/check_daemon_cwd_usage.py`)

The predecessor passed with **all 28 violations present**, because its
line-pattern allowlist matched the production sites themselves
(`allowed_root: std::env::current_dir`, `let cwd = std::env::current_dir`, …).
A guard that passes while the defect is live is worse than none, because it gets
cited as evidence.

Rewritten: file-scoped exemptions only (`src/main.rs`, `src/exec.rs`, each with a
written justification), `#[cfg(test)]` module skipping, and a much wider scan
surface (`src/tool/`, `src/scheduler/`, `src/core/`, `src/agent/worker.rs`,
`src/agent/turn_runtime.rs`, Git services) that had previously omitted
`terminal.rs`, `formatter.rs`, `tool_program.rs`, `lsp_read.rs`, `git_read.rs`,
and `tool/mod.rs`.

**Self-test** — the same detector, synthetic sources:

```
$ python3 scripts/check_daemon_cwd_usage.py --self-test
  ok   a production line that defaults a tool root to the process cwd is caught
  ok   a bare current_dir() call is caught too
  ok   set_current_dir (a daemon-wide chdir) is caught
  ok   the same call inside a #[cfg(test)] module is not a violation
  ok   a comment describing the old behaviour is not a violation
  ok   the declared bootstrap file is exempt
  ok   a file not on the exemption list is still checked even next to one
  ok   a normal rooted call is not a violation
self-test passed: 8 case(s).
```

This self-test immediately caught a real bug in the new guard: it passed
`"placeholder"` as the path, so file-scoped exemptions could never match.

**Live injection** — the required negative case, against the real tree:

```
INJECTED a production cwd read into src/tool/read.rs
$ python3 scripts/check_daemon_cwd_usage.py
  src/tool/read.rs:148: allowed_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
1 violation(s) found.
exit: 1
--- reverting injection ---
$ python3 scripts/check_daemon_cwd_usage.py
cwd usage check passed — no process-cwd reads in daemon-owned execution
exit after revert: 0
```

## 7. Decisions

1. **Delete `build_session_tool_registry_legacy`; do not migrate it.** Zero
   callers, and its whole purpose was a cwd-rooted fallback this milestone
   retires.
2. **Sentinel root, not `Option` plumbing.** `Option<PathBuf>` through every
   tool's execute path would have been a much larger diff. One sentinel plus the
   two shared validators makes the whole class fail closed.
3. **Fail closed at validation, not at construction.** `Default` cannot return
   an error, so the typed refusal happens where the tool is used. The registry
   additionally refuses *before* a tool exists, via `UnrootedTool`.
4. **Keep `allowed_paths.first()` precedence.** An explicit caller-stated root
   still wins; only the empty case changed meaning. `allowed_paths` re-authorizing
   is pre-existing and out of scope (§8).
5. **`BashTool`'s two roots stay independent.** `workspace_root` defaults the
   workdir; `child_workspace_ceiling` remains the child-Git-only lexical limit.
6. **Left `SubAgentJobDispatcher` in place.** It is dead, but deleting it means
   editing `docs/execution-ownership.toml` and `check_scheduler_bypass.py`, which
   is separate cleanup. Its `None` producer was not reworked because nothing
   reaches it.

## 8. Not covered

- **`allowed_paths` as authority.** The executor still trusts
  `allowed_paths.first()` to name the child's root, so a payload can point a
  child anywhere. Pre-existing; W001 is about the *fallback*, not re-authorizing
  the input.
- **`ToolProgramExecutor::catalog()`.** Still builds an unrooted registry for
  `validate_runtime`'s manifest resolution. It cannot execute (production
  dispatch goes through `job_tool_context(&ctx.workspace_root)`), and contracts
  are now delegated so the answer is correct — but "a registry that exists only
  for introspection" remains a shape worth revisiting.
- **Dead `SubAgentJobDispatcher`** (§7.6).
- **`skill.rs` asset discovery.** Now root-bound, but a snapshot-less skill
  lookup in a daemon session still resolves global roots only.
- The 3 pre-existing failures in §5.

## 9. Verification

| Check | Result |
|---|---|
| `tests/workspace_root_execution_authority.rs` | **6 passed** (failed for the diagnosed reason at baseline, §3) |
| `cargo test -p codegg --lib` | **6379 passed; 3 failed; 4 ignored** — all 3 failures pre-existing, reproduced at clean HEAD (§5) |
| `agent_loop_harness` (C001, regression-checked) | **58 passed** |
| `subagent` | **22 passed** |
| `tool_program_runtime` | **7 passed** |
| `tool_program_m014_artifacts` | **13 passed** |
| `cargo fmt --check` | clean |
| `cargo clippy --all-targets -- -D warnings` | clean |
| `scripts/verify.sh quick` | **"Quick verification passed."** (exit 0) |
| `python3 scripts/check_daemon_cwd_usage.py` | 0 violations |
| `python3 scripts/check_daemon_cwd_usage.py --self-test` | 8/8 |
| guard on injected violation | exit 1, then exit 0 after revert |

One integration test also needed a root: `agent_loop_harness.rs`'s
`shell_escalation_ask_waits_for_explicit_approval_before_dispatch` registered a
`BashTool` with no workspace root and asserted the command dispatched after
approval. Under the new contract that tool correctly refuses, so the test now
calls `.with_workspace_root(...)`. Worth recording: it is the one place where a
*previously passing* test encoded the old behaviour, and the failure was the
contract working, not a regression.

Failure accounting across the milestone: **47** lib tests broke when the cwd
fallbacks were removed; **44** were fixed by naming a root; **3** were proven
pre-existing by running them in a clean worktree at `b77c8156`.

No hosted CI run. No native cross-platform qualification. No merge-readiness
claim beyond the local evidence above.