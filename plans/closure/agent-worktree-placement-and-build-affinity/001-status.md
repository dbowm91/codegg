# Agent Worktree Placement and Build Affinity A001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/agent-worktree-placement-and-build-affinity/001-explicit-isolated-placement-contract.md`

Source subsystem roadmap:

- `plans/subsystems/agent-worktree-placement-and-build-affinity-corrective-addendum.md#milestones-and-boundaries`

Repository baseline reviewed: `80171f45295bebb9ab0b521262c0b295acb93074`

Implementation commits or pull requests:

- PR #207, merged at `fed10ce13be7fd99a6d5aa5f8b6639ba98d007bc` — explicit isolated placement is honored independently of child mutability and fails closed when checkout preparation is unavailable.
- `77ca73a1e` and `a76d9bd69` — production-shaped qualification fixture, including daemon writer-fence and durable attempt-store wiring.
- Hosted Linux qualification run `40592341646`, process job `122499711946` — contained two-checkout success case passed at source commit `a76d9bd69`; that same source head contains A003's Cargo lock-wait capture.

Closed: 2026-10-27

Primary class: invariant

## 1. Executive finding

A001 is strictly closed. An explicit read-only `IsolatedNew` delegated run now obtains its own managed checkout through the qualified contained Git creator and live scheduler dispatch. Two children receive distinct checkout roots, read the committed base content rather than a divergent parent file, and leave the parent unchanged. Repository-less and unavailable-creator requests terminate with the typed `explicit_isolation_unavailable` failure rather than falling back to an inherited root. Existing `Auto` and `SerializedShared` placement behavior remains covered by the surrounding trajectory tests.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Explicit `IsolatedNew` prepares a checkout even for read-only children | Executor placement predicate and hosted live-scheduler test `explicit_read_only_isolated_new_uses_two_distinct_contained_checkouts` | pass | Linux test runs through `SubagentJobExecutor`, `JobSubmissionService`, `JobScheduler`, `WorktreeService`, and `ContainedGitWorktreeCreator` |
| Each child gets a distinct managed checkout with preserved project/repository lineage | Hosted A001 test records two distinct checkout paths and checks each `WorktreeRecord` lineage | pass | Same canonical project and repository; distinct worktree IDs and roots |
| Child observes the committed base, not the parent's divergent working tree | Hosted A001 test reads `README` through the child's actual relative `read` tool result | pass | Parent file is changed to `parent-only`; child observes committed `base` |
| Parent checkout remains unchanged | Hosted A001 test re-reads the parent's `README` after both children | pass | Neither child receives a parent-root tool default |
| Repository-less explicit isolation fails terminally with an actionable typed reason | `explicit_read_only_isolated_new_refuses_a_repositoryless_workspace` | pass | Asserts `explicit_isolation_unavailable`, registered-repository guidance, and no worktree record |
| Missing contained creator fails closed; no inherited-root fallback | `explicit_read_only_isolated_new_fails_closed_without_contained_creator`; baseline-red reproduction below | pass | Failure is terminal and typed |
| Existing `Auto` and `SerializedShared` behavior is unchanged | Agent-loop workspace-root and M008 blackboard trajectory filters | pass | Focused host run: 16 passed, 554 skipped |

## 3. Production implementation evidence

PR #207 changed the executor's preparation decision so requested `IsolatedNew` placement is independent of the child's denied-tool/read-only profile. The placement arm still goes through daemon-owned repository resolution, `WorktreeService`, and managed checkout creation. Missing repository, worktree service, contained creator, creation, or linkage does not downgrade the request to `Auto` or an inherited root.

The hosted regression uses a real linked Git checkout creator and live scheduler dispatch. Its subagent pool, SQLite `SqliteAgentRunStore`, attempt provenance, writer-permit service, and `DaemonWriterFenceFactory` share the durable stores as the daemon does. No uncontained creator or test-only containment bypass was added.

## 4. Verification executed

### Commands run

```bash
CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER=/tmp/codegg-linker-sdk/cc MACOSX_DEPLOYMENT_TARGET=26.0 LIBZ_SYS_STATIC=1 LZMA_API_STATIC=1 CARGO_BUILD_JOBS=2 rtk cargo nextest run -p codegg --test agent_loop --locked --profile ci -E 'test(explicit_read_only_isolated_new_fails_closed_without_contained_creator) | test(blackboard_trajectory_m008) | test(workspace_root)'
rtk scripts/verify.sh quick
rtk cargo clippy --workspace --all-targets --locked -- -D warnings
rtk cargo fmt --check
rtk git diff --check
```

