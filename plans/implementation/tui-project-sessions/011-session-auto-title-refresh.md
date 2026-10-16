# TUI Project Sessions Milestone 011 — Session auto-title reaches the TUI

Status: implemented (uncommitted working tree; `scripts/verify.sh quick` passed)
Repository baseline: `b4e9a92d`

Source roadmap: `plans/subsystems/tui-project-sessions-roadmap.md`
Long-term requirements:
- `plans/000-long-term-specification.md#17-tui-target-architecture`
- `plans/002-long-term-roadmap.md#phase-4--multi-project-and-multi-session-tui`
Applicable ADRs: None required (no identity, auth, scheduler, or protocol-meaning change; one additive optional event field with serde default).
Primary class: capability (user-visible: sessions stop sticking on "Untitled")

## 1. Objective

One bounded outcome: when the daemon auto-titler writes a session title, the bound TUI session header, sidebar entry, and session dialog show it without a manual `/sessions` reload.

## 2. Why this milestone is ready

- Root cause is localized and evidenced: `start_refresh_session` sends `SessionList { project_id: "" }`, daemon rejects empty identity with `project_context_required`, `apply_session_refreshed` drops the error with only `tracing::debug!`.
- Interface deps stable: `CoreRequest::SessionList`, `CoreEvent::SessionUpdated`, `SessionStore::update`, `maybe_auto_title_session` all exist; fix is a caller-context correction plus one additive optional field.
- No hard deps on open milestones.

## 3. Current implementation evidence

- Creation default: `crates/codegg-core/src/session/store.rs` `create_with_id` defaults title to `"Untitled"`, slug `untitled` (`generate_slug`).
- Writer: `src/agent/title.rs` `maybe_auto_title_session` — only when stored title is exactly `"Untitled"`, only after `Ok(events)` turn, only when `pool.is_some()`, excerpts non-empty, provider stream yields sanitizable text. Publishes `AppEvent::SessionUpdated { id }` + `CoreEvent::SessionUpdated { session_id }` (id only).
- Consumer: `src/tui/app/mod.rs` `E::SessionUpdated` arm calls `start_refresh_session`, which issues `SessionList` with empty `project_id` (`src/tui/commands/sessions.rs`). Daemon `SessionList` handler (`src/core/daemon_sessions.rs`) resolves via `resolve_session_list_project` (`src/core/daemon_refresh.rs`): empty fails `IdentityParseReason::Empty`, `resolve_directory("")` yields `DirectoryNotFound` → `project_context_required`. Completion carries `error: Some`, `apply_session_refreshed` returns early.
- Contrast: `start_reload_sessions` uses `app.active_project_key()` and succeeds.
- `SessionStore::update` updates `title` but leaves `slug` as `untitled`.
- Existing tests call `apply_session_refreshed` directly, never the wire request, so the empty-project bug is uncovered.

## 4. Invariants that must not regress

- TUI remains a client projection; daemon owns session storage, project identity, workspace binding.
- `SessionUpdated` stays backward compatible (additive optional field only, serde-defaulted; old clients ignore it).
- No new unbounded reads: keep `limit: 200` semantics or remove the extra read entirely via the event-carried title.
- Generation guards preserved: stale-refresh generation check, other-session ignore, explicit rename never overwritten (writer re-checks stored title).
- `check_tui_project_authority`-style rule: TUI never derives project identity from process cwd; only from `active_project_key()` / event-bound context.

## 5. Scope

In:
- Fix `start_refresh_session` to carry real project context.
- Carry the new title in `SessionUpdated` (additive `Option<String>`) and apply it directly, keeping the `SessionList` refetch as fallback.
- Update `slug` alongside `title` in the auto-title write path (and manual rename path if it shares `SessionStore::update`) so search/list stays consistent.
- Regression tests for the wire-level failure and the direct-apply path.

