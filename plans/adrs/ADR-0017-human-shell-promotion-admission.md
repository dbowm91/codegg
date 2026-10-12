# ADR-0017: Human-Shell Promotion Staging and Turn Admission

Status: accepted

Date: 2026-10-12

Accepted: 2026-10-12, by the project maintainer, selecting Option A: consume
promotions at durable turn admission and mark accepted turns interrupted after
restart without automatic replay.

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/001-terminology-and-domain-model.md#8-scheduler-and-execution-terms`

Affected subsystem roadmaps:

- `plans/subsystems/human-execution-corrective-roadmap.md`

Related ADRs:

- `plans/adrs/ADR-0007-shared-session-control-ownership.md`
- `plans/adrs/ADR-0010-desktop-frontend-and-shared-client-boundary.md`

## Context

Human-shell promotion is currently acknowledged only when its redacted text is
included in a later turn. Local TUI pending state lives in frontend memory, and
remote `/shell-include` places text in the composer without a distinct staging
acknowledgement. The daemon has no canonical promotion record or query operation.

`SessionPromptSubmit` is authenticated and session-scoped through the existing
`AgentInvoke` authorization descriptor, but it carries no promotion identity.
`TurnSubmit` carries provider messages without a durable receipt. Its request
envelope already has a client-generated `request_id`; this decision promotes
that field from correlation metadata to the stable submission identity for
turn requests. The daemon creates an in-memory active turn and separately
writes a durable session-controller lease. That lease stores control ownership,
not prompt data or an accepted-turn receipt. The runtime receives an ephemeral
`TurnRunInput`, and the request handler returns `Ack` after runtime acceptance.
There is no durable receipt that resolves a daemon restart between admission
and provider delivery.

The session SQLite schema is at migration v70 and can support an additive narrow
ledger. Reusing ordinary conversation messages would conflate a staged artifact
with a submitted user turn. Reusing `session_turn_controller` would conflate
control ownership with pending context and still would not provide promotion
idempotency or restart state.

The unresolved boundary is what `exactly once` means across a provider call whose
outcome can be unknown after a daemon crash. CodeGG can define one durable
accepted turn and include a promotion in that turn's initial provider-facing
message composition. It cannot atomically commit SQLite state and an external
provider's receipt without a provider idempotency contract or a durable turn
delivery/recovery subsystem.

## Decision drivers

- Only explicitly promoted, redacted, bounded shell evidence may enter context.
- Authenticated server authority supplies principal, session, project, and
  workspace identity; request payload fields cannot assert them.
- A stage acknowledgement survives frontend reconnect and daemon restart until
  accepted by a turn or explicitly expired/refused.
- Busy or rejected turn admission must leave pending evidence untouched.
- A duplicate stage or submit retry must not silently create a second promotion
  or second accepted turn.
- Raw shell output remains in its existing governed store and is never copied to
  this lifecycle.
- The delivery contract must state what happens when the daemon stops after
  accepting a turn but before provider delivery is confirmed.

## Considered options

### A. Narrow session-owned promotion ledger and durable admission receipt

Add a dedicated session-scoped table for redacted pending promotions, with a
stable promotion ID, authenticated principal, exact session/workspace binding,
bounded payload, expiry, and lifecycle state. Add an idempotent turn-submission
identity using `RequestEnvelope.request_id` and an accepted-turn receipt. A
retry resends the same request ID and identical request content. In one
daemon-owned SQLite transaction,
validate the pending promotion set, reserve the accepted turn, and mark its
promotion IDs consumed by that turn. Return an acknowledgement only after the
receipt commits. Busy/rejected requests leave the stage pending; a retry with
the same identity returns the same receipt.

The proposed crash rule is **consume at durable turn admission**. If the daemon
stops after the receipt commits and before the provider outcome is known, recover
the record as interrupted and do not automatically replay it. The client can
show that the turn was accepted/interrupted, and a human can explicitly submit a
new turn. This provides at-most-once admission per submission identity and
exactly-once inclusion in that accepted turn's initial message composition; it
does not claim exactly-once network delivery to a provider.

