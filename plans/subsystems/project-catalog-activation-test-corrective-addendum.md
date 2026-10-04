# Project Catalog Activation Test Corrective Addendum

Status: active

Repository planning baseline: `30e00916`

Controlling process:

- `plans/003-planning-process.md#7-corrective-passes`

Controlling architecture:

- `architecture/project-catalog.md`

Triggering evidence:

- Hosted `CI / verify` run `37223675386` on `b3ad2a31` — `Workspace tests`
  failed at
  `codegg::project_activation::concurrent_same_owner_activation_coalesces_scope_and_bundle`
  (`tests/project_activation.rs:146`), truncating the sweep at 5,292 of
  12,180.
- The compiled workspace at `b3ad2a31` is byte-identical to `cdfd6257`
  (`git diff cdfd6257 b3ad2a31` is `plans/*.md` only), and
  `git diff main...HEAD -- tests/project_activation.rs` is empty: neither the
  causal timing corrective nor M006-A touches this test or its subject.
- Surfaced only because the causal frontier timing corrective
  (`cdfd6257`) let the sweep run past roughly test 5,300. Under `nextest`
  fail-fast this test had been reached but never been the thing that went
  red.

## 1. Purpose

Make the same-owner activation coalescing test assert the property it claims
to assert, deterministically.

The test spawns eight tasks that each call `activate_project_workspace` for
the same owner and asserts all eight received the same lease id. But
`ProjectActivationLease::drop` decrements the activation's handle count and
removes the registry entry when it reaches zero
(`src/core/project_activation.rs:182-191`), and each task's body returned only
`(lease_id, coalesced)` — so every lease was released as its own task
completed. Coalescing is a concurrency property: the registry reuses an
existing activation only while at least one lease on it is alive. The test
therefore asserted a concurrency property while holding no concurrency, and
its outcome depended on whether a later task acquired before any earlier
task's release landed. On a loaded shared runner a task could finish and
release first, and the next task correctly minted a new lease id.

The product behavior is correct and intentional. The defect is in the test.

## 2. Corrective scope

One milestone:

- `plans/implementation/project-catalog-activation-test-corrective/001-concurrent-coalescing-lease-lifetime.md`

Status: implemented.

C001 must:

- hold all eight leases alive until every task has acquired, using a
  `Barrier(8)`, so the precondition the assertion describes is guaranteed by
  construction rather than by scheduler luck;
- keep the same assertion, the same eight-way concurrency, the same runtime
  flavor, and the same post-conditions;
- change no production code, contract, or behavior;
- obtain green hosted `CI / verify` and `Desktop E2E` on the fix head.

## 3. Invariants

This corrective MUST NOT:

- change `ProjectActivationRegistry::acquire`, `ProjectActivationLease`,
  handle accounting, capacity policy, lease TTL, or eviction;
- weaken the assertion — all eight lease ids must still be equal, and
  `active_count()` / `workspace_services.active_count()` must still be checked
  afterward;
- reduce the concurrency or convert the test to `current_thread`;
- change any other test in `tests/project_activation.rs`;
- rewrite any historical closure record, including
  `plans/closure/project-catalog/001`–`004-status.md`.

## 4. Completion definition

C001 closes only when:

- the root cause is demonstrated deterministically rather than inferred;
- the test passes repeatedly, including under induced load;
- `cargo fmt --all -- --check` and workspace clippy under `-D warnings` pass;
- hosted `CI / verify` runs the full workspace sweep to completion, and
  `Desktop E2E` is green, on the fix head;
- the additive closure record is committed and registered.
