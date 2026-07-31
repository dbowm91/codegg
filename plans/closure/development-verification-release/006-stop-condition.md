# Development Verification and Release Milestone 006 — Stop Condition

Status: **blocked**

Source implementation plan:

- `plans/implementation/development-verification-release/006-final-evidence-and-release-documentation-closure.md`

Source subsystem roadmap:

- `plans/subsystems/development-verification-release-roadmap.md`
- `plans/subsystems/development-verification-release-final-evidence-closure-addendum.md`

Repository baseline reviewed: `d58a37a6160f18a5b336ca7bbc0e32e0f057b755`
Reviewed head (with M006 in-scope work and M018 fixture correction applied): pending M018 commit
on `main`, with M006 corrections to `scripts/check-tokio-test-flavors.py`,
`scripts/tests/test_check_tokio_test_flavors.py`,
`plans/closure/development-verification-release/005-package-inventory.md`,
`plans/closure/development-verification-release/006-package-inventory.md`,
`plans/registry.md`,
`plans/subsystems/development-verification-release-final-evidence-closure-addendum.md`,
`plans/implementation/development-verification-release/006-final-evidence-and-release-documentation-closure.md`,
and `RELEASING.md`.

## 1. Why this milestone is blocked

M006's exit conditions require `scripts/verify.sh full` to exit zero on the
final implementation SHA. Running `scripts/verify.sh full` on the M006
implementation head fails with exit code 101 because of pre-existing test
failures in two unrelated subsystems. The plan explicitly forbids absorbing
unrelated implementation into M006:

> M006 does not own:
> - additional scheduler, process-tree, Tool Programs, LSP, TUI, storage, provider, Git, or agent-loop refactors;
> - changing product behavior to make an unrelated test green.

> Work package E rules:
> - do not patch additional unrelated tests in this milestone;
> - if a product/runtime test fails, stop and register a separate blocker.

This document registers the blocker, captures the exact evidence, and
proposes the next plan boundary. It does not propose absorbing the
failures into M006.

## 2. Exact evidence

### 2.1 Tool Programs runtime integration failures

| Field | Value |
|---|---|
| Commit SHA | `80e0919fb8a567eea8914c31cb2b9c0b6743efd4` (M006 in-scope work committed) |
| Command | `scripts/verify.sh full` (and the underlying `cargo test --workspace --locked -- --test-threads=1`) |
| Failing test binary | `tests/tool_program_runtime.rs` |
| Exit code | 101 |
| Minimal failure output | `runtime contract resolution failed: Tool Programs require at least one frozen runtime contract` |
| Test names | `emit_constant_completes`, `for_loop_program_completes`, `if_else_program_completes`, `list_operations_program_completes`, `nested_loop_program_completes`, `string_operations_program_completes` |
| Owning subsystem | Tool Programs (M001–M017) |
| M006 scope? | **No** — Tool Programs is explicitly outside M006's ownership boundary |

The failing tests pass `allowed_tools: Vec::new()` to the
`ToolProgramExecutor::default()` and expect it to execute simple programs
like `emit({"ok": true})`. The current `ToolProgramExecutor` calls
`resolve_contract_snapshot(broker, allowed_tools)` which rejects an empty
`allowed_tools` with the error above. The tests were authored in M005
(commit `4b0907de`) before the contract-enforcement tightening in
M011–M014. They were updated in M013 (commit `bc3e8b32`) to populate
`contract_snapshot_json` but the value is still an empty contracts list,
which the runtime now correctly rejects. The tests need to either
provide a frozen contract for at least one tool, or be replaced with
broker-mocked variants. Both fixes belong to the Tool Programs
subsystem, not to M006.

The failure is **not** introduced by M006. Confirmed by re-running the
test binary against `d58a37a6` with the M006 working tree stashed:
identical failures.

### 2.2 Current codegg-core migration assertion

| Field | Value |
|---|---|
| Commit SHA | `80e0919fb8a567eea8914c31cb2b9c0b6743efd4` (M006 in-scope work committed) |
| Command | `cargo test --workspace --locked -- --test-threads=1` (running the full lib test binary, not a focused subset) |
| Failing test | `provider_connections::tests::migration_is_idempotent_and_store_crud_is_revision_safe` |
| Exit code | 101 |
| Minimal failure output | `assertion left == right failed; left: 35; right: 33` |
| Owning subsystem | codegg-core provider-connection/storage migration |
| M006 scope? | **No** — storage/provider migration is explicitly outside M006's ownership boundary |

The failure reproduces in isolation with:
`cargo test -p codegg-core provider_connections::tests::migration_is_idempotent_and_store_crud_is_revision_safe -- --test-threads=1`.
The repository currently declares `STORAGE_LAYOUT_VERSION = 35`, while this
test still asserts 33. This is unrelated to verification/release machinery and
is not introduced by M018.

## 3. Why M006 in-scope work is still complete and shippable

The M006 ownership boundary limits the in-scope work to:

- narrow Tokio guard corrections;
- package inventory regeneration;
- `RELEASING.md` correction;
- focused guard tests;
- planning reconciliation.

All five items landed in the M006 implementation head. Verified
independently:

| Item | Verification | Result |
|---|---|---|
| Tokio guard `--self-test` | `python3 scripts/check-tokio-test-flavors.py --self-test` | 8/8 pass, exit 0 |
| Tokio guard focused tests | `python3 -m unittest scripts.tests.test_check_tokio_test_flavors` | 21/21 pass, exit 0 |
| Tokio guard repository baseline | `python3 scripts/check-tokio-test-flavors.py` | 1049 bare tests in baseline, no new violations, exit 0 |
| Tokio guard deterministic emit | `diff -u scripts/tokio-test-flavor-baseline.txt <(scripts/check-tokio-test-flavors.py --emit-current)` | empty diff, exit 0 |
| Quick verification (excluding workspace tests) | `scripts/verify.sh quick` | exit 0 |
| Package inventory against `cargo metadata` | `cargo metadata --format-version 1` plus targeted manifest inspection | layers and dep columns match, all 10 manifests consistent |
| Leaf package `cargo publish --dry-run` | one command per leaf crate | exit 0 for all 7 leaves |
| Dependent package `cargo publish --dry-run` | one command per dependent crate | exit 101 with "no matching package" — expected registry-sequencing |
| `RELEASING.md` first-vs-subsequent paths | document inspection | distinguishes initial name availability from ownership, queries just-published names |
| Hosted `verify` for `80e0919` (M006 in-scope work commit) | GitHub Actions run 30587211454 | M006 in-scope steps all `success`: agent TOML schema, builtin agents match source, Tokio guard self-test, Tokio guard regression guard, codegg-core boundary guard, formatting, workspace check, workspace clippy. Only `Workspace tests` failed, on the pre-existing `tests/tool_program_runtime.rs` integration failures. URL: `https://github.com/dbowm91/codegg/actions/runs/30587211454`. |
| Hosted `verify` for `2f94608` (M006 stop-condition record) | GitHub Actions run 30587244426 | Same M006 in-scope steps `success`; `Workspace tests` failed identically on `tests/tool_program_runtime.rs`. URL: `https://github.com/dbowm91/codegg/actions/runs/30587244426`. |

The pre-existing test failures in section 2 do not regress any of
those items. M006 in-scope work is fully landed; what is missing is
the strict-closure evidence from `scripts/verify.sh full` exit 0 and
a successful hosted `verify` job, both of which are blocked by the
unrelated failures. The hosted runs above prove that every M006-owned
CI step succeeds on the implementation head; only the externally
owned workspace-test failures block the run.

## 4. Proposed next plan boundaries

The unrelated failures must be fixed by their owning subsystems. Each
is a separate, narrowly owned corrective plan.

### Proposed Plan Boundary A — Tool Programs test isolation

- Owning subsystem: Tool Programs (M001–M017)
- Scope: update `tests/tool_program_runtime.rs` (or replace the failing
  tests with broker-mocked variants) so the six integration tests pass
  without changing the runtime contract enforcement in
  `src/tool/tool_program_context.rs`.
- Excludes: any change to production runtime that would loosen the
  frozen-contract enforcement in `resolve_contract_snapshot`.
- Acceptance: `cargo test --test tool_program_runtime -- --test-threads=1`
  exits 0 against the same head.

### Proposed Plan Boundary B — codegg-core migration assertion reconciliation

- Owning subsystem: codegg-core provider connections/storage
- Scope: reconcile the migration test assertion with the current storage
  layout contract and run the focused/core/full evidence needed by DVR M006.
- Excludes: Tool Programs, verification resource changes, and unrelated storage
  migrations.
- Acceptance: `cargo test --workspace --locked -- --test-threads=1`
  no longer fails on the stale layout-version assertion.

Neither boundary belongs to M006. A separate owner should create the
corrective implementation plan when ready. The previously reported projection
transport stack behavior did not reproduce in the M018 canonical rerun, so no
projection plan is registered.

## 5. Registry and planning implications

Until at least one of the proposed boundaries is closed and the
failures stop blocking `scripts/verify.sh full`:

- The M006 implementation plan cannot be moved from `closing` to
  `closed`. It is recorded in `plans/registry.md` as M006 implementation
  landed but verification blocked by external subsystem failures.
- The M006 closure record `plans/closure/development-verification-release/006-status.md`
  must remain absent. The plan forbids the implementation agent from
  creating it.
- The DVR subsystem roadmap (`plans/subsystems/development-verification-release-roadmap.md`)
  remains `closed` for M001–M004 and `conditionally closed` for M005.
  M006 cannot become `closed` while the unrelated blockers remain.

If a corrective plan for Boundary A or Boundary B is registered as a
follow-on, that plan's dependency graph must list M006 as a
hard-interface dependency (M006 cannot be closed until those plans
land) — but only after this stop-condition record is accepted as the
authoritative description of the gap.
