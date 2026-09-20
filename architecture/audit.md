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

Known actions (`AuditAction::ALL`, 26): `authentication`,
`authorization_decision`, `membership_change`, `node_enrollment`,
`session_create`, `session_attach`, `prompt_submit`,
`provider_select`, `model_select`, `agent_delegate`,
`permission_decision`, `tool_invoke`, `command_execute`,
`file_mutate`, `git_operation`, `worktree_lifecycle`, `job_submit`,
`job_cancel`, `job_complete`, `remote_execute`,
`chat_triggered_action`, `config_change`, `asset_refresh`,
`audit_export`, `audit_query`, `work_order_lifecycle`.

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

## Instrumentation coverage (M005 closed; M004 qualification current)

The required Phase-11 event-coverage matrix lives in
`crates/codegg-core/src/audit_instrumentation.rs`
(`REQUIRED_AUDIT_COVERAGE`, one row per `AuditAction::ALL`). Each row
names the canonical owner (never a duplicate wrapper), the trusted
actor source, the authorization scope, the representative decision
operation, the allowed structural metadata keys, the causation linkage,
and whether the action is live in this milestone.

Coverage has four distinct categories (M004):

- daemon request operation mappings (`INSTRUMENTED_OPERATIONS`): the
  control-plane seam emits these live after the M003 gate;
- executor-owned live hooks (`EXECUTOR_LIVE_AUDIT_HOOKS`):
  `command_execute` at the canonical tool dispatch, `git_operation` at
  `GitMutationExecutor`, `job_complete` at the scheduler terminal
  transition — each with executable owner evidence, never satisfied by a
  daemon operation row or by listing the name in
  `UNINSTRUMENTED_OPERATIONS`;
- intentionally future/distributed actions
  (`FUTURE_DISTRIBUTED_AUDIT_ACTIONS`): `node_enrollment`,
  `remote_execute` — builders landed, no live single-host emission;
- intentionally content-free/high-volume domains
  (`UNINSTRUMENTED_OPERATIONS`): bounded reads/listings plus the
  `interactive_process_*` daemon operations, whose live evidence is the
  interactive `command_execute` hook below rather than a daemon mapping.

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

Explicit gaps (builders landed, no live single-host emission by design):

- `node_enrollment`, `remote_execute` — future node protocol
  (`FUTURE_DISTRIBUTED_AUDIT_ACTIONS`, `live_mapped: false`).

Live executors (M004 qualification current):

- `command_execute` is live at the canonical tool dispatch owners
  (bash/terminal/test plus interactive create) through the M001 seam.
  No daemon operation mapping exists by design; the guard requires the
  declarative executor-hook table plus owner pins.
- `git_operation` is live at `GitMutationExecutor` (typed, network, and
  recovery transitions plus raw paths) through the M001 seam. No daemon
  operation mapping exists by design.
- `job_complete` is live at the durable scheduler terminal attempt
  transition (`persist_completion` for executor
  success/failure/cancelled/timed_out/interrupted, `mark_unschedulable`
  for validation failure, queued `request_cancel` for pre-admission
  cancel) through the M001 seam (`src/scheduler/job_complete_audit.rs`).
  Attribution is resolved from the durable `OriginAttribution` row
  (scope `job`) with explicit `legacy-local` fallback; one deterministic
  event id per attempt/outcome dedupes replays; `job_retry` remains the
  retry-request event, not a surrogate completion. Guard:
  `check_m003_scheduler_job_complete_present` plus the M004
  executor-table checks; matrix:
  `tests/identity_m003_scheduler_job_complete.rs` plus the M004
  trajectory `tests/identity_live_execution_audit.rs`.

Interactive-process create treatment: the `interactive_process_*`
daemon operations stay explicitly uninstrumented (`Global` execution
surface, per-process attachment-registry authority). Their live audit
evidence is one structural `command_execute` event (family
`interactive`, argv digest only) emitted on successful create through
the daemon-built transport-bound hook
(`CoreDaemon::interactive_audit_hook`). Terminal input, cwd, and env
overrides never enter audit metadata; attach/detach/input/resize/list/
resume/terminate/remove emit nothing.

