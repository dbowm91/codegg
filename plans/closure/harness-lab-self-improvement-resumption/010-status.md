# Harness Lab Self-Improvement S010 — Closure

Status: **closed offline**. The paired native optimizer campaigns, prolonged
resource-pressure exercise, and integrated native-trial SIGKILL/restart gate
all passed. Together with S011's trusted intake and refusal gates, this closes
the offline operational-readiness criteria; `stable_offline` is established.
Live improvement evidence remains separate and is not established.

## Evidence

The S010 controller/test implementation is [83adb19f927918756f70443d9efe16317d616817](https://github.com/dbowm91/codegg/commit/83adb19f927918756f70443d9efe16317d616817). Its paired native qualification receipt is [`s010-integrated-offline.json`](s010-integrated-offline.json), and the resource-pressure samples are [`evidence/s010-resource-pressure.json`](evidence/s010-resource-pressure.json).

- `S010IntegratedNativeOptimizerTests.test_two_private_native_campaigns_reject_noop_and_accept_seeded_correction` passed twice from independent private roots, **1 test in 1488.285 seconds**. Both runs used real `run_optimizer`, native candidate builds, ACP trials, frozen positive and negative cases, and the independent verifier. Each no-op was rejected and each private seeded correction was accepted; both protected controls retained `assertion_failed`. Eight native loopback trials were recorded across the two campaigns.
- Both campaigns used the same baseline binary digest `a3f30bce79877d038664ebf005ed868cb0053600c83282bbc44d82d1d178e3f1` and accepted corrective binary digest `a0f3449a582c8d9cb6da3f4477bb3f60ca44784d5c791364ad4c306f88abc3c8`. Their source, patch, checkpoint, trial, and verifier identities are in the machine receipt.
- The bounded pressure exercise overlapped the second campaign's corrective build for **180.29 seconds**: 174 rounds, 348 successful one-worker offline Cargo checks across disjoint targets, zero provider requests, and a minimum observed free-disk value of 35,401,793,536 bytes (15 GiB configured floor). CPU/RSS samples cover the Cargo controller process; rustc child metrics are unavailable and are labeled accordingly.
- The existing six-phase generic optimizer SIGKILL/resume-or-refuse matrix and five compile-correction mechanics tests passed in a focused run (**6 tests total**). This is separate evidence; it did not SIGKILL an optimizer while its real native candidate trial was in progress.
- S010 made zero provider requests and has no billed USD cost.

The missing native composition then passed in `S010NativeTrialCrashRecoveryTests.test_sigkill_during_native_acp_trial_resumes_once_without_duplicate_rows` (**1 test in 196.000 seconds**). The committed controller at `9f11fdfdc90b6a775b7f2142897f21558d6d2944` ran the real optimizer and a native ARM64 CodeGG ACP write trial against a local loopback fixture. After the native tool terminal was `completed` and the fixture had accepted the tool result, the controller sent SIGKILL to the isolated optimizer/ACP process group while the provider response remained active. The same checkpoint resumed once: the deliberate no-op was rejected as `NO_IMPROVEMENT`, the proposer was not called again, one final `passed` ledger row existed for the pair, and no duplicate row was created. The process-group children and every private lane daemon were reaped; no default daemon or external provider was used. The exact identities and outcomes are recorded in [`s010-native-trial-crash-recovery.json`](s010-native-trial-crash-recovery.json).

Reproduction command (set the two paths to the qualified ARM64 binary and its Cargo target root):

```bash
CODEGG_S010_NATIVE_CRASH_INTEGRATION=1 \
CODEGG_S010_CRASH_BINARY=/path/to/target/debug/codegg \
CODEGG_S010_CRASH_TARGET_CACHE=/path/to/target \
CODEGG_S010_CRASH_RECEIPT_PATH=plans/closure/harness-lab-self-improvement-resumption/s010-native-trial-crash-recovery.json \
python3 -m unittest dev.harness_lab.tests.test_improve.S010NativeTrialCrashRecoveryTests.test_sigkill_during_native_acp_trial_resumes_once_without_duplicate_rows -v
```

| S010 acceptance gate | Evidence | Result |
|---|---|---|
| Two complete private `run_optimizer` campaigns; native candidate build, ACP trials, independent verifier, no-op rejection, seeded correction acceptance, protected negative control | [`s010-integrated-offline.json`](s010-integrated-offline.json), two independent campaigns, eight native trials | Pass |
| Native optimizer trial interrupted by SIGKILL and resumed/refused safely without duplicate proposal/trial row | [`s010-native-trial-crash-recovery.json`](s010-native-trial-crash-recovery.json); same checkpoint resumed, no-op rejected, one final pair row | Pass |
| Repeated bounded Cargo contention plus predeclared prolonged pressure and disk floor | [`evidence/s010-resource-pressure.json`](evidence/s010-resource-pressure.json); 180.29 seconds, 174 rounds, 348 checks, disk floor respected | Pass |
| Full cleanup and no provider spend | Crash/restart receipt; isolated groups and private lane daemons reaped, external requests 0, USD not applicable | Pass |
| Cross-suite and source guards | clean-worktree `scripts/verify.sh quick` at `9f11fdfdc`; all guards, 471 tests (3 skipped), locked all-target workspace check | Pass |

## Remaining strict gate and limits

No S010 acceptance gate remains open. This proves bounded offline operational readiness only; the seeded correction and crash-recovery no-op are private synthetic controls, not newly discovered current CodeGG defects. S011 found `eligible_real_defects=0`, so S013/S014 remain blocked and no live-improvement rate or `stable_live_evidenced` claim is made. No daemon, scheduler, ACP protocol, or provider change is implied by these results.

The runs were executed on Apple Silicon in ARM64 mode (`aarch64-apple-darwin`). Task-owned Cargo targets have been removed after preserving the built binary and receipts; the default CodeGG daemon was not used as the subject daemon.

## Cross-suite check

The complete Harness Lab suite passed from a clean source worktree with the qualified ARM64 binary supplied for the native H006/H007 tests: `python3 -m unittest discover -s dev/harness_lab/tests -q` — **470 tests, 11 skipped** in 85.134 seconds. The first `scripts/verify.sh quick` run from the dirty primary worktree completed its guards but the suite phase repeated two environment-bound failures (the pre-existing fixture edits and no H006 binary configured). A clean-worktree `scripts/verify.sh quick` passed at `401447a7d` with all guards, **470 tests (10 skipped)**, and `cargo check --workspace --all-targets --locked`. It was repeated against the final S010 controller commit `9f11fdfdc90b6a775b7f2142897f21558d6d2944`: all guards passed, the suite ran **471 tests (3 skipped)**, and the locked all-target workspace check passed. Rust's `rust-objcopy` emitted a non-fatal warning because that toolchain installation lacks `libLLVM.dylib`.
