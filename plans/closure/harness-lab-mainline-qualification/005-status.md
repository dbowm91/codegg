# Harness Lab Mainline Qualification H005 — Closure Status

Status: **blocked — the sole approved live attempt was consumed and did not qualify as sustained; no further live attempt is authorized**

Source implementation plan: `plans/implementation/harness-lab-mainline-qualification/005-genuine-sustained-multi-stage-rust-workload.md`

Source subsystem roadmap: `plans/subsystems/harness-lab-mainline-and-long-horizon-qualification-addendum.md`

Branch: `plans/harness-lab-h005-sustained-workload-20261027`; PR #211 is draft at `26f2d8030d6d84bfb26a25fa006bc486202d08d8`.

## 1. Finding

H005's multi-crate event-ledger workload, fixture/verifier, autonomy manifest, constrained job projection, request-boundary cap and disk-floor watcher are implemented and pushed. The sanitized exact PR head `26f2d8030d6d84bfb26a25fa006bc486202d08d8` passed hosted CI run [40632587553](https://github.com/dbowm91/codegg/actions/runs/40632587553), including GitGuardian, strict Clippy and the workspace test sweep. An earlier code head also passed CI run [40629696300](https://github.com/dbowm91/codegg/actions/runs/40629696300). At the operator's direction, each of the two stored OpenCode Go credentials was tested through a separate CodeGG standalone completion using `opencode_go/muse-spark-1.3-contributor`; both were accepted, with one provider request admitted each. The non-secret result receipt is `evidence/005-credential-preflight.json`. No secret or response body was printed or saved. The operator selected the newer account. A read-only local-daemon listing verified an active connection matching that selected account; no connection was created or replaced by this check. The sole H005 live attempt subsequently ran and is recorded in `evidence/005-live-attempt.json`.

The sole live case ran for 614,218 ms (10m14.218s), ended normally, and failed the hidden verifier. It did not meet the 30-minute threshold. The verifier observed only one of two required failure-diagnosis-repair cycles. Seven test jobs were submitted, but every projected status was null; no running/terminal observation or exact-ID wait was captured. The request receipt reports zero provider-boundary admissions despite 65,608 tokens. The lane did not receive `CODEGG_HARNESS_MAX_PROVIDER_REQUESTS`, so the actual outbound request count and request-cap enforcement are unknown. This attempt is not a sustained qualification, and the one-run authorization is consumed.

## 2. Requirement evidence

| Requirement | Evidence | State |
|---|---|---|
| Distinct H005 multi-crate workload and hidden verifier | `dev/harness_lab/fixtures/agent_autonomy_h005/`; event-ledger source spans five shards and 50,000 records with an independent verifier | Implemented; deterministic fixture tests passed in hosted CI |
| Versioned H005 manifest and privacy-bounded job identity | H005 case manifest is isolated from prior autonomy manifests; schema-v3 projection exposes constrained job identity/action/status | Implemented; hosted CI passed |
| Request cap and disk floor | Provider-boundary admission capped at 300 total; runner fails closed on unreadable/below-10-GiB disk and cancels on breach | Offline cap code implemented; the live lane missed the environment binding, so its boundary count is unusable (0 admissions reported despite nonzero token usage); cap enforcement unqualified |
| Exact-head hosted CI for the implementation | PR #211 head `26f2d8030d6d84bfb26a25fa006bc486202d08d8`; run #40632587553 concluded success | Passed; includes the cap-wiring corrective, strict Clippy and workspace tests |
| Local quick, Clippy and default workspace tests | Earlier baseline: ARM64 quick/strict Clippy and default nextest (16,650) passed; harness cap fix: 15 supervisor tests, 31 autonomy-run tests, 18 sustained tests passed | Focused harness tests passed; scoped workspace suite failed while linking `codegg` for x86_64 because native `libiconv`/`liblzma` symbols were unavailable |
| Local feature-gated library suite | 7,828 passed, 4 ignored, 1 unrelated interactive-terminal clear test failed; isolated rerun of the same test passed | Not a clean full-suite pass; failure was not reproduced in isolation |
| One successful local `verify.sh full` invocation | Earlier wrapper deliberately stopped during feature-gated rebuild when free disk approached the 10 GiB stop threshold | Not passed; no claim made |
| Credential authentication | Each stored account returned successful CodeGG completion; one provider request admitted per account | Both accepted; see `evidence/005-credential-preflight.json` |
| Selected active provider connection | Operator selected newer account `newer stored account`; read-only daemon listing found matching active connection with 26 catalog models | Selected route observed; see `evidence/005-selected-route-preflight.json` |
| 30–60-minute dependent Rust task, recovery, job overlap and continuation | `evidence/005-live-attempt.json`; 10m14s, verifier failed, one recovery cycle, seven job submissions with null status | Failed; not sustained-qualified |
| Final live receipt and resource cleanup | `evidence/005-live-attempt.json`; daemon stop error null, process check clean, lane removed after digest | Cleanup passed; evidence retained without raw transcript |

## 3. Budget and route accounting

Approved one-run route: `opencode_go/muse-spark-1.3-contributor` only. Maximum 300 provider requests across credential qualification and the H005 attempt, maximum 60 minutes for the H005 attempt, cancel below 10 GiB free disk. Provider USD cost may be unreported. Credential qualification admitted two requests, one per account. The case cap was configured as 297 after two credential probes and one conservatively reserved catalog request. The run receipt reported zero admitted requests despite 65,608 tokens, and the lane lacked the cap environment variable. Actual request count is unknown, so no remaining balance is asserted. The sole live attempt authorization is consumed; no further live run is authorized.

Provider qualification observed two stored credential record IDs, `older stored account` and `newer stored account`. Each separately produced a successful completion on the approved model, with one request admitted. Their exact secret values differ; neither values nor hashes are recorded. CodeGG has no per-account use counter or last-used field, and its daemon log mentions are not tied to provider request admissions, so historical frequency cannot be established. The older record's creation date is not usage evidence. The operator chose the newer account explicitly. The active connection listing matched the selected account by its masked credential hint. Since no historical usage counter exists, this selection reflects the operator's preference, not a claim about past usage. One request is reserved for the already-populated connection catalog because the creation-time admission record is unavailable. The configured case budget was reduced to 297 requests. During the sole live attempt, the request admission record was missing while token usage was nonzero; see `evidence/005-live-attempt.json`. Provider cost was unavailable.

## 4. Verification details

- Hosted exact-head CI run [#40632587553](https://github.com/dbowm91/codegg/actions/runs/40632587553): `success` at `26f2d8030d6d84bfb26a25fa006bc486202d08d8`; GitGuardian and the workspace verification job passed.
- Earlier hosted CI run [#40629696300](https://github.com/dbowm91/codegg/actions/runs/40629696300): `success` on the implementation head before evidence-only identifier scrubbing.
- Local strict workspace Clippy: passed.
- Local default workspace nextest: passed, 16,650 tests.
- Local Harness Lab focused suite and `python3 -m unittest dev.harness_lab.tests.test_sustained`: passed; the latter ran 18 tests.
- Local feature-gated library suite: 7,828 passed, 4 ignored, 1 failed in `tui::commands::interactive_terminal::tests::a_refused_clear_reports_itself_and_leaves_the_screen_intact`; isolated rerun passed. This result is reported as a suite failure with a non-reproducing isolated test, not a clean pass.
- Disk was monitored throughout builds. The feature target was isolated in `/tmp`; free space remained 56 GiB after the last run. The prior repository target was cleaned after preserving the candidate binary outside the checkout.

## 5. Registry and handoff

Keep H005 **blocked and unqualified**. The one authorized live attempt has been consumed. Do not start a replacement run under this authorization. The offline request-cap wiring correction passed exact-head hosted CI, but the already completed live attempt preceded that correction and its actual outbound request count remains unknown. Any follow-on live task requires new explicit budget authorization. The present evidence does not establish sustained reliability.

No downstream plan is newly unblocked. PR #211 remains draft and unmerged; merging was not part of this authorization.