Benefits: narrow storage boundary; durable stage/reconnect; busy-turn safety;
stable retry identity; no raw output; no automatic duplicate delivery.

Costs: one additive migration and versioned capability; status/query API; turn
admission transaction changes; an accepted turn may be reported interrupted
without confirmed provider receipt.

### B. Persist complete turn envelopes and replay after restart

Persist the composed provider-facing turn input and run it through a durable
turn-delivery/recovery owner after restart, with idempotent provider dispatch
where available.

Benefits: can recover accepted turns that were not dispatched before a daemon
failure.

Costs: changes broader turn ownership and storage semantics, persists additional
user prompt/context material, and still cannot guarantee exactly-once provider
effects unless providers honor stable idempotency keys. This exceeds C004's
bounded shell-promotion scope and needs its own plan.

### C. Keep promotion staging frontend-local

Continue attaching pending evidence to the next local prompt and describe remote
composer insertion as a draft.

Benefits: no daemon schema or protocol change.

Costs: does not survive frontend loss, cannot provide canonical stage status, and
does not satisfy C004's accepted objective. This is only acceptable if the
product deliberately lowers C004's contract.

## Decision

Option A is accepted: use a narrow session-owned promotion ledger and durable
admission receipt. Consume at durable admission. If the daemon stops after the
receipt commits and before the provider outcome is known, mark the turn
interrupted and do not automatically replay it. A human may explicitly submit
a new turn.

## Consequences

### Positive

- A promotion has a canonical durable owner and stable lifecycle identity.
- Busy/rejected admission preserves the pending record; duplicate retries
  converge on one turn receipt.
- No raw shell output or general-purpose context data is introduced into
  persistence.
- Restart behavior is explicit and prevents automatic duplicate provider
  submissions.

### Negative

- An accepted turn may be interrupted before provider delivery is confirmed.
- Turn admission, capability negotiation, and TUI acknowledgement/query all
  need additive changes and migration tests.

### Neutral or deferred

- Provider-level idempotency and replay remain separate work; no exactly-once
  network-delivery claim is made.
- Workspace-owned human-shell execution (C002) remains blocked until C001/C004
  close.

## Compatibility and migration

Use additive SQLite migrations after v70 (v71 promotion ledger, v72 turn
admission receipts). The typed `TurnAdmissionReceipt` response identifies a
daemon that implements durable receipt semantics; a legacy `Ack` remains
ordinary prompt acceptance and must not be represented as receipt-backed
idempotency. Older daemons cannot stage promotions and clients must not report
a staged state without the typed stage acknowledgement. Stage payloads are
redacted UTF-8 and bounded to 32 KiB; only the stage ledger stores them. No
legacy conversation rows are backfilled or reinterpreted.

## Security and reliability implications

Stage/query/consume handlers derive principal and session/project/workspace
binding from trusted transport authority and the canonical session binding.
Observer-only callers cannot stage or consume. Promotion IDs are opaque and
idempotent within one principal/session; payloads expire under finite retention.
Redaction failure, stale binding, unsupported capability, expired state, and
storage failure fail closed. No raw command output, shell environment, or
credentials are persisted. The durable admission receipt is the consumption
boundary; interrupted accepted turns are not replayed automatically.

## Verification

Prove transactional stage and admission behavior for duplicate
stage, duplicate submit, busy/rejected admission, authorization denial, stale
workspace/session, expiry, concurrent requests, reconnect/restart, and an
interrupted accepted turn. Capture fake-provider input to prove the redacted
projection appears once in the intended accepted turn composition. Verify old
daemon capability behavior and run the focused human-shell/session tests plus
`scripts/verify.sh quick`.

## Supersession

None.
