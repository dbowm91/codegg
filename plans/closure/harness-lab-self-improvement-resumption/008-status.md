# Harness Lab Self-Improvement S008 — Closure

Status: **closed conditionally — deterministic offline corrective matrix repeated successfully; `stable_offline` not established**.

## Fault matrix exercised

The full machine-readable map is [`evidence/s005-offline-fault-matrix.json`](evidence/s005-offline-fault-matrix.json). The affected offline set was run twice from fresh temporary roots:

- `python3 -m unittest dev.harness_lab.tests.test_improve dev.harness_lab.tests.test_supervisor dev.harness_lab.tests.test_budget dev.harness_lab.tests.test_proposer_gateway dev.harness_lab.tests.test_defect_intake dev.harness_lab.tests.test_autonomy_run -q`: **162 passed, 1 optional skip** in each run.
- Native exact-ID background-job observation: **passed** using the prebuilt subject binary SHA-256 `80292d55a3ba9ba2291e2daffa67d083a450b9a4ebaeaf578ff882dda15bc04c`, from source revision `ab22d66f28681321b8d2b46001ed6698764c9484`. There is no product-source diff under `src/`, `crates/` or `Cargo.toml` between that revision and S008's source baseline `e8dbb8af3793acf46741eae89b3c5509fc952e47`.
- Controller parent-death recovery: a real ACP controller was SIGKILLed after private daemon/session creation. The exact lane identity was reconciled; no lane daemon remained, the unrelated sentinel survived, and the default daemon fingerprint was unchanged.
- Cargo contention: two offline `cargo check` processes ran concurrently with one Cargo worker each and disjoint private target directories in two rounds. Both succeeded; temporary target directories were removed.
- Existing checkpoint and ledger SIGKILL tests proved pointer/append reconciliation and exactly-once pair rows. The new proposer N+1 fault test proved the sanitized admission receipt is retained in the campaign checkpoint without publishing an iteration or retrying it.
- The actual OpenCode CLI test observed multiple provider HTTP requests, admitted/forwarded one under ceiling 1, refused N+1 before forwarding, stopped the CLI and returned the bounded receipt. No live provider request occurred.

## Campaign transition inventory

Admission and request refusal map to budget and gateway tests. Proposal and patch capture map to route, scope and N+1 checkpoint tests. Build correction maps to bounded retry tests. Independent verifier and score falsifiers map to the score-contract suite. ACP/timeout/cancellation map to supervisor and sustained-lane tests. Ledger and checkpoint publication map to the existing SIGKILL/exactly-once tests. Cleanup maps to successful/failed lane removal plus the new parent-death test.

The injection coverage is boundary-specific, not a SIGKILL at every individual phase: process death is directly injected at checkpoint-pointer and ledger-append boundaries and at the ACP controller/daemon boundary. Build, verifier, ACP evaluation, publication and ordinary cleanup faults have deterministic failure/cancellation tests, but no separate process kill is inserted after each one.

## Verdict and limits

`stable_offline` remains **not established**. The repeated test set covers deterministic controller outcomes, the newly closed HTTP admission path, bounded Cargo contention and exact private-subject cleanup. It is not a prolonged optimizer-under-saturation campaign, and it does not inject process death at every side-effect boundary. The native job test used an already-built binary with the exact S005 product source; a local rebuild in this x86_64 shell could not link because available MacPorts libraries were arm64. That generated 9.4 GiB target tree was removed; filesystem free space returned to 75 GiB.

No real model or provider was used, no new campaign was started, H007's consumed authorizations remain consumed, and USD is `unknown`. Hosted exact-head CI is pending. A future qualification that needs `stable_offline` must close the remaining phase-by-phase crash/sustained-optimizer stress evidence without broadening daemon or ACP architecture absent a reproduced defect.
