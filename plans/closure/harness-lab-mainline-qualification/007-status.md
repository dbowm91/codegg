# H007 — Deep Rust Workload and Separate Horizon Measures

Status: **offline implementation and calibration complete; seven H007 live attempts across three explicit authorizations are consumed without sustained qualification. Five additional authorized retries exercised the same Muse Spark route; none met the full qualification. Exact-head hosted CI passed through `647ac905`; the diagnostic-only repair-reason correction is committed separately.**

Implementation source SHA: `d7f1f528df622e8e9bfe4a2d17e2f22ce90d93f5`.

Plan: [`007-deep-workload-and-separate-horizon-measures.md`](../../implementation/harness-lab-mainline-qualification/007-deep-workload-and-separate-horizon-measures.md)

## Outcome boundary

H007 supplies a deeper five-crate build-provenance catalog fixture, an independent holdout verifier, and a versioned result projection that reports task outcome separately from horizon outcome. A correct early completion is a task pass with horizon `not_observed`; duration cannot turn correctness into failure. Horizon qualification still requires a 30-minute active span, no activity gap above five minutes, exact-ID job lifecycle and productive overlap, continuation, at least two repair cycles, and complete verifier/event evidence. H006 receipts and scoring remain unchanged.

The H007 plan delivered offline fixture and measurement calibration; its offline completion did not authorize a live provider run. The seven attempts used `opencode_go/muse-spark-1.3-contributor`, each capped at 60 provider requests and 3,600 seconds, with a 10 GiB disk floor, 300-second ACP inactivity cancellation, user cancellation, and billed USD recorded as unknown when unavailable. The first two attempts had separate authorizations; a third authorization covered attempts 3–7. All are consumed. The results do not establish sustained autonomous qualification or billed USD cost.

## Separately authorized live attempt — 2026-10-29

The first launch was refused before daemon or provider startup because the autonomy case package lacked the editable fixture. The fixture was copied from the frozen calibration source and a regression now checks every packaged file digest. A second launch was refused before provider startup because the default lane's immediate parent did not yet exist for the free-disk probe; the actual run used a dedicated pre-created private lane root. Both refusals had zero provider requests and are retained in the sanitized evidence record.

The live turn then ended after 488,641 ms with typed `provider_timeout`. The provider-boundary receipt recorded 19 requests against the 60-request ceiling. The OpenCode Go allowance receipt reported `0.011578` allowance units; this is quota metadata, not a cash charge. Billed USD was unavailable and is recorded as `unknown`.

The agent made 25 tool calls. Event capture was complete: 25 starts and 25 terminals, zero event gaps, and zero dropped events. Four test-tool calls returned distinct job IDs, but no `job` status or wait calls followed; no same-ID running-to-terminal lifecycle, productive overlap, or continuation was established. The hidden verifier ran and exited 1. The result is `task_outcome=inconclusive`, `horizon_outcome=not_qualified`, reason `measurement_incomplete`; the 301,922 ms activity span was below the 1,800,000 ms threshold. No repair cycle was confirmed. The disk watcher observed a minimum of 143,434,752,000 bytes free, did not trigger, and daemon cleanup reported no error.

This run does not show an ACP event-loss, scheduler, or daemon defect: every observed tool event had a terminal and the event stream reported no gaps. It also does not establish that the model would have completed the task, because the provider timeout ended the turn. The exact-ID requirement was not exercised after four job submissions; no failure of the job-status tool surface is demonstrated. The narrow next owner, if another live qualification is authorized, is the existing OpenCode Go provider-timeout boundary: preserve enough typed timeout/continuation evidence to distinguish route timeout from agent task failure. Do not reopen daemon, scheduler, ACP projection architecture, or multi-provider integration on this evidence.

Machine-readable sanitized receipt: [`evidence/007-live-attempt.json`](evidence/007-live-attempt.json).

## Separately authorized retry — 2026-10-29

