# Presence and Observation Milestone 001 — Project-Scoped Presence Leases

Status: ready for handoff (unblocked by identity M003 closure at `plans/closure/identity-authorization-audit/003-status.md`)

Repository baseline: `9dfbc6e1f842a76ab6154d1bd99cbadfdff4b2e6`

Source roadmap: `plans/subsystems/presence-observation-roadmap.md#M001--project-scoped-presence-leases`

Long-term requirements: `plans/000-long-term-specification.md#14-presence-and-real-time-team-awareness`, `#27-security-requirements`; `plans/002-long-term-roadmap.md#phase-7--presence-and-collaborator-awareness`.

Applicable ADRs: none. Primary class: infrastructure.

## 1. Objective

Add daemon-owned project-scoped ephemeral presence leases for canonical principals, client attachments and bounded session/agent activity summaries, with heartbeat/idle/expiry/reconnect and privacy semantics.

## 2. Why this milestone is ready

Blocked on `identity-authorization-audit` M003 because presence visibility must be enforced by the canonical daemon authorization seam. Multi-project sessions, durable agent-run projections and client registry foundations are closed.

## 3. Current implementation evidence

`ClientRegistry` records client id/name/connection time/attached sessions/capabilities but not canonical principal or project presence. Session projections contain bounded activity state. No canonical presence lease owner exists. Presence is explicitly deferred from the closed session-projection subsystem.

## 4. Invariants that must not regress

Presence is ephemeral; membership/authorization is durable authority; unauthorized projects are indistinguishable from absent; one principal may have multiple clients/sessions; lease expiry cannot delete session history; high churn cannot create unbounded timers/tasks/maps.

## 5. Scope

In: presence key/value/lease model, activity contribution aggregation, heartbeat/activity renewal, idle/expiry/disconnect/reconnect, privacy-filtered query/snapshot/events, bounded cleanup. Out: TUI rendering (M002), observation (M003), chat typing/read markers, durable history, distributed node presence.

## 6. Required production changes

Core/domain: ephemeral presence service keyed by typed principal/project/client/session references. Storage: none by default; settings only if current config needs lease intervals. Protocol: bounded presence snapshot/update capability with semantic activity enums. Runtime: one bounded cleanup strategy rather than task-per-lease. Security: query/update derives principal from connection; authorize project visibility. Observability: gauges for live leases/expiry/churn, no identity secrets.

## 7. Ordered work packages

A — define lease/activity DTOs and aggregation rules for multiple clients/sessions.

B — implement daemon presence service with monotonic expiry/idle and bounded cleanup.

C — integrate connection/session/agent meaningful-activity updates without making those systems depend on presence for correctness.

D — add authorized project snapshot/subscription or existing event integration and privacy negatives.

E — restart/churn/bounds documentation and tests.

## 8. Failure, cancellation, restart, and contention semantics

Disconnect shortens/expires connection contribution; reconnect creates/renews without duplicate principal rows. Daemon restart may clear all leases and rebuild from active connections. Late heartbeat for an expired/old connection generation cannot resurrect stale state. Contended renew/remove is deterministic.

## 9. Compatibility and migration

No durable migration expected. Older clients ignore unknown presence capability/events. Existing client snapshots remain compatible while new principal/project fields are versioned/defaulted as needed.

## 10. Required tests

Heartbeat/expiry/idle; disconnect/reconnect; two clients one principal; several sessions per principal; activity aggregation; unauthorized project privacy; high-churn bounded memory/task count; daemon restart; stale-generation heartbeat.

## 11. Required verification commands

```bash
cargo test --workspace presence --no-fail-fast
cargo test --workspace client_registry --no-fail-fast
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

## 12. Documentation updates

Presence architecture/protocol/core docs and terminology cross-links; clearly state ephemeral/non-authoritative semantics.

## 13. Acceptance criteria

Authorized project member receives bounded accurate current collaborator/session activity; stale disconnects expire; multi-client aggregation works; restart does not fabricate durable presence; unauthorized principal gets no project/activity signal.

## 14. Stop conditions

Authorization M003 not closed; implementation needs durable presence history; efficient cleanup would require a new scheduler/runtime rather than a local bounded service.

## 15. Closure evidence required

Dependency closure, lease state machine, privacy matrix, churn/restart/task-bound tests, protocol compatibility, exact verification results.

## 16. Handoff notes

Presence must never be consulted to authorize an operation. It is a projection of liveness/activity only.