Live in M003: `chat_triggered_action` is emitted by the daemon
collaboration owner for every authorized structured chat action
(message -> decision -> action -> job; structural locators only).

Live in work orders M001: `work_order_lifecycle` is emitted by the
daemon work-order owner for every authorized work-order and lane
mutation (project -> work order; decision id plus durable revision;
identity and state only — never prompt bodies, secrets, or
reasoning). Creation and mutation operations that mint or change
durable identity skip the pre-side-effect emit and are recorded
post-mutation with their durable ids; retried (duplicate) submissions
emit nothing new.

## Trusted execution audit seam (M001)

One cloneable bounded emission service plus one trusted context carry
audit attribution to canonical execution owners without fabricating
identity or giving ToolBroker/Git/scheduler direct store ownership.

- `codegg-core::audit_instrumentation::ExecutionAuditEmitter` owns the
  bounded append timeout (`EXECUTION_AUDIT_EMIT_TIMEOUT`, 500 ms), the
  existing emit counters/warn behavior, no separate queue/store/
  schema, and no authorization decisions. `CoreDaemon::audit_emitter`
  builds it from the same pool/store policy as
  `CoreDaemon::append_audit_event`, which now delegates to it so family
  handlers and injected owners share one policy.
- `codegg-core::audit_instrumentation::TrustedExecutionAuditContext`
  carries only the cloned transport-bound `AuthenticatedPrincipal`,
  the gate-copied `AuditDecisionProvenance`, and the
  `AuditChainContext` locators. No command/body/secret content. No
  `serde` impls, so request DTOs, tool inputs, and model outputs cannot
  supply it over the wire.
- `CoreDaemon::execution_audit_context` copies the admitted
  authority/decision/chain at the admission boundary. The value may
  travel through `BrokerInvocationContext`/`ToolExecutionContext`
  (`execution_audit: Option<...>`) and scheduler/Git composition
  (`GitMutationExecutor::with_execution_audit/with_audit_emitter`,
  `JobScheduler::set_audit_emitter`), but it is constructed only from
  trusted daemon-owned state — never from `principal_ref`, tool input,
  model output, or a grant string.
- `ToolBroker::execute_with_retry` preserves the supplied context into
  `ToolExecutionContext` via `apply_execution_audit`, projecting the
  trusted origin strings without synthesis. Legacy/local callers pass
  `None` and use `TrustedExecutionAuditContext::legacy_local` (explicit
  `legacy-local` decision with `LocalOwner` binding) when they need
  emission, never an invented human identity.
- M001 emits no new `command_execute`/`git_operation`/`job_complete`
  events; M002/M003 consume this seam. Seam tests pin principal
  preservation, correlation, no-secret shape, timeout/failure counters,
  and single-store policy.

## Live execution hooks (M002)

`command_execute` and `git_operation` are emitted live at the
canonical execution owners through the M001 seam
(`src/live_execution_audit.rs`, `src/git_mutations.rs`).

- Ownership per family — one real dispatch emits at most one event,
  and the broker itself never emits. `command_family_for_tool` is the
  single ownership matrix: `bash` → `shell`, `terminal` → `process`,
  `test` → `test`; every other tool (reads, chat, artifacts, `git`)
  maps to no event. The Git route inside bash stays silent because
  `GitMutationExecutor` owns that single `git_operation` event, so
  native and bash-routed mutations converge on one audit event.
- Shell/test/process emission: `BashTool`, `TerminalTool`, and
  `TestTool` emit from their structured-execution path only when the
  broker threaded BOTH the trusted context and the shared emitter
  (`ToolExecutionContext::live_audit_hook`); legacy direct calls stay
  silent. Success/failure come from the terminal exit status;
  spawned-then-timed-out dispatches emit `timeout`; pre-dispatch
  denials and errors emit nothing (the denial is audited as
  `authorization_decision` elsewhere). Retries share the broker
  `invocation_key` in the idempotency scope, so a replayed terminal
  outcome reuses the deterministic event id while distinct
  invocations and distinct outcomes scope separately.