The second authorization used the same frozen case, route, request/time caps, disk floor, cancellation rules, and independent verifier in a fresh private lane. It ended normally after 316,694 ms with 35/60 provider requests. The turn completed, and the verifier ran all five checks. Four passed; the remaining check found one successful failure/repair cycle where two were required. The task result is therefore `failed`, not inconclusive. The billed USD amount was unavailable (`unknown`); the allowance receipt reported `0.027364` quota units, not a cash charge.

The retry establishes the production-shaped job path in this live run: one submitted ID was observed as `running` and then `completed`, with productive work between those observations. A separate submitted job reached `failed`. Event capture was complete (48 starts, 48 terminals, zero gaps/drops); the maximum observed activity gap was 49,461 ms. Continuation was confirmed. Active work spanned 307,886 ms, short of the 1,800,000 ms horizon. Disk never approached the 10 GiB floor; the lowest observed free space was 147,300,352,000 bytes. No stall cancellation or daemon cleanup error occurred.

The runner's original `horizon_reason` was `continuation_missing`, even though `continuation_confirmed` was true, because that branch also represented fewer than two repair cycles. The actual missing requirement was the second repair. Follow-up commit `75613e050625c9d39456b5e69132fa665a3cb13a` separates these reasons as `repair_cycles_insufficient` and `continuation_missing`, with a focused regression; the historical run result remains unchanged.

### What the paired live attempts show

- Attempt 1 ended with typed `provider_timeout` after 19 requests and did not reach job-status observation. Its complete event capture showed no ACP event loss.
- Attempt 2 completed normally after 35 requests. It proved same-ID running/terminal observation, productive overlap, continuation, and complete event capture, but the agent completed only one of two required repair cycles and the verifier failed that criterion.
- The observed job and ACP surfaces behaved correctly in the retry. The first provider timeout did not recur, so these two trials do not establish a persistent provider reliability defect. They do show that the selected agent/provider route can complete a live turn, while this run's immediate task failure was insufficient repair work. The next narrow owner is the agent task-execution/recovery behavior that stops after one repair; do not reopen daemon, scheduler, or ACP architecture. Track the OpenCode Go timeout separately if it recurs.

The second attempt's sanitized machine-readable record is [`evidence/007-live-attempt-2.json`](evidence/007-live-attempt-2.json).

## Five additional authorized retries — 2026-10-29

The user authorized up to five additional runs of the same frozen case and route. Attempts 3–7 each used a fresh private lane and the existing limits: 60 requests, 3,600 seconds, a 10 GiB free-disk floor, 300-second ACP inactivity cancellation, and user cancellation. No run hit its request, wall-clock, disk, or inactivity cap. USD billing was unavailable; the OpenCode Go allowance observations are quota metadata, not cash charges.

| Attempt | Elapsed | Requests | Active span | Task result | Exact-ID running/terminal with overlap | Repairs | Verifier |
| --- | ---: | ---: | ---: | --- | --- | ---: | --- |
| 3 | 456 s | 8 | 191 s | Inconclusive: typed `provider_timeout` | No | 0 | Error before tests completed |
| 4 | 686 s | 17 | 674 s | Failed implementation | No | 0 | Cargo test setup failed after `catalog-index` missing-key panics |
| 5 | 121 s | 27 | 111 s | Failed qualification | No qualifying lifecycle | 0 | 4/5 passed; exact-ID productive overlap absent |
| 6 | 612 s | 21 | 582 s | Failed implementation | No | 0 | Cargo test setup failed on `catalog-query` lineage provenance assertion |
| 7 | 248 s | 40 | 235 s | Failed qualification | Yes | 1 of 2 | 4/5 passed; second repair absent |

All five runs had complete ACP tool-event capture: starts equaled terminals, with zero gaps and zero dropped events. The lowest observed free-disk measurement across them was 124 GiB, well above the 10 GiB floor. Continuation was confirmed in attempts 5–7. Attempt 7 demonstrated the exact same job ID observed running and completed, with productive work during the running interval. No run reached the 30-minute active-work threshold; the longest span was 674 seconds.

