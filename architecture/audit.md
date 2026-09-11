# Audit Foundation and Instrumentation (Identity M004/M005)

Identity, authorization, and audit M004. The coordinator owns an
append-only structural audit record/store with typed attribution,
bounded redacted metadata, deterministic ordering/idempotency,
authorized bounded query/pagination/filter/export, and
content-retention separation. M005 (instrumentation) consumes this
store; this document owns the store contract plus the M005 coverage
contract (action/owner matrix, correlation/causation, bounded
best-effort emission, redaction, backpressure, and operator reads).

## Ownership

- Coordinator (daemon) is the single sequence authority. Leaf/node
  sequencing is out of scope for this milestone (single-host SQLite
  deployment); future distributed work assigns the global deployment
  sequence at the coordinator while leaves keep node-local source
  sequences with idempotent at-least-once acceptance.
- Canonical implementation: `crates/codegg-core/src/audit.rs`
  (`AuditStore`, `AuditEventBuilder`, `AuditWriter`).
- Storage: introduced by migration v54 (storage layout 54), which creates
  `audit_event` and `audit_body`.
- Protocol: `AuditQueryRequestDto`, `AuditExportRequestDto`,
  `AuditEventDto`, `AuditCapabilitiesDto` plus
  `CoreRequest::AuditQuery/AuditExport/AuditCapabilities` and
  `CoreResponse::AuditPage/AuditExport/AuditCapabilities` in
  `crates/codegg-protocol/src/core.rs`.
- Authorization: `audit_query`/`audit_export` are `DirectProject` +
  `audit.read` in `operation_descriptor`; `audit_capabilities` is
  global. Daemon dispatch in `src/core/daemon.rs` runs through the
  standard M003 gate before touching the store.

## Schema

`audit_event` (append-only; no `UPDATE`/`DELETE` path in production):

- `seq INTEGER PRIMARY KEY AUTOINCREMENT` — coordinator sequence,
  restart-safe via `sqlite_sequence`, unique under concurrency.
- `event_id TEXT UNIQUE` — idempotency key (`AuditEventId`);
  retransmission returns the stored row, never a second sequence.
- `action`, `visibility`, `actor_principal/kind`,
  `auth_method`, `transport_class`, `policy`, `decision_id`,
  `correlation_id`, `causation_parent`.
- Correlation locators: `project_id`, `session_id`, `turn_id`,
  `run_id`, `job_id`, `worktree_id`, `provider_connection_id`.
- `metadata_json` + `metadata_digest` (SHA-256 over sorted
  `key=value` lines), `content_digest` (SHA-256 of the body),
  `body_ref`, `body_expires_at`, `time_created`.
- Indexes on `(project_id, seq)`, `(actor_principal, seq)`,
  `(action, seq)`, `decision_id`.

`audit_body` (separate retention):

- `body_ref PRIMARY KEY`, `event_id`, `content_digest`,
  `body BLOB`, `byte_length`, `expires_at`, `time_created`.
- Indexes on `event_id` and `expires_at`.

## Action and visibility taxonomy

Known actions (`AuditAction::ALL`, 25): `authentication`,
`authorization_decision`, `membership_change`, `node_enrollment`,
`session_create`, `session_attach`, `prompt_submit`,
`provider_select`, `model_select`, `agent_delegate`,
`permission_decision`, `tool_invoke`, `command_execute`,
`file_mutate`, `git_operation`, `worktree_lifecycle`, `job_submit`,
`job_cancel`, `job_complete`, `remote_execute`,
`chat_triggered_action`, `config_change`, `asset_refresh`,
`audit_export`, `audit_query`.

Writers use strict parsing (unknown actions fail closed at build
time). Readers use lenient parsing: unknown stored actions degrade to
`Unknown` and unknown filters degrade to empty pages — never to an
error that leaks existence.

Visibility (`AuditVisibility`): `project`, `session_participants`,
`actor_only`, `administrators`. The default is `project`.

## Attribution (caller cannot rewrite)

`AuditEventBuilder::new(action, principal, decision)` copies the
transport-bound `AuthenticatedPrincipal` plus the captured M003
`AuthorizationDecision`. There are no setters for actor, decision,
policy, or sequence. Correlation defaults to the decision's
correlation id. This preserves the M003 invariant: request payloads
supply locators, never authority.

