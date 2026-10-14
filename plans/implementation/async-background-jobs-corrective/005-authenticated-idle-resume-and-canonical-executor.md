# Async Background Jobs Corrective C005 — Authenticated Idle Resume Grant and Canonical Executor

Status: blocked — successor corrective, NOT implemented
Source baseline: `fcbb915a7909a19f96be276b9ec3c12a52794604` on `work/async-background-jobs-production-wiring-c004-plans` (C004 delivery slice). Revalidate actual implementation HEAD before editing.
Subsystem: `plans/subsystems/async-background-jobs-idle-completion-corrective-addendum.md` (new C005 successor).
Historical closures: C004 delivery record `plans/closure/async-background-jobs-corrective/004-status.md` (corrective pass required: delivery landed, resume blocked). M001–M004 and C001–C003 closures remain historical and unaltered.
Normative: `plans/000-long-term-specification.md`; `plans/001-terminology-and-domain-model.md`; `plans/003-planning-process.md` §§2.4, 7–10; ADR-0001 programmatic execution authority; ADR-0002 AgentRun/worktree isolation; `architecture/{agent,authorization,jobs,projection,scheduler}.md`.
Priority: high for autonomous-resume readiness, explicitly not required for notify-only production use.

## 1. Objective, missing boundary and stop rule

C004 (`fcbb915a`) delivers daemon-owned completion attention: one durable notice plus one persisted owner-scoped `CoreEvent::BackgroundJobCompleted` per logical terminal, with zero model tokens. The daemon owns the `ContinuationService` store but wires no coordinator/executor and never invokes `maybe_dispatch` in production. `ContinuationService::authorize(NewGrant)` still accepts caller-supplied lineage strings, so existing grant rows are non-autonomous and every completion stays notify-only (fail-closed, proven by `continuation_store_owned_but_idle_resume_stays_blocked`).

**End state:** an authenticated interactive user/task control approves, revokes, and inspects exactly one bounded idle continuation per background wait through existing daemon request authority (server-resolved principal/session/workspace/model, audited, no default admin, no silent approval); on the durable terminal the daemon automatically admits and starts exactly one canonical owner turn through the real session admission/turn runtime with the bounded host-authored terminal result, without harness `maybe_dispatch` and without forging a user or bypassing authorization.

**Stop immediately** if no independently verifiable host-trusted grant path and no delegated (non-forged) turn-admission entry can be established. In that case leave resume blocked, record evidence, and never substitute a model-supplied `on_complete` flag or a fake `client_id`.

## 2. Defect matrix (why C004 left this blocked)

| Finding | Source evidence | Unclosed requirement | Why C004 did not close it |
|---|---|---|---|
| C005-F1: grant issuance unauthenticated | `src/scheduler/idle_continuation.rs::ContinuationService::authorize` takes `NewGrant` strings; no `CoreRequest` grant path, no `operation_descriptor` arm, no server-resolved principal/session/workspace | Authenticated principal explicitly approves each bounded continuation | C004 scoped to delivery; store ownership alone confers no authority |
| C005-F2: no production executor | No `ContinuationExecutor` production field on `CoreDaemon`; only `FakeContinuationExecutor` exists; production never calls `maybe_dispatch` | Canonical owner turn via real session admission/`TurnRuntime`, exactly once, with budgets/permissions/sandbox/active-turn exclusivity | Wiring a real executor without F1 would mint turns from untrusted rows |
| C005-F3: no delegated admission entry | `src/core/daemon_turns.rs` `TurnSubmit` requires transport-bound authority + trusted `client_id`; no daemon-internal delegated entry bound to a grant | Idle turn attributed to the granting owner without forging a user session | Inventing one without an ADR/authority review would violate ADR-0001/0007 |

## 3. Hard invariants / non-goals