- Interactive create: `InteractiveProcessProtocol::create` emits one
  `command_execute` (family `interactive`) on successful dispatch
  using the daemon-built transport-bound hook
  (`CoreDaemon::interactive_audit_hook`: registry-bound principal,
  daemon-asserted `interactive_transport` provenance — this path
  predates the M003 gate, so no gate decision exists to copy —
  correlation bound to the requesting envelope). The digest covers
  the argv only; terminal input, cwd, and env overrides never enter
  audit metadata. Attach/detach/input/resize/list/resume/terminate/
  remove emit nothing. Scope binds the fresh process handle: every
  spawn is a distinct real execution.
- Git mutations: `GitMutationExecutor::emit_git_operation` runs at the
  end of `execute` (typed mutations, network `fetch`/`pull`/`push`,
  remote/config, and recovery transitions via `run_recovery`, which
  funnels through `execute`) and in `run_raw_mutation` (unparsed
  variants like `git add -A`). Labels are bounded
  (`stage`/`commit`/`branch_create`/`merge`/`rebase`/`push`/`pull`/
  `fetch`/`recover_*`/…; read-only operations map to no event).
  `git.ref_digest` covers target refs, remote NAMES, and refspecs
  only — remote URLs (credential-bearing or not), commit messages,
  path lists, and subprocess output never enter even the digest
  preimage (defense-in-depth URL scrub before hashing). The outcome
  is the terminal `MutationOutcome` label
  (`completed`/`no-op`/`fast-forward`/`conflict`/`rejected`); spawn
  and timeout errors emit nothing rather than a fabricated outcome.
  The idempotency scope binds op, ref digest, outcome, and post-state
  snapshot, so replays of one committed transition dedupe while
  distinct real transitions stay distinct.
- The tool raw-subcommand fallback (`GitTool` untyped mutations)
  owns its single event with the sanitized subcommand token
  (`unknown` for non-token input) and the empty ref digest.
- `scripts/check_audit_coverage.py` pins every hook site
  (`check_m002_live_execution_hooks_present`); the regression matrix
  lives in `tests/identity_m002_live_audit_hooks.rs`. Full
  live-vs-builder guard tightening (event samples,
  duplicate/secret negatives across trajectories) is closed in M004
  (see below).

## Scheduler terminal hook (M003)

`job_complete` is emitted live at the durable scheduler terminal
attempt transition through the M001 seam
(`src/scheduler/job_complete_audit.rs`, `src/scheduler/scheduler.rs`).

- Ownership — the canonical terminal transition persists first, then
  emits: executor completions via `persist_completion`
  (success/failure/cancelled/timed_out/interrupted), validation
  rejections via `mark_unschedulable` (bounded `failure`), and queued
  pre-admission cancels via `request_cancel` (bounded `cancelled`).
  TUI polling, projection/event observers, retry requests, and
  completion consumers never emit.
- Attribution — resolved from the durable `OriginAttribution` row
  (scope `job`, first-write-wins) and rebuilt losslessly via
  `AuthenticatedPrincipal::reconstructed` plus the gate-copied decision
  linkage; legacy rows without attribution use explicit
  `legacy-local`/`LocalOwner`, never a fabricated team principal.
- Correlation — project/session/turn/run/job locators from the
  durably-accepted `JobRecord` where available; metadata carries ids,
  bounded outcome/state labels, and `decision.outcome = allow` only.
  No payload, tool output, command text, or secret material.
- Idempotency — one terminal transition scopes one deterministic event
  id (`attempt:outcome`, or `job:cancelled` for pre-admission cancel);
  the failed prior attempt and its retry successor stay distinct while
  replays reuse the stored row. `job_retry` remains the retry-request
  event, not a surrogate completion.
