# Post-Implementation Maintainability Closure Roadmap

Status: ready

Repository baseline: `f04866b1d817eee06fb3e51e73cc8f7b6679ad86`

Long-term references:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#42-explicit-ownership`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/002-long-term-roadmap.md#phase-19--operational-hardening-and-scale-closure`

Related completed work:

- `plans/subsystems/post-audit-maintainability-surface-roadmap.md` — closed; established the repository precedent for correctness-neutral physical decomposition.
- `plans/subsystems/residual-runtime-consolidation-roadmap.md` — closed; decomposed `CoreDaemon` while preserving one composition/lifecycle authority.
- `plans/subsystems/identity-authorization-audit-roadmap.md` — closed; defines the canonical identity, authorization, and audit ownership boundary.
- `plans/subsystems/project-collaboration-roadmap.md` — closed; defines the canonical collaboration/message/action ownership boundary.

No new ADR is required. This work preserves the accepted owners and contracts. If implementation discovers that a canonical owner, durable contract, or authority boundary must change, M001 must stop rather than improvise a new architecture.

## 1. Purpose

The September 2026 identity/presence/collaboration/interactive-process/Tool-Program implementation tranche is functionally closed. The remaining issue is local maintainability concentration inside two newly expanded canonical `codegg-core` modules plus a small stale planning-registry artifact.

This roadmap owns one bounded polish milestone. It does not reopen the feature work that produced these modules and does not authorize further architecture convergence.

## 2. Current state

At the baseline, semantic ownership is substantially cleaner than before the implementation tranche: authorization is centralized under the accepted identity/auth/audit boundary; project chat and typed chat actions are centralized under collaboration; the old duplicate team/shell-session surfaces are gone; and the daemon/scheduler remain authoritative.

The residual maintenance concern is physical concentration rather than duplicated ownership:

- `crates/codegg-core/src/authorization.rs` now contains several thousand lines spanning authorization domain descriptors/policy evaluation, authenticated principal/session/project context, persistence helpers, capability/role mapping, and related tests.
- `crates/codegg-core/src/collaboration.rs` now contains several thousand lines spanning channels/messages, validation, persistence, structured chat actions, action idempotency/status, audit-safe metadata, and related tests.
- `plans/registry.md` contains a stale sentence claiming that "These four plans may be implemented in parallel" after those plans have already disappeared from the dependency-ready section.

The large modules are not evidence that ownership is wrong. The goal is to improve source locality while retaining the exact same semantic owners and external behavior.

## 3. Invariants

- Authorization remains one canonical subsystem. Physical files must not become independent authorization engines.
- Collaboration remains one canonical subsystem. Channels/messages and structured actions must not acquire separate workflow/scheduler authority.
- Transport-derived identity and daemon authorization remain authoritative; no module may mint or infer new authority from DTOs, paths, UI state, or message text.
- Free-text collaboration messages remain inert. Only separately typed/authorized action requests may create execution work.
- Structured chat actions continue to submit jobs through the canonical `JobSubmissionService`/scheduler boundary.
- Audit attribution and redaction remain unchanged.
- Persistence schemas, storage layout version, wire protocol, public error codes, serialized enum/string values, and capability semantics remain unchanged.
- Restart, idempotency, ordering, revocation, cancellation, projection, and boundedness semantics remain unchanged.
- Public API/re-export compatibility must be preserved unless a currently public item is proven crate-internal and tightening visibility is behaviorally safe.
- No new coordinator, service bus, policy engine, repository layer, DI framework, trait hierarchy, or generic abstraction may be introduced solely to make files smaller.

## 4. Non-goals

- New authorization roles, capabilities, policy semantics, authentication mechanisms, or audit events.
- New collaboration features, DMs, channel administration, workflow semantics, execution-from-free-text, or new action kinds.
- Storage normalization or schema migrations.
- Protocol/DTO cleanup or renaming.
- Reworking `CoreDaemon`, scheduler, projection, TUI, or Tool Program architecture.
- Decomposing `interactive_process.rs`, `tool_program.rs`, or other large modules absent a separately evidenced milestone.
- Line-count gates, dependency graphs enforced by CI, new test lanes, scanners, coverage requirements, benchmark gates, or binary-size gates.
- Splitting code into many tiny modules merely to meet an aesthetic size target.