- Same daemon/scheduler/turn authority as C004: sole privileged daemon, `JobScheduler`/`JobSubmissionService` + ToolBroker/sandbox/permissions only, no second scheduler, no provider call from completion callback, no workflow engine.
- `on_complete=notify_only` default never spends tokens; `resume_owner` without a valid host grant stays notify-only; past C002/C004 grant rows without attestable origin stay non-autonomous.
- One logical terminal → at most one accepted owner turn (DB CAS, deterministic turn/intention id + receipt before provider entry, fenced daemons, crash-after-provider-entry becomes `needs_attention`, never blind replay).
- Mutual exclusion with live owner loops; honor cancellation, steering, permission prompts; dropped frontend never licenses impersonation.
- Preserve C004 delivery (fanout, `BackgroundJobCompleted`, replay, router hint) and all M001–M004/C001–C003 history. Scope is grant workflow + executor + tests, not messaging platforms, webhooks, or expanded sandbox guarantees. An ADR is required if the delegated admission entry changes a canonical identity/ownership boundary.

## 4. Ordered work packages

### A. Authenticated grant workflow

Add the smallest `CoreRequest` family (`grant`/`revoke`/`status` for one background wait) through existing daemon request authority: resolve principal/session/workspace/model server-side, never trust client-supplied identity fields; scope to a single notice, explicit model/connection, budgets, expiry, cancellation/revocation; audit grant and revocation; add exhaustive `operation_descriptor` arms, representative requests, matrix/audit guards, and privacy-safe denials. No default credentials, no silent approval, no upgrade of legacy rows.

### B. Canonical idle executor

Implement a production `ContinuationExecutor` that submits through the existing session controller/turn admission with the bounded host-authored terminal result and a typed idle cause. Preserve provider identity, permissions, sandbox ceiling, budgets, active-turn exclusivity, and owner attribution; use an explicit delegated authorization bound to the grant (new narrow internal typed entry only if `TurnSubmit` cannot represent it). Reject absent/stale session, revoked control, unauthorized project, invalid connection, or fail to `needs_attention` before inference. Persist turn/intention id and receipt at the canonical controller; clarify `TurnAccepted` vs provider-start.

### C. Automatic daemon wake

Wire the C004 post-commit fanout to a bounded daemon-owned continuation check: after the durable notice + live event commit, evaluate exactly one `maybe_dispatch` per terminal with host-verified owner/idle/provider state. No per-job task; missed wakes heal by restart/reconciliation; preserve fairness, shutdown drain, and no long-held permits. This MUST NOT trigger on `on_complete` alone.

### D. Verification, docs and truthful closure

Add daemon-level tests using a real `CoreDaemon`, real session/workspace, core request dispatch for grant + background work, subscribed event clients, and the turn-runtime boundary with a fake provider (no fake coordinator, no manual `maybe_dispatch` after completion). Cover prod resume, notify-only negatives, live+replay with two clients, consent/revocation matrix, 50-race CAS/crash, and full interop. Update `architecture/{agent,authorization,jobs,projection,scheduler}.md` and tool copy. Required closure: `plans/closure/async-background-jobs-corrective/005-status.md`; update addendum and registry with unblock audit. Obtain exact-head hosted `CI / verify` + Desktop E2E before claiming merge-ready; reconcile `main` divergence on a separate integration head.

## 5. Mandatory acceptance (pass/fail, no simulation loopholes)

1. Prod idle resume via real daemon + host grant + real `TurnRuntime` (fake provider): exactly one admitted turn with receipt, correct lineage/budgets, one provider entry; no harness dispatch.
2. Notify-only and all unapproved/expired/revoked/mismatched cases: one notice + one live event, zero turns.
3. Live+replay with two clients, disconnect/reconnect, restart: identical notice id/status exactly once, no turn needed.
4. Consent/revocation matrix fails closed before inference with actionable attention.
5. 50-race/restart-before-after at every boundary: one notice, one turn, no duplicate provider entry; ambiguity becomes `needs_attention`.
6. Interop suites green; no foreground semantic change.
7. `cargo fmt`, Clippy `-D warnings` (no `--all-features`), `verify.sh quick`, scheduler/execution-ownership/core-boundary/authorization/projection guards, exact-head hosted CI + Desktop E2E recorded.
8. Mainline integration reconciled on a separate head with gates rerun.

## 6. Failure disposition

No closure if the daemon cannot own the dispatcher, if a model can mint its grant, if completion cannot reach a subscribed frontend while idle, or if tests manually invoke coordinator code production never invokes. Do not rewrite history. Follow `plans/003-planning-process.md` with exact SHAs, requirement/evidence table, and unblocking disposition.
