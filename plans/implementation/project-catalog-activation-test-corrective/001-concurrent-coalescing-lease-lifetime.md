# Project Catalog Activation Test Corrective Milestone 001 — Concurrent Coalescing Lease Lifetime

Status: implemented

Repository baseline: `6a679bd2` (fix head `30e00916`)

Source corrective addendum:

- `plans/subsystems/project-catalog-activation-test-corrective-addendum.md`

Source implementation plan this corrective addresses:

- `plans/implementation/project-catalog/003-lazy-activation-and-health.md`

Source closure record this corrective addresses:

- `plans/closure/project-catalog/003-status.md` (implementation `27cbd43`)

Source subsystem roadmap:

- `plans/subsystems/project-catalog-roadmap.md#milestone-3--lazy-activation-and-health`
  (Milestone 3 — lazy activation and health)

Long-term requirements:

- `architecture/project-catalog.md` — bounded owner leases and lazy activation

Applicable ADRs:

- none required. The correction restores a test's agreement with the
  documented lease semantics; it changes no ownership, boundary, or contract,
  and the semantics it now asserts are already implemented in
  `src/core/project_activation.rs`.

Primary class: infrastructure

## 1. Objective

Make `concurrent_same_owner_activation_coalesces_scope_and_bundle` assert
same-owner coalescing deterministically, without weakening the assertion or
touching production code.

## 2. Why this milestone is ready

- The defect is fully root-caused against the implemented lease semantics, not
  inferred from a red run.
- The fix is test-only. No hard dependency, no ADR, no production behavior.
- The precondition the test needs — all eight leases alive at once — is
  expressible with an existing primitive, so no new mechanism is introduced.

## 3. Current implementation evidence

`ProjectActivationLease` owns a reference count on the shared
`ActiveActivation`. `release_inner` decrements it and removes the registry
entry when the count reaches zero
(`src/core/project_activation.rs:178-192`). `acquire` reuses an existing
entry only when `ActiveActivation::retain` succeeds, which increments the
count (`src/core/project_activation.rs:283-294`).

The test at `tests/project_activation.rs:121-151` spawned eight tasks whose
bodies returned `(lease_id, coalesced)`. The `ProjectActivationLease` was
therefore dropped at the end of each task body. The assertion
`results.iter().all(|(lease_id, _)| lease_id == &results[0].0)` therefore only
held if every later task acquired before any earlier task released.

Sibling evidence that the production behavior is correct and already covered:
`activation_refreshes_assets_and_is_idempotent_per_owner`
(`tests/project_activation.rs:69`) asserts the same coalescing property
synchronously, holding both leases, and is stable.

## 4. Invariants that must not regress

- The assertion still requires all eight lease ids to be equal.
- Eight-way concurrency and `flavor = "multi_thread", worker_threads = 2`
  are retained.
- `daemon.project_activation.active_count() == 0` and
  `daemon.workspace_services.active_count() == 1` are still asserted
  afterwards.
- `ProjectActivationRegistry::acquire`, `release_inner`, handle accounting,
  capacity policy, lease TTL, and eviction are unchanged.
- Every other test in `tests/project_activation.rs` is unchanged.
- No historical closure record is edited.

## 5. Scope

### In scope

- The lease lifetime inside
  `concurrent_same_owner_activation_coalesces_scope_and_bundle`.

### Explicitly out of scope

- Any production change in `src/core/project_activation.rs` or
  `src/core/daemon_refresh.rs`.
- The other five tests in `tests/project_activation.rs`.
- The `Desktop E2E` `m004-session.e2e.ts` failure observed in the same run
  window. That is a separate flake with a different mechanism (a 60 s wait in
  a file whose comparable waits are 120–180 s) and is not touched here.
- Widening the test, adding a new assertion class, or adding a new guard.
- Any edit to a historical project-catalog closure record.

## 6. Required production changes

None. This milestone changes no production code.

### Core/domain

None.

### Storage and migrations

None.

### Protocol and DTOs

None.

### Runtime and concurrency

None in production. The test's concurrency is *strengthened*, not reduced: it
now guarantees genuine overlap instead of hoping for it.

### Frontend or operator surface

None.

### Security and authorization

None.

### Documentation and static guards

- No new static guard. The regression evidence is the deterministic mechanism
  demonstration plus repeated and load-induced runs, recorded in the closure.
- Planning registration in `plans/registry.md` and the corrective addendum.

## 7. Ordered work packages

### Work package A — Hold the leases across the acquisition window

Intent: make the precondition the assertion describes hold by construction.

Required changes: in
`concurrent_same_owner_activation_coalesces_scope_and_bundle`, create a
`Barrier(8)` shared by all eight tasks. Each task extracts `lease_id` and
`coalesced` from its activation, awaits the barrier, and only then returns —
so no lease drops until all eight have been acquired. Add a comment
explaining that coalescing is a concurrency property and that
`ProjectActivationLease::drop` removes the registry entry at zero handles.