Out:
- New single-session `SessionGet` RPC (unnecessary if title rides the event).
- Retrying failed title generation on a timer, changing the title prompt/model, synchronous naming at creation.
- Toasting every background title failure (stays best-effort, daemon-log only; TUI surfaces only successful titles).
- Desktop client changes beyond compiling against the additive field.

## 6. Required production changes

- Core/protocol (`crates/codegg-protocol/src/core.rs`, `src/core/mod.rs` mapping, projection replay `publication.rs`/`safe_publication.rs` if they match on the variant): add `title: Option<String>` with `#[serde(default)]` to `CoreEvent::SessionUpdated`; update `map_app_event_to_core_event`, event-log filter, and any exhaustive matches (daemon socket tests, server `ws.rs`, desktop `route.rs`).
- Daemon writer (`src/agent/title.rs`): after successful `store.update`, publish `SessionUpdated { session_id, title: Some(title) }` on both bus and event log.
- TUI consumer (`src/tui/app/mod.rs` `E::SessionUpdated` arm, `src/tui/commands/sessions.rs`): prefer event-carried title — apply via existing `apply_session_refreshed` logic without a round trip; fall back to `start_refresh_session` only when title is `None`. Fix fallback to use `app.active_project_key().unwrap_or_default()` (same as reload) instead of `""`, and thread the key through the async task.
- Storage (`crates/codegg-core/src/session/store.rs` `update`): when `input.title` is `Some`, also recompute `slug = generate_slug(&input.title)` in the same `UPDATE` (single statement, no new migration; slug is derived).
- Logging: keep generation failures at `warn` in daemon log; change refresh-failure log from `debug` to at least `debug` with `code` preserved, and do not toast on background refresh (avoid noise). Document where to look (`auto-title generation failed`, `session auto-titled`, `session refresh failed`).

## 7. Ordered work packages

### WP1 — Fix the refresh context (unblocks TUI even without protocol change)

Intent: make the existing fallback path succeed.
Changes:
- `start_refresh_session(app, session_id)`: capture `app.active_project_key()` and send it as `SessionList.project_id`; keep `show_archived: true, limit: 200`, keep generation guard.
- If no active project key (startup edge), skip the request rather than sending `""` (avoids guaranteed `project_context_required`).
Acceptance: with a fake `core_client` asserting `project_id != ""`, `SessionRefreshed` returns the title and `apply_session_refreshed` updates bound session + both dialog copies.

### WP2 — Carry title in `SessionUpdated`, apply directly

Intent: remove the fragile extra round trip (limit-200 miss, cross-project lookup, archived-project rejection).
Changes:
- Protocol + mappings + projection classification + desktop route exhaustive matches.
- Writer publishes title; TUI `E::SessionUpdated { session_id, title }` applies `Some(title)` directly via the existing retitle closure, else falls back to WP1 refresh.
Acceptance: daemon-published event with title updates header/sidebar/dialog with no `SessionList` issued (assert via task registry / request spy); `None` title still triggers one bounded refresh.

### WP3 — Keep slug consistent

Intent: titled sessions remain searchable/listable by their new name.
Changes: `SessionStore::update` recomputes slug when title changes.
Acceptance: store-level test — create `Untitled`, update title to `Fix login redirect loop`, assert `title` and `slug == "fix-login-redirect-loop"`; rename path covered if shared.

### WP4 — Regression tests + guards

- TUI: `start_refresh_session` sends non-empty project id (fake core client); `SessionUpdated` with title applies without round trip; stale-generation and other-session guards still hold (extend existing `refreshed_title_*` tests).
- Daemon: auto-title publishes `SessionUpdated` carrying the new title (bus/event-log capture).
- Store: slug-on-rename test.
- Run `scripts/verify.sh quick` (or at minimum: `cargo fmt`, focused `cargo test -p codegg` for touched modules, workspace `cargo ck`).

## 8. Failure, cancellation, restart, contention semantics

