# W002 — Session-Switch Root Rebinding — Closure Record

Plan: `plans/implementation/workspace-authority-and-session-navigation/002-session-switch-root-rebinding.md`
Roadmap: `plans/subsystems/workspace-authority-and-session-navigation-corrective-addendum.md`
Baseline: `80171f45295bebb9ab0b521262c0b295acb93074`
Implementation branch head: `5a479cea9924205c8828dcbeef72a40bf3391f63` (worktree changes remain uncommitted)
Implementation commit/PR: none; source and closure edits are uncommitted in the requested branch worktree.
Closed: 2026-10-27
Class: invariant
Recommendation: **closed**

## 1. Requirement → evidence matrix

| Requirement | Evidence | Status |
|---|---|---|
| A session selection resolves the daemon-owned canonical binding before rebinding | `SessionLoad` response is validated for session, project/workspace identity, binding state, and canonical absolute root; client compatibility directory is overwritten from that root | met |
| A→B→A restores the selected session's directory and project context | `tui_project_tabs` family, including root-rebinding regressions; 20 tests passed | met |
| Late or mismatched responses cannot retarget the active tab | activation token/tab identity and binding-mismatch regressions in the TUI suite | met |
| Existing in-flight work retains its captured root | `ExecutionContext` snapshot coverage plus the existing no-retarget tests | met |
| Session routes converge on authorized activation | attach, continue, fork, new/create/import, task, and work-order routes use the activation path; initial CLI sessions resolve via `SessionStore` then attach through Core | met |
| `/sessions` does not imply cross-project completeness | scope is stated in the TUI and session documentation; daemon enumeration remains project-scoped | met |
| Launch and daemon cwd behavior remains unchanged | no daemon `chdir`; `check_daemon_cwd_usage.py` self-test and normal check passed | met |

## 2. Production changes

Daemon refresh and session-load responses now derive the directory from the
authoritative workspace binding. The TUI validates that response before
committing session, project, workspace, display, and tool-default state as one
activation. An activation generation prevents a stale response from changing a
later selection. Initial standalone/in-process startup uses the same attach
path as subsequent session selections.

## 3. Verification

- `cargo nextest run -p codegg --lib --locked --profile ci -E 'test(tui::)'`: **2,795 passed, 0 failed**.
- `cargo nextest run --test tui_render --locked --profile ci -E 'test(tui_project_tabs)'`: **20 passed**.
- `cargo nextest run --test core_daemon --locked --profile ci -E 'test(launch_directory_bootstrap) | test(session_launch)'`: **18 passed**.
- Focused activation, prompt-create, and working-directory tests: **26 passed**.
- `python3 scripts/check_daemon_cwd_usage.py --self-test` and normal check passed.
- `scripts/verify.sh quick` passed, including all repository guards, 354 harness tests (1 skipped), and workspace all-target check.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` passed.
- `git diff --check` passed.

## 4. Scope and unblock audit

Compatibility: no session/workspace identity migration or protocol widening
was needed. Persisted and in-flight contexts keep their captured roots; the
existing project-scoped list remains the only `/sessions` scope.

No protocol expansion or cross-project `/sessions` listing was introduced.
W003 remains eligible only after its separately recorded product decisions on
workspace-row nesting and handling a pending decision in another session.
W004's hard W002 dependency is discharged; ADR-0055 Option A was accepted on
2026-10-27, and W004 is closed in its own record.
