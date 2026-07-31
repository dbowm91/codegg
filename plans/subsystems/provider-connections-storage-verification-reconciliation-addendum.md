# Provider Connections Storage Verification Reconciliation Addendum

Status: active — Milestone 006 ready for handoff

Canonical subsystem roadmap:

- `plans/subsystems/provider-connections-roadmap.md`

Historical strict-closure record:

- `plans/closure/provider-connections/005-status.md`

Current corrective implementation:

- `plans/implementation/provider-connections/006-storage-layout-assertion-and-verification-reconciliation.md`

Cross-subsystem blocker and evidence record:

- `plans/closure/development-verification-release/006-stop-condition.md`

Related Tool Programs implementation/evidence:

- `plans/implementation/tool-programs/018-runtime-fixture-contract-alignment-and-dvr-unblock.md`
- `plans/closure/tool-programs/018-status.md`

Target independent closure record:

- `plans/closure/provider-connections/006-status.md`

## 1. Purpose

Provider Connections M001–M005 remain accepted for their production behavior. A later repository-wide verification pass exposed a stale assertion in a provider migration integration test:

```text
provider_connections::tests::migration_is_idempotent_and_store_crud_is_revision_safe
left: 35
right: 33
```

The test invokes the global storage migration path, which now advances beyond provider migration v33 through repository-wide migrations v34 and v35. The test still treats the historical provider migration number as the terminal schema version.

This addendum reactivates the provider-connections planning track for one narrowly bounded verification correction:

> Reconcile the provider migration test with the canonical global storage-layout contract, preserve migration and provider semantics, close the remaining Tool Programs fixture-isolation evidence question, and restore independently reviewed local/hosted closure sequencing for Tool Programs M018 and DVR M006.

M006 is not a reopening of provider architecture or lifecycle design.

## 2. Accepted predecessor behavior

The following M001–M005 outcomes remain accepted and must not regress:

- daemon-owned, credential-free provider connection metadata;
- opaque secret references and separate credential ownership;
- endpoint and TLS validation;
- personal, project, and deployment scope semantics;
- revision-safe durable CRUD;
- lifecycle, rotation, health, and reconciliation semantics;
- deterministic selection and provider identity behavior;
- migration compatibility through provider storage introduction and lifecycle expansion;
- existing closure evidence at `plans/closure/provider-connections/005-status.md` except for the newly demonstrated stale verification assertion.

M006 does not transfer ownership of unrelated provider or Eggpool feature work.

## 3. Trigger evidence

Repository baseline:

- `c0aa7852685b916cd11f7dd807198e1d82729366`

Observed facts:

1. Tool Programs M018 removed the prior six runtime-fixture failures.
2. The canonical full test run then reaches the provider migration test.
3. `STORAGE_LAYOUT_VERSION` is 35.
4. Provider migration/lifecycle storage is historically associated with version 33.
5. Later global migrations advance the shared schema through versions 34 and 35.
6. The provider test invokes the global migration function twice and asserts literal 33 before exercising CRUD.
7. Hosted workflow run `30599468088`, job `91058839160`, fails in `Workspace tests`; M006 must bind the exact hosted failure from the logs during implementation.

The expected correction is a test-contract reconciliation, not a production migration change. Investigation remains mandatory because a version mismatch can also signal a real migration defect.

## 4. Milestone 006 ownership boundary

M006 owns:

- the stale terminal-version assertion in the named provider migration test;
- inspection of the current global migration sequence and schema-version meaning;
- focused provider migration/CRUD/revision evidence;
- bounded `codegg-core` regression evidence;
- repeated Tool Programs M018 runtime-target execution and test-isolation proof;
- exact evidence SHA and workflow binding;
- correction of M018 self-closure language and DVR blocker placeholders;
- canonical local quick/full and hosted verification;
- independent provider closure handoff.

M006 does not own:

- provider feature expansion;
- provider lifecycle, health, selection, rotation, endpoint, or credential changes;
- migration renumbering or framework redesign;
- new storage migrations unless investigation proves a genuine defect and a new plan is approved;
- production Tool Programs behavior;
- verification topology or resource changes;
- projection transport;
- release execution;
- implementation-authored strict closure.

## 5. Dependency graph

```text
Provider Connections M001–M005
(strict historical production closure)
        |
        v
Global storage migrations v34 and v35 land
        |
        v
DVR M006 full verification
        |
        +--> Tool Programs stale fixture exposed
        |       |
        |       v
        |     Tool Programs M018 implementation
        |     (focused evidence green; independent closure pending)
        |
        v
Provider migration test reaches stale 33 assertion
        |
        v
Provider Connections M006
(storage-layout assertion and verification reconciliation)
        |
        +--> provider independent closure
        +--> Tool Programs M018 independent closure review
        +--> DVR M006 independent closure review
```

M006 is dependency-ready against `c0aa7852685b916cd11f7dd807198e1d82729366`.

## 6. Closure authority

Until M006 is independently closed:

- Provider Connections status is `active`;
- M001–M005 remain historical accepted production milestones;
- M006 is the sole dependency-ready implementation handoff;
- Tool Programs M018 remains implementation-complete but not independently strictly closed;
- the existing M018 status record is provisional conditional implementation evidence;
- DVR M006 remains blocked;
- `plans/closure/provider-connections/006-status.md` must remain absent during implementation;
- `plans/closure/development-verification-release/006-status.md` must remain absent;
- no implementation agent may convert M018, Provider M006, or DVR M006 to strict `closed`.

## 7. Strict M006 completion requirements

Provider Connections M006 may close only when:

1. the 35-vs-33 failure is reproduced and bound to an exact baseline SHA;
2. the global migration contract and version sequence are inspected directly;
3. the test expectation uses the canonical terminal storage-layout contract;
4. no production migration semantic is changed merely to satisfy the test;
5. migration remains idempotent across two calls;
6. provider CRUD and stale-revision rejection execute and pass;
7. the focused provider test passes;
8. the full `codegg-core` package tests pass;
9. Tool Programs M018 runtime tests pass twice sequentially with actual isolation evidence;
10. no production Tool Programs behavior changes;
11. quick and full local verification pass;
12. one same-executable-revision hosted `verify` job passes, including Workspace tests;
13. the DVR stop-condition contains exact SHAs, workflow IDs, and no placeholder;
14. the registry contains Provider M006 as the sole handoff during implementation and no completed M018 dependency-ready row;
15. closure governance is restored to an independent reviewer;
16. no unresolved high or medium provider finding remains.

## 8. Milestone disposition

| Milestone | Status | Disposition |
|---|---|---|
| 001–004 | historical closed | Provider identity, persistence, lifecycle, and integration foundations retained |
| 005 | historical closed | Corrective lifecycle, rotation, health, ownership, and strict production closure retained |
| 006 | ready | Narrow storage-layout assertion, verification, evidence, and closure-governance reconciliation |

## 9. Handoff rules

The implementation agent must:

- follow `plans/implementation/provider-connections/006-storage-layout-assertion-and-verification-reconciliation.md`;
- change the smallest truthful executable surface;
- stop if a real production migration defect is found;
- preserve exact evidence;
- move M006 only to `closing` after implementation;
- leave final closure to a separate reviewer.

The independent reviewer must inspect migration semantics, not merely green tests, before creating `plans/closure/provider-connections/006-status.md`.

No additional provider milestone should be registered after M006 unless a new production or verification defect is demonstrated.