- Title write stays best-effort: failures never fail the turn; next successful turn retries (title still `"Untitled"`).
- Raced manual rename still wins: writer re-reads stored title before writing; TUI generation guard drops stale completions; other-session events ignored.
- Detached `tokio::spawn` title task dying with the daemon is acceptable (next turn retries); no new detached tasks introduced except the existing bounded TUI refresh task.
- `SessionList` fallback remains bounded (`limit: 200`, `show_archived: true` so the bound active session is found even if archived mid-flight).

## 9. Compatibility and migration

- No migration: slug recompute happens in the existing `UPDATE`; old rows keep `untitled` slugs until next title write (acceptable) — or backfill opportunistically on read? Out of scope; do not migrate.
- Protocol additive: `title: Option<String>` defaulting to `None`; old TUI/server/desktop binaries ignore it and keep the (now fixed) refresh fallback. New TUI handles both shapes.
- No auth/scheduler/identity semantics change.

## 10. Required tests

- Focused: new/updated tests in `src/tui/commands/sessions.rs` (refresh request project id, direct-apply, stale/other-session guards), `src/agent/title.rs` or daemon-turns harness (event carries title), `crates/codegg-core` session store (slug on title update).
- Integration: one daemon→TUI path test if harness allows (publish `SessionUpdated` with title, assert bound session retitled with zero `SessionList` calls); otherwise two unit tests proving each half plus the WP1 wire test.
- Existing: keep `refreshed_title_is_applied_to_the_bound_session`, `stale_session_refresh_is_dropped`, `session_refresh_ignores_other_sessions` green.

## 11. Required verification commands

```bash
scripts/verify.sh quick
# narrowest-first alternative while iterating:
cargo test -p codegg --lib agent::title
cargo test -p codegg --lib tui::commands::sessions
cargo test -p codegg-core session::
cargo ck
```

Both verify modes cap build jobs; do not use `--all-features`.

## 12. Documentation updates

- `architecture/tui.md` `SessionUpdated` row: title now rides the event; refresh is fallback-only.
- `architecture/session.md` (if it documents `update`/slug): slug recomputed on title change.
- `architecture/protocol.md` session event list if it enumerates fields.
- Skill index unchanged (no new skill; `tui` + `session-storage` skills already cover this).

## 13. Acceptance criteria (externally observable)

1. Complete one turn in a fresh `Untitled` session with a working provider → header/sidebar/session-dialog show a generated title with no manual reload.
2. Stall the provider (or force sanitizer reject) → session stays `Untitled` with no toast/spinner hang; next successful turn retries.
3. Manual rename raced with auto-title → manual value wins and is not clobbered by a late completion.
4. Daemon DB `session.title` and TUI-visible title agree after auto-title; `session.slug` matches the new title slug.

## 14. Stop conditions

- If carrying `title` in `SessionUpdated` requires breaking (non-additive) protocol change or a new RPC with capability negotiation, stop and report rather than forking the event type.
- If `active_project_key()` is unavailable in the refresh path (e.g., socket-daemon frontend without project context), do not fall back to `""`; report the context gap.
- If slug recompute collides with a uniqueness assumption anywhere (check indexes/consumers of `slug`), stop and report instead of adding uniqueness.

## 15. Closure evidence required

- Implementation commit(s) + focused test names/results + `scripts/verify.sh quick` result.
- Proof the old failure is gone: fake-core test asserting refresh request carries non-empty project id, plus (if WP2) proof no `SessionList` is issued when the event carries a title.
- Requirement-to-evidence matrix mapping §13 criteria to tests/observations.

## 16. Handoff notes

- Suggested subsystem: `tui-project-sessions` (roadmap closed; this is a corrective milestone — register as `011` or as a corrective addendum if maintainers prefer).
- Registry: add `TUI Project Sessions Milestone 011` under dependency-ready plans as `ready` (no hard deps); move to `active` on start.
- Do not edit `src/agent/builtins/generated.rs` (title agent untouched); no `assets/agents/*.toml` change needed.
