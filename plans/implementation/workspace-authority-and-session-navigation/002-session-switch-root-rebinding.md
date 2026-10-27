# Workspace Authority and Session Navigation W002 — Session-Switch Root Rebinding

Status: **closed** — closure `plans/closure/workspace-authority-and-session-navigation/002-status.md`
Repository baseline: `80171f45295bebb9ab0b521262c0b295acb93074`
Source roadmap: `plans/subsystems/workspace-authority-and-session-navigation-corrective-addendum.md` W002
Long-term: `plans/000-long-term-specification.md` §§6, 17, 26–29; `plans/001-terminology-and-domain-model.md`
Accepted ADR: ADR-0033 Option A. Primary class: **invariant**.

## 1. Objective
Make selecting an existing session resolve that session's canonical workspace/project binding and atomically update the active client-tab execution context, tool default root and displayed directory, rather than inheriting the previous tab's root. Retain project-scoped sessions UI unless there is a separately authorized cross-project listing.

## 2. Why ready
W001 strictly closed: `plans/closure/workspace-authority-and-session-navigation/001-status.md`. No daemon `chdir` allowed, and a rootless tool now fails closed. ADR-0033 is accepted; no new ADR is required for session-switch conformance.

## 3. Current implementation evidence
`src/tui/app/state/execution_context.rs` captures `workspace_root` from the tab at dispatch; its no-leak test proves later tab switches cannot retarget an already-dispatched action. `src/tui/commands/sessions.rs` and `src/tui/app/project_session.rs` select sessions under a tab; the source roadmap records `App::set_session` adopting the session root only if the tab root is `None`, hence a cross-directory session can keep a stale root. `SessionStore`/project bindings contain authoritative directory/workspace identity; M006 session-worktree launch adds `workspace_project_binding.worktree_id`. `/sessions` currently lists the active project's sessions only; no cross-project reachability claim.

## 4. Invariants
Captured turns/jobs/children do not retarget after a selection; daemon cwd never moves; canonical project/workspace and session binding must agree; neither a display path nor a session title grants ownership; dirty/unavailable managed worktrees and ambiguous historical bindings fail visibly.

## 5. Scope
In: session-selection root adoption and display convergence, reliable A→B→A restore, accurate sessions-list scope label, managed-worktree session launch reuse. Out: arbitrary in-place session project transfer, model-facing persistent cwd tool (W004/ADR-0055), cross-tenant listing, WorkOrder reclassification, new worktree allocator.

## 6. Production changes
Daemon/core: use the authorized session's canonical workspace/project binding on selection; do not trust a client path alone. Protocol: prefer existing SessionList/session project DTOs; add a bounded typed locator query only if no already-authorized response carries the root. TUI: update `App::set_session`, session commands and active-tab execution context with one atomic rebinding, invalidate stale async completions via view epoch, refresh cwd/project status line and tool defaults. Docs: `docs/tui.md`, `architecture/workspace.md`.

## 7. Ordered work packages
A. Capture failing same-tab A→B→A test where A and B are two authorized directories/worktrees and tab root initially non-null. B. Resolve selected session to immutable canonical binding and validate authorization/availability. C. Atomically update tab locator, selected project/workspace/session IDs and dependent caches; do not adopt the previous root. D. Make `/sessions` accurately label its scope; do not widen search before approved protocol. E. Add linked-worktree branch display and navigation regression.

## 8. Failure/cancellation/restart/contention
A late SessionList/selection response must not change the current tab after an intervening switch. A deleted/moved checkout surfaces a typed unavailable state and leaves prior valid context unchanged. Reattach/restart revalidates persisted selection against daemon authority; an in-flight agent turn uses its submitted snapshot.

## 9. Compatibility
No existing session ID or project-to-workspace invariant change; old TUI protocol data should remain decodable. If an additional field is needed, additively default it and test mixed-version response.

## 10. Tests
One same-root selection, A→B→A cross-root, same-repository distinct worktrees, distinct projects through authorized picker, stale generation/reordered replies, missing checkout, reconnect/restart, mid-flight tool request. Assert both user display and default relative read/bash resolution choose the selected session root; daemon cwd unchanged.

## 11. Verification commands
```bash
python3 scripts/check_daemon_cwd_usage.py --self-test
python3 scripts/check_daemon_cwd_usage.py
cargo test -p codegg --lib tui::
cargo test --test tui_render -- tui_project_tabs
cargo test --test core_daemon -- session_launch
scripts/verify.sh quick
cargo clippy --workspace --all-targets -- -D warnings
```
Check test names with `cargo test -- --list`; zero selected tests are not evidence. Run all commands in implementation; none are claimed run by this plan.

## 12. Documentation
Reconcile `docs/tui.md` and `architecture/workspace.md` with user-visible session-switch cwd, project-tab scope and no daemon `chdir`.

## 13. Acceptance
A→B→A restores canonical execution and display roots; no tools execute against A after B selection; old in-flight A job remains A; unbound session refuses safely; `/sessions` never implies cross-project completeness.

## 14. Stop conditions
Any scheme changing daemon process cwd, cross-project authorization by frontend path, silent session mutation/retarget, or use of a read-only selection to acquire a worktree lease must stop for owner decision.

## 15. Closure evidence
Failing baseline and passing corrected test, exact branch SHA, protocol/authorization proof, active-status/read/Bash cwd trace, restart and stale-event test, focused+quick+Clippy results, named host limits.

## 16. Handoff
Keep W001 closed; do not reopen ADR-0033 or couple W002 to blocked W004. Coordinate with session-worktree launch to avoid treating a managed checkout as an unrelated root-created project.
