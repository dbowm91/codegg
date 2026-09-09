# Project Collaboration Milestone 002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/project-collaboration/002-tui-project-chat-and-observer-routing.md`

Source subsystem roadmap:

- `plans/subsystems/project-collaboration-roadmap.md#M002--TUI-project-chat-and-observer-input-routing`

Repository baseline reviewed: `5a16a87eaaed1b0cabb096693586bc7e3dc6dbcd`

Implementation commits or pull requests:

- (this commit) — feat(collaboration): M002 TUI project chat and
  observer input routing (chat reducer, chat.v1 command pipeline,
  observer insert seam, 16 unit + 11 integration tests, docs).

## 1. Executive finding

M002 is closed. A project member can communicate in the correct project
while observing work: the TUI renders daemon-owned `chat.v1` state as a
bounded per-project panel with send/reply/mention/reference/edit/redact/
read/composing flows, deterministic reconnect/paging, and per-project
draft isolation. Bare observer insert-mode input routes to the observed
project's chat and emits zero turn steering/control requests; the
read-only invariant is preserved (observer allowlist gains only the nine
`/chat*` commands; every control family stays denied). Unsupported and
unauthorized states degrade to the identical unavailable panel with
cached content cleared. No unresolved high, medium, or low M002 finding
remains.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Chat reducer/cache with bounded window + cursor paging per project/channel (package A) | `src/tui/app/state/chat.rs`: `ChatState`/`ProjectChat`/`ChatStatus`; 16 projects LRU, 100-message sliding window, 50-row panel virtualization, 500-byte display truncation, 8 KiB drafts; `apply_history` (replace) / `apply_sync` (merge-or-resync) with `next_cursor`/`retention_floor_seq` | pass | One active-channel window per project; multi-channel windows explicitly deferred |
| Panel/tab message/reference rendering + bounded composer (package B) | `panel_lines`/`header_summary`/`message_line`; `InfoType::ProjectChat` panel via generic info dialog; references render as `kind:target_id` handles with optional hint; composer bound 8 KiB with fail-fast typed error | pass | Full bodies stay daemon-side; panel caps at display bound + chrome |
| Send/reply/mention/edit/redact/read/composing through existing async pipeline (package C) | `src/tui/commands/chat.rs`: capability negotiation + op in one registered `TuiTaskKind::Command` task; 10 `TuiCommand::Chat*` completions; `/chat*` slash commands (9 new, registry 127 → 136); no task-per-message spinner | pass | Mentions extracted client-side as convenience; daemon re-validates, never confers authority |
| Observer-mode insert routing + hard zero-control regression (package D) | `route_observer_insert_to_chat` (observed project preferred, active-tab fallback, placeholder fail-closed); `send_prompt` routes bare observer input to chat; integration test asserts recorded core requests ⊆ `chat_*` and observer still blocks 13 control probes | pass | `!` shell input while observing routes literally to chat, never executes |
| Multi-project/reconnect/lag/retention/focus tests + docs (package E) | 16 reducer unit tests + 11 integration tests (`tests/collaboration_m002_chat_tui.rs`); `architecture/collaboration.md` (M002 section) + `architecture/tui.md` (Project Chat section) + help entries | pass | Focus follows standard info-dialog convention; reconnect resumes M001 cursor |

## 3. Production implementation evidence

- `src/tui/app/state/chat.rs` (new, ~1100 lines with 16 tests):
  bounded `ChatState` reducer — per-project entries with channels,
  active-channel window, cursors, forward-only read markers with
  derived unread, expiry-filtered composing, per-project drafts,
  request-id + epoch stale guards, cross-project/cross-channel routing
  guards, LRU eviction, identical unavailable rendering, bounded
  `panel_lines`/`header_summary`/`message_line`, `extract_mentions`.
- `src/tui/commands/chat.rs` (new, ~1500 lines): `start_*`/`apply_*`
  for ensure/history/sync/send/edit/redact/read/composing, panel
  refresh-on-apply, `route_observer_insert_to_chat`, daemon event
  intake (`on_chat_message_committed/edited/redacted`,
  `on_chat_composing_hint`).
- `src/tui/app/mod.rs`: `App::{chat, chat_panel_project}` fields
  (both constructors), 10 `TuiCommand::Chat*` variants, pub
  `show/refresh/sync/send/mark-read/set-composing/apply_*` wrappers,
  observer event intake wrappers, `send_prompt` observer→chat routing,
  tab-switch chat refresh, reconnect chat resync, 9 `/chat*`
  `execute_command` arms.
- `src/tui/runtime/command_dispatch.rs`: 10 dispatch arms (completions
  route through the pub `App::apply_*` wrappers, mirroring presence).
- `src/tui/app/state/observe.rs`: `/chat*` allowlisted for observers;
  placeholder reworded (fallback-only now that the seam is live).
- Rendering/help: `InfoType::ProjectChat` + `DialogType::ProjectChat`
  (generic info dialog, no new `Dialog` variant),
  `InfoDialog::info_type()` accessor, 3 help entries.
- `src/tui/command.rs`: 9 new slash commands (count test 127 → 136).
- `tests/collaboration_m002_chat_tui.rs` (new, 11 tests): recording
  `FakeChatClient` over the full `chat.v1` surface; isolation,
  send/reply/render, history+sync+resync, unread/read, composing
  set/list/expiry, denial + draft retention, observer zero-control,
  reconnect, panel focus/bounds, old-daemon hiding, edit/redact.
- Docs: `architecture/collaboration.md` (M002 section + testing),
  `architecture/tui.md` (Project Chat section + enforcement reword),
  help entries for `/chat`, `/chat-send`, `/chat-history`.

Distinguished as absent (downstream, not M002 scope): structured chat
actions with canonical task/job linkage (M003), DMs, external bridges,
membership administration, channel rename/delete, multi-channel
windows.

## 4. Verification executed

All local (no hosted `CI / verify` claimed).

### Commands run

```bash
cargo test -p codegg --lib tui::app::state::chat
cargo test --test collaboration_m002_chat_tui --test presence_m002_collaborators --test presence_m003_observation --test collaboration_m001_chat
cargo test --test tui --test tui_render --test tui_project_tabs --test tui_project_routing
RUST_MIN_STACK=16777216 cargo test -p codegg --lib
cargo test -p codegg-core --lib
cargo test -p codegg-protocol
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
bash scripts/check-core-boundary.sh
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/generate_builtin_agents.py --check
scripts/verify.sh quick
```

### Results

- `codegg --lib tui::app::state::chat`: 16 passed (routing
  isolation, drafts, failed-send retention, bounded window, sync
  merge/resync, unread forward-only, composing expiry, denial identity,
  reconnect, live-event channel match, stale-id drop, locator guards,
  reference rendering, mention bounds, eviction, hints).
- `collaboration_m002_chat_tui`: 11 passed (matrix §2, last row set).
- M001 + presence M002/M003 regression suites: 12/12/11 passed,
  zero failures (observer read-only policy intact with chat
  allowlisted).
- TUI suites: `tui` 164, `tui_render` 99, `tui_project_tabs` 20,
  `tui_project_routing` 27 — all passed.
- `codegg --lib`: 4424 passed, 0 failed (with the repo-precedent
  `RUST_MIN_STACK=16777216` debug-harness workaround).
- `codegg-core --lib`: 625 passed. `codegg-protocol`: 177 passed.
- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: pass
  (after collapsing one nested `if` and using `?` in the composing-refresh task).
- All static guards: pass (core boundary holds — no new
  `codegg-core` deps; execution-ownership/scheduler-bypass/cwd clean;
  builtin agents fresh).
- `scripts/verify.sh quick`: passed.

Plan §11 lists `cargo test --workspace tui --no-fail-fast` and
`cargo test --workspace collaboration --no-fail-fast`: no `tui` or
`collaboration` workspace crates exist, so the justified substitutes
above were used (same convention as the M001 closure: focused TUI
suites + lib + guards). No verification was skipped without a
substitute.

## 5. Invariant review

| Plan invariant | Evidence | Result |
|---|---|---|
| TUI renders daemon chat state, owns no durable messages | Reducer caches only; every mutation round-trips `chat.v1`; failed sends fabricate nothing (pinned) | pass |
| Chat routes by ProjectId/ChannelId | Project/channel/request/epoch guards on every apply; cross-project + cross-channel pages dropped (pinned) | pass |
| Observer typing cannot become turn steering | Insert path issues only `Chat*` requests (recorded-kinds assertion); 13 control probes still blocked; permission/question answers untouched | pass |
| Unauthorized/stale project data clears | Denial clears messages/channels/composing/drafts; denied ≡ absent panel (pinned) | pass |
| Large references remain handles | `message_line` renders `kind:target_id` + hint only; no content fetch exists | pass |
| No unbounded message history in memory | 100-row window, 50-row panel, 16-project LRU, 8 KiB drafts, 500-byte display truncation (pinned) | pass |

