# Harness Lab Mainline Qualification H005 — Closure Status

Status: **blocked — implementation and hosted exact-head qualification complete; sustained live qualification not yet measured**

Source implementation plan: `plans/implementation/harness-lab-mainline-qualification/005-genuine-sustained-multi-stage-rust-workload.md`

Source subsystem roadmap: `plans/subsystems/harness-lab-mainline-and-long-horizon-qualification-addendum.md`

Branch: `plans/harness-lab-h005-sustained-workload-20261027`; PR #211 is draft at `b85e0db4891c71f4ac1b7bb175ce5e6d6a0643cb`.

## 1. Finding

H005's multi-crate event-ledger workload, fixture/verifier, autonomy manifest, constrained job projection, request-boundary cap and disk-floor watcher are implemented and pushed. The exact PR head passed hosted CI run [40595507603](https://github.com/dbowm91/codegg/actions/runs/40595507603). The independently authorized live attempt has not started: CodeGG lists `opencode_go/muse-spark-1.3-contributor`, but provider preflight refuses because two stored OpenCode Go credentials are not bound to an active provider connection. No credential was selected, no binding was changed, and no model request was sent. The 300-request / 60-minute authorization remains unused.

This record does not qualify H005 as sustained. The 30-minute minimum, dependent live stages, two recovery sequences, real background-job overlap while coding continues, same-session autonomous continuation, and the sustained receipt remain unmeasured. Completion requires the operator's credential choice, successful read-only route preflight, and the already-approved one-run task. No second attempt is authorized if that attempt begins and fails or ends below threshold.

## 2. Requirement evidence

| Requirement | Evidence | State |
|---|---|---|
| Distinct H005 multi-crate workload and hidden verifier | `dev/harness_lab/fixtures/agent_autonomy_h005/`; event-ledger source spans five shards and 50,000 records with an independent verifier | Implemented; deterministic fixture tests passed in hosted CI |
| Versioned H005 manifest and privacy-bounded job identity | H005 case manifest is isolated from prior autonomy manifests; schema-v3 projection exposes constrained job identity/action/status | Implemented; hosted CI passed |
| Request cap and disk floor | Provider-boundary admission capped at 300; runner fails closed on unreadable/below-10-GiB disk and cancels on breach | Implemented; local focused tests and hosted CI passed; live enforcement unmeasured |
| Exact-head hosted qualification | PR #211 head `b85e0db4891c71f4ac1b7bb175ce5e6d6a0643cb`; run #40595507603 concluded success | Passed |
| Local quick, Clippy and default workspace tests | ARM64 quick and strict Clippy passed; default workspace nextest passed 16,650 tests; native ACP safety/cleanup Python suite passed 18 tests | Passed |
| Local feature-gated library suite | 7,828 passed, 4 ignored, 1 unrelated interactive-terminal clear test failed; isolated rerun of the same test passed | Not a clean full-suite pass; failure was not reproduced in isolation |
| One successful local `verify.sh full` invocation | Earlier wrapper deliberately stopped during feature-gated rebuild when free disk approached the 10 GiB stop threshold | Not passed; no claim made |
| Selected provider connection and first live request | Two stored `opencode_go` credentials are unbound; no user selection received | Blocked before request; authorization unused |
| 30–60-minute dependent Rust task, recovery, job overlap and continuation | No live task run | Unmeasured |
| Final live receipt and resource cleanup | No live lane or daemon was started | Not applicable yet |

## 3. Budget and route accounting

Approved one-run route: `opencode_go/muse-spark-1.3-contributor` only. Maximum 300 provider requests, maximum 60 minutes, cancel below 10 GiB free disk. Provider USD cost may be unreported. Current consumption is 0 requests and 0 seconds of live task runtime; no run resources were created. The approval remains unused and applies to one attempt only.

Provider preflight observed two stored credential record IDs, `d04e5c1e-8b22-4691-b9e9-7d44dda3b7be` and `6d24402a-e00b-48bb-b21a-3b72152a429b`. Their secrets are not recorded here. CodeGG's provider-auth rules prohibit choosing between unbound credentials by order or recency; binding requires the operator to identify the intended record.

## 4. Verification details

- Hosted exact-head CI run #40595507603: `success` at `b85e0db4891c71f4ac1b7bb175ce5e6d6a0643cb`.
- Local strict workspace Clippy: passed.
- Local default workspace nextest: passed, 16,650 tests.
- Local Harness Lab focused suite and `python3 -m unittest dev.harness_lab.tests.test_sustained`: passed; the latter ran 18 tests.
- Local feature-gated library suite: 7,828 passed, 4 ignored, 1 failed in `tui::commands::interactive_terminal::tests::a_refused_clear_reports_itself_and_leaves_the_screen_intact`; isolated rerun passed. This result is reported as a suite failure with a non-reproducing isolated test, not a clean pass.
- Disk was monitored throughout builds. The feature target was isolated in `/tmp`; free space remained 56 GiB after the last run. The prior repository target was cleaned after preserving the candidate binary outside the checkout.

## 5. Registry and handoff

Keep H005 **blocked** pending the named provider connection prerequisite and live evidence. The implementation is ready for the single approved attempt after the operator identifies which stored credential should be bound and the read-only route check passes. Do not mark this milestone closed or claim sustained reliability until the requirement matrix above is satisfied with the live receipt and cleanup proof.

No downstream plan is newly unblocked. PR #211 remains draft and unmerged; merging was not part of this authorization.
