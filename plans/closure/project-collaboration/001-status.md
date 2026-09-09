# Project Collaboration Milestone 001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/project-collaboration/001-project-channel-message-protocol.md`

Source subsystem roadmap:

- `plans/subsystems/project-collaboration-roadmap.md#M001--project-channel-message-and-synchronization-contract`

Repository baseline reviewed: `026b14bb0e0cfd5abdb50fab081b1fa49b1eea11`

Implementation commits or pull requests:

- `184e2712` — feat(collaboration): M001 project channel/message/sync contract (domain store, chat.v1 protocol, daemon service, 14 unit + 12 integration tests, docs).

## 1. Executive finding

M001 is closed. An authorized project member (`project.chat`,
`Contributor` and above) can ensure a default project channel,
exchange/reply/edit/redact messages with mentions and typed CodeGG
object references, advance read markers, publish ephemeral composing
leases, and incrementally sync bounded pages across restart and
reconnect. Unauthorized callers (outsiders and `Viewer`s without the
chat grant) cannot enumerate channels, read messages, or infer
existence: every project-scoped denial uses `project_not_found`,
indistinguishable from absent. Retries converge on client idempotency
keys without duplication; concurrent sends converge on distinct
deterministic sequences; edits/redactions are author-only with typed
revision conflicts and append-only revision history; composing
expires and drops on restart; secret-bearing bodies persist redacted;
chat storage remains separate from the append-only audit store; free
text has no execution semantics. No unresolved high, medium, or low
M001 finding remains.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Domain schema, author/reference/visibility/retention and idempotency semantics (package A) | `crates/codegg-core/src/collaboration.rs`: `ChatChannel`/`ChatMessage`/`ChatReference{kind,target_id,display_hint?}`/`ChatReadMarker`/`ComposingEntry`; `ChatMessageId` in `identity.rs` (`ChannelId` reused); `CollaborationConfig` bounds (8 KiB bodies, 16 mentions, 8 references, 16 channels/project, 1000 messages/channel, 100-row pages, 30s composing TTL); `redact_secrets_in_body` | pass | Author is transport-derived principal + optional agent label; references are opaque locators under project-level gating |
| Durable channel/message/read-marker storage + restart/migration/order tests (package B) | `migrate_v55` (`session/schema.rs`, `STORAGE_LAYOUT_VERSION` 54 → 55): `chat_channel`, `chat_message` (per-channel `seq`, idempotency unique index), append-only `chat_revision`, `chat_read_marker`; `enforce_retention` (newest-1000 window, revisions survive pruning); restart/order tests in module + boundary suites | pass | No legacy chat data; purely additive `IF NOT EXISTS` migration |
| Authorized service APIs for send/reply/edit/redact/read and ephemeral composing state (package C) | `CollaborationService::{ensure_default_channel,ensure_channel_by_name,send_message,edit_message,redact_message,history,sync,set_read_marker,get_read_marker,set_composing,list_composing}`; 12 `chat_*` arms in the M003 `operation_descriptor` matrix (all `DirectProject` + `project.chat`, capabilities global); daemon `handle_chat_request` resolves channel→project server-side for gate and dispatch | pass | Author-only edit/redact; markers forward-only; composing pool-independent and ephemeral |
| Bounded incremental sync protocol/events/cursors and reconnect/idempotency tests (package D) | `chat.v1` DTOs (`ChatMessageDto`, `ChatChannelDto`, `ChatCapabilitiesDto`, `ChatComposingDto`), 12 `CoreRequest`s, 8 `CoreResponse`s, 4 `CoreEvent`s (`ChatMessageCommitted/Edited/Redacted`, `ChatComposingUpdated`); `sync(from_seq)` with `next_cursor`/`retention_floor_seq`/`resync_required`; duplicate retries emit no second event; failed sends emit nothing | pass | Additive surface; no `PROTOCOL_VERSION` bump; older clients ignore chat |
| Reference privacy/redaction/retention/audit integration and docs (package E) | Secret scan at write + hint sanitization; project-level reference gating with not-found denials; `audit_metadata_for_message` exposes structural locators only; `architecture/collaboration.md` + protocol/authorization/storage doc updates | pass | No live audit mapping by design (same convention as `chat_triggered_action`, owned by M003); separation proven by test |