## 6. Failure and recovery review

- Failed send: draft + typed error retained, window untouched, warning
  toast (pinned, incl. unauthorized and oversized-fail-fast paths).
- Duplicate delivery: `merge_page` dedups by `message_id`; sends carry
  uuid idempotency keys; duplicate completions suppress the toast and
  converge (pinned at reducer level).
- Reconnect: epoch bump drops pre-reconnect completions; entries flag
  `needs_resync`; resume uses cached `next_cursor`; expired cursors take
  the bounded resync-replace path (pinned).
- Rapid project switching: per-project drafts/windows; stale completions
  dropped by request id; cross-project pages rejected (pinned).
- Observer target disconnect: chat refresh is independent of
  observation state; composing clears per project while chat stays
  usable (reducer `clear_composing` + independent refresh paths).
- Malformed input: empty bodies show usage; oversized fail fast with
  `chat_body_too_large` semantics; bad locators fail closed with no
  state change (pinned).

## 7. Migration and compatibility review

- No storage migration (TUI owns no durable chat state).
- Protocol: purely additive consumption of the M001 `chat.v1` surface;
  no `PROTOCOL_VERSION` bump. When the daemon lacks the capability the
  panel hides as unavailable and tabs/session operation is unchanged
  (pinned by the old-daemon test).
- Config: no new settings; bounds are code constants.
- Rollback: dropping the binary removes the panel/commands; daemon
  state untouched.

## 8. Security review

- Authorization is never inferred client-side: every chat operation
  round-trips the daemon gate (`project.chat`); denials render as the
  generic unavailable panel with cached content cleared.
- Principals derive from transport authority; mention extraction is a
  display convenience the daemon re-validates.
- Free text has no execution semantics: the insert path constructs only
  `ChatSend` (no command parsing, no template rendering, no shell
  classification); `!` input while observing routes literally to chat.
- Observer hardening preserved: allowlist gains only the nine `/chat*`
  commands; turn/permission/model/session/file/Git/job/shell/plugin
  families remain denied (pinned with 13 negative probes).
- Secrets/DoS: 8 KiB composer bound, display truncation, content-free
  composing, bounded pages/history — same envelope as M001.

## 9. Documentation and operations

- Updated: `architecture/collaboration.md` (M002 section: reducer,
  commands, observer seam, panel, slash commands; testing commands),
  `architecture/tui.md` (Project Chat section; read-only enforcement
  reworded for the live seam), TUI help (`/chat`, `/chat-send`,
  `/chat-history` entries).
- Operator diagnostics: typed send/history/sync/edit/redact/read
  failure toasts with draft-retained guidance; `💬 N (M unread)`
  header; stale/resync panel notes; `ChatHint` coalescing (no task per
  hint).
- Static guards: core-boundary, execution-ownership, scheduler-bypass,
  daemon-cwd, builtin-agents all green; no new CI lane added per
  verification policy.

## 10. Unresolved findings

There are no unresolved critical, high, medium, or low M002 findings.

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| — | None | — | — |

One pre-existing test-harness note (not an M002 finding): the
`codegg --lib` suite requires the repo-precedent
`RUST_MIN_STACK=16777216` workaround in debug harness mode (same as
the M001 closure); no production impact.

## 11. Roadmap disposition

Milestone closed. Dependency audit: project-collaboration M003
(structured chat actions) hard-depends on M001 only (already closed)
and proceeds in parallel with — not after — M002 per the roadmap
dependency graph; M002 closure adds no new hard dependency to M003
(M003's TUI ergonomics package may optionally reuse the M002 panel,
but its contract is the M001 protocol, which is unchanged). No other
registered plan lists M002 as a hard or interface dependency, so no
further plan moves from `blocked`/`proposed` to `ready` in this
commit. No corrective follow-up is required; no new dependency-ready
plan was created.

## 12. Registry updates

- `plans/registry.md`: collaboration row `active / M001 closed, M002
  active, M003 ready` → `active / M001+M002 closed, M003 ready`;
  dependency-ready table: remove the M002 TUI-chat row (implemented);
  execution order §4 reworded (M001+M002 closed; M003 proceeds);
  closure control points: add M002 closed row.
- `plans/subsystems/project-collaboration-roadmap.md`: Status stays
  `active`; M002 `ready` → `closed` with closure link; M003 stays
  `ready`.
- `plans/implementation/project-collaboration/002-tui-project-chat-and-observer-routing.md`:
  `active` → `implemented` with closure link.
