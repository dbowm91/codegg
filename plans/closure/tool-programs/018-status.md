# Tool Programs Milestone 018 — Closure Status

Status: conditionally closed

Source implementation plan: `plans/implementation/tool-programs/018-runtime-fixture-contract-alignment-and-dvr-unblock.md`

Source subsystem roadmap: `plans/subsystems/tool-programs-runtime-fixture-closure-addendum.md`

Repository baseline reviewed: `9686338ad6aa8b0ff5ebfe8b07d74e1451180791`

Implementation commit: `42354429767f706754ce7fbe1850a03d1b2d979d` — fix(tool-programs): align runtime fixture contracts

## 1. Executive finding

M018 is conditionally closed. The stale M005-era runtime fixture now uses one
real test-local read-only tool and the production canonical contract helpers.
All focused and adjacent Tool Programs evidence is green, and the six
historical completion failures are gone. The canonical full gate remains
nonzero for an unrelated codegg-core migration assertion (`35` versus `33`),
so M018 does not unblock DVR M006 strict closure or claim a green workspace.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Non-empty frozen contract for positive fixtures | `RuntimeFixture` registers `runtime_fixture_read` and resolves its contract through the fixture broker | Pass |
| One source of truth | `allowed_tools`, canonical snapshot, contract digest, authority digest, grant, and job payload derive from `RuntimeFixture` | Pass |
| Production-shaped execution | Every positive case uses `ToolProgramExecutor::new` with the fixture broker and registry | Pass |
| Emit-only semantics | Six completion tests assert `ExecutorStatus::Completed` and zero fixture calls | Pass |
| Cancellation semantics | `pre_cancelled_program_returns_cancelled` and `cancelled_program_returns_cancelled` pass with zero calls | Pass |
| Empty contract rejection | `empty_frozen_contract_is_rejected` preserves the typed resolution failure | Pass |
| Snapshot/tool mismatch rejection | `allowed_tools_snapshot_mismatch_is_rejected` fails before tool execution | Pass |
| Canonical consistency assertions | `runtime_fixture_contract_bundle_is_canonical_and_consistent` checks snapshot, allowed tools, digest, and grant agreement | Pass |
| No production runtime change | Diff is limited to the integration fixture, Tokio baseline cleanup, and planning records | Pass |

## 3. Production implementation evidence

No production source or default registry path changed. The fixture tool is
test-local, deterministic, read-only, `DirectOrProgrammatic`, and has no
filesystem, process, network, clock, environment, or random behavior.

The fixture uses `resolve_contract_snapshot`, `contract_entry`,
`canonical_contract_json`, `canonical_contract_digest`, `authority_digest`,
and `build_authority_grant` without hand-authored positive-path contract JSON
or digest values. Program IDs include the source digest prefix to avoid replay
of stale durable test records across repeated local runs.

## 4. Verification executed

All commands were run locally on the implementation revision unless noted.

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | Pass |
| `cargo test --test tool_program_runtime -- --test-threads=1` | Pass, 13 tests |
| `cargo test --test tool_program_read_palette -- --test-threads=1` | Pass, 21 tests |
| `cargo test --test tool_program_context_artifacts -- --test-threads=1` | Pass, 9 tests |
| `cargo test --test tool_program_m014_authority_pipeline -- --test-threads=1` | Pass, 9 tests |
| `cargo test --test tool_broker_integration -- --test-threads=1` | Pass, 25 tests |
| `scripts/verify.sh quick` | Pass |
| `scripts/verify.sh full` | Blocked, exit 101 at `provider_connections::tests::migration_is_idempotent_and_store_crud_is_revision_safe` |
| Focused reproduction of full-gate blocker | Fails deterministically: current `STORAGE_LAYOUT_VERSION` is 35, test expects 33 |

The canonical full run reached the corrected Tool Programs runtime target;
all 13 runtime tests passed. The previously reported projection daemon-socket
stack overflow did not reproduce, so no projection corrective plan is
registered.

No hosted green verification run exists for this revision. DVR M006 therefore
remains blocked pending the separate codegg-core reconciliation and a
successful hosted `verify` run.

## 5. Invariant review

Production empty-contract rejection, frozen snapshot validation, authority
integrity, caller policy, effect class, broker ownership, and native execution
semantics remain unchanged. The fixture is not in the production default
palette. No tests were ignored, deleted, or excluded.

## 6. Failure and recovery review

Cancellation continues to return `ExecutorStatus::Cancelled` before contract
execution. Emit-only programs complete without a broker call. Repeated test
runs use source-bound program identities so durable result records from an
earlier run cannot contaminate a later run.

## 7. Migration and compatibility review

This is an integration-test fixture correction only. It introduces no runtime,
storage, protocol, scheduler, or migration compatibility change. The stale
Tokio baseline entries for converted tests were removed as required by the
repository’s baseline-aware guard.

## 8. Security review

The fixture has read-only effect class, `DirectOrProgrammatic` caller policy,
deterministic schemas and output, and no mutation or external authority. The
negative tests preserve fail-closed behavior for empty and mismatched contract
state.

## 9. Planning reconciliation and downstream unblock audit

- M018 is recorded as conditionally closed; its implementation plan is marked
  implemented and conditional.
- The Tool Programs runtime-fixture addendum and roadmap transfer M018 to
  conditional closure; M017 remains conditionally accepted without a new
  predecessor closure record.
- `plans/registry.md` has no dependency-ready implementation plan after M018.
- DVR M006 remains blocked by the unrelated codegg-core migration assertion and
  missing hosted green verification.
- No projection plan is registered because the prior projection failure did
  not reproduce in the M018 canonical rerun.
- The separate reviewer/owner may upgrade this record to strict closure after
  the full/hosted gate is green and the planning documents are re-reviewed.