Baseline-red reproduction at A001's original baseline `80171f45295bebb9ab0b521262c0b295acb93074`:

```bash
CARGO_TARGET_X86_64_APPLE_DARWIN_LINKER=/tmp/codegg-linker-sdk/cc MACOSX_DEPLOYMENT_TARGET=26.0 LIBZ_SYS_STATIC=1 LZMA_API_STATIC=1 CARGO_BUILD_JOBS=2 rtk cargo test -p codegg --test agent_loop --locked explicit_read_only_isolated_new_fails_closed_without_contained_creator -- --nocapture
```

The baseline run failed as expected: the executor recorded generic `executor` failure instead of `explicit_isolation_unavailable`. The pre-W004 baseline fixture payload was adjusted only to remove the later `default_working_directory` field.

The hosted Linux step was:

```bash
python3 scripts/check_test_filter_execution.py --run -- cargo test --locked --test agent_loop explicit_read_only_isolated_new_uses_two_distinct_contained_checkouts -- --nocapture --test-threads=1
```

### Results

- Focused placement / workspace-root / M008 coverage: **16 passed, 554 skipped, 0 failed**.
- Hosted Linux contained-success test: **1 passed, 570 filtered out, 0 failed** in run `40592341646`, process job `122499711946`.
- The same run's restricted-userns lane passed. The run was later canceled while its remaining M004 process/PTY/LSP lane was still running; that unrelated remainder is not represented as passed evidence.
- `scripts/verify.sh quick`: **passed**, including 357 harness tests (356 passed, 1 skipped), all quick guards, and workspace all-target check.
- Workspace Clippy with `-D warnings`: **passed; no issues found**.
- Formatting and `git diff --check`: passed.
- The Linux success test is intentionally host-qualified; macOS does not claim the Linux contained-creator tier. The exact hosted Linux qualification passed.

## 5. Invariant review

- Explicit `IsolatedNew` is intent, not an inference from write-capable tools; it allocates independently of the child permission profile.
- A missing repository or unavailable contained backend refuses before child execution.
- Worktree identity and lineage come from daemon-owned workspace/project/repository records, not model-supplied paths.
- Every isolated child has a distinct managed root; the parent root is unchanged.
- `Auto` and `SerializedShared` keep their separate allocation and permission contracts.
- Lease generation and cleanup remain owned by `WorktreeService`; no parent merge or manual worktree adoption was introduced.

## 6. Failure and recovery review

Checkout preparation, creation, and linkage errors become terminal child-run failures rather than inherited-root execution. Durable attempt provenance and writer-fence authority are present before the child can enter its provider. Existing scheduler idempotency and run-attempt handling remain the authority for retries; A001 adds no retry class or lease-release path. The production-shaped success fixture runs two sequential child allocations and verifies separate roots and durable lineage. Cancellation and lease cleanup remain covered by the existing worktree/scheduler suites and are not changed by this milestone.

## 7. Migration and compatibility review

No protocol variant or storage migration was introduced. Existing serialized payloads continue to decode to `Auto` through the existing default. Only a caller that explicitly requests `IsolatedNew` gains the checkout-preparation behavior. Worktree and run storage formats are unchanged.

## 8. Security review

Managed checkout creation uses the qualified contained Git backend and existing filesystem grant. The test obtains the production containment tier on hosted Linux and exercises the real creator. Writer-fence resolution is installed from daemon-owned run/worktree stores and refuses if no fence can be resolved; the fixture shares SQLite run/attempt state rather than synthesizing provider provenance. Parent-root non-mutation and committed-base reads are asserted. No permission expansion, containment bypass, path-derived identity, or arbitrary shared process execution is added.

## 9. Documentation and operations

The merged implementation and existing architecture guidance document explicit placement and fail-closed checkout preparation. The Linux M004 workflow now runs the production-shaped A001 success regression after the C008 contained-creator qualification. Operators can distinguish unavailable explicit isolation from the `Auto` in-place compatibility route through the typed failure class and message.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| none | No A001 correctness or evidence finding remains | None | None |

## 11. Roadmap disposition

Milestone A001 is closed. Its hard dependency for A002 is satisfied; A002 may be registered as eligible for fresh planning. This closure does not authorize existing-checkout mutation, shared writable placement, or lease transfer.

## 12. Registry updates

- Mark A001 closed in its implementation plan, subsystem roadmap, and `plans/registry.md`; move it from active implementation work to recently closed work.
- Move A002 from blocked to eligible for fresh planning; record A001's strict closure as the dependency resolution.
- Keep A005 blocked on A002 and A003/A004 decision evidence. A003 is audited independently in its own closure record.