## 3. Production implementation evidence

- `crates/codegg-core/src/collaboration.rs` (new, ~1500 lines with 14
  tests): domain types, validation/redaction, `CHAT_SCHEMA_STATEMENTS`,
  `channel_project` resolver, `CollaborationService` (durable pool +
  ephemeral `ComposingState`), transactional ordered sends with
  idempotency convergence and contention retries, author-only
  edit/redact with revision history, forward-only read markers,
  retention window, bounded history/sync with resync signal,
  structural audit-metadata hook.
- `crates/codegg-core/src/identity.rs`: new `ChatMessageId` typed
  identity (opaque, lexical contract, round-trip tests).
- `crates/codegg-core/src/session/schema.rs` + `storage/mod.rs`:
  `migrate_v55` + `STORAGE_LAYOUT_VERSION` 55.
- `crates/codegg-protocol/src/core.rs`: `CHAT_CAPABILITY`/`CHAT_PROTOCOL_VERSION`,
  chat DTOs, 12 requests, 8 responses, 4 events (all `serde`
  backward-compatible defaults on new optional fields).
- `crates/codegg-core/src/authorization.rs`: 12 `chat_*` operation
  descriptors + representative requests (matrix grows 138 → 150 rows;
  `project.chat` for all project-scoped ops).
- `crates/codegg-core/src/projection_replay/safe_publication.rs`:
  chat events classify `Safe` (redacted content to authorized project
  subscribers; hints carry identity/revision only).
- `src/core/daemon.rs`: daemon-owned `Arc<CollaborationService>`
  (fresh instance per daemon drops composing on restart);
  `chat_channel_id_for_request` + channel→project resolution in
  `resolve_authorization_project`; chat denials mapped to
  `project_not_found`; dedicated boxed `handle_chat_request` (keeps
  the main dispatch future small) with transport-derived principals,
  typed error codes, and event publication for commits/edits/
  redactions/composing hints.
- `tests/collaboration_m001_chat.rs` (new, 12 tests): capabilities,
  ensure/send/history/sync, reply/edit/redact/revision flow,
  idempotent retry, outsider+viewer non-enumeration, unknown-channel
  indistinguishability, cross-project isolation, composing/read
  markers, two-daemon restart, 4-way concurrent sends, bounds,
  secret redaction, free-text inertia (6/6 bodies verbatim, history
  exact), audit separation via maintainer `audit.read` page.
- Docs: `architecture/collaboration.md` (contract, failure/recovery,
  migration, security, testing); `protocol.md` (`chat.v1` summary);
  `authorization.md` (12 matrix rows + not-found convention);
  `storage.md` (v51–v55 entries).

Distinguished as absent (downstream, not M001 scope): TUI chat
rendering and observer input routing (M002), structured chat actions
with canonical task/job linkage (M003), DMs, external bridges,
membership administration, channel rename/delete.

## 4. Verification executed

All local (no hosted `CI / verify` claimed).

### Commands run

```bash
cargo test -p codegg-core collaboration
cargo test --test collaboration_m001_chat
cargo test -p codegg-core --lib
cargo test -p codegg-protocol
cargo test --test presence_m001_leases --test presence_m002_collaborators --test identity_m003_daemon_authorization --test identity_m004_audit_foundation
cargo test --test presence_m003_observation --test identity_m005_audit_instrumentation
RUST_MIN_STACK=16777216 cargo test -p codegg --lib
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
python3 scripts/check_authorization_matrix.py
bash scripts/check-core-boundary.sh
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
scripts/verify.sh quick
```

### Results

- `codegg-core collaboration`: 14 passed (schema/redaction/bounds,
  default+named channel idempotency, ordering+idempotency, reply/
  mention/reference linkage + cross-channel rejection, edit/redact/
  conflicts + 3-row history, retention + resync, markers, composing
  expiry/clear, secret persistence, restart order/markers with
  composing dropped).
