# Project Collaboration — Channels, Messages, Synchronization (M001), TUI Chat (M002)

## Purpose

Daemon-owned durable project-scoped channels and messages with
replies/threads, mentions, typed CodeGG object references,
edits/redactions, read markers, ephemeral composing state, retention,
and bounded idempotent incremental synchronization over the native
protocol — plus the TUI project-chat panel and observer input routing
(M002).

Chat is **project communication, not general chat** and never an
execution interface: message bodies are inert bounded text. Free text —
including command-like, mention-like, or action-like text — never
executes privileged work by parsing, prefix, or model inference.
Structured actions that create tasks/jobs belong to M003 and pass
through ordinary authorization as explicit typed operations.

Long-term requirements:
`plans/000-long-term-specification.md#21-project-communication`,
`#22-audit-architecture`, `#27-security-requirements`;
`plans/002-long-term-roadmap.md#phase-12--project-communication`.
Terminology: `plans/001-terminology-and-domain-model.md#11`
(ProjectChannel, ChatMessage, Mention, ReadMarker).
Roadmap: `plans/subsystems/project-collaboration-roadmap.md#M001`.
Implementation plan:
`plans/implementation/project-collaboration/001-project-channel-message-protocol.md`.
Closure: `plans/closure/project-collaboration/001-status.md`.
TUI chat (M002) plan:
`plans/implementation/project-collaboration/002-tui-project-chat-and-observer-routing.md`.
Closure: `plans/closure/project-collaboration/002-status.md`.

## Where It Lives

```
crates/codegg-core/src/collaboration.rs   # domain service: channels/messages/revisions/
                                          # markers/retention/ephemeral composing (M001)
crates/codegg-core/src/identity.rs        # ChatMessageId (ChannelId reused for channels)
crates/codegg-core/src/session/schema.rs  # v55 chat tables (additive, IF NOT EXISTS)
crates/codegg-core/src/storage/mod.rs     # STORAGE_LAYOUT_VERSION = 55
crates/codegg-core/src/authorization.rs   # 12 chat_* operation descriptors (project.chat)
crates/codegg-core/src/projection_replay/safe_publication.rs  # chat events classify Safe
crates/codegg-protocol/src/core.rs        # chat.v1 DTOs, 12 CoreRequests, 8 CoreResponses,
                                          # 4 CoreEvents (CHAT_CAPABILITY, CHAT_PROTOCOL_VERSION)
src/core/daemon.rs                        # daemon-owned CollaborationService, channel->project
                                          # resolver, privacy denials, dedicated chat handler
tests/collaboration_m001_chat.rs          # 12 daemon boundary tests
src/tui/app/state/chat.rs                 # M002: bounded per-project chat reducer (ChatState)
src/tui/commands/chat.rs                  # M002: async chat commands + observer insert routing
tests/collaboration_m002_chat_tui.rs      # M002: 11 TUI boundary tests
```

## How It Works

### Domain service (`codegg-core::collaboration`)

`CollaborationService` holds an optional durable pool plus an
in-memory composing map (`DashMap`, no timers, no background tasks).
One shared instance lives on the daemon so composing leases survive
across requests; restart constructs a fresh instance and drops every
lease. Pool-less daemons serve composing state only and fail durable
operations with `chat_unavailable`.

- **Channels**: `ensure_default_channel` converges on one `general`
  channel per project (oldest wins); `ensure_channel_by_name` is the
  narrow named extension point (idempotent per name, no rename/delete
  in this milestone). At most 16 channels per project.
- **Messages**: `send_message` validates bounds, redacts secrets,
  checks reply/thread targets name durable messages in the same
  channel, then assigns `seq = MAX(seq)+1` per channel inside one
  transaction with a `UNIQUE(channel_id, seq)` backstop and one
  contention retry. Bodies ≤ 8 KiB, ≤ 16 mentions, ≤ 8 references;
  NUL and non-whitespace control characters rejected.
- **Idempotency**: `(channel_id, idempotency_key)` is unique. A retry
  returns the original message with `duplicate: true` and assigns no
  new sequence — including across the contention-retry path.
