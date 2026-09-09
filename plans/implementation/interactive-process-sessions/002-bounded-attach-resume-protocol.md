# Interactive Process Sessions Milestone 002 — Bounded Attach/Resume Protocol and Authority

Status: blocked

Repository baseline: `9dfbc6e1f842a76ab6154d1bd99cbadfdff4b2e6`

Source roadmap: `plans/subsystems/interactive-process-sessions-roadmap.md#M002--bounded-attach-resume-protocol-and-authority`

Long-term requirements: `plans/000-long-term-specification.md#4-architectural-principles`, `#17-job-scheduling-and-execution-backends`, `#27-security-requirements`.

Applicable ADRs: none for local protocol extension. Primary class: capability.

## 1. Objective

Expose M001 through versioned bounded daemon operations for create/list/attach/detach/input/resize/terminate/resume, with transport-derived attachment ownership, sequence cursors/resync and explicit authorization seams.

## 2. Why this milestone is ready

Blocked on M001. Native request/event envelopes, connection ownership, bounded queues and replay/resync patterns are already established by session projections and daemon socket/WebSocket work.

## 3. Current implementation evidence

No PTY protocol exists. Existing projection transport provides useful lifecycle patterns: daemon-issued client identity, bounded queues, owner-scoped subscriptions, disconnect cleanup and resume cursors. These should inform but not be duplicated wholesale; PTY byte/output semantics remain a separate local-process stream.

## 4. Invariants that must not regress

Request payload cannot claim process ownership/capabilities; output/input queues bounded; one process engine owner; attach is not human Session attachment; disconnect does not automatically kill unless explicit policy; terminate requires authority; secret environment never returned; lag/resync is typed.

## 5. Scope

In: protocol DTOs/capability negotiation, process handle/list metadata, create/attach/detach/input/resize/terminate, bounded output event chunks/cursors/resync, client connection ownership and cleanup, local-owner authorization seam. Out: TUI rendering, team-role final integration if auth roadmap not yet closed, remote node transport, raw session observation.

## 6. Required production changes

Protocol: additive interactive-process capability and typed operations/events with size/sequence bounds. Core: daemon handler family delegates to M001 service; attachment registry keyed by client/connection and process handle. Runtime: per-attachment bounded queue or shared broadcast with explicit lag; no unbounded task creation. Security: construct authority from transport; later identity roadmap plugs semantic capabilities without wire change. Docs: lifecycle/ownership.

## 7. Ordered work packages

A — define handle metadata, operation DTOs, output sequence/chunk bounds and capability negotiation.

B — implement daemon handlers and attachment ownership using M001 only.

C — implement output cursor/resume/resync and bounded queue/backpressure.

D — connection close/EOF/panic cleanup and multi-client attachment policy tests.

E — authorization seam/negative tests and docs.

## 8. Failure, cancellation, restart, and contention semantics

Create only returns handle after successful admitted spawn. Failed attach/input/resize never mutates another process. Disconnect drops attachment but follows process owner/idle policy. Daemon restart invalidates ephemeral handles with typed gone/resync response. Lag reports last available sequence; input write errors surface terminal state.

## 9. Compatibility and migration

Older clients ignore capability. No existing protocol variant is repurposed. If auth roadmap lands later, semantic capability checks replace LocalOwner default through the same authority context.

## 10. Required tests

Create/list/attach/detach/input/resize/terminate; spoofed owner/process ID; two clients policy; disconnect without accidental kill; explicit kill; output sequence/lag/resync; queue saturation; writer failure; daemon shutdown/restart gone-handle; unknown capability compatibility.

## 11. Required verification commands

```bash
cargo test --workspace interactive_process --no-fail-fast
cargo test --workspace transport --no-fail-fast
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

## 12. Documentation updates

Protocol/core/server interactive-process docs and ownership/security matrix.

## 13. Acceptance criteria

A client can create and reattach to a live local PTY within bounds, resume output or receive typed resync, and cannot control another process by supplying IDs; disconnect/lag/shutdown cleanly release transient resources.

## 14. Stop conditions

M001 not closed; required semantics imply durable cross-restart/cross-node terminal identity; transport would require unbounded byte queues; authorization would be caller-supplied.

## 15. Closure evidence required

M001 closure, wire/capability contract, ownership/negative matrix, saturation/reconnect/shutdown tests, resource baseline evidence, exact verification results.

## 16. Handoff notes

Reuse connection-task ownership and bounded-channel patterns, not session-projection event types themselves. Terminal output is not the canonical collaboration projection.
