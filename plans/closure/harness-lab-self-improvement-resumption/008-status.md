# Harness Lab Self-Improvement S008 — Closure

Status: **closed conditionally — the offline fault matrix is complete and repeated; `stable_offline` is not established**.

## Fault matrix and exact local verification

The machine-readable map is [`evidence/s005-offline-fault-matrix.json`](evidence/s005-offline-fault-matrix.json). Implementation and tests are at `5bf63e271e396d28daee2e2cea766f1bc32bad73`.

- The affected offline set passed twice from fresh temporary roots: `python3 -m unittest dev.harness_lab.tests.test_improve dev.harness_lab.tests.test_supervisor dev.harness_lab.tests.test_budget dev.harness_lab.tests.test_proposer_gateway dev.harness_lab.tests.test_defect_intake dev.harness_lab.tests.test_autonomy_run dev.harness_lab.tests.test_sustained dev.harness_lab.tests.test_h007_native -q` — **190 tests, one optional skip, per run**.
- Native `scripts/verify.sh quick` passed: **451 Harness Lab tests, two optional skips**, all 84 guards, and `cargo check --workspace --all-targets --locked`.
- Native CodeGG binary: Mach-O arm64, SHA-256 `2e381609e7517ada5a09fa3ef9daa5d781798ae4ba5c2cfa4baf293ef6e40d7f`, built from Rust product source at `d64bd207333fb1a5fec4f85c0b3514e7b16f83ce`. Shell initially reports x86_64 under Rosetta (`sysctl.proc_translated=1`); the successful build and tests explicitly ran arm64 with Rust 1.98.1 and MacPorts arm64 `pkg-config` paths.
- Native H007 exact-ID Rust job sequence passed: dependent format/store/index/query/CLI repairs, two repair cycles, exact submitted job ID observed running then completed, useful work while that job ran, zero event gaps, and the independent verifier passed.
- S005's separate native private-daemon parent-death test passed: controller SIGKILL after ACP session creation, exact lane cleanup/reconciliation, unrelated sentinel survived, and default daemon fingerprint remained unchanged.
- The actual OpenCode 1.19.3 CLI used an isolated loopback fixture. Its request ceiling was one; request N+1 was refused before fixture forwarding. No live provider request occurred.

## Campaign phase faults and resource contention

`CrashRecoveryTests.test_campaign_controller_sigkill_recovers_each_durable_phase_once` SIGKILLs the optimizer at six durable phases: proposal request, patch capture, candidate build, independent verifier, ACP evaluation, and score publication. Resume/refusal uses the persisted phase; a second proposal/admission is rejected by the test, and exactly one iteration remains recorded. `OptimizerCampaignSequenceTests.test_campaign_fault_matrix_recovers_or_refuses_without_duplicate_proposal` injects an ordinary failure at each of those same boundaries and checks recovery/refusal without duplicate proposal. Existing checkpoint-pointer and ledger-append SIGKILL tests retain their no-duplicate-row assertions.

`CampaignResourceContentionTests.test_optimizer_progresses_under_repeated_parallel_private_cargo_contention` runs the real optimizer while two offline Cargo checks with one worker each remain active in disjoint private target roots. The scenario repeats twice; both builds and the controller complete and their temporary targets are removed. This is a bounded overlap qualification, not a prolonged saturation run.

## Verdict and limits

The named offline fault matrix is complete. `stable_offline` remains **not established** because the repeated optimizer sequence uses a synthetic proposer and qualification callback; native CodeGG ACP/job qualification is exercised separately rather than as an optimizer-integrated paired campaign. The bounded Cargo overlap is not a prolonged optimizer stress campaign. No candidate was launched because S004 found no independently eligible current defect. No real model or provider was used; live provider calls are zero and billed USD is `unknown`.

Hosted exact-head CI for `5bf63e2` is pending. S006 remains `stable_live_not_established`; H007's consumed authorizations remain consumed. No wider daemon, scheduler, ACP or provider redesign is supported by this evidence.