- Failure policy — shared bounded emitter: timeouts/failures increment
  counters with a warn and never fail terminalization.
- `scripts/check_audit_coverage.py` pins the hook
  (`check_m003_scheduler_job_complete_present`); the regression matrix
  lives in `tests/identity_m003_scheduler_job_complete.rs`.

## Live execution qualification and coverage guard (M004)

M004 closes the original M005 low single-host live-hook findings. The
three corrected actions are live with executable evidence; distributed
`node_enrollment`/`remote_execute` remain the only future actions.

- Declarative executor-hook table:
  `codegg-core::audit_instrumentation::EXECUTOR_LIVE_AUDIT_HOOKS`
  (exactly `command_execute`, `git_operation`, `job_complete` with
  `executor:*` owners and `live_mapped: true`) plus
  `FUTURE_DISTRIBUTED_AUDIT_ACTIONS` (exactly `node_enrollment`,
  `remote_execute` with `live_mapped: false`). Core unit tests pin the
  four-category distinction and reject `UNINSTRUMENTED_OPERATIONS`
  evasion.
- Declarative owner pins: `src/executor_audit_hooks.rs`
  (`EXECUTOR_HOOK_OWNER_PINS`) names every canonical emit owner file
  and symbol. The guard parses this table (not ad-hoc call text) and
  fails closed if an owner moves; Rust tests pin the table against the
  core executor table. Guard:
  `check_executor_hook_table_is_authoritative` +
  `check_m004_executor_owner_pins_present` (plus `--self-test`);
  matrix: `tests/identity_live_execution_audit.rs`
  (`executor_hook_table_is_authoritative_and_fail_closed`).
- Deterministic single-host trajectory
  (`single_host_trajectory_is_ordered_correlated_and_secret_free`):
  Owner turn/job → ToolBroker bash → Git stage → interactive create →
  scheduler terminal success → Owner audit query. Asserts ordered
  `command_execute`/`git_operation`/`command_execute(interactive)`/
  `job_complete` rows on one correlation with trusted actor, decision
  ids, project/session/turn/run/job linkage where available
  (workspace-scoped job terminals carry no project by design),
  structural-only metadata/bodies, and no duplicates after
  replay/restart. Owner project query returns the three project rows;
  the job terminal joins the same correlation via the store chain.
- Negatives (`credential_git_url_secret_command_and_terminal_input_stay_structural`,
  `unauthorized_project_actor_is_denied_without_execution_leak`):
  viewer query denied, credential-bearing Git URL emits without URL
  material, secret-looking command emits digest only, terminal input
  never enters audit, and no secret leaks through error strings.

## Verification

`scripts/check_audit_coverage.py` parses the canonical authorization
descriptor source (`crates/codegg-core/src/authorization/policy.rs`,
`operation_descriptor`) — never the historical `authorization.rs`
location — and fails closed with a non-empty inventory check if the
table moves. `crates/codegg-core/src/audit_instrumentation.rs`
(`INSTRUMENTED_OPERATIONS` / `UNINSTRUMENTED_OPERATIONS`) must classify
every canonical operation; the guard pins that list so a new privileged
operation cannot hide as uninstrumented.

```bash
cargo test -p codegg-core audit
cargo test --test identity_m004_audit_foundation
cargo test --test identity_m005_audit_instrumentation
cargo test --test identity_m002_live_audit_hooks -- --test-threads=1
cargo test --test identity_m003_scheduler_job_complete -- --test-threads=1
cargo test --test identity_live_execution_audit -- --test-threads=1
cargo test --test storage_migrations
python3 scripts/check_audit_invariants.py --verbose
python3 scripts/check_audit_coverage.py --verbose
python3 scripts/check_audit_coverage.py --self-test
python3 scripts/check_authorization_matrix.py --verbose
python3 scripts/check_project_catalog_invariants.py --verbose
bash scripts/check-core-boundary.sh
cargo fmt --all -- --check
scripts/verify.sh quick
```
