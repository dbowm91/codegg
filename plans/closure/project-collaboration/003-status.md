# Project Collaboration Milestone 003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/project-collaboration/003-structured-chat-actions.md`

Source subsystem roadmap:

- `plans/subsystems/project-collaboration-roadmap.md#M003--separately-authorized-structured-chat-actions`

Repository baseline reviewed: `550b29ad632fec78021558320de1a8fea2e11311`

Implementation commits or pull requests:

- (this commit) — feat(collaboration): M003 separately authorized structured chat actions (action taxonomy, chat_action table v56, daemon dispatcher with dual-capability gate and scheduler-boundary submits, audit causation, TUI explicit commands, 3 unit + 10 integration tests, docs).

## 1. Executive finding

M003 is closed. An authorized project member can invoke an explicit
typed message-associated action — agent task, review request, generic
job submit, or job reference — through the existing daemon
authorization, durable job, and audit owners, while free-text messages
(including command-like text) never execute privileged work. The
action taxonomy maps each kind to its ordinary semantic capability
(`agent.delegate` / `job.submit` / `session.read`) plus `project.chat`;
retries converge on `(channel, idempotency_key)` with exactly one
canonical job; message/project mismatches, denials, and secret-bearing
payloads create nothing; audit links message -> decision -> action ->
job with structural locators only. The TUI exposes explicit
`/chat-action-task`, `/chat-action-review`, and `/chat-action-list`
commands with the standard stale-completion guard; observer insert
input still routes literally to `ChatSend` and never constructs an
action. No unresolved high, medium, or low M003 finding remains.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Action taxonomy mapped to existing capability/canonical service (package A) | `ChatActionKind::{AgentTask,ReviewRequest,JobSubmit,JobReference}` in `crates/codegg-core/src/collaboration.rs` with `required_capability()` (`agent.delegate`/`agent.delegate`/`job.submit`/`session.read`); wire `ChatActionKindDto`/`ChatActionSubmitDto` in `crates/codegg-protocol/src/core.rs` (agent_task/review → `JobKind::Subagent` via `JobSubmissionService`, job_submit → generic `JobSubmitDto` via the same boundary with `ToolProgram` rejected, job_reference → `JobStore::get_job` read with no new execution) | pass | No new workflow engine; no new agent/job semantics |
| Daemon action dispatcher with message/project validation, authorization, idempotency (package B) | `src/core/daemon.rs::handle_chat_action_submit` (+ `action_capability_allows`, `action_session_project`): channel→project server-side resolution, message-in-channel linkage (`chat_project_mismatch` on cross-link), `find_action_by_idempotency` fast-path with kind/message conflict check, payload validation (bounds + secret rejection), dual-capability gate (`project.chat` at the M003 gate + semantic recheck), `JobSubmissionService::submit` with chat-derived submission key, `insert_action` unique backstop with winner re-read | pass | Free text never reaches this path; only `ChatActionSubmit` can create work |
| Persist/project action result references and audit causation; surface status in chat (package C) | `chat_action` table (`migrate_v56`, `STORAGE_LAYOUT_VERSION` 55 → 56, `CHAT_ACTION_SCHEMA_STATEMENTS` + `insert/find/get/list` in `collaboration.rs`); `audit_metadata_for_action` (structural locators only); `chat_triggered_action_event` builder live-mapped (`audit_instrumentation.rs`: coverage entry `future:collaboration` → `daemon:collaboration`, `live_mapped: false` → `true`, `INSTRUMENTED_OPERATIONS` += `chat_action_submit`, `UNINSTRUMENTED_OPERATIONS` += `chat_action_get/list`); daemon post-creation `chat_triggered_action` + `job_submit` + `JobCreated` + `ChatActionUpdated` emission; `ChatActionGet/List` status projection | pass | Chat stores only the reference/status projection; jobs stay canonical in scheduler/job stores |
| TUI explicit invocation ergonomics and observer capability behavior (package D) | `src/tui/command.rs`: 3 new slash commands (registry 136 → 139); `src/tui/app/state/observe.rs`: action commands allowlisted (daemon enforces the semantic capability); `src/tui/commands/chat.rs`: `start_chat_action_task/review/list` + `apply_chat_action_submitted/list_loaded` + `on_chat_action_updated` with the standard request-id/epoch stale guard; `src/tui/app/state/chat.rs`: bounded action projection (50/channel) + `action_line` rendering + panel footer; `src/tui/app/mod.rs` + `runtime/command_dispatch.rs`: `ChatActionSubmitted/ListLoaded/Hint` completions | pass | Explicit commands only; `route_observer_insert_to_chat` unchanged (zero-control proof intact) |
| Injection/retry/revocation/cancellation/end-to-end tests (package E) | `tests/collaboration_m003_chat_actions.rs` (10 tests) + 3 new `collaboration.rs` unit tests (see §4) | pass | Matrix §10 covers every plan-required case |