- `collaboration_m001_chat`: 12 passed, stable across 5 consecutive
  runs (matrix §2, last row set).
- `codegg-core --lib`: 625 passed (611 pre-existing + 14 new).
- `codegg-protocol`: 177 passed.
- Presence M001/M002, identity M003/M004 regression suites: 15/12/9/13
  passed respectively, zero failures; presence M003 (11 passed) and
  identity M005 (13 passed) regression suites green.
- `codegg --lib`: 4408 passed, 0 failed (with the repo-precedent
  `RUST_MIN_STACK=16777216` debug-harness workaround).
- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: pass
  (after factoring the 16-tuple message row into `MessageRow` and
  boxing the chat resolver error for the large-error lint).
- All four static guards: pass (authorization matrix covers all 12
  `chat_*` operations; core boundary holds — collaboration is
  UI/server/plugin/auth-free).
- `scripts/verify.sh quick`: passed (fmt, builtin-agents check,
  core-boundary, sandbox contract, execution-ownership,
  `cargo check --workspace --all-targets --locked`).

Plan §11 lists `cargo test -p codegg-core chat` and
`cargo test --workspace collaboration --no-fail-fast`: no `chat`
module and no `collaboration` workspace crate exist, so the
justified substitutes above were used (same convention as the
presence M003 closure). No verification was skipped without a
substitute.

## 5. Invariant review

| Plan invariant | Evidence | Result |
|---|---|---|
| Project authorization guards channel enumeration/content/references | All project-scoped ops `DirectProject` + `project.chat`; channel→project resolved server-side twice (gate + dispatch); outsider+viewer non-enumeration pinned (5 surfaces × 2 roles) | pass |
| Message IDs/idempotency prevent retry duplication | `(channel, key)` unique index; duplicate returns original with `duplicate: true`, no new seq, no second event; pinned unit + boundary | pass |
| Edits/redactions preserve append-only audit history | `chat_revision` rows per revision (3-row proof), survive retention pruning; redacted messages refuse further edits | pass |
| Large/secret content uses redacted bounded text or handles | 8 KiB bound with `chat_body_too_large`; secret values redacted pre-write (key=value + token prefixes); hints truncated+scanned | pass |
| Typing is ephemeral | In-memory leases, 30s TTL, single eviction path, restart drops (two-daemon proof); DTOs carry no content | pass |
| Free text has no execution semantics | Handler never parses bodies; 6 command-like bodies round-trip verbatim with exact history and no side effect | pass |

## 6. Failure and recovery review

- Duplicate delivery: idempotent sends converge on the stored row
  (pinned, incl. contention-retry path); duplicate retries publish no
  event.
- Cancellation races: no cancellable chat operation exists; read
  markers are forward-only so stale writers cannot regress state
  (pinned).
- Daemon restart: durable order/markers survive via the catalog pool;
  composing drops via a fresh service (pinned by a two-daemon test);
  retention floor resumes via bounded resync.
- Contention: transactional `MAX(seq)+1` with unique backstop and a
  10-round retry budget; 4-way concurrent sends converge on seq
  1–4, stable across 5 runs (one pre-fix flake at a 2-retry budget
  is recorded in §10).
- Malformed input: empty/oversized/NUL bodies, bad locators,
  cross-channel reply targets, overlong names/keys all fail typed
  with zero side effect (pinned).
- Bounded behavior: 100-row pages, 16 channels/project, 1000
  messages/channel, content-free composing/audit surfaces.

## 7. Migration and compatibility review

- No durable migration beyond the additive v55 tables: `IF NOT
  EXISTS`, restart-safe, no backfill (no legacy chat data exists).
  `STORAGE_LAYOUT_VERSION` 54 → 55.
- Protocol is purely additive: `chat.v1` capability, no
  `PROTOCOL_VERSION` bump, `serde` defaults on new optional fields.
  Older clients ignore chat; older daemons answer capability
  `supported: false` by absence of the handler (no old-daemon test
  matrix exists; the surface is new).
- Config: no new settings surface; bounds are code constants via
  `CollaborationConfig`.