## 5. Target source shape

The exact module names are implementation-time decisions after a responsibility map, but the expected direction is:

```text
crates/codegg-core/src/
  authorization.rs              # facade/domain surface and high-level policy contract
  authorization/
    policy.rs                   # existing policy/capability/role evaluation
    identity.rs                 # authenticated/canonical authority context helpers, if cohesive
    store.rs                    # existing persistence/query helpers, if cohesive
    tests.rs or colocated tests # only where it improves locality

  collaboration.rs              # facade/domain surface and shared contract
  collaboration/
    messages.rs                 # channel/message validation + domain behavior
    store.rs                    # existing durable channel/message/action persistence
    actions.rs                  # structured-action validation/idempotency/status/audit-safe shaping
    tests.rs or colocated tests # only where it improves locality
```

These names are illustrative, not mandatory. Prefer two or three coherent extraction units per subsystem over a deep module tree. Existing public import paths should remain stable through re-exports where needed.

## 6. Dependency graph

M001 has no unresolved hard dependency. Identity/auth/audit, project collaboration, residual runtime consolidation, presence/observation, interactive process sessions, and Tool Program expansion are all closed at the baseline.

The two older operational-evidence blockers currently recorded in `plans/registry.md` are unrelated to this polish milestone and do not block implementation.

## 7. Milestone

### M001 — Authorization/collaboration physical decomposition and registry reconciliation

Class: polish.

Implementation plan:

- `plans/implementation/post-implementation-maintainability-closure/001-authorization-collaboration-physical-decomposition.md`

Objective: reduce local maintenance concentration in the two canonical core modules through responsibility-oriented code movement, preserve all behavior/authority/storage/protocol contracts, and leave the planning registry internally consistent.

Exit conditions:

- a before/after responsibility map demonstrates that extracted modules each have one stable conceptual owner;
- `authorization.rs` is primarily a facade/domain-policy surface rather than a catch-all implementation file;
- `collaboration.rs` is primarily a facade/domain surface rather than containing all message/action/storage implementation details;
- no new execution, policy, persistence, scheduler, or coordination owner exists;
- focused authorization, audit, collaboration, restart/idempotency, and structured-action tests remain green;
- ordinary broad verification remains the existing minimal repository contract;
- `plans/registry.md` contains no stale dependency-ready prose and accurately records this milestone/closure state.

## 8. Verification strategy

Verification remains deliberately light and proportional to a source-organization refactor. Use focused existing suites for moved behavior, then the repository's normal broad posture:

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

Do not add CI or verification infrastructure. Add a new test only when code movement exposes an invariant that is not already covered.

## 9. Stop conditions

Stop and report rather than broadening scope if:

- decomposition requires changing authorization semantics or canonical ownership;
- decomposition requires a schema/protocol/storage-layout migration;
- borrow/lifetime pressure appears solvable only by cloning mutable authority state, adding global state, or detaching async work;
- a proposed trait/facade/repository abstraction has only one implementation and no independent contract value;
- moving persistence code would change transaction boundaries, ordering, idempotency, or restart behavior;
- implementation would need to alter scheduler/job or daemon authorization paths;
- current HEAD has materially restructured these modules since the baseline.

## 10. Completion definition

This roadmap closes when M001 has an accepted closure record showing a correctness-neutral physical decomposition, minimal verification evidence, unchanged public/durable behavior, and a truthful compact registry. No additional polish milestone should be inferred from file size alone.

## 11. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 | ready | `plans/implementation/post-implementation-maintainability-closure/001-authorization-collaboration-physical-decomposition.md` | `plans/closure/post-implementation-maintainability-closure/001-status.md` | — |