## 3. Production implementation evidence

- `crates/codegg-protocol/src/core.rs`: `ChatActionKindDto`,
  `ChatActionSubmitDto` (tagged `kind` enum; `JobSubmit.spec` boxed for
  the large-variant lint), `ChatActionDto`, `actions_supported` +
  action bounds on `ChatCapabilitiesDto` (serde defaults, version 1
  unchanged), `CoreRequest::ChatActionSubmit/Get/List`,
  `CoreResponse::ChatAction/ChatActionList`,
  `CoreEvent::ChatActionUpdated` (full action to `project.chat`
  subscribers; prompts stay job-side).
- `crates/codegg-core/src/collaboration.rs`: `ChatActionKind`
  (+ `required_capability`, `parse` closed on unknown),
  `ChatAction`/`ActionOutcome`, `CHAT_ACTION_STATUS_*`,
  `validate_action_title/prompt/agent/workspace/job_id` (bounds +
  NUL/control + credential rejection, zero side effect),
  `audit_metadata_for_action` (no titles/prompts),
  `CHAT_ACTION_SCHEMA_STATEMENTS` + `ensure_collaboration_tables`
  extension, `ActionRow` alias, `find/insert/get/list/count` with
  channel/project binding and cross-channel `ProjectMismatch`;
  `CollaborationConfig` gains action bounds;
  `CollaborationError` gains `ActionNotFound`/`ActionConflict`/
  `ActionDenied`/`ProjectMismatch` with stable `chat_action_*` codes;
  `validate_idempotency_key` + `invalid` made public for the daemon
  boundary.
- `crates/codegg-core/src/session/schema.rs` + `storage/mod.rs`:
  `migrate_v56` + `STORAGE_LAYOUT_VERSION` 56.
- `crates/codegg-core/src/authorization.rs`: 3 `chat_action_*`
  operation descriptors (all `DirectProject` + `project.chat`) +
  representative requests (matrix 150 → 153 rows).
- `crates/codegg-core/src/projection_replay/safe_publication.rs`:
  `ChatActionUpdated` classifies `Safe` (bounded projection to
  authorized project subscribers).
- `crates/codegg-core/src/audit_instrumentation.rs`:
  `chat_triggered_action_event` builder + coverage entry promotion to
  live-mapped with action/job/status metadata.
- `src/core/daemon.rs`: `chat_channel_id_for_request` +
  `is_chat_request` extended; `handle_chat_request` takes the gate
  `authority`/`decision`/`request_id`; new `handle_chat_action_submit`
  (boxed `PendingAction::SubmitJob` with boxed `NewJob` for the
  large-variant lint, chat-derived submission key, origin attribution
  mirroring `JobSubmit`, dual `ChatActionUpdated`/`JobCreated`
  publication, post-creation `chat_triggered_action` + `job_submit`
  audit); `action_capability_allows` (local-owner broad passes, else
  membership grant); `action_session_project` (fail-closed `None`);
  `authorization_denial` maps action denials to `project_not_found`;
  `emit_audit_for_authorized` skips `ChatActionSubmit` (post-creation
  owns it).
- `src/tui/…`: as listed in §2 package D (139 commands; observer
  allowlist gains the 3 action commands; reducer/panel/dispatch/help
  updated).
