# Architecture Convergence M005 — Durable Run Rerun/Replay Completion

Status: ready

Repository baseline: `3c4890035513cd4d74430b6f64523c8be676024e`

Source subsystem roadmap:

- `plans/subsystems/architecture-convergence-incomplete-verticals-roadmap.md`

Hard dependency:

- M003 Git ownership convergence must close.

Relevant long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#4.7-correctness-before-transparent-magic`
- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/003-planning-process.md`

Primary class: capability

## 1. Objective

Complete the already-exposed rerun capability so an eligible historical run can be re-executed as a new durable run through the normal daemon/scheduler/tool authority. Replace the current TUI placeholder with a typed end-to-end rerun request that creates fresh execution identity, reconstructs only safe durable inputs, reacquires secrets/credentials where required, records parent-child linkage, and emits normal run projections.

Rerun is not in-place replay. Historical runs remain immutable evidence.

## 2. Explicit non-goals

M005 must not:

- store raw authenticated Git URLs, provider keys, authorization headers, or shell secrets to make rerun easy;
- mutate the original run record into a second attempt;
- bypass scheduler admission or worktree policy;
- silently rerun in a dirty/current UI checkout when the original run required isolated repository state;
- guarantee byte-for-byte deterministic reproduction of model responses or external services;
- implement arbitrary historical transcript replay;
- add automatic retry loops beyond existing explicit policies.

## 3. Current implementation evidence to inspect

Re-inspect at least:

- `src/tui/app/types.rs` `RunRerun` / `ShellRerun` messages;
- `src/tui/components/dialogs/run_detail.rs`;
- TUI command dispatch/handler currently documented as a placeholder;
- `crates/codegg-protocol/src/core.rs` `RunRerunLinked` and relevant run requests/events;
- `architecture/run_store.md`;
- RunStore run manifest/rerun metadata and `can_rerun` derivation;
- `docs/validation/git-rerun-secret-lifecycle.md`;
- Git/run reconstruction helpers after M003;
- scheduler/job/AgentRun submission APIs;
- worktree creation/base/result commit validation;
- projection/replay safe-publication logic.

## 4. Required rerun contract

A rerun request must identify the historical parent run by typed durable identity and perform host-side validation. The service must derive or reconstruct a safe rerun specification containing only durable non-secret inputs plus explicit references needed for credential reacquisition.

Required outcomes:

```text
eligible -> new child run accepted
ineligible_missing_spec
ineligible_missing_or_invalid_base
ineligible_secret_reacquisition_required_or_failed
ineligible_authority_changed
ineligible_runtime_asset_or_provider_unavailable
scheduler_denied
cancelled
```

Exact names may differ.

A successful rerun must:

- allocate a new run identity;
- preserve owner/session/project/workspace authority rules;
- create fresh scheduler/job attempt identity as appropriate;
- validate repository/base state through M003's canonical Git owner;
- reacquire current credentials/secrets rather than reading persisted raw values;
- link parent and child durably;
- emit `RunRerunLinked` or the canonical equivalent;
- show the new child in TUI/projection consumers;
- never modify the historical parent manifest/result.

## 5. Ordered work packages

### WP1 — Define rerunnable specification

Determine which durable run classes are safely rerunnable and what minimal non-secret specification is required. Do not overclaim support. It is acceptable for initial support to cover a bounded subset such as structured build/test/tool runs before every possible run kind.

`can_rerun` must be derived from actual reconstructability, not merely run status.

### WP2 — Daemon/core rerun request

Add or complete a typed daemon request/service operation. Authorization must be evaluated at rerun time using the current principal/project/session/agent authority, not assumed from the historical run.

### WP3 — Secret and credential reacquisition

Implement explicit reacquisition hooks for any rerunnable operation that may need credentials. If the credential cannot be safely reacquired, return an actionable denial rather than persisting/using stale secret material.

### WP4 — Git/worktree reconstruction

For code-mutating or Git-dependent runs, use M003's canonical Git/worktree workflow. Validate recorded base/result/repository identity. Create a fresh worktree/lease where current policy requires one.

### WP5 — Scheduler submission and linkage

Submit the new run through existing scheduler/AgentRun/Tool Program authority. Persist parent-child rerun linkage atomically enough that accepted reruns cannot become untraceable children.

### WP6 — TUI completion

Replace the placeholder `RunRerun` path with the typed request. Surface accepted child identity, denial diagnostics, and progress through existing projections. Remove the fake/sentinel shell-rerun behavior once no supported caller needs it.

### WP7 — Restart and replay evidence

Verify that after daemon restart the parent remains immutable, the child remains visible, and linkage/projection reconstruction remains correct.

### WP8 — Documentation

Update `architecture/run_store.md`, protocol docs, rerun secret-lifecycle validation docs, and TUI help if needed.

## 6. Storage, protocol, migration, compatibility

Prefer existing run linkage storage/events. If durable parent-child linkage is not currently stored authoritatively, add the smallest schema extension and migration required. The schema must not store raw secrets or unaudited command text that current run storage intentionally redacts.

Protocol changes should be additive. Preserve readers of existing run manifests/events.

## 7. Security and failure semantics

Current authorization is authoritative. A user who could execute the historical run but no longer has permission must not rerun it.

Credential values must be reacquired from current secret/provider/Git credential mechanisms and remain outside durable rerun metadata.

Dirty/conflicted/missing Git state fails closed. Model/provider nondeterminism is acceptable and should be documented; rerun means re-execute the same structured intent under current valid authority, not reproduce historical bytes.

Cancellation applies to the child run like any ordinary run.

## 8. Verification

Focused coverage must include:

- eligible rerun -> fresh child identity;
- immutable parent;
- durable/linkage event correctness;
- restart visibility;
- changed/revoked authorization denial;
- secret reacquisition success/failure without persisted raw values;
- missing/invalid Git base denial;
- TUI request path no longer emits placeholder `id: 0` behavior;
- cancellation.

Then run:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

## 9. Acceptance criteria

M005 is complete only when:

- at least one meaningful supported run class reruns end-to-end from TUI through daemon/scheduler;
- the rerun receives fresh durable execution identity;
- parent/child linkage survives restart;
- `can_rerun` reflects actual reconstructability;
- secrets are reacquired rather than persisted/replayed;
- Git-dependent reruns use the canonical M003 owner;
- placeholder rerun behavior is removed;
- focused and quick verification pass.

## 10. Stop conditions

Stop if rerun requires weakening redaction/secret persistence, bypassing current authorization, or inventing a second scheduler/run model. Narrow supported rerun classes instead.

## 11. Closure evidence required

Record:

- implementation commits;
- supported/ineligible run-class matrix;
- storage/protocol migration evidence if any;
- secret lifecycle evidence;
- Git/base/worktree evidence;
- TUI end-to-end evidence;
- restart/linkage evidence;
- verification outcomes and residual limitations.
