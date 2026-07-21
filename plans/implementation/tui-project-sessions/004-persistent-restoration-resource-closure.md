# Multi-Project TUI Milestone 004 — Persistent Restoration, Resource Bounds, and Closure

Status: blocked

Repository baseline: `f569386e4cb68d9752505c3b8d4205161a40c3c4` (`main`; planning-only commits after this baseline do not alter production behavior)

Activation criterion:

- `plans/implementation/tui-project-sessions/003-project-correct-event-routing-lifecycle.md` must be strictly closed.

Source roadmap:

- `plans/subsystems/tui-project-sessions-roadmap.md#milestone-4--persistent-restoration-and-closure`

Primary class: capability / polish / closure

## 1. Objective

Complete the multi-project TUI roadmap by making open-project/tab intent safely restorable across TUI process restart, proving bounded long-running behavior, removing obsolete single-project frontend authority, and producing complete closure evidence.

Restoration must remain a frontend convenience layer. The daemon catalog and canonical project/workspace/session bindings remain authoritative. Persisted TUI state may remember what the user intended to reopen; it may not assert that a project, workspace, or session still exists or remains bound the same way.

The milestone succeeds when a TUI can restart, reconnect to the single daemon, validate and restore a bounded ordered tab set, activate exactly one heavy view, recover gracefully from stale or missing identities, and run for long periods without unbounded tab/task/event/resource growth.

## 2. Dependency and current-state assumptions

This plan assumes Milestones 001–003 are closed and provide:

- stable frontend-local `ProjectTabId` within one process;
- ordered project tabs and visible navigation;
- bounded picker/session summaries;
- one active heavy view;
- project-correct async/live-event routing;
- explicit task/subscription/lease ownership;
- safe tab close/reconnect/session-rebind lifecycle.

At baseline `f569386`, TUI 002 is closed but TUI 003 is not yet implemented. The persisted format and restore coordinator must therefore be authored now but not implemented until TUI 003 fixes the ownership model it will restore into.

## 3. Invariants

- Persisted frontend state is never project/session authority.
- Restore validates every project/workspace/session identity through daemon APIs before opening a live tab.
- Paths, labels, cwd, and compatibility directories are not persisted as identity.
- Secrets, credentials, provider headers, prompt text, messages, tool outputs, file bodies, diffs, logs, terminal frames, and environment values are never persisted in the tab manifest.
- The persisted format is versioned, bounded, additive, and corruption tolerant.
- Restore does not eagerly activate every project, scan discovery roots, start workspace services, or load every session history.
- At most one heavy session view is loaded after startup.
- Archived, deleted, missing, rebound, unsupported, or unauthorized objects are skipped or represented as bounded unavailable placeholders; they are never recreated implicitly.
- A failed or partial restore cannot prevent the TUI from opening in a safe empty/compatibility state.
- Tab close updates persisted intent atomically but never mutates daemon-owned sessions/projects.
- Frontend-local tab IDs are regenerated or namespaced per process; persisted records use canonical daemon IDs plus stable order keys, not stale runtime object identity.
- File writes are atomic, permission-safe, size-bounded, and do not follow untrusted symlinks.
- Long-running resource use remains bounded by explicit tab, task, subscription, summary, and persisted-byte caps.

## 4. Scope

### In scope

- Versioned persistent tab-manifest schema.
- Atomic local persistence under the existing CodeGG application-state/config location.
- Debounced save scheduling and explicit flush on clean shutdown.
- Bounded ordered tab intent, active-tab intent, selected workspace/session IDs, and safe presentation preferences.
- Startup capability negotiation and daemon-authoritative validation.
- Lazy restore pipeline with one active heavy view.
- Missing/archived/rebound/unsupported recovery behavior.
- Persistence enable/disable/reset operator controls.
- Migration from no manifest and older additive manifest versions.
- Cleanup of obsolete single-project TUI authority and compatibility access paths where closure evidence permits.
- Resource caps and long-running stress tests.
- Crash/corruption/partial-write/security tests.
- Architecture, operations, troubleshooting, compatibility matrix, and closure documentation.

### Explicitly out of scope

