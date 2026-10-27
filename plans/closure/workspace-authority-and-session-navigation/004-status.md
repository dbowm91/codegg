# W004 — User-Directed Main-Agent Relative CWD — Closure Record

Plan: `plans/implementation/workspace-authority-and-session-navigation/004-user-directed-main-agent-relative-cwd.md`
Roadmap: `plans/subsystems/workspace-authority-and-session-navigation-corrective-addendum.md`
Baseline: `80171f45295bebb9ab0b521262c0b295acb93074`
Implementation branch head: `5a479cea9924205c8828dcbeef72a40bf3391f63` (worktree changes remain uncommitted)
Implementation commit/PR: none; source and closure edits are uncommitted in the requested branch worktree.
Closed: 2026-10-27
Class: capability
Recommendation: **closed**
Decision: ADR-0055 Option A accepted by the user on 2026-10-27

## 1. Requirement → evidence matrix

| Requirement | Evidence | Status |
|---|---|---|
| Persistent default is session-scoped, revisioned, and additive | Migration v91 and `SessionWorkingDirectoryStore`; persistence, default, session separation, reset, and compare-and-set tests | met |
| Model-facing set/reset is bounded to an existing directory under the immutable workspace ceiling | `SessionWorkingDirectoryTool` rejects absolute paths, traversal, symlink escape, missing paths, and stale revisions | met |
| Relative tool defaults use the captured effective cwd without replacing the security root | tool registry options and `ExecutionContext` carry both values separately; broker normalization coverage spans Bash, Test, Python, Git, file, list, and LSP tools | met |
| Explicit per-tool paths remain authoritative | normalization tests preserve absolute and caller-supplied paths | met |
| In-flight turns and children cannot be retargeted | effective directory is captured at turn construction; delegated payload carries a relative snapshot and child tools cannot register the parent mutation tool | met |
| User can observe the selected session/root scope | TUI scope and root/current-directory documentation updated | met |
| Cross-root work uses authorized session launch/fork/switch | documentation and tool guidance route cross-root changes through existing authorized launch flows; no runtime root rebinding was added | met |

## 2. Production changes

The new session store keeps a relative directory and revision per session. The
model-facing `session_working_directory` tool performs `get`, compare-and-set
`set`, and revisioned `reset` through the ordinary mutating-tool authority and
audit path. Turn construction snapshots the selected directory, while the
workspace root remains the sandbox and authorization ceiling. Main-agent tool
defaults and delegated-child snapshots use the effective directory without
changing repository identity or the session binding.

## 3. Verification

- Focused root unit tests for session store/tool/default propagation: **4 passed**.
- Focused `agent_loop` workspace-root/delegation coverage: **15 passed**.
- Core session store tests: **2 passed**; repository-context resolution test: **1 passed**.
- `check_daemon_cwd_usage.py --self-test` and normal check passed.
- `scripts/verify.sh quick` passed, including all repository guards, 354 harness tests (1 skipped), and workspace all-target check.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` passed.
- `git diff --check` passed.

## 4. Compatibility and unblock audit

Existing sessions default to `.` and no session binding, repository identity,
or sandbox ceiling is rewritten. Storage is additive. ADR-0055's explicit
session launch/fork/switch path governs cross-root changes. W004 is closed;
W003 remains a separate, decision-gated capability.
