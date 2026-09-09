# Identity, Authorization, and Audit Milestone 005 — Audit Instrumentation and Attribution Closure

Status: blocked

Repository baseline: `9dfbc6e1f842a76ab6154d1bd99cbadfdff4b2e6`

Source roadmap: `plans/subsystems/identity-authorization-audit-roadmap.md#M005--audit-instrumentation-and-attribution-closure`

Long-term requirements: `plans/000-long-term-specification.md#22-audit-architecture`; roadmap Phase 11.

Applicable ADRs: none. Primary class: capability.

## 1. Objective

Instrument the canonical authentication/authorization/project/session/agent/tool/process/file/Git/worktree/job/provider/configuration surfaces so representative activity is reconstructable from principal through causal execution chains using the M004 store.

## 2. Why this milestone is ready

Blocked on M004. Durable agent runs, jobs, worktrees, assets and provider connections are already implemented and expose correlation identities.

## 3. Current implementation evidence

Subsystems emit traces/projections/run records but not one structural audit stream. M004 will supply append/query/redaction; this milestone connects required Phase-11 actions without capturing arbitrary bodies.

## 4. Invariants that must not regress

Instrumentation is emitted by canonical owners, not duplicate wrappers; actor/correlation derives from trusted context; audit failure policy is preserved; secrets/body retention rules apply uniformly; high-volume paths remain bounded; chat remains separate.

## 5. Scope

In: executable event-coverage matrix and instrumentation for authentication, authorization/membership, sessions/prompts structural metadata, provider/model selection, agent delegation, permission/tool/command, file mutation, Git/worktree, jobs, config/assets, audit export. Out: full prompt/file/output retention by default, project chat actions (collaboration M003), remote node events.

## 6. Required production changes

Add narrow audit append calls at existing state-transition owners using typed builders. Link correlation/causation through request -> session/turn -> run/task -> job/tool/process/Git/worktree. Ensure permission/authorization decision references are captured. Add counters/diagnostics for bounded audit failure/backpressure.

## 7. Ordered work packages

A — create required event matrix mapping action to owner/actor/scope/decision/metadata.

B — instrument identity/session/provider/config/asset control-plane actions.

C — instrument agent/tool/process/file/Git/worktree/job execution actions and causation.

D — end-to-end attribution fixtures and high-volume/negative secret tests.

E — docs/coverage guard ensuring new privileged canonical operations cannot land unclassified where practical.

## 8. Failure, cancellation, restart, and contention semantics

Cancellation/completion produce terminal structural events where canonical state does so. Duplicate/replayed state transitions use deterministic/idempotent event identity where required. Audit pressure does not create unbounded queues or reorder canonical state.

## 9. Compatibility and migration

No historical logs are promoted to authoritative audit. Older records lacking origin principal remain explicitly legacy rather than attributed falsely.

## 10. Required tests

Authentication/denial/membership chain; prompt->root run->child->tool/job->Git/worktree chain; provider selection; permission allow/deny; cancellation/failure; asset/config change; secret-negative metadata; duplicate/retry; high-volume bounded writer; audit reader authorization.

## 11. Required verification commands

```bash
cargo test --workspace audit --no-fail-fast
cargo test --workspace agent --no-fail-fast
cargo test --workspace worktree --no-fail-fast
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

## 12. Documentation updates

Audit action/owner matrix, architecture diagrams, retention/visibility/operator docs.

## 13. Acceptance criteria

A project owner can query a representative operation and trace it to authenticated principal, authorization decision, project/session/turn, agent descendants, jobs/tools and worktree/Git outcome as applicable; required privileged actions have named audit owners; secrets remain absent.

## 14. Stop conditions

M004 not closed; instrumentation requires wrapping/bypassing canonical owner; event volume cannot be bounded under M004 contract; chat/remote-node scope would be pulled in.

## 15. Closure evidence required

Complete required-event matrix, end-to-end chains, negative secret scans/tests, pressure/failure results, exact command output, unresolved event gaps by severity.

## 16. Handoff notes

Prefer structural metadata/digests/handles over content bodies. This milestone is the hard dependency for privileged structured chat actions and later distributed execution audit.
