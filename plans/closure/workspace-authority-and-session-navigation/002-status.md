# Workspace Authority and Session Navigation W002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/workspace-authority-and-session-navigation/002-session-switch-root-rebinding.md`

Source subsystem roadmap:

- `plans/subsystems/workspace-authority-and-session-navigation-corrective-addendum.md#milestone-w002--session-switch-working-directory-rebinding`

Repository baseline reviewed: `fed10ce13be7fd99a6d5aa5f8b6639ba98d007bc` (PR #207 merge commit; equal to the fetched `origin/main` at review)

Implementation commits or pull requests:

- PR #207, merged at `fed10ce13be7fd99a6d5aa5f8b6639ba98d007bc` — session activation binds all tab workspace authority from the daemon's canonical `SessionLoad` response.

Closed: 2026-10-27

Primary class: invariant / capability

## 1. Executive finding

W002 is strictly closed on merged main. Activating a different session now resolves and validates its daemon-owned canonical workspace binding before atomically changing the selected tab's session, project, workspace, display directory, and tool root. A stale activation cannot retarget the tab; already-created execution contexts retain their captured root. The behavior is exercised by TUI and daemon tests and the root-authority guard remains active.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Resolve canonical binding before rebinding | `load_session_activation` obtains `SessionLoad`; activation validates session, project/workspace identity, binding state, and canonical absolute root before commit | pass | Compatibility `Session.directory` cannot override daemon binding |
| A→B→A restores each selected root and project | `session_activation_rebinds_a_b_a_to_authoritative_worktree_roots`; full `tui::` library filter | pass | Includes same-project distinct worktree selection in the focused TUI coverage |
| Stale or mismatched responses cannot retarget the active tab | `stale_or_unbound_session_activation_does_not_retarget_tab`, `session_activation_changes_projects_only_from_a_daemon_binding`; activation generation/tab identity checks | pass | Late response is refused |
| In-flight work keeps the root captured at construction | `ExecutionContext` snapshot behavior plus no-retarget activation coverage | pass | Switching the visible tab does not mutate a running request's root |
| Attach, continue, fork, create/import, task and work-order activation use authorized resolution | `load_session_activation` call sites in session commands and app startup/restore paths | pass | Initial startup uses the same Core activation request |
| Missing checkout is refused | `session_load_refuses_a_missing_workspace_root` | pass | No process-cwd fallback |
| `/sessions` remains project-scoped | Session list request and architecture/TUI documentation | pass | This plan does not add cross-project enumeration |
| Daemon cwd remains unchanged | `check_daemon_cwd_usage.py` normal and self-test modes | pass | No daemon `chdir` introduced |

## 3. Production implementation evidence

Daemon session-load and refresh responses derive the directory from the
authoritative workspace binding. The TUI activation transaction validates the
returned binding and then updates session, project, workspace, display root,
and tool-default state together. An activation token prevents reordered
responses from applying to a later selection. Initial attach/continue/new/fork
flows resolve through the Core client as well, including in-process mode.

No protocol expansion, project identity migration, or cross-project session
listing was introduced. Existing in-flight execution snapshots are immutable.

## 4. Verification executed

### Commands run

```bash
CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER=/tmp/codegg-linker-sdk/cc MACOSX_DEPLOYMENT_TARGET=26.0 LIBZ_SYS_STATIC=1 LZMA_API_STATIC=1 CARGO_BUILD_JOBS=2 rtk cargo nextest run -p codegg --lib --locked --profile ci -E 'test(tui::)'
CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER=/tmp/codegg-linker-sdk/cc MACOSX_DEPLOYMENT_TARGET=26.0 LIBZ_SYS_STATIC=1 LZMA_API_STATIC=1 CARGO_BUILD_JOBS=2 rtk cargo nextest run -p codegg --test tui_render --test core_daemon --locked --profile ci -E 'test(tui_project_tabs) | test(launch_directory_bootstrap) | test(session_load_returns_canonical_workspace_binding_for_activation) | test(session_load_refuses_a_missing_workspace_root) | test(session_launch)'
rtk python3 scripts/check_daemon_cwd_usage.py --self-test
rtk python3 scripts/check_daemon_cwd_usage.py
rtk proxy git diff --check
```

### Results

- TUI library filter: **2,818 passed, 4,964 skipped, 0 failed**.
- Focused TUI and daemon integration members: **38 passed, 1,587 skipped across 2 binaries, 0 failed**.
- CWD guard self-test: **11 cases passed**; normal guard passed.
- Diff whitespace check passed.
- The initial macOS build attempt without the recorded linker/library environment failed at link time on host SDK symbols; rerunning with the command environment above completed the tests. This is a host setup issue, not a product test failure.
- PR #207 hosted `verify` was **not fully green**: it ran 9,666 tests, 9,665 passed, and one unrelated existing LSP marker-limit assertion failed at `tests/ide_protocols/lsp.inc:2159`; the job stopped before 7,054 tests could run. Hosted Desktop E2E, Linux M004 process/IPC and restricted-userns qualification, macOS shared-checkout qualification, and Windows process-launcher/native-probe checks passed. The hosted failure is retained as a mainline CI finding and is not represented as W002 evidence.

## 5. Invariant review

- Session/project/workspace identity is committed only from the daemon binding.
- A missing or invalid root refuses activation; no process-global cwd fallback exists.
- A tab switch does not mutate the root captured by existing execution contexts.
- Stale activation generations cannot overwrite the current selection.
- Project-scoped session enumeration remains project-scoped.
- Daemon cwd is never changed to follow a session.

## 6. Failure and recovery review

An unavailable daemon, missing session, absent checkout, or invalid binding
does not activate an unbound session. Activation errors are surfaced or logged
according to the startup/interactive route. Reordered responses are rejected
by activation generation. Daemon restart and reconnect use the same
`SessionLoad` binding contract; no separate local-directory authority is
introduced. There is no new durable state or partial multi-row transaction to
recover.

## 7. Migration and compatibility review

No schema migration or persisted identity change was required. Existing
session payloads continue to decode; the daemon-provided binding is
authoritative over the compatibility directory field. No protocol negotiation
change was made. Rollback to pre-W002 behavior would restore the stale-root
defect, so the fix is intentionally part of the merged mainline contract.

## 8. Security review

The daemon remains the authority for project/workspace binding. The TUI does
not accept a caller-supplied path as a substitute for that binding. The
canonical root is validated before it is used, and missing roots fail closed.
No filesystem write authority, process authority, or daemon cwd mutation is
added.

## 9. Documentation and operations

The merged implementation updates the session/workspace authority documentation
and keeps `scripts/check_daemon_cwd_usage.py` as an automatic guard. Operators
should treat the daemon binding as the source of truth when diagnosing a
session that opens in the wrong checkout; reconnect/startup resolves through
the same binding path.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | PR #207 hosted `verify` had one unrelated LSP marker-limit test failure and aborted the remaining tests | Mainline hosted test evidence is incomplete for that run | Track/fix under the LSP test owner; rerun the hosted suite on a later exact head |
| low | Windows native TUI execution was not run locally | No local Windows-specific evidence from this workstation | Rely on applicable hosted lanes and retain platform limits in future changes |

No W002 correctness or authorization finding remains open.

## 11. Roadmap disposition

Milestone closed and the W004 hard dependency may proceed. W003 remains
separate capability work and still requires its product decisions on workspace
row nesting and pending-decision handling. ADR-0033's no-daemon-`chdir`
contract remains unchanged.

## 12. Registry updates

`plans/registry.md` already records W002 as closed and W004 as closed. This
record reconciles W002's evidence to merged main; no status transition is
required. The source roadmap's W002 status remains closed. W003 is not
unblocked by this record because its separately named product decisions remain
open.