Acceptance evidence: the assertion and both post-conditions are unchanged; the
test passes deterministically.

## 8. Failure, cancellation, restart, and contention semantics

The barrier is scoped to the test and holds no production state. If a task
panics before reaching the barrier, the remaining tasks would block, so the
test would hang rather than silently pass; this is acceptable for a test whose
inputs are local and whose failure mode is a panic on `.unwrap()`. The barrier
is not `select`-guarded because adding a timeout would reintroduce exactly the
timing sensitivity this milestone removes.

Because all eight leases are now held simultaneously, the test also exercises
a stronger property than before: eight concurrent acquisitions of one
activation, not eight sequential acquire/release cycles.

## 9. Compatibility and migration

None. No schema, storage, protocol, configuration, or public API change. MSRV
1.89 is preserved; `tokio::sync::Barrier` is long stable.

## 10. Required tests

### Focused unit tests

None added. The fix is to an existing integration test; adding a second test
asserting the same thing would duplicate coverage.

### Integration tests

- `cargo test --test project_activation` — all six tests.
- `concurrent_same_owner_activation_coalesces_scope_and_bundle` run
  repeatedly, including under induced CPU load and as many concurrent
  instances, to demonstrate stability.

### Restart and recovery tests

Not applicable; the existing `restart_hydrates_catalog_and_asset_metadata_without_activation`
is unchanged and still passes.

### Contention and cancellation tests

The barrier-based test is itself the contention test. Repeated and
load-induced runs are the acceptance evidence.

### Security and negative tests

Not applicable: no authorization surface changes.

### Migration and compatibility tests

Not applicable.

## 11. Required verification commands

```bash
# the affected integration target
cargo test --test project_activation

# repetition, including under induced load
# (25 sequential runs with background CPU load; 24 concurrent instances x3 rounds)

# formatting and linting
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings

# broader suite
cargo nextest run --workspace --locked --profile ci

# static guards
scripts/verify.sh quick
```

Hosted `CI / verify` supplies the full-sweep evidence.

## 12. Documentation updates

- `plans/subsystems/project-catalog-activation-test-corrective-addendum.md`
  (new).
- `plans/implementation/project-catalog-activation-test-corrective/001-concurrent-coalescing-lease-lifetime.md`
  (this plan).
- `plans/closure/project-catalog-activation-test-corrective/001-status.md`
  (new).
- `plans/registry.md` rows.
- No `architecture/` change: no production contract, ownership, or behavior
  changed. The in-test comment carries the reasoning for the next reader.

## 13. Acceptance criteria

- All eight lease ids are still asserted equal, at eight-way concurrency, on a
  `multi_thread` runtime with two workers.
- Both post-conditions (`active_count() == 0`,
  `workspace_services.active_count() == 1`) still hold.
- The root cause is demonstrated deterministically: a lease held across a
  second activation coalesces, and a lease released before one does not, with
  the registry emptying in between.
- The test passes 25/25 under induced CPU load and 24/24 across three rounds
  of 24 concurrent instances.
- `git diff` for the change touches only
  `concurrent_same_owner_activation_coalesces_scope_and_bundle` in
  `tests/project_activation.rs`.
- `cargo fmt --all -- --check` and workspace clippy under `-D warnings` are
  clean.
- Hosted `CI / verify` completes the full workspace sweep and `Desktop E2E` is
  green on the fix head.

## 14. Stop conditions

Stop and report rather than improvise when:

- the fix appears to require changing `acquire`, handle accounting, or
  eviction — that would mean the product is wrong, not the test;
- the deterministic demonstration shows coalescing fails while a lease is
  genuinely held, which would contradict the documented semantics;
- the assertion can only be made to pass by weakening it, dropping
  concurrency, or switching to `current_thread`;
- the work would expand into `m004-session.e2e.ts` or any other subsystem.

## 15. Closure evidence required

- The deterministic mechanism demonstration output, including the registry
  `active_count()` observed between the released and subsequent acquisition.
- Repeated and load-induced run counts for the fixed test.
- The `tests/project_activation.rs` diff proving the change is confined to the
  one test.
- `cargo fmt`, clippy, and the six-test suite result.
- The hosted `CI / verify` run id with an explicit statement of whether the
  full sweep completed, plus the `Desktop E2E` run id.
- An explicit statement that no production code and no historical closure
  record changed.

## 16. Handoff notes

- Do not "fix" this by weakening the assertion or by serializing the test. The
  product semantics are correct; only the test's lease lifetime was wrong.
- An earlier local reproduction attempt under induced load did **not** fail the
  original test, so local repetition alone is not proof either way. The
  deterministic demonstration is the actual evidence; do not present
  repetition as root-cause proof.
- The `Desktop E2E` `m004-session.e2e.ts` failure from the same run window is
  a separate flake and is deliberately out of scope here.
