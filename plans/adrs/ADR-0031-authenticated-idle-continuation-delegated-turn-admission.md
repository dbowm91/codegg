# ADR-0031: Authenticated Idle Continuation Grant and Delegated Turn Admission

Status: accepted

Accepted: 2026-10-14 (C005 implementation authorized on this contract; grant-family + delegated-entry slices landed under it)

Date: 2026-10-14

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#6-canonical-identity-relationships`
- `plans/000-long-term-specification.md#8-deployment-profiles-and-authentication`
- `plans/000-long-term-specification.md#15-read-only-session-observation`
- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/000-long-term-specification.md#22-audit-architecture`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/001-terminology-and-domain-model.md` — execution context, session, turn, principal, delegation

Affected subsystem roadmaps:

- `plans/subsystems/async-background-jobs-idle-completion-corrective-addendum.md`

Related ADRs:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`
- `plans/adrs/ADR-0002-agent-run-worktree-isolation-and-control.md`
- `plans/adrs/ADR-0007-shared-session-control-ownership.md`

## Context

C004 (`fcbb915a`) delivers daemon-owned idle completion attention: one
durable `JobTerminalNotice` plus one persisted owner-scoped
`CoreEvent::BackgroundJobCompleted` per logical terminal, with zero model
tokens. The daemon owns the `ContinuationService` store but wires no
coordinator/executor and never invokes `maybe_dispatch` in production.

Three gaps block autonomous resume (C005-F1/F2/F3):

- `ContinuationService::authorize(NewGrant)` accepts caller-supplied
  lineage strings (`notice_job_id`, `session_id`, `owner_principal`,
  `workspace_id`, `origin_turn_id`, `allowed_model`, `allowed_connection`).
  No `CoreRequest` grant path, no `operation_descriptor` arm, no
  server-resolved principal/session/workspace/model, no audit. Existing
  grant rows are therefore non-autonomous by construction.
- No production `ContinuationExecutor` exists; only
  `FakeContinuationExecutor` (test-only). Production never calls
  `maybe_dispatch`.
- `CoreDaemon::handle_turns_request` (`src/core/daemon_turns.rs`)
  admits every turn from transport-bound authority
  (`RequestAuthorityContext` + `trusted_client_id` + `AuthorizationDecision`
  + controller lease per ADR-0007). No daemon-internal delegated entry
  exists that can admit one owner turn while idle without forging a user
  session.

The existing authorization architecture (`operation_descriptor` exhaustive
over `CoreRequest`, transport-bound `AuthenticatedPrincipal`, project scope
via `direct_project`/`via_session`/`via_job`, capability narrowing,
`OriginAttribution`, controller lease narrowing per ADR-0007) can express
the *grant management* half with no new semantics: a new `CoreRequest`
family through the existing daemon gate. It cannot safely express the
*dispatch* half without a new decision, because every turn today is
submitted by a transport-bound principal and the lease is attributed to
that submitting principal. A daemon-initiated turn attributed to the grant
owner is a new identity/ownership boundary: the first daemon-initiated
turn that is not a transport-bound submission.

This ADR is required before C005 implementation because it changes
authentication/authorization semantics (grant issuance), scheduler/turn
admission authority (daemon-initiated owner turn), and identity attribution
(controller lease for an idle owner). Without it, any implementation would
either forge a user session or mint turns from untrusted rows.

## Decision drivers

- Preserve the singleton daemon and `JobScheduler`/`JobSubmissionService`
  + ToolBroker/sandbox/permissions as the only execution authority. No
  second scheduler, no provider call from a completion callback, no
  workflow engine.
- Preserve `notify_only` as complete with zero inference. `resume_owner`
  without a valid host grant stays notify-only; legacy C002/C004 rows
  without attestable origin stay non-autonomous and are never upgraded.
- Exactly one bounded owner turn per logical terminal (DB CAS,
  deterministic turn/intention id + receipt before provider entry, fenced
  daemons, crash-after-provider-entry becomes `needs_attention`, never
  blind replay).
- Mutual exclusion with live owner turns; honor cancellation, steering,
  permission prompts, revocation, expiry, budgets; a dropped frontend never
  licenses impersonation.
- Grant issuance must be attributable and auditable under the existing
  `AuthorizationDecision` + `OriginAttribution` + audit chain. Dispatch
  must be attributable to the grant owner, not to the daemon service
  identity, while remaining distinguishable from a transport-bound user
  submission.
- No model-supplied flag, raw frontend metadata, unauthenticated socket, or
  fake `client_id` may substitute for grant verification.
- Cross-instance safety must hold under `flock` bypass (stale lock,
  divergent `CODEGG_DAEMON_HOME`, pool-less construction): DB CAS is the
  authority, not process memory.
- Crash between notice commit and live-hint publish must not lose durable
  attention; live hints stay hints, the notice row stays authoritative.

## Considered options

### Option A — Grant as new CoreRequest family; dispatch as daemon-internal delegated admission (selected)

Grant management becomes three new `CoreRequest` variants
(`ContinuationGrantCreate` / `ContinuationGrantRevoke` /
`ContinuationGrantStatus`, names bikesheddable at implementation) through
the existing daemon request authority:

- Scope `via_session` (locator: owning `session_id`, resolved server-side
  through the session row; `notice_job_id` is a durable job/watch locator,
  not an authority root).
- Capability `agent.invoke` (same gate as `TurnSubmit`/`SessionPromptSubmit`;
  creating a future turn requires the authority sufficient to create that
  turn; reads use `session.read`).
- Server-resolved principal/session/workspace/model/connection; client
  fields for identity are ignored. Budgets, expiry, cancellation/revocation,
  single-notice scope, explicit model/connection binding resolved
  server-side. No default credentials, no silent approval.
- Durable `continuation_grant` row captures `OriginAttribution` immutably
  (first-write-wins); grant and revocation audit through the existing chain
  (`audit_chain_for_request`, `session_id_for_request`,
  `REQUIRED_AUDIT_COVERAGE`); `operation_descriptor` arms +
  `representative_requests` + matrix doc table updated in the same change;
  denials use the existing privacy-safe shapes.

Dispatch becomes a new **daemon-internal** (non-`CoreRequest`) delegated
admission entry, e.g. `CoreDaemon::submit_idle_continuation_turn`:

- Not a wire operation: no `CoreRequest` variant, no transport route, no
  bearer. Only the daemon's post-commit fanout path may call it, once per
  terminal, after the durable notice + live event commit.
- Revalidates, in order, against durable state at wake time: grant exists,
  `approved`, unexpired, unused budget; session exists and is not
  archived/deleted; stored owner principal still holds `agent.invoke` on
  the session project at the current membership revision; workspace matches;
  terminal status resumable; owner idle (no active turn); provider/
  connection available and authorized (`authorize_provider_use`); request
  fingerprint matches (no reuse of one grant for a different wait).
- CAS-claims exactly one `(notice_id, grant_id)` intent
  (`Pending → Claimed`, `UNIQUE(notice_id, grant_id)`, deterministic
  `turn_id = idlet-sha256(notice|grant)`), persists accepted turn id + cause
  `completed_background_job` + receipt before provider entry, then admits
  through the **canonical** turn admission (same controller store, receipt
  table, `TurnStarted` event, `TurnRunInput`, budgets/permissions/sandbox/
  active-turn exclusivity) with authority derived from the grant's stored
  origin (rechecked), attributed to the grant owner principal, with a
  delegated client marker (e.g. `idle:<grant_id>`) recorded as
  audit/presence metadata, never as a second authority.
- Establishes the ADR-0007 controller lease for the **owner**, not the
  daemon service identity, through the same `acquire_turn_controller_*`
  path (durable `admit_with_promotions` first, receipt on `submission_id`,
  `turn_id` unique). Same-principal second devices retain existing
  semantics; observers/Viewers/chat grants never satisfy the predicate.
- Ambiguity after provider submission becomes `needs_attention` (never
  blind replay); `TurnAccepted` (admitted, receipt durable) is distinct
  from provider-start (first provider entry observed).
- Missed wakes heal by restart reconciliation and `reconcile_terminals`,
  bounded, without long-held permits.

Benefits:

- Grant issuance reuses the proven transport-bound gate, scope resolvers,
  capability model, attribution, and audit; no new principal Mint.
- Dispatch cannot be invoked by any client (no wire surface), so a stolen
  grant id alone confers no turn; only the daemon wake path with full
  revalidation can admit.
- Owner attribution is explicit and auditable, preserving ADR-0007
  narrowing (lease narrows, never widens) while making daemon-initiated
  turns distinguishable in audit (`delegated_idle_continuation` provenance).
- DB CAS remains the exactly-once authority across instances; process
  memory is only a hint.

Costs and failure modes:

- New narrow internal entry must be kept from becoming a general
  daemon-may-submit-as-anyone backdoor: restricted to one call site (idle
  fanout), one grant scope, full revalidation, CAS, receipt, and audit.
  Guarded by code review + a new static check (no other callers).
- Controller lease for an idle owner while the owner has no live transport
  is new: takeover/transfer/reconnect semantics must treat the delegated
  lease identically to a transport-submitted lease (explicit in this ADR).
- Grant table grows a server-resolved origin; legacy rows stay
  non-autonomous (migration never upgrades them).

### Option B — Dispatch as a new CoreRequest variant callable by clients

A new `ContinuationDispatch` request would let an authenticated client ask
the daemon to start the idle turn.

Benefits: reuses the entire request pipeline (gate, audit, idempotency)
with no internal entry.

Costs and failure modes (rejected as the dispatch mechanism):

- Any holder of `agent.invoke` could mint idle turns at will, bypassing
  the terminal-wake binding (dispatch without a terminal). Rate/budget
  scoping becomes advisory.
- Receipt/request-id semantics would conflate user submissions with daemon
  wakes, weakening the exactly-once-per-terminal proof.
- Expands the wire surface for no client need: no frontend needs to *ask*
  for dispatch; the daemon already knows when the terminal commits.

Rejected. Grant *management* is a `CoreRequest` family (Option A); the
*wake* is not.

### Option C — Daemon submits TurnSubmit as LocalOwner broad policy

The daemon would synthesize a `TurnSubmit` with the local-owner broad
authority and the owner's session id.

Benefits: no new code path.

Costs and failure modes (rejected):

- LocalOwner broad policy is explicitly not a bypass; using it to act as
  another principal violates its composition contract and destroys
  attribution (turn would be attributed to the daemon/local owner, not the
  grant owner, or worse, would pass where the owner lacks `agent.invoke`).
- Breaks ADR-0007 controller semantics (controller would be the daemon,
  locking the owner out of their own turn).
- Indistinguishable in audit from genuine local-owner work.

Rejected. Attribution must be to the grant owner with delegated
provenance, never to the daemon identity.

### Option D — Keep everything fail-closed (no resume)

Preserve C004 delivery-only behavior indefinitely.

Benefits: zero new authority.

Costs: the C002/C005 opt-in resume use case (explicitly authorized single
bounded owner turn while idle) remains unimplemented; the workstream stays
open.

Rejected as the terminal state, retained as the safe fallback: if this
ADR's implementation cannot meet its verification, resume stays blocked
per the C005 stop rule.

## Decision

Adopt **Option A**:

1. **Grant management is ordinary principal-authorized project mutation**
   through a new minimal `CoreRequest` family (`via_session` +
   `agent.invoke` for create/revoke, `session.read` for status), with
   server-resolved principal/session/workspace/model/connection, immutable
   `OriginAttribution` capture, full audit, exhaustive
   `operation_descriptor` arms, and privacy-safe denials. Legacy grant rows
   without attestable origin remain non-autonomous; no migration upgrades
   them; `on_complete=resume_owner` without a valid host grant stays
   notify-only.

2. **Idle dispatch is a daemon-internal delegated admission**, not a wire
   operation. Exactly one call site (the C004 post-commit fanout path, plus
   its bounded restart-reconciliation twin) may invoke
   `submit_idle_continuation_turn`, which revalidates the grant against
   current team/session/provider state, CAS-claims the `(notice, grant)`
   intent, persists receipt + accepted turn id before provider entry, and
   admits through the canonical controller/turn runtime attributed to the
   grant owner with delegated provenance. No other caller is permitted; a
   static guard enforces the call-site restriction.

3. **The ADR-0007 controller lease applies unchanged**, with the owner as
   controller and the delegated marker as client metadata. Transfer,
   takeover, release, terminal release, revocation effectiveness, and
   restart reconciliation treat delegated leases identically to
   transport-submitted leases. Ambiguous provenance fails closed until
   explicit recovery takeover with `expected_revision: 0`.

4. **Exactly-once authority is the durable intent row**, not process
   memory, channel order, or `EventLog` sequence. Deterministic
   `turn_id`, receipt dedupe (`submission_id` PK, `turn_id` unique,
   `request_sha256` match), and `needs_attention` on post-accept ambiguity
   are mandatory. Two daemons over one DB may both publish the same live
   hint (dedupe-keyed, harmless); they may never start two provider turns
   for one terminal.

5. **Live hints stay hints.** The `job_notification` notice row is the
   authority; `BackgroundJobCompleted` is a persisted hint. Crash between
   commit and hint publish heals by bounded startup reconciliation that
   republishes missing hints deduped by `notice_id` (never a second notice,
   never a turn). No blind replay of provider turns.

## Consequences

### Positive

- Autonomous resume becomes expressible without forging users, minting
  authority from model flags, or opening a client-callable dispatch.
- Grant issuance, attribution, audit, and denial privacy follow existing
  proven paths; reviewers reason about one small new surface.
- Delegated turns are auditable as delegated (owner + grant + notice +
  membership revision), preserving accountability.
- Cross-instance and crash semantics rest on durable CAS/receipts, not on
  the `flock` or in-memory singletons alone.

### Negative

- One new privileged internal path exists that must be reviewed as
  security-critical on every touch (call-site guard + audit + tests).
- Delegated controller leases add recovery/test surface (restart with an
  active delegated turn, revocation mid-turn, takeover of a delegated
  turn).
- Grant storage grows server-resolved origin columns; legacy rows are
  permanently second-class (documented, not migrated).

### Neutral or deferred

- No change to provider selection, semantic routing, sandbox, permission
  broker, scheduler fairness, or chat/collaboration gates.
- No new messaging platform, webhook, multi-CI abstraction, or global TUI
  redesign.
- TUI/desktop rendering of `BackgroundJobCompleted` (toast + actionable
  handle through existing authorized controls) is a separate frontend
  slice under this ADR's delivery contract, not a new authority.

## Compatibility and migration

- Protocol: additive `CoreRequest`/`CoreResponse` variants for grant
  management only; no change to `TurnSubmit` wire shape. Old clients
  receive typed `unsupported`/version negotiation per existing policy.
- Storage: additive migration for server-resolved grant origin columns
  (nullable; legacy rows read as non-autonomous) and delegated provenance
  columns on intent/receipt rows (nullable). `STORAGE_LAYOUT_VERSION`
  tracks the highest migration; catalog-invariants guard stays green. No
  backfill of authority; no rewrite of history.
- Config: no new credentials; explicit model/connection binding reuses
  existing selection/connection identity. No default auto-resume; global
  default remains `notify_only`.
- Operations: standard `verify.sh quick`, matrix/audit/boundary guards,
  plus a new call-site guard for the delegated entry. Dashboards/toasts
  consume the existing `BackgroundJobCompleted` event; no new event type.

## Security and reliability implications

- Grant creation requires `agent.invoke` on the owning session project at
  decision time; decisions bind the membership revision; `TeamStore`
  revision checks reject stale re-grants. Revoked/suspended principals
  immediately lose dispatch effectiveness even if a grant row remains
  (revalidation at wake time, plus in-turn revocation checks through the
  existing controller/permission paths).
- Knowledge of `job_id`/`notice_id`/`grant_id` alone confers no rights;
  exact-owner session equality + capability + live membership are all
  required. Forged session/turn, guessed ids, second-principal access fail
  with the same privacy-safe shapes as existing job/watch surfaces.
- In-flight delegated work keeps its immutable origin; cancellation and
  steering obey the controller lease (owner controller). Takeover requires
  Maintainer/Owner + bounded reason + audit, identical to
  transport-submitted turns.
- Budgets (tokens, tool calls, duration, max continuations, TTL) are
  enforced before and during the turn; exhaustion suppresses before
  inference with actionable attention.
- Provider turns started before a crash are never blindly replayed;
  ambiguity becomes `needs_attention` with a bounded manual recovery path.
- Secrets/credentials never enter grant rows, intents, receipts, events,
  toasts, or audit metadata (credential refs only; bounded redacted
  summaries).
- Denial-of-service: grant creation bounded per session/owner (caps exist:
  `GRANT_MAX_CONTINUATIONS_CAP`, TTL clamp); fanout stays one channel +
  one forwarder, no per-job task; reconciliation bounded (existing 256-row
  pattern); no unbounded poller.

## Verification

Implementations conform only when tests prove, on a real `CoreDaemon`
with real session/workspace, core request dispatch for grant + background
work, subscribed event clients, and the turn-runtime boundary with a fake
provider (no fake coordinator, no manual `maybe_dispatch` after
completion):

- Authenticated grant: server-resolved principal/session/workspace/model;
  client-supplied identity fields ignored; legacy/model-only grants stay
  notify-only; revocation/expiry/budget/mismatch matrix fails closed
  before inference with actionable attention.
- Delegated admission: exactly one admitted turn with receipt per terminal;
  correct owner attribution + delegated provenance in attribution/audit;
  controller lease held by owner; same-principal second device passes;
  observer/Viewer/chat denied; takeover/transfer/revocation semantics match
  ADR-0007; `TurnAccepted` vs provider-start distinguished.
- Exactly-once under contention: 50-race CAS at every boundary
  (notice/intent/turn/provider), restart before/after each boundary, no
  duplicate provider entry; ambiguity becomes `needs_attention`.
- Delivery preserved: live + replay + restart + duplicate-suppression for
  `BackgroundJobCompleted` (existing C004 proofs extended, not replaced);
  startup reconciliation republishes missing hints deduped by `notice_id`
  without duplicating notices or turns.
- Cross-instance: two daemons over one pool may duplicate hints (dedupe
  absorbs) but never duplicate turns (CAS + receipt + controller).
- Interop: existing M001–M004/C001–C004 suites green; no foreground
  semantic change; `cargo fmt`, Clippy `-D warnings`, `verify.sh quick`,
  scheduler/execution-ownership/core-boundary/authorization/projection
  guards green; exact-head hosted `CI / verify` + Desktop E2E recorded;
  mainline reconciliation on a separate head with gates rerun.
- Static: `check_authorization_matrix.py` (new arms), 
  `check_audit_coverage.py` (new family), plus a new delegated-call-site
  guard (or extension of `check_scheduler_bypass.py` /
  `check_execution_ownership.py` pattern) proving the internal entry has
  exactly its two permitted callers.

## Supersession

None. Complements ADR-0001 (execution authority), ADR-0002 (worktree
isolation), and ADR-0007 (controller lease); does not supersede or
reinterpret them. If a future collaborative-control system supersedes
ADR-0007 per its own supersession clause, this ADR's delegated-lease
section must be re-evaluated under the successor.