- `tests/collaboration_m003_chat_actions.rs` (new, 10 tests): free-text
  corpus (6 bodies verbatim, 0 jobs, 0 actions), role matrix
  (Contributor job-submit allow / agent-task `chat_action_denied`,
  Maintainer agent/review allow, Viewer/outsider `project_not_found`),
  message/project mismatch, duplicate + cross-message conflict,
  two-daemon restart convergence, revocation race, reference (no new
  execution) + unknown-job not-found + canonical cancel with history
  intact, audit linkage (channel/message/action/job present, no
  prompt/title), secret rejection (prompt/title/ToolProgram) + clean
  audit, list/get status projection.
- Docs: `architecture/collaboration.md` (M003 section + testing),
  `architecture/protocol.md` (action surface),
  `architecture/authorization.md` (14 chat denials, 153 ops, 3 new
  rows), `architecture/storage.md` (v56 entry),
  `architecture/audit.md` (`chat_triggered_action` live),
  `architecture/tui.md` (action commands + panel),
  `docs/execution-ownership.toml` (daemon chat-action submission
  noted as `JobSubmissionService`).

Distinguished as absent (downstream, not M003 scope): DMs, external
bridges, membership administration, channel rename/delete,
multi-channel TUI windows, arbitrary shell execution from chat,
natural-language parsing into actions, custom workflow engine, new
agent/job semantics, session-less cross-project job references beyond
chat-gating (per-object session recheck only when a session is
present).

## 4. Verification executed

All local (no hosted `CI / verify` claimed).

### Commands run

```bash
cargo test -p codegg-core --lib collaboration
cargo test -p codegg-protocol
cargo test --test collaboration_m001_chat --test collaboration_m003_chat_actions
cargo test --test collaboration_m002_chat_tui --test presence_m002_collaborators --test presence_m003_observation --test identity_m003_daemon_authorization --test identity_m004_audit_foundation --test identity_m005_audit_instrumentation
cargo test -p codegg --lib tui::app::state::chat
cargo test --test tui --test tui_render --test tui_project_tabs --test tui_project_routing
RUST_MIN_STACK=16777216 cargo test -p codegg --lib
cargo test -p codegg-core --lib
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
python3 scripts/check_authorization_matrix.py
bash scripts/check-core-boundary.sh
python3 scripts/check_execution_ownership.py
python3 scripts/check_scheduler_bypass.py
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/generate_builtin_agents.py --check
scripts/verify.sh quick
```

### Results

- `codegg-core --lib collaboration`: 17 passed (14 pre-existing +
  3 new: kind/capability mapping, secret rejection, insert/find/
  mismatch/audit-metadata).
- `codegg-protocol`: 177 passed.
- `collaboration_m001_chat`: 12 passed (ordering/retry/restart/
  privacy/audit-separation intact).
- `collaboration_m003_chat_actions`: 10 passed (matrix §2, last row
  set).
- `collaboration_m002_chat_tui`: 11 passed; `presence_m002` 12,
  `presence_m003` 11, `identity_m003` 9, `identity_m004` 13,
  `identity_m005` 13 — all passed, zero failures (observer read-only
  policy intact with action commands allowlisted; audit
  instrumentation now live-maps `chat_triggered_action`).
- `codegg --lib tui::app::state::chat`: 16 passed (existing reducer
  suite; new action methods compile under the same bounds).
- TUI suites: `tui` 164, `tui_project_routing` 27,
  `tui_project_tabs` 20, `tui_render` 99 — all passed.
- `codegg --lib`: 4424 passed, 0 failed (with the repo-precedent
  `RUST_MIN_STACK=16777216` debug-harness workaround).
- `codegg-core --lib`: 628 passed (625 pre-existing + 3 new).
- `cargo fmt --all -- --check`: pass.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: pass
  (after boxing `JobSubmit.spec` and `PendingAction.new_job` for the
  large-variant lint and factoring the `ActionRow` alias for the
  complexity lint).
- All static guards: pass (authorization matrix covers all 3 new
  `chat_action_*` operations; core boundary holds — collaboration is
  UI/server/plugin/auth-free; execution-ownership/scheduler-bypass/
  cwd clean; builtin agents fresh).
- `scripts/verify.sh quick`: passed (fmt, builtin-agents check,
  core-boundary, sandbox contract, execution-ownership,
  `cargo check --workspace --all-targets --locked`).

Plan §11 lists `cargo test --workspace collaboration/agent/jobs/audit
--no-fail-fast`: no `collaboration`, `agent`, `jobs`, or `audit`
workspace crates exist, so the justified substitutes above were used
(same convention as the M001/M002 closures: focused core lib +
daemon boundary + regression suites + guards). No verification was
skipped without a substitute.

## 5. Invariant review

| Plan invariant | Evidence | Result |
|---|---|---|
| Free-text body never becomes a command by parsing, mention, prefix, or model inference | Handler has no text parsing: only `ChatActionSubmit` creates work; 6-body corpus round-trips verbatim with 0 jobs/0 actions; observer insert path still issues only `ChatSend` (M002 zero-control test green) | pass |
| Structured action is a separate explicit protocol operation with capability check and idempotency key | `ChatActionSubmit` names channel/message + typed payload + key; gate enforces `project.chat`, handler rechecks `agent.delegate`/`job.submit`/`session.read`; `(channel, key)` unique backstop with `duplicate` convergence and `ActionConflict` on reuse | pass |
| Resulting task/run/job is canonical durable state; chat stores only reference/status projection | Submits go through `JobSubmissionService::submit` (scheduler authority); `chat_action` stores ids/kind/title/status only; prompts stay job-side; reference kind creates no job (pinned) | pass |
| Audit links message -> auth decision -> action -> task/run/job | Post-creation `chat_triggered_action` event carries channel/message/action/kind/job + decision provenance; `job_submit` + `JobCreated` mirrored; audit test pins all four locators and the absence of prompts/titles | pass |
| Observer read-only rules remain unless caller separately possesses the action capability | TUI allowlists action commands (same as chat); daemon denies observers without the semantic capability (`project_not_found` at the gate for Viewers/outsiders, `chat_action_denied` for chat-holders lacking `agent.delegate`); revocation race pinned | pass |

## 6. Failure and recovery review

- Duplicate delivery: idempotency-row fast-path plus unique backstop
  with winner re-read; duplicate retries publish no second event and
  emit no second audit (pinned, incl. cross-message conflict).
- Revocation race: post-revocation submits deny at the gate
  (`project_not_found`) with zero side effect (pinned).
- Daemon restart: durable actions/jobs survive via the catalog pool;
  a fresh daemon converges retries on the stored row (pinned by a
  two-daemon test); composing still drops (M001 behavior unchanged).
- Contention: concurrent same-key submits serialize on the unique
  index; submission-key fingerprint conflicts map to
  `chat_action_conflict` (never a second job).
- Canonical failure/cancel: once the job exists its own
  cancel/retry/recovery semantics apply (pinned via `JobCancel`;
  chat projection still links the same job and history is intact).
- Malformed input: empty/oversized/NUL/control bodies, bad locators,
  cross-channel message linkage, overlong keys/titles/prompts, and
  secret-bearing titles/prompts all fail typed with zero side effect
  (pinned); `ToolProgram` smuggling rejected.
- Bounded behavior: 512-byte titles, 8 KiB prompts, 100-row action
  pages, 50-row TUI action window, content-free composing/audit
  surfaces.

## 7. Migration and compatibility review

- Durable migration is additive v56 (`chat_action` + indexes, all `IF
  NOT EXISTS`, restart-safe, no backfill — no legacy action data
  exists). `STORAGE_LAYOUT_VERSION` 55 → 56.
- Protocol is additive: `chat.v1` stays version 1 with new
  `actions_supported`/bound capability fields (serde defaults);
  `ChatAction*` requests/responses/events are new variants older
  clients ignore; no `PROTOCOL_VERSION` bump. Older daemons answer
  action requests with `unsupported`/unavailable and the TUI hides
  affordances; older clients render messages without action UI.
- No text syntax is reserved as executable compatibility behavior.
- Config: no new settings surface; bounds are code constants via
  `CollaborationConfig`.