- Persisting message history, prompts, tool output, terminal state, Git/LSP caches, dialogs, pending credentials, or provider secrets.
- Persisting daemon subscriptions or workspace-service leases.
- Restoring active turns by replaying local UI state; canonical daemon snapshot/replay is used.
- Cross-device synchronization of TUI preferences.
- Team presence, observer mode, chat, or authorization policy.
- Session Projection frontend adoption unless Session Projections Milestone 004 lands in the same coordinated release.
- Discovery scans or implicit registration during restore.
- More than one heavy active view.

## 5. Persistent schema

Define a bounded schema such as:

```text
TuiWorkspaceManifestV1
|-- schema_version
|-- written_at
|-- daemon_instance_hint: optional non-authoritative identifier
|-- ordered_tabs: <= MAX_PERSISTED_TABS
|   `-- PersistedProjectTab
|       |-- project_id
|       |-- workspace_id: optional
|       |-- session_id: optional
|       |-- label_hint: bounded display-only text
|       |-- selected_model_id: optional bounded display intent
|       |-- selected_agent: optional bounded display intent
|       `-- order_key
|-- active_project_id: optional
|-- active_session_id: optional
`-- preferences: bounded safe UI-only settings
```

Requirements:

- no `ProjectTabId` durability guarantee;
- no paths or locators as identity;
- all strings length-limited before serialization and after deserialization;
- total file-size cap;
- unknown additive fields ignored safely;
- unsupported future major versions produce an actionable reset/fallback diagnostic;
- duplicate project records are deterministically deduplicated;
- malformed IDs are rejected before daemon requests.

## 6. Persistence service

Create a focused frontend-local service with:

- `load_manifest()`;
- `validate_manifest()`;
- `schedule_save(snapshot)`;
- `flush()`;
- `reset()`;
- metrics/diagnostic snapshot.

Use atomic write-to-temp, fsync where appropriate, rename, restrictive permissions, and parent-directory validation. Do not overwrite arbitrary symlink targets. Treat parse and permission failures as bounded diagnostics; startup proceeds without restored tabs.

Saving must be debounced and coalesced. A rapid sequence of tab/session changes should produce bounded writes. Shutdown flush has a strict deadline and may fail without blocking terminal restoration indefinitely.

## 7. Restore coordinator

Implement restore as explicit phases:

1. load and validate local manifest;
2. connect/negotiate daemon capabilities;
3. fetch bounded project catalog summaries;
4. validate each persisted project through `ProjectGet` or equivalent;
5. validate requested workspace membership;
6. validate optional session binding through session get/snapshot;
7. build lightweight tabs in persisted order up to the cap;
8. choose active tab deterministically;
9. load exactly one active heavy view through the Milestone 003 transaction;
10. mark stale/unavailable entries with bounded diagnostics or omit them according to policy;
11. save the normalized manifest.

Restoration must use concurrency caps and cancellation. Do not issue unbounded requests for a large or malicious manifest.

## 8. Recovery policy

Required cases:

- project archived: show unavailable/archived placeholder or skip according to user setting; never restore services;
- project missing: skip and record bounded diagnostic;
- workspace missing/rebound: request project default/explicit user selection, never choose by path;
- session missing/archived: restore project tab with no active session;
- session moved to another canonical project: reject stale association and offer opening the canonical project;
- unsupported older daemon: fall back to one compatibility tab without rewriting a good manifest destructively;
- daemon unavailable: start disconnected with manifest intent visible only if it cannot be mistaken for validated state;
- manifest corrupted/oversized: quarantine or reset, then start safe;
- partial write/temp file: prefer last valid committed manifest;
- duplicate tabs: deterministic one-project-one-tab normalization.

## 9. Resource and long-running closure work

Define and enforce caps for:

- open tabs;
- persisted tabs;
- catalog/session summaries per tab;
- concurrent restore validations;
- frontend tasks and subscriptions per tab;
- inactive activity counters;
- diagnostics/toasts;
- manifest bytes and write frequency;
- reconnect attempts/backoff;
- retained closed-tab metadata: zero unless explicitly required.

Add soak/stress tests for:

- thousands of tab switches;
- repeated close/open cycles;
- reconnect loops;
- daemon restart during restore;
- ongoing events in several projects;
- manifest churn;
- archived/missing objects;
- terminal resize and narrow rendering;
- cancellation/shutdown.

Demonstrate stable memory/task/subscription counts within documented tolerances.

## 10. Legacy frontend-authority cleanup

After Milestone 003 routing is stable, inventory and remove or narrow obsolete assumptions including:

- direct reads of `session_state.project_dir` as project authority;
- global active-session mutations that bypass route tokens;
- compatibility accessors no longer required by supported startup paths;
- route variants that cannot represent project/session scope;
- duplicated tab/session selection state;
- comments/docs describing single-project ownership as canonical.

Do not remove wire compatibility fields owned by protocol/domain roadmaps. Do not force Session Projection adoption before its roadmap closes.

Add static guards for reintroduction of path/current-focus authority in TUI code.

## 11. Work packages

### A — Manifest contract and secure storage

- Define schema, limits, validation, migrations, and atomic storage.
- Add corruption, permissions, symlink, oversized, and partial-write tests.

### B — Restore validation pipeline

- Implement bounded daemon-authoritative validation.
- Build lightweight tabs and one heavy active view.
- Handle missing/archived/rebound/unsupported cases.

### C — Save lifecycle and controls

- Hook tab/session/order/preference mutations into debounced saves.
- Flush on shutdown and expose reset/disable/status controls.
- Avoid save loops during normalization.

### D — Legacy cleanup and guards

- Remove obsolete single-project frontend authority.
- Add path/focus/global-mutation guards.
- Preserve documented compatibility fallback.

### E — Resource validation and closure

- Add stress/soak/restart/reconnect suites.
- Document caps and metrics.
- Complete roadmap closure matrix and operational docs.

## 12. Required tests

- empty/no manifest starts safely;
- valid two-project manifest restores order and active intent;
- only active session heavy view is loaded;
- persisted `ProjectTabId` is not required/reused;
- duplicate project entries dedupe deterministically;
- malformed/oversized/corrupt manifest is rejected safely;
- symlink/path traversal cannot redirect manifest writes;
- partial write preserves last valid manifest;
- archived/missing project and workspace recovery follows policy;
- missing session restores project tab without session;
- rebound session never restores under stale project;
- older daemon compatibility does not destroy manifest;
- daemon unavailable/disconnect during restore is cancellable;
- reconnect resumes validation without duplicate tabs;
- rapid mutations coalesce writes;
- close/open updates intent without daemon deletion;
- shutdown flush is bounded and terminal restoration remains reliable;
- stress switching/open/close/reconnect remains within resource caps;
- no secret-bearing field appears in serialized fixtures;
- static guards reject path-derived frontend authority;
- all TUI, render, picker, routing, lifecycle, protocol, and daemon regression suites remain green.

## 13. Acceptance criteria

- A versioned bounded manifest persists only safe frontend intent.
- Every restored identity is validated by the daemon before becoming live state.
- Restore loads at most one heavy session view.
- Missing, archived, rebound, corrupt, unsupported, and disconnected cases are explicit and safe.
- Writes are atomic, permission-safe, symlink-safe, debounced, and size-bounded.
- No messages, prompts, secrets, outputs, file bodies, logs, terminal frames, subscriptions, or leases are persisted.
- Long-running tab/task/subscription/memory behavior is bounded and evidenced.
- Obsolete path/current-focus/single-project TUI authority is removed or explicitly retained with a documented compatibility reason.
- Existing single-project workflows and older-daemon fallback remain functional.
- Multi-Project TUI roadmap exit criteria are fully evidenced.
- Architecture, operations, compatibility, troubleshooting, and closure records are complete.

## 14. Verification commands

At minimum:

```bash
cargo fmt -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
CARGO_BUILD_JOBS=1 cargo check --workspace --all-features
cargo test -p codegg --lib tui::
cargo test --test tui_project_tabs
cargo test --test tui_project_picker
cargo test --test tui_project_routing
cargo test --test tui --test tui_render
cargo test --test session_selection
cargo test --test single_daemon_lifecycle
cargo test -p codegg-protocol
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_git_forbidden_patterns.py
bash scripts/check-core-boundary.sh
```

Add focused manifest/restore/security/soak test targets and record their exact results in the closure file.

## 15. Roadmap closure

When this plan closes:

- mark the Multi-Project TUI and sessions roadmap closed;
- move all four milestones to recently closed work in `plans/registry.md`;
- retain later Session Projection frontend migration as a separate cross-subsystem adoption effort rather than reopening TUI project identity work.