### What the five retries establish

- The ACP event stream and provider request accounting remained complete in every receipt. The CodeGG job surface demonstrated the required same-ID lifecycle and productive overlap in attempt 7. These runs provide no evidence for a daemon, scheduler, ACP projection, or event-loss defect.
- One of five retries ended in a typed provider timeout (attempt 3). The other four ended normally. That isolated timeout shows route variability, but it did not recur often enough to establish a persistent OpenCode Go transport failure.
- The recurring miss was agent task execution. Attempts 4 and 6 ended with failing Rust implementation tests; attempts 3–6 did not demonstrate the exact-ID productive job lifecycle; no retry completed both distinct repair cycles. Attempt 7 completed the job lifecycle and continuation checks but stopped after one repair cycle and 235 seconds of active work.
- Therefore the harness is not the supported corrective owner. The evidence points to the selected model/provider route's inconsistent completion of the task's required recovery sequence and sustained horizon, with one separate provider timeout. Any further live attempt needs fresh authorization. Keep the next corrective narrow to task-execution/recovery qualification or a separately evaluated model/provider choice; do not reopen daemon, scheduler, ACP event projection, or multi-provider architecture.

Sanitized machine-readable receipts: [`attempt 3`](evidence/007-live-attempt-3.json), [`attempt 4`](evidence/007-live-attempt-4.json), [`attempt 5`](evidence/007-live-attempt-5.json), [`attempt 6`](evidence/007-live-attempt-6.json), and [`attempt 7`](evidence/007-live-attempt-7.json). The five-run authorization is consumed; additional live provider execution requires fresh explicit authorization.

## Fixture and verifier calibration

- Five dependent crates implement versioned records, durable import/recovery, incremental dependency indexing, provenance queries, and CLI/export.
- The untouched starter fails implementation checks; the reference solution passes the independent verifier. Frozen public inputs and manifest mutation checks pass.
- Corpus: 96 records across three snapshots and four input files (3,924 bytes). The generated index integration case validates 250,000 records in 3.29 CPU seconds, with 316,874,752 bytes peak RSS on this host. There are no external crate dependencies or time-padding loops.
- Hidden verifier digest: `13617671ceedfe1b4cff89c9a96ad5380fd94ca4799f861f95a404f84090d33c`.
- Frozen manifest digest: `c703c1cb256f268500029cf0ca35faf9a8f246485afb2e3e5d98f166f15e4358`.
- Native CodeGG ACP regression submits a real managed Cargo background job, records and re-observes the same job ID as running and completed, performs productive dependent edits during the running interval and after terminal observation, and proves two failed-check/repair cycles plus continuation. The bounded job wait is observation of actual work, not horizon padding.
- H007 projection tests cover early correct completion, long-running task failure, verifier/integrity uncertainty, exact activity and gap boundaries, missing or mismatched lifecycle, continuation, repairs, and independent task/horizon summary denominators.

## Verification record

Completed focused evidence before the final hosted run:

- H007 scoring and existing autonomy-run tests: 61 passed.
- Native ACP job lifecycle regression: passed.
- Fixture reference verifier: passed; untouched starter rejected as expected; frozen-input checks passed.
- `python3 scripts/check_test_assertion_quality.py`: passed.
- `python3 scripts/check_test_duplication.py`: passed.
- After the live preflight found the missing runner fixture, added a source-digest regression and packaged the frozen starter in the autonomy case root. `python3 -m unittest dev.harness_lab.tests.test_h007 dev.harness_lab.tests.test_autonomy_run`: 62 passed; both assertion and duplication guards passed again.
- `python3 -m unittest dev.harness_lab.tests.test_h007`: 14 passed after adding the live-case fixture packaging regression.
- After the second live run exposed the conflated `continuation_missing` reason, commit `75613e050625c9d39456b5e69132fa665a3cb13a` split repair shortfall from missing continuation. `python3 -m unittest dev.harness_lab.tests.test_h007`: 15 passed; assertion and duplication guards passed.
- `scripts/verify.sh scoped origin/main`: passed with `PKG_CONFIG=/usr/local/bin/pkg-config CMAKE_PREFIX_PATH=/usr/local`; 16,693 passed and 10 skipped across the scoped workspace sweep.
- `scripts/verify.sh full`: passed with `PKG_CONFIG=/usr/local/bin/pkg-config CMAKE_PREFIX_PATH=/usr/local NEXTEST_TEST_THREADS=2`. Quick guards, workspace check, strict Clippy, 410 Harness Lab tests (1 skipped), workspace sweep (16,693 passed, 10 skipped), and feature-gated subset (8,014 passed, 5 skipped) all passed. A previous unconstrained run had a provider timeout; the first full rerun then hit one timing-sensitive egglsp restart test. That restart test passed in isolation, and this complete two-worker rerun passed.
- Lowest free disk observed during final verification was 112 GiB; 162 GiB was available before the feature build and 121 GiB remained after it. Temporary fixture Cargo targets are removed before handoff.

### Exact-head hosted verification

The offline implementation at `d7f1f528df622e8e9bfe4a2d17e2f22ce90d93f5` passed exact-head hosted CI: [run 40949588693](https://github.com/dbowm91/codegg/actions/runs/40949588693). The live preflight packaging correction at `959509dc3a945eb7c465b975c05561d5f98f5eb3` passed exact-head hosted CI in [run 40963335943](https://github.com/dbowm91/codegg/actions/runs/40963335943). The diagnostic-only repair-reason correction and attempt-2 closure evidence at `d2ae03c201b575dadef0bf9d9506dc084d93eed7` passed exact-head hosted CI in [run 40967248271](https://github.com/dbowm91/codegg/actions/runs/40967248271). The additional attempt-3-through-7 closure records in this update do not change runtime or harness code; hosted checks will run on the resulting PR head.

## Changed files

The implementation source commit changes exactly the following files (50 files):


- `dev/autonomy_fixtures/build-provenance-catalog/verifier/test_catalog_contract.py`
- `dev/harness_lab/autonomy_run.py`
- `dev/harness_lab/fixtures/agent_autonomy_h007/cases.jsonl`
- `dev/harness_lab/fixtures/build_provenance_catalog.case.json`
- `dev/harness_lab/fixtures/build_provenance_catalog/fixture/CONTINUATION.md`
- `dev/harness_lab/fixtures/build_provenance_catalog/fixture/Cargo.lock`
- `dev/harness_lab/fixtures/build_provenance_catalog/fixture/Cargo.toml`
- `dev/harness_lab/fixtures/build_provenance_catalog/fixture/README.md`
- `dev/harness_lab/fixtures/build_provenance_catalog/fixture/catalog-cli/Cargo.toml`
- `dev/harness_lab/fixtures/build_provenance_catalog/fixture/catalog-cli/src/main.rs`
- `dev/harness_lab/fixtures/build_provenance_catalog/fixture/catalog-format/Cargo.toml`
- `dev/harness_lab/fixtures/build_provenance_catalog/fixture/catalog-format/src/lib.rs`
- `dev/harness_lab/fixtures/build_provenance_catalog/fixture/catalog-index/Cargo.toml`
- `dev/harness_lab/fixtures/build_provenance_catalog/fixture/catalog-index/src/lib.rs`
- `dev/harness_lab/fixtures/build_provenance_catalog/fixture/catalog-index/tests/corpus.rs`
- `dev/harness_lab/fixtures/build_provenance_catalog/fixture/catalog-query/Cargo.toml`
- `dev/harness_lab/fixtures/build_provenance_catalog/fixture/catalog-query/src/lib.rs`
- `dev/harness_lab/fixtures/build_provenance_catalog/fixture/catalog-store/Cargo.toml`
- `dev/harness_lab/fixtures/build_provenance_catalog/fixture/catalog-store/src/lib.rs`
- `dev/harness_lab/fixtures/build_provenance_catalog/fixture/contract.json`
- `dev/harness_lab/fixtures/build_provenance_catalog/fixture/corpus/invalid-checksum.records`
- `dev/harness_lab/fixtures/build_provenance_catalog/fixture/corpus/snapshot-0.records`
- `dev/harness_lab/fixtures/build_provenance_catalog/fixture/corpus/snapshot-1.records`
- `dev/harness_lab/fixtures/build_provenance_catalog/fixture/corpus/snapshot-2.records`
- `dev/harness_lab/fixtures/build_provenance_catalog/reference/CONTINUATION.md`
- `dev/harness_lab/fixtures/build_provenance_catalog/reference/Cargo.lock`
- `dev/harness_lab/fixtures/build_provenance_catalog/reference/Cargo.toml`
- `dev/harness_lab/fixtures/build_provenance_catalog/reference/README.md`
- `dev/harness_lab/fixtures/build_provenance_catalog/reference/catalog-cli/Cargo.toml`
- `dev/harness_lab/fixtures/build_provenance_catalog/reference/catalog-cli/src/main.rs`
- `dev/harness_lab/fixtures/build_provenance_catalog/reference/catalog-format/Cargo.toml`
- `dev/harness_lab/fixtures/build_provenance_catalog/reference/catalog-format/src/lib.rs`
- `dev/harness_lab/fixtures/build_provenance_catalog/reference/catalog-index/Cargo.toml`
- `dev/harness_lab/fixtures/build_provenance_catalog/reference/catalog-index/src/lib.rs`
- `dev/harness_lab/fixtures/build_provenance_catalog/reference/catalog-index/tests/corpus.rs`
- `dev/harness_lab/fixtures/build_provenance_catalog/reference/catalog-query/Cargo.toml`
- `dev/harness_lab/fixtures/build_provenance_catalog/reference/catalog-query/src/lib.rs`
- `dev/harness_lab/fixtures/build_provenance_catalog/reference/catalog-store/Cargo.toml`
- `dev/harness_lab/fixtures/build_provenance_catalog/reference/catalog-store/src/lib.rs`
- `dev/harness_lab/fixtures/build_provenance_catalog/reference/contract.json`
- `dev/harness_lab/fixtures/build_provenance_catalog/reference/corpus/invalid-checksum.records`
- `dev/harness_lab/fixtures/build_provenance_catalog/reference/corpus/snapshot-0.records`
- `dev/harness_lab/fixtures/build_provenance_catalog/reference/corpus/snapshot-1.records`
- `dev/harness_lab/fixtures/build_provenance_catalog/reference/corpus/snapshot-2.records`
- `dev/harness_lab/h007_results.py`
- `dev/harness_lab/tests/test_h007.py`
- `dev/harness_lab/tests/test_h007_native.py`
- `plans/implementation/harness-lab-mainline-qualification/007-deep-workload-and-separate-horizon-measures.md`
- `plans/registry.md`
- `plans/subsystems/harness-lab-mainline-and-long-horizon-qualification-addendum.md`

### Separate live-attempt and diagnostic follow-ups

Commit `959509dc3a945eb7c465b975c05561d5f98f5eb3` adds:

- `dev/autonomy_fixtures/build-provenance-catalog/fixture/` (20 frozen starter files required by the live case loader)
- `dev/harness_lab/tests/test_h007.py` (canonical-to-packaged source digest regression)

This documentation and evidence follow-up records the consumed authorization and typed provider timeout. Neither follow-up changes the binary used for the live attempt. H005/H006 fixture contents, receipts, outcomes, and authorization records are preserved; no live sustained qualification pass is claimed.

Commit `75613e050625c9d39456b5e69132fa665a3cb13a` changes only:

- `dev/harness_lab/h007_results.py` — reports `repair_cycles_insufficient` separately from `continuation_missing`.
- `dev/harness_lab/tests/test_h007.py` — regression for the distinct reason values.

The correction clarifies new H007 receipts; it does not rewrite either recorded live attempt or any H006 history. Exact-head hosted CI for this commit is pending.