- **Edits/redactions**: author-only with optimistic revision checks
  (`RevisionConflict` on stale). Every revision lands in
  `chat_revision`, which survives retention pruning, so history is
  append-only. Redacted bodies become `[REDACTED]`; redacted messages
  refuse further edits.
- **Read markers**: per `(channel, principal)`, forward-only; stale
  writes leave the stored row untouched.
- **Sync**: `history` pages ascending by `seq` with `next_cursor` and
  `truncated`. `sync(from_seq)` resumes incrementally at or above the
  retention floor; expired cursors return `resync_required: true` with
  the oldest retained page. Pages clamp to 100 rows (default 50).
- **Retention**: each channel keeps the newest 1000 messages, oldest
  pruned first on send (plus explicit `prune_retention`). Revision
  rows are never pruned by the window.
- **Composing**: `set_composing` renews a 30s lease per
  `(channel, principal, client)`; every read evicts expired leases
  through the single cleanup path. Leases carry identity and expiry
  only — never content.

### References and redaction

`ChatReference{kind, target_id, display_hint?}` names sessions, agent
runs, jobs, commits, artifacts, worktrees, or runs as opaque locators
(≤ 128 bytes, identity lexical contract). Project-level gating is the
M001 privacy boundary: only `project.chat` holders on the message's
project ever observe references, and cross-project channel access
denies as not-found. Display hints truncate to 200 chars and pass the
same secret scan as bodies. Per-object rechecks defer to the canonical
owners when M003 structured actions resolve references into work.

`redact_secrets_in_body` replaces `key=value` / `key: value` secrets
(password, api_key, tokens, bearer, …) and high-confidence token runs
(`AKIA`, `ghp_`, `sk-`, `xoxb-`, …) with `[REDACTED]` before durable
write — over-redacting adjacent text rather than persisting a secret.
Bare key mentions in prose (no assignment operator) are left alone.

### Protocol (`chat.v1`, version 1)

Requests: `ChatCapabilities`, `ChatChannelEnsure/List`,
`ChatHistory/Send/Edit/Redact`, `ChatReadSet/Get`,
`ChatComposingSet/List`, `ChatSync`. Responses mirror with bounded
pages plus `next_cursor` / `retention_floor_seq` / `resync_required`.
Events (`ChatMessageCommitted/Edited/Redacted`,
`ChatComposingUpdated`) are structural liveness hints; redaction and
composing hints carry identity/revision only. The surface is purely
additive: older clients ignore it, no `PROTOCOL_VERSION` bump, no
legacy chat data to migrate, and future bridge IDs must map as
external provenance without replacing CodeGG message IDs.

### Authorization and daemon boundary

All project-scoped chat operations are `DirectProject` +
`project.chat` (`Contributor` and above; `Viewer` is denied).
Channel-scoped requests carry only the channel locator; both the gate
(`resolve_authorization_project` via `channel_project`) and dispatch
(`resolve_chat_channel`) resolve the owning project server-side, and
unknown channels fail closed. Denials use `project_not_found`,
indistinguishable from absent — including for members probing unknown
channel ids. Principals always derive from transport authority.
Failed sends publish no event; duplicate retries publish no second
event.

### Audit separation

Chat storage remains separate from the append-only audit store. Chat
operations have no live audit mapping (same convention as
`chat_triggered_action`, which M003 owns):
`audit_metadata_for_message` exposes only structural locators
(channel/message/project ids, seq, revision, redaction flag) — never
bodies, hints, or mentions — as the hook for future linkage. The
integration suite proves maintainer-visible audit pages contain no
chat body or secret material.

## Failure, restart, contention

- Duplicate delivery converges on the idempotency row; concurrent
  sends converge on distinct sequences via transactional assignment
  plus the unique backstop (pinned by a 4-way join test).
- Edit/redact races surface typed `RevisionConflict`; non-authors
  receive `chat_not_author`.
- Restart preserves durable order and read markers through the
  catalog pool (pinned by a two-daemon test) and drops composing.
- Expired sync cursors resync to a bounded page; cursors past the
  high-water mark return empty pages, never errors.
- Malformed input (empty/oversized/NUL bodies, bad locators,
  cross-channel reply targets, overlong names/keys) fails with typed
  `chat_*` codes and zero side effect.

## Migration and compatibility

