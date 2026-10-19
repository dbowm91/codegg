# Linux Trusted Git Cost Corrective C002 — Closure Status

Status: conditionally closed — contained live-read and performance pilots complete for test-only use; production remains blocked

Source implementation plan:

- `plans/implementation/linux-trusted-git-cost-corrective/002-contained-live-odb-read-pilot.md`

Source subsystem roadmap:

- `plans/subsystems/linux-trusted-git-cost-corrective-roadmap.md#milestones-and-handoffs`

Repository baseline reviewed: `59ab0d2dc1ff5bea7d5fd79fea0cd1cb57cdadbd`

Implementation commits or pull requests:

- `31291fc8` — latest cost-pilot source and schema/command-registry reconciliation; Git cost implementation unchanged through the reviewed head.
- Hosted test-only performance run [39255303376](https://github.com/dbowm91/codegg/actions/runs/39255303376), source SHA `31291fc81f5e7cb15d91491e61b058f3c685d977`.
- Durable report: `research/linux-trusted-git-cost-corrective/c002-cost-pilot-39255303376.md` and its raw JSON sample file.

## 1. Executive finding

The contained live-ODB read pilot and bounded cost experiment are complete as test-only evidence. Seven typed read-operation parity cases, enforced contained execution, selected Git-root identity checks, race probes, and a packed/fragmented cost matrix have passed on hosted Linux. The pilot shows much lower cold-advised latency than private snapshots for the measured routes, but it does not establish immutable generation consistency, routine workflow compatibility, or production admission. C002 is conditionally closed only for this test-only pilot; the production route remains blocked.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Keep immutable snapshot as production default and live reads test-only | Source review at `59ab0d2d`; feature/test-gated pilot | pass | No production backend selector/default change. |
| Typed read parity for representative operations | Exact-source native parity suite in run 39182280516; seven operations recorded in C002 plan | pass | Covers selected structured read outputs, not full Git semantic parity. |
| Actual M004/M005 containment and selected-root identity | Hosted M004 runs recorded in C002 plan, including 39182280516; M005 offline owner tests | pass | Candidate path only; protected mount, actual offline network enforcement and identity checks are asserted. |
| Cold/warm measurement on packed and fragmented repositories | Run 39255303376; 30 cold-advised and 100 warm samples for two operations, three routes, two shapes | pass | 105 MiB fixtures; packed one pack, fragmented 4,099 loose objects; `POSIX_FADV_DONTNEED` is advisory. |
| Warm 1/4/8 contention | Run 39255303376; 30 batches per fan-out and fixture/operation | pass | Simultaneous calls do not represent distinct agent identities. |
| Compare child resources and setup/copy costs across all routes | Raw run report | partial | Only contained-live RSS/CPU sampled; GNU time resolution is 10 ms. Native/snapshot resources, setup split, actual copied bytes/I/O and retained scratch were not measured. |
| Prove writer-race consistency, stale-result classification and external metadata bounds | C001/C002 probes and source review | partial | No immutable-generation claim; arbitrary external writers and broad Git metadata/workflow compatibility remain unresolved. |
| Independent production admission | Proposed ADR-0044; C010 and M003 records | blocked | This closure grants no production authority and leaves historic no-admission intact. |

## 3. Production implementation evidence

No ordinary Git dispatch route changed. `GitEnvPolicy` still prepares the private immutable ODB view for trusted production reads. The contained-live route is opt-in/test-only, uses the existing `ManagedProcessService`, requests actual M004/M005 enforcement, and is not registered as a production `GitReadBackend`. No public consistency receipt or default switch was added.

## 4. Verification executed

### Commands run

```bash
python3 scripts/check_adr_integrity.py
python3 scripts/check_execution_ownership.py
python3 scripts/check_sandbox_contract.py
python3 scripts/check_git_forbidden_patterns.py
scripts/verify.sh quick
cargo test --locked --test linux_m004_process_ipc --features linux-git-test-support -- --nocapture
```

The hosted C002 cost run invokes:

```bash
CODEGG_GIT_COST_FILESYSTEM="$(stat -f -c %T "$GITHUB_WORKSPACE")" \
  cargo test --locked --test linux_git_c002_performance --features linux-git-test-support -- --nocapture
```

### Results

- Run 39255303376 passed on source SHA `31291fc8`; measurement test result: 1 passed, 0 failed, in 698.40 seconds. Runner was Ubuntu 24.04 x86_64, kernel `6.17.0-1022-azure`, Bubblewrap 0.9.0, Git 2.55.0, runner-reported `ext2/ext3`.
- On the 105 MiB packed/fragmented fixtures, contained-live cold-advised p95 was 16.791/137.073 ms for packed/fragmented `log -1`, and 30.254/41.382 ms for packed/fragmented `status --short`. Immutable-snapshot cold-advised p95 was 2,542.139/2,893.360 ms for packed/fragmented `log -1`, and 3,239.586/3,776.338 ms for packed/fragmented status.
- Contained-live warm fan-out-8 p95/max ranged from 53.722/60.784 ms to 99.324/103.773 ms across the four operation/shape combinations. Full per-route measurements and all fan-out values are in the research report and raw JSON.
- C002's native parity and race evidence is recorded in the source plan; latest completed full M004 owner matrix for the same test-only implementation is run 39182280516. A newer refresh, run 39254354958, was canceled during PTY qualification and is not counted as a pass.
- Exact-head macOS target compile run 39255837320 and protected-mount run 39256064866 passed at `59ab0d2d`. Full CI run 39255875342 exposed a stale v69 migration fixture (`runtime_preferences` missing before migration v83); the fixture now represents the schema available at version 69, and all four `cached_models_composite_key` tests pass locally. Full CI must be rerun on the final pushed source before merge.

## 5. Invariant review

- The immutable snapshot remains the ordinary trusted production path.
- Live reads are labeled weaker and never reported as an immutable generation.
- No-writer leases are not treated as external-writer exclusion; C012 remains valid.
- Required M004/M005 enforcement is obtained from the managed-process owner; no FullHost fallback or candidate-flag-only offline claim is introduced.
- Remote transport and destructive mutation authority remain outside this pilot.

## 6. Failure and recovery review

The test-only suite records allow/deny probes, contained child teardown, live metadata identity, and concurrent repack/ref-race behavior as described in the implementation plan. These samples do not prove safe consistency under arbitrary external writers, cancellation/crash cleanup for a production read service, independent-agent contention, or restart recovery. The production snapshot route remains available as rollback because no production selector changed.

## 7. Migration and compatibility review

No C002 database, wire, or public result migration was introduced. Windows/macOS trusted Git behavior remains on the snapshot path. The broader branch's additive network-intent migration was moved to v83 after main's v80–v82 migrations; that branch reconciliation does not activate the C002 backend.

## 8. Security review

Hosted probes used actual contained launch receipts and checked host credential/proc/network/IPC denial as recorded in C001/C002 evidence. The candidate remains limited to the ordinary repository shape validated by the pilot. External gitdirs/common dirs, linked or nested repositories, alternates/promisor metadata, concurrent host writers, and all Git configuration-driven helpers are not comprehensively qualified. These are production blockers, not assumptions waived by this closure.

## 9. Documentation and operations

- Preserved the new raw JSON result and summarized the run in `research/linux-trusted-git-cost-corrective/c002-cost-pilot-39255303376.md`.
- Updated the C002 plan, Linux trusted Git roadmap, closure record, and registry to distinguish test-only completion from production admission.
- ADR-0044 remains proposed; C010 no-admission and C012's negative no-writer-lease decision remain unchanged.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | Live ODB has no immutable-generation guarantee against arbitrary writers | Routine operations could observe an inconsistent object view | Keep test-only until a separate decision approves the consistency model and race handling. |
| high | C002 data does not cover full compatibility, external metadata, independent-agent contention, and all resource/copy costs | The measured latency cannot qualify general developer workflows | Complete broader compatibility/resource matrix and independent admission review. |
| high | ADR-0044 and M005 production-owner admission remain open | No authority to use this route in ordinary production operations | Obtain accepted ADR and separately qualify M005/M003/C010. |
| medium | Cold-cache eviction is advisory and comparison is end-to-end | Cold p95 may not equal truly cold storage or isolate setup overhead | Preserve methodology limits and do not use these numbers to rewrite C010. |

## 11. Roadmap disposition

C002 is conditionally closed as a test-only contained-read and cost pilot. It does not close Linux C002, M003, C010, or C005, and does not admit production live reads. C010 no-admission and C012 remain unchanged.

## 12. Registry updates

- Mark this bounded C002 test-only pilot conditionally closed and link this closure and the raw cost evidence.
- Keep the Linux trusted Git corrective roadmap active for C005/production gates; do not switch the immutable-snapshot route.