- Rollback: dropping the binary leaves v55 tables unused; forward
  migration is idempotent.

## 8. Security review

- Authorization precedes lookup on every chat path; team principals
  fail closed on unknown channels; local-owner broad policy is the
  only project-less path (reports typed `chat_channel_not_found`).
- Principals derive from transport authority; DTOs carry locators
  only (no principal/role/capability fields exist).
- Privacy: all eleven project-scoped denials map to
  `project_not_found`; members probing unknown channels observe the
  same shape (pinned); stop/clear drops composing state.
- Secrets: pre-write redaction for bodies and hints; audit pages
  proven free of chat bodies and secrets via a maintainer
  `audit.read` query; banners/events carry ids/revisions only.
- Path validation: channel/message locators are opaque identities
  (never paths); reference targets obey the identity lexical
  contract.
- DoS bounds: body/mention/reference/channel/page/retention/TTL caps
  (§2); no per-message tasks; failed sends emit nothing.
- Audit: chat has no live audit mapping by design; denial path flows
  through the existing denial-audit seam; chat storage is separate
  from the audit store (pinned).

## 9. Documentation and operations

- New: `architecture/collaboration.md` (contract, reference/
  redaction model, protocol, authorization, audit separation,
  failure/restart, migration, security, testing).
- Updated: `architecture/protocol.md` (`chat.v1` surface),
  `architecture/authorization.md` (12 matrix rows, 138 → 150
  operations, not-found convention), `architecture/storage.md`
  (v51–v55 entries).
- Operator diagnostics: typed `chat_*` error codes
  (`chat_channel_not_found`, `chat_revision_conflict`,
  `chat_not_author`, `chat_body_too_large`, `chat_unavailable`,
  …); `ChatCapabilities` advertises live bounds; composing/read
  state re-fetches through authorized list/history/sync paths.
- Static guards: authorization-matrix, core-boundary,
  execution-ownership, scheduler-bypass all green; no new CI lane
  added per verification policy.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Initial 2-retry contention budget flaked once under a 4-way same-thread burst (all writers read the same `MAX(seq)` twice) | Test-harness only: one `collaboration_m001_chat` failure in six pre-fix runs; no production corruption (losers error, never duplicate) | Fixed in-tree: 10-round retry budget with re-read per round; 5/5 green post-fix. No further action. |

There are no unresolved critical, high, or medium M001 findings.

## 11. Roadmap disposition

Milestone closed. Dependency audit: project-collaboration M002 (TUI
chat) hard-depends on M001 with transitive presence-observation M003
(closed) and multi-project TUI (closed) — no other open hard
dependency, so M002 is unblocked to `ready`. M003 (structured chat
actions) hard-depends on M001 with transitive identity/audit M005
(closed) and already-closed agent-run/job owners as consumers — no
other open hard dependency, so M003 is unblocked to `ready`. M002
and M003 may then proceed in parallel per the roadmap dependency
graph. No corrective follow-up is required; no new
dependency-ready plan was created.

## 12. Registry updates

- `plans/registry.md`: collaboration row `active / M001 ready` →
  `active / M001 closed, M002+M003 ready`; dependency-ready table:
  remove the M001 channel/message-protocol row, add M002 TUI-chat
  and M003 structured-actions rows (both unblocked by this
  closure); execution order §4 reworded (M001 closed; M002/M003 may
  proceed in parallel); blocked work: remove both
  collaboration-M002/M003-on-M001 rows; closure control points: add
  M001 closed row.
- `plans/subsystems/project-collaboration-roadmap.md`: Status stays
  `active`; M001 `ready` → `closed` with closure link; M002/M003
  `blocked` → `ready` with plan links.
- `plans/implementation/project-collaboration/001-project-channel-message-protocol.md`:
  `ready for handoff` → `implemented` with closure link.
- `plans/implementation/project-collaboration/002-tui-project-chat-and-observer-routing.md`:
  `blocked` → `ready for handoff` with unblocked-by-M001 note.
- `plans/implementation/project-collaboration/003-structured-chat-actions.md`:
  `blocked` → `ready for handoff` with unblocked-by-M001 note.
