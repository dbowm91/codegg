# Linux Trusted Git Cost Corrective C004 — Closure Status

Status: conditionally closed — test-only private staging and execution ownership are qualified; production publication and mutation ownership remain blocked

Source implementation plan:

- `plans/implementation/linux-trusted-git-cost-corrective/004-private-index-odb-promotion-prototype.md`

Source subsystem roadmap:

- `plans/subsystems/linux-trusted-git-cost-corrective-roadmap.md#milestones-and-handoffs`

Repository baseline reviewed: `59ab0d2dc1ff5bea7d5fd79fea0cd1cb57cdadbd`

Implementation commits or pull requests:

- `0fa4e1e5` — route test-only private `git add` staging through `GitExecutionService` and `ManagedProcessService`.
- Hosted Linux M004 qualification [run 39200873597](https://github.com/dbowm91/codegg/actions/runs/39200873597) — exact-source test-only service-owner evidence.

## 1. Executive finding

C004's bounded test-only private-index/object staging prototype is complete. It proves that a typed `Add` operation can stage through the existing service and managed-process owner while protected source metadata remains unchanged. It does not implement a production mutation transaction or publisher. This conditional closure applies only to the test-only prototype; production mutation remains blocked because durable journal ownership, production cancellation/recovery, external-writer conflict safety, and safe multi-resource publication are not established, and proposed ADR-0044 is not accepted.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Typed staging owner and reject other mutation kinds before path access | `GitExecutionService::execute_private_git_add_staging_test_only`; hosted run 39200873597 | pass | Feature-gated test-only entry; no normal dispatch caller. |
| Private index/new-object staging with protected source metadata | `test_only_contained_git_add_stages_index_and_objects_in_private_gitdir`; hosted run 39200873597 | pass | Contained stage behavior only. |
| Process/IPC, offline network, cancellation and source preservation | Hosted process/IPC step, 15/15; cancellation/filter probe in the suite | pass | Test harness process ownership; not production broker cancellation. |
| Publisher conflict handling and interrupted-index recovery | Prior broker-model runs 39182280516, 39160562247, 39167611554 | partial | Tests model lock/index recovery and identity changes; do not exercise the production Git service or durable daemon journal. |
| Arbitrary external-writer exclusion, concurrent path-swap publication safety, refs/worktree multi-resource transaction | Source-plan review and C012 closure | not run | `WorkspaceLockTable` cannot fence external writers; index/worktree/refs are not atomic together. |
| Full production dispatch, durable recovery, and ADR/admission | Source and registry review | blocked | No ordinary Git mutation caller, production publisher, accepted ADR-0044, or M003/C010 admission exists. |

## 3. Production implementation evidence

The current implementation adds a feature-gated test method on `GitExecutionService` that accepts only a typed `GitOperation::Add`, validates scratch and workspace-relative paths, requests protected metadata plus M004/M005, and delegates child execution to `ManagedProcessService`. The repository index and objects remain protected during staging. No production mutation executor route, durable journal, ref/index publisher, storage schema, or restart reconciler was added.

## 4. Verification executed

### Commands run

```bash
cargo test --locked --test linux_m004_process_ipc --features linux-git-test-support -- --nocapture
cargo test --locked -p codegg --lib interactive_process::tests::m004_live_owner_preserves_tty_and_private_process_view -- --test-threads=1 --nocapture
cargo test --locked --test interactive_process_attach_resume m004_daemon_constrained_pty_uses_principal_profile_and_cancels_descendant -- --nocapture
cargo test --locked -p codegg --lib lsp::m004::tests::constrained_lsp_process_is_owned_by_m004_scope --features lsp-test-support -- --test-threads=1 --nocapture
cargo test --locked --lib protected_mount_ -- --nocapture
cargo test -p egggit --locked trusted_git -- --test-threads=1
```

### Results

- Hosted run 39200873597 passed the managed-process step 15/15, including typed staging-owner routing, typed live-read parity, cancellation/source preservation, and refusal preflight.
- The same run passed constrained PTY live-owner/routing, constrained LSP duplex/procfs, protected-mount/Git regressions 20/20, and restricted-user-namespace refusal checks.
- Cross-process recovery, lock symlink refusal, and process-death gitdir identity probes are present in earlier exact-source successful runs recorded in the implementation plan. They remain test-only broker-model evidence.
- A newer M004 refresh, run 39254354958, was canceled during the PTY stage when another workflow dispatch superseded it. It is not counted as a pass. The prior complete owner run remains the evidence for the unchanged runtime implementation.
- Full CI and exact-head cross-platform results are tracked in the roadmap; they do not establish production publication safety.

## 5. Invariant review

- Agent-controlled Git never receives a writable whole-source `.git` mount: preserved by the protected-mount launch and the staging test.
- Publisher helpers are not admitted outside M004/M005: staging requests those scopes; production publication does not exist.
- External writers are not assumed to cooperate with CodeGG leases: C012's no-writer-lease rejection remains in force.
- Index, working-tree and ref updates are not represented as one atomic transaction: no production transaction was introduced.
- No remote Git, credentials, FullHost fallback, or default policy change was added.

## 6. Failure and recovery review

Test-only probes cover cancellation of a contained filter child, preservation of source index/ODB, interruption after a synced index lock, no-follow lock denial, staged-byte validation, process death, and replacement gitdir identity refusal. They do not prove production daemon cancellation, restart reconciliation, durable idempotency, recovery for every publication phase, or safe concurrent validation-to-publish behavior. No production publisher currently exists to recover.

## 7. Migration and compatibility review

No database migration, public DTO, protocol, or ordinary tool behavior changed for C004. The test-only method is feature gated. Windows/macOS production Git routes remain the immutable-snapshot path. A future durable journal or serialized receipt requires separate schema/protocol review.

## 8. Security review

Typed operation validation, path validation, protected metadata, and contained child ownership are demonstrated for the staging prototype. Source secret/host IPC/network denial is exercised by the M004/M005 test suite. Production publisher authorization, external-writer races, arbitrary metadata path expansion, object collision/corruption handling, and audit receipt ownership remain unproven.

## 9. Documentation and operations

The implementation plan and Linux trusted Git roadmap record the test-only owner boundary and unresolved production gates. No operator-facing production enablement or recovery command exists. ADR integrity, execution ownership, sandbox contract and Git forbidden-pattern guards are tracked in the branch verification record.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | No production journal or publisher owns index/object/ref publication | Ordinary authorized local Git mutations cannot use this path safely | Implement and qualify daemon-owned journal/publication across crash phases before production dispatch. |
| high | External writers and validate-to-publish path races are not fenced | Concurrent IDE/Git edits could be overwritten or partially published | Add deterministic external-writer and path-swap race tests with typed conflict/recovery outcomes. |
| high | ADR-0044 remains proposed and M003/C010 admission is unchanged | No authority to promote this prototype or switch defaults | Obtain independent ADR acceptance and separate production admission. |
| medium | Cancellation/restart and multi-resource promotion are only partially modeled | Staging evidence does not cover full Git workflow recovery | Integrate fault injection through production owners after the architecture gate. |

## 11. Roadmap disposition

C004 is conditionally closed for test-only staging and promotion-model evidence. No production staging or publication is admitted. C005, Linux C002 and M003 remain blocked on their own gates. The historical C010 no-admission and C012 no-writer-lease decisions are unchanged.

## 12. Registry updates

- Record C004 as conditionally closed for the test-only staging-owner proof, with production publication blocked; do not mark the capability complete.
- Keep the roadmap active and C005 blocked until the listed publication and ADR/admission gates are resolved.