## Bounded redacted metadata

- At most 16 entries; keys 1–64 bytes matching
  `[a-z0-9][a-z0-9_.-]*`; values 0–512 bytes; 4096 bytes total.
- Keys containing secret-bearing substrings (`password`, `secret`,
  `token`, `api_key`, `bearer`, `credential`, `private_key`,
  `cookie`, `authorization`, …) are rejected.
- Values scanning as credential text (`-----BEGIN`, `ghp_`,
  `sk-live`, `AKIA`, `aws_secret`, `password`, `token=`, …) are
  rejected, as are bodies that scan as credential text.
- Rejection surfaces `audit_secret_detected` before any write; the
  secret never reaches storage or export. Metadata carries structural
  IDs, digests, and bounded labels — never prompts, file content,
  tool output, or credentials.

## Content digest and retention separation

Bodies are opaque bytes (max 64 KiB) referenced by SHA-256 digest.
`expire_bodies(now_ms)` deletes `audit_body` rows at or before the
cutoff and reports the count; structural rows, metadata, and digests
are preserved. Operators expire content on their own schedule without
losing attributable structure.

## Query, pagination, filter, export

- `AuditQueryFilter { project_id, action, principal, from_seq, limit }`.
  Query limits clamp to 1–100 (default 50); export envelopes clamp to
  200 events.
- Pages order by `seq ASC` with cursor pagination (`next_cursor`,
  `truncated`). Filters are conjunctive; unknown action/principal
  filters return empty pages.
- `export_digest` is SHA-256 over the canonical per-event line
  `seq:event_id:action:actor:decision:metadata_digest:content_digest`.
  Reordering or tampering changes the digest.
- Authorization: team principals MUST scope reads to one project and
  hold `audit.read` there (`Maintainer`/`Owner` by default;
  `Viewer`/`Contributor` are denied). `LocalOwner` broad policy may
  query across projects. Unscoped team queries fail closed with
  `authorization_denied`. Single-project denials carry no existence
  signal beyond the operation/capability names the caller supplied.
- Capability negotiation: `AuditCapabilities` reports
  `max_query_limit`, `max_export_events`, `max_metadata_entries`,
  `max_metadata_total_bytes`, `max_body_bytes`. Clients MUST clamp to
  these bounds before sending.

## Failure, backpressure, observability

- `AuditStore::append` runs in one transactional statement with the
  pool's busy timeout; it never blocks forever and never fabricates
  success — storage errors surface as typed `AuditError`.
- `AuditWriter` bounds in-flight appends with a semaphore (default
  32) and a per-write timeout (default 2000 ms).
  `try_append` fails fast with `audit_backpressure` when saturated;
  `append` bounds permit acquisition and the write with the same
  timeout (`audit_write_timeout`).
- `AuditWriterMetrics { appended, duplicates, failed,
  dropped_backpressure, timeouts, queue_depth }` exposes the failure
  and pressure surface. Both failure policies (`FailClosed`,
  `FailVisible`) report the error to the caller; neither invents an
  appended event.
- Attribution writes in the daemon remain best-effort (warn); audit
  appends themselves are explicit and their outcome is always visible
  to the caller.

## Security review

- Fail-closed reads; privacy-preserving denials; no secret material in
  metadata, bodies that scan as secrets, digests (SHA-256 only), or
  error messages.
- `codegg-core` boundary guard passes (no UI/server/plugin/auth
  imports); `scripts/check_audit_invariants.py` pins the migration,
  the no-`UPDATE`/`DELETE` rule for structural rows, the bounds and
  deny-list, the authorized read path, and this document.
- Existing logs/traces are not backfilled as authoritative audit.
  Unknown event actions/metadata degrade safely in readers.

## Operator runbook

- After upgrading, the daemon migrates the catalog to v54 on next
  start (additive; restart-safe; no backfill).
- Grant `Maintainer` or `Owner` to team members who need audit reads;
  `Viewer`/`Contributor` cannot query or export.