- Rollback: dropping the binary leaves v56 tables unused; forward
  migration is idempotent.

## 8. Security review

- Authorization precedes lookup on every action path; team principals
  fail closed on unknown channels/messages; local-owner broad policy
  is the only project-less path.
- Principals derive from transport authority; DTOs carry locators
  only (no principal/role/capability fields exist).
- Privacy: project-scoped denials map to `project_not_found`;
  cross-project job references fail closed as `chat_action_not_found`;
  message/channel mismatch uses `chat_project_mismatch` without
  leaking which side exists.
- Secrets: titles/prompts scanned pre-write and rejected on hit
  (fail closed, no job, no row, no audit); message bodies keep the
  M001 redact-before-write convention; audit pages proven free of
  prompts/titles/secrets via a maintainer `audit.read` query.
- Path validation: channel/message/action/job locators are opaque
  identities (never paths); agent/workspace names obey the identity
  lexical contract.
- DoS bounds: title/prompt/agent/key/page caps (§2); no per-message
  tasks; failed submits emit nothing; duplicate retries emit nothing.
- Audit: `chat_triggered_action` is now live-mapped with structural
  locators only; denial path flows through the existing denial-audit
  seam; chat storage remains separate from the audit store.

## 9. Documentation and operations

- Updated: `architecture/collaboration.md` (M003 section: taxonomy,
  dispatcher, storage, audit, TUI, testing), `architecture/protocol.md`
  (action surface), `architecture/authorization.md` (14 chat denials,
  153 ops, 3 new rows), `architecture/storage.md` (v56 entry),
  `architecture/audit.md` (`chat_triggered_action` live),
  `architecture/tui.md` (action commands + panel),
  `docs/execution-ownership.toml` (daemon chat-action submission
  noted as `JobSubmissionService`).
- Operator diagnostics: typed `chat_action_*` error codes
  (`chat_action_not_found/conflict/denied`, `chat_project_mismatch`,
  `chat_action_submit_failed`); `ChatCapabilities` advertises action
  bounds; action failures toast with no fabricated row; `⚡ id kind
  job [status] title` panel lines; stale/resync notes shared with
  chat.
- Static guards: authorization-matrix, core-boundary,
  execution-ownership, scheduler-bypass, daemon-cwd, builtin-agents
  all green; no new CI lane added per verification policy.

## 10. Unresolved findings

There are no unresolved critical, high, medium, or low M003 findings.

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| — | None | — | — |

One pre-existing test-harness note (not an M003 finding): the
`codegg --lib` suite requires the repo-precedent
`RUST_MIN_STACK=16777216` workaround in debug harness mode (same as
the M001/M002 closures); no production impact.

## 11. Roadmap disposition

Milestone closed. The collaboration subsystem is now complete:
M001 (channel/message/sync) + M002 (TUI chat) + M003 (structured
actions) are all closed, satisfying the roadmap completion definition
(authorized project communication is durable and synchronized;
observer chat UX works; structured actions are explicit, separately
authorized/idempotent/audited; chat remains distinct from audit and
execution). Dependency audit: no registered implementation plan lists
collaboration M003 as a hard or interface dependency (residual M003
waits on residual M002; tool-program expansion M002/M003 wait on TP
expansion M001), so no further plan moves from `blocked`/`proposed`
to `ready` in this commit. No corrective follow-up is required; no
new dependency-ready plan was created.

## 12. Registry updates

- `plans/registry.md`: collaboration row `active / M001+M002 closed,
  M003 ready` → `closed / M001+M002+M003 closed`; dependency-ready
  table: remove the M003 structured-actions row (implemented);
  execution order §4 reworded (M001+M002+M003 closed; subsystem
  complete); blocked work: unchanged (no entry waited on M003);
  closure control points: add M003 closed row.
- `plans/subsystems/project-collaboration-roadmap.md`: Status
  `active` → `closed`; M003 `ready` → `closed` with closure link;
  milestone table notes subsystem complete.
- `plans/implementation/project-collaboration/003-structured-chat-actions.md`:
  `ready for handoff` → `implemented` with closure link.
