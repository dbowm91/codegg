# Harness Lab Mainline Qualification H003 — Closure Status

Status: closed for the deterministic native Rust/job workload; sustained live-model reliability remains unmeasured and belongs to H004.

## Fixture and verifier

Case: `coherent-rust-parser-repair-background-job` (schema v2). Fixture source SHA-256: `a33e1139c58646db8e06b3b837c2863c6c8c645f2da2b60ebd69f97f3632662c`. Hidden verifier SHA-256: `8ec486e216bb0826b7e2af0e0309901b093d422fd37c76e1c30d411e774e1d05`. Frozen public contract test SHA-256: `7f65eaf0b371069e816acc4e0d3ec8af2ad8c50749d8ebbe0feffb6a2f58df74`.

The native scripted ACP trajectory reads the crate, starts a real CodeGG background `test` job, reads while it runs, and waits by the exact returned job ID. The first job exposes a compile error; the second exposes independent logic failures; after two native `write` edits the final job passes the hidden verifier. The verifier and its frozen test digest stay outside the writable lane. A test rewrite/deletion shortcut fails with a named reason.

Each run gets a private workspace, daemon, Cargo home and Cargo target directory. Two simultaneous native lanes passed independently with distinct job IDs and no shared target state. A failed lane cannot prevent the following lane from completing. The report schema carries binary/fixture/verifier digests, session and job IDs, native call hashes/statuses, per-call state revisions, event-gap health, provider request count, elapsed time, hidden-verifier result, cache class and cleanup state. Raw output and credentials are omitted; unavailable provider cost and context-transition instrumentation remain explicitly unavailable.

## Verification

Final focused command: `python3 -m unittest dev.harness_lab.tests.test_sustained.TypedRecoveryControlTest dev.harness_lab.tests.test_sustained.LongHorizonNativeJobTest dev.harness_lab.tests.test_recovery_controls_v2 dev.harness_lab.tests.test_long_horizon_report -v` — **21 passed in 65.320 seconds**. H003 contributed 9 report/scenario tests; H002 contributed 12 controls. The native correct trajectory, two-lane concurrency, test-tamper rejection, unsolved-fixture rejection and frozen-digest checks all passed. No OpenCode Go route or external network was used.

Context compaction/epoch transitions are not exposed by this deterministic ACP fixture and remain `unavailable_not_exposed`; this closure makes no compaction-continuation claim. Delegation, MCP/LSP and multi-minute compile cases remain deferred as specified by the plan.

At the time this H003 closure was written, exact resulting-main integration and fresh H004 authorization were still pending. H001 subsequently closed on merge SHA `9185b9c34ce30fd52a41aacd0aaae75f39e54895`; H004's one approved live measurement is recorded in `plans/closure/harness-lab-mainline-qualification/004-status.md` and remains conditional because its 112.7-second duration did not demonstrate the planned sustained horizon.
