# Workspace Authority and Session Navigation W004 — User-Directed Main-Agent Relative CWD

Status: **closed** — closure `plans/closure/workspace-authority-and-session-navigation/004-status.md`; ADR-0055 Option A accepted 2026-10-27
Repository baseline: `80171f45295bebb9ab0b521262c0b295acb93074`
Source roadmap: `plans/subsystems/workspace-authority-and-session-navigation-corrective-addendum.md` W004 (addendum)
Long-term: `plans/000-long-term-specification.md` §§6, 9, 17, 26–29; `plans/001-terminology-and-domain-model.md`
Applicable: accepted ADR-0033 and ADR-0055 Option A. Primary class: **capability**.

## 1. Objective
Give the main agent a host-authorized, user-observable way to request **persistent default working-directory changes inside its session's existing workspace** without changing daemon cwd, identity or security ceiling. Crossing to a different repository or managed checkout uses the existing authorized session launch/fork/switch path and must not silently rewrite an active session.

## 2. Why ready
W001 and W002 are strictly closed, and the user accepted ADR-0055 Option A. The implementation keeps the session workspace root as the security ceiling and adds only a session-relative default.

## 3. Current evidence
`src/tool/bash.rs` accepts per-call `workdir` with session-root default, and command `cd` lives only in its subprocess. `src/tui/app/state/execution_context.rs` snapshots a tab's workspace root at dispatch. `SessionLaunchService` supports a new/fork session in a new/existing **managed** checkout. No model-facing durable `get|set|reset` session-relative cwd or canonical cross-project workspace-transfer request is present.

## 4. Invariants
Workspace root/project/session/worktree binding is unchanged when setting relative cwd; validated canonical path remains within the original workspace ceiling; child run inherits an immutable snapshot and cannot change parent's directory; in-flight work unaffected; no daemon `chdir` or environment-global setting.

## 5. Scope
In after ADR acceptance: typed session relative-cwd `get|set|reset`, versioned storage, authorization/audit, model-facing tool/protocol, tool default resolution, UI/status and recovery. Out: arbitrary host path, adopting unmanaged worktree by path, unmediated shared writers, in-place transfer of existing session to another repo, hidden automatic promotion of permissions.

## 6. Production changes
`codegg-core`: validated versioned state keyed to session, authoritative root bound to existing workspace. Daemon: typed request and CAS update with actual authorization; tool registry: explicit effective-cwd snapshot for relative paths and per-command workdir defaults while root remains authorization/sandbox ceiling. Frontend: show root and working subdirectory distinctly, observe acceptance/failure; external checkout change via `codegg launch` or TUI fork/new action. No schema/policy implementation before ADR.

## 7. Work packages
A. Confirm accepted ADR and W002; write a failing multi-turn `set→read→bash→reset` baseline. B. Define typed revisioned state and canonicalize path against immutable workspace root with symlink escape defense. C. Route effective-cwd default through canonical prompt, idle continuation, file/Git/Bash/test/LSP-adjacent tools **without** changing their separate security ceiling; enumerate sinks rather than editing just Bash. D. Wire explicit user request/approval and audit, TUI + CLI/protocol help; cross-root handoff must create/select another authorized session. E. Fault/restart, cross-TUI and subagent no-retarget evidence.

## 8. Failure/restart/contention
CAS stale revision refuses; missing directory/symlink change refuses rather than guessing; two concurrent turns snapshot their default before execution; switch during active Bash retains old default for that process. Crash persists accepted choice only and reconnect uses same authority.

## 9. Compatibility
Additive optional state/default root, backwards-compatible DTOs; existing sessions initially have no relative-cwd override. The old `--fork` semantics remain unchanged; session launch remains idempotent.

## 10. Tests
Two same-session turns, tool family parity, relative+absolute and traversal+symlink tests, audit/principal denial, concurrent request and stale revision, session switch A→B→A, restart, subagent snapshot, existing unmanaged checkout rejection, no daemon cwd, platform failures.

## 11. Verification commands
```bash
python3 scripts/check_daemon_cwd_usage.py --self-test
python3 scripts/check_daemon_cwd_usage.py
cargo test -p codegg-core --lib
cargo test -p codegg --lib tui::
cargo test --test agent_loop -- workspace_root
scripts/verify.sh quick
cargo clippy --workspace --all-targets -- -D warnings
```
Resolve actual family test names; do not count zero-selection invocations as evidence.

## 12. Docs
`docs/tools.md`, `docs/tui.md`, `docs/cli.md`, `architecture/workspace.md`, `architecture/session.md` — distinguish per-call workdir, persistent relative cwd, and cross-root launch.

## 13. Acceptance
User says "work under crates/foo until told otherwise"; main agent's subsequent tool requests use that subdirectory, survive reconnect, and report it visibly; root security remains unchanged. User asks to work in another repo/checkout: an authorized new/fork/switch flow occurs or explicit refusal, never a silent session rebind.

## 14. Stop
Unaccepted ADR, failed W002, unqualified dynamic cwd sinks, sandbox expansion, global cwd mutation, or introduction of model-owned arbitrary worktree lease is a stop condition.

## 15. Closure evidence
Decision acceptance link, baseline fail + final green end-to-end, restart/concurrency/scope tests, CI/host distinctions, generated-guard coverage, source head and actual observed default dirs.

## 16. Handoff
Blocked for ownership decision, not a green light to implement. The plan names a narrow successor to W002 rather than requiring all of W003's optional dashboard UI.