- Query with bounded pages (`limit` ≤ 100, resume via `next_cursor`);
  export bounded envelopes (≤ 200 events) and verify `digest` with
  `export_digest` over the received order.
- Expire bodies with `expire_bodies(now_ms)`; structural events and
  digests survive. Bodies are never required for attribution.
- Monitor `AuditWriterMetrics`: rising `dropped_backpressure` or
  `timeouts` means the writer is saturated — shed load or raise
  `max_inflight` deliberately; do not retry blindly with fresh event
  ids (reuse the id for idempotent retry).

## Instrumentation coverage (M005)

The required Phase-11 event-coverage matrix lives in
`crates/codegg-core/src/audit_instrumentation.rs`
(`REQUIRED_AUDIT_COVERAGE`, one row per `AuditAction::ALL`). Each row
names the canonical owner (never a duplicate wrapper), the trusted
actor source, the authorization scope, the representative decision
operation, the allowed structural metadata keys, the causation linkage,
and whether the daemon maps it live in this milestone.

Live daemon seam (`src/core/daemon.rs`):

- `emit_audit_for_authorized` runs after the M003 gate and before any
  side effect for every `audit_action_for_request` mapping except
  creation/audit-read operations that mint their identity or count in
  the handler (`SessionCreate`, `JobSubmit`, `AuditQuery`,
  `AuditExport`, which emit post-creation with durable ids/counts).
- `emit_audit_for_denial` emits one terminal `authorization_decision`
  event with the operation/capability the caller supplied and the
  denial reason. Direct project locators are preserved so
  project-scoped denials stay queryable by the project owner through
  the `audit.read` gate; unresolvable scopes stay `None` and never
  leak existence through the page itself.
- Post-creation emits: session creation (`SessionCreate`,
  `SessionImportData`, `SessionCreateFromTemplate`) with the durable
  session id; job submission with the durable job/session/turn linkage;
  provider selection with connection/model; audit query/export with
  the returned count (post-read so envelopes never contain their own
  event).
- All emits are best-effort and bounded: one 500 ms per-write timeout,
  no unbounded queue, never fail the operation. Outcomes are visible
  via `audit_instrumentation::emit_counters_snapshot`
  (`appended`/`failed`/`dropped_no_pool`) plus warn logs. High-volume
  reads/listings stay explicitly uninstrumented
  (`UNINSTRUMENTED_OPERATIONS`); the coverage guard pins that list so
  a new privileged operation cannot hide there.

Correlation and causation:

- Every event carries the gate-copied `decision_id` plus a
  `correlation_id` defaulting to the M003 decision correlation.
  Children preserve the parent correlation and point
  `causation_parent` at the parent `event_id`, so
  prompt -> root run -> child delegate -> tool/job -> Git/worktree
  chains reconstruct with one ordered project query.
- Retries reuse `deterministic_event_id(decision, action, correlation,
  scope)` so replays return the stored event instead of assigning a
  second sequence number.
- Builders accept only structural locators, SHA-256 digests, bounded
  labels, and outcome enums. Prompt/file/tool output bodies are never
  accepted; paths/argv/refs are digested. The underlying builder still
  rejects secret-bearing keys/values/bodies with
  `audit_secret_detected` before any write.

Explicit gaps (builders + store fixtures landed, no live single-host
emission by design):

- `node_enrollment`, `remote_execute` — future node protocol.
- `command_execute`, `git_operation` — execution-surface chains are
  proven via builders/fixtures in M005; the live tool-broker and
  git-executor hooks are deferred follow-ups.

Live in M003: `chat_triggered_action` is emitted by the daemon
collaboration owner for every authorized structured chat action
(message -> decision -> action -> job; structural locators only).

## Verification

```bash
cargo test -p codegg-core audit
cargo test --test identity_m004_audit_foundation
cargo test --test identity_m005_audit_instrumentation
cargo test --test storage_migrations
python3 scripts/check_audit_invariants.py --verbose
python3 scripts/check_audit_coverage.py --verbose
python3 scripts/check_authorization_matrix.py --verbose
python3 scripts/check_project_catalog_invariants.py --verbose
bash scripts/check-core-boundary.sh
cargo fmt --all -- --check
scripts/verify.sh quick
```