- `STORAGE_LAYOUT_VERSION = 55`; `migrate_v55` creates
  `chat_channel`, `chat_message`, `chat_revision`,
  `chat_read_marker` plus indexes (all `IF NOT EXISTS`, restart-safe,
  no data backfill — no legacy chat data exists).
- Rollback drops the binary: pre-M001 databases migrate forward
  cleanly; chat tables sit unused.

## Security review

- Authorization precedes lookup on every chat path; the gate and the
  handler resolve the project independently and both fail closed.
- Secrets never reach durable rows, events, hints, markers, or audit
  pages (pinned by unit + boundary tests).
- DoS bounds: 8 KiB bodies, 16 mentions, 8 references, 100-row pages,
  16 channels/project, 1000 messages/channel, 30s composing TTL, no
  per-message tasks. Failed sends emit nothing.

## Testing

```bash
cargo test -p codegg-core collaboration       # 14 unit tests (domain/store/retention/composing)
cargo test --test collaboration_m001_chat     # 12 daemon boundary tests
cargo test -p codegg --lib tui::app::state::chat  # 16 chat reducer tests (M002)
cargo test --test collaboration_m002_chat_tui # 11 TUI boundary tests (M002)
python3 scripts/check_authorization_matrix.py # matrix covers all 12 chat_* operations
bash scripts/check-core-boundary.sh           # collaboration stays UI/server/plugin/auth-free
```

## TUI project chat and observer input routing (M002)

The TUI renders daemon chat state and owns no durable messages. Chat
routes by `ProjectId`/`ChannelId` through the M001 `chat.v1` surface
only — no second websocket, no polling loop, no task-per-message.

- **Reducer** (`tui::app::state::chat::ChatState`): bounded per-project
  projection keyed by canonical `project_id`. At most 16 projects, a
  100-message sliding window per active channel, 50-row panel
  virtualization, per-message display truncation (500 bytes), per-project
  drafts (8 KiB bound, never cross-routed), daemon read markers
  (forward-only, unread derived), and content-free composing leases
  (expiry-filtered at display). One channel window per project is cached
  (the active channel); multi-channel windows are future work. Every
  apply path verifies project/channel/request-id/epoch routing, so rapid
  project switching cannot leak one project's messages or drafts into
  another. Unauthorized and feature-absent projects render the identical
  unavailable panel and clear cached content.
- **Commands** (`tui::commands::chat`): capability negotiation then
  channel ensure / history / sync / send / edit / redact / read-marker /
  composing through `CoreClient` on registered tasks, landing as
  `TuiCommand::Chat*` completions with the standard stale-completion
  guard. Failed sends retain the editable draft with the typed error and
  fabricate nothing. History replaces the window; sync merges by
  `message_id` (duplicate deliveries converge) or replaces on
  `resync_required`. Reconnect resumes the M001 `next_cursor` or resyncs
  the bounded window.
- **Observer seam** (`route_observer_insert_to_chat`): while observing
  another session, bare insert-mode input targets the observed project's
  chat. Only `Chat*` core requests are issued on this path — turn
  submit/steer/cancel and permission/question answers stay blocked by
  `ObserverState`, and chat slash commands (`/chat*`) are allowlisted
  while every control family stays denied. When no project is available
  the read-only placeholder toast is the fail-closed fallback.
- **Panel** (`InfoType::ProjectChat` via the generic info dialog, `j`/`k`
  scroll, `Esc`/`Enter` close): active-channel window newest-last with
  unread separators, composing indicator, resync/stale notes, and the
  `/chat-*` command footer. References render as typed locators
  (`kind:target_id` plus optional hint); large content stays daemon-side.
  Slash commands: `/chat`, `/chat-send`, `/chat-reply`, `/chat-history`,
  `/chat-sync`, `/chat-read`, `/chat-edit`, `/chat-redact`,
  `/chat-composing`.

## Related Docs

- `architecture/authorization.md` — `project.chat` matrix rows and
  not-found denial convention
- `architecture/protocol.md` — `chat.v1` surface summary
- `architecture/storage.md` — v55 migration entry
- `architecture/audit.md` — why chat has no live audit mapping yet
- `architecture/presence.md` — ephemeral-state design precedent
