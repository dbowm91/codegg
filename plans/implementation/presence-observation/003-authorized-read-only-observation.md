# Presence and Observation Milestone 003 — Authorized Read-Only Session Observation

Status: ready for handoff

Unblocked by M002 closure (`plans/closure/presence-observation/002-status.md`); session-projections M012 interface already closed.

Repository baseline: `9dfbc6e1f842a76ab6154d1bd99cbadfdff4b2e6`

Source roadmap: `plans/subsystems/presence-observation-roadmap.md#M003--authorized-read-only-observation`

Long-term requirements: `plans/000-long-term-specification.md#15-read-only-session-observation`, `#27-security-requirements`; roadmap Phase 8.

Applicable ADRs: none. Primary class: capability.

## 1. Objective

Allow an authorized project member to select another session and follow its canonical projection snapshot/replay/live stream in an explicitly read-only TUI mode with hard control/mutation negatives.

## 2. Why this milestone is ready

Blocked on M001/M002 and identity authorization M003 transitively. Session-projections M012 is already closed and supplies replay/redaction/artifact/transport lifecycle; no new streaming architecture is needed.

## 3. Current implementation evidence

Projection subscriptions already carry transport-derived access context, semantic observe capabilities, project/session scope checks, bounded queues, replay/resync and redaction. The TUI can render session activity but does not yet provide the team observation product state. Existing docs explicitly reserve team authorization to a future layer.

## 4. Invariants that must not regress

Observation cannot answer permission/question requests, steer turns, cancel runs/jobs, mutate files/worktrees, change provider/model/agent settings or acquire control merely through attachment. Redaction before durable replay remains authoritative. Provider-hidden reasoning is never exposed. Raw terminal frames are not observation protocol.

## 5. Scope

In: `session.observe` authorization integration, session chooser/action from collaborators/session list, observer subscription adapter over existing projection APIs, read-only TUI state/banner/focus, reconnect/replay/resync, explicit forbidden-control tests. Out: control transfer/suggestions, chat persistence (project-collaboration), raw PTY observation, remote node replication.

## 6. Required production changes

Core/protocol: use canonical authorization to construct projection access context for target session and reuse subscription ownership. Frontend: observer mode state renders canonical activity and disables/reroutes mutating commands/input. Security: authorization is rechecked on subscribe/resume as policy requires; artifact reads retain scope. Docs: observer visibility/control matrix.

## 7. Ordered work packages

A — map existing projection capabilities to canonical `session.observe` decision and remove synthetic authority assumptions.

B — implement observe/stop-observing frontend flow on existing subscription/replay APIs.

C — enforce read-only command/input policy centrally in TUI observer state; add explicit negatives for every control family.

D — reconnect/lag/resync/multiple-observer/load/privacy tests.

E — observer UX/docs and collaboration input seam placeholder only; no chat storage yet.

## 8. Failure, cancellation, restart, and contention semantics

Observer disconnect tears down only observer-owned subscription. Target session disconnect does not grant control and replay can resume when history permits. Revoked observe authorization prevents new/resumed delivery and cleans transient state. Multiple observers share no mutable ownership.

## 9. Compatibility and migration

Versioned observe capability; old clients/servers remain usable without observation. No new projection storage format unless existing M012 extension mechanism requires additive fields.

## 10. Required tests

Observe allow/deny; unauthorized project non-enumeration; attempted steering/cancel/permission/question/model/provider/file/Git mutation; redacted prompt/tool output snapshot+replay+live; reconnect/resume/resync; target owner disconnect; multiple observers and queue bounds; artifact handle scope.

## 11. Required verification commands

```bash
cargo test --workspace projection --no-fail-fast
cargo test --workspace observation --no-fail-fast
cargo test --workspace tui --no-fail-fast
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

## 12. Documentation updates

Session projection, TUI, protocol, security and observer-mode docs with explicit allowed/forbidden matrix.

## 13. Acceptance criteria

Authorized user can follow another active session in real time and across reconnect from canonical projection state; ordinary observer input causes no mutation/control; unauthorized users learn nothing; redaction/artifact bounds are unchanged.

## 14. Stop conditions

Dependencies not closed; implementation needs raw screen mirroring or a second replay stream; existing projection contract cannot express required safe visibility without an architecture decision.

## 15. Closure evidence required

Dependency references, allow/deny and forbidden-operation matrix, real transport replay/resync evidence, multi-observer bounds, secret-redaction proof, exact commands/results.

## 16. Handoff notes

Do not implement project chat here. It is sufficient to expose a stable frontend focus seam that the project-collaboration roadmap will consume.
