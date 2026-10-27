# Harness Lab Mainline Qualification H001 — Closure Status

Status: **closed**
Source implementation plan: `plans/implementation/harness-lab-mainline-qualification/001-mainline-integration-and-exact-head-verification.md`
Source subsystem roadmap: `plans/subsystems/harness-lab-mainline-and-long-horizon-qualification-addendum.md`
Repository baseline reviewed: `09dc761c7663316066e0f7f427dab89e71600e98`
Implementation candidate: `7b2e51b5a2547058a319f097743812a9a1e4978f` on `plans/harness-lab-integration-20261027`
Integration PR: [#205](https://github.com/dbowm91/codegg/pull/205)
Merge commit and exact resulting-main head: `9185b9c34ce30fd52a41aacd0aaae75f39e54895` (2026-10-27)

## 1. Executive finding

PR #205 merged to `main`. The exact merge SHA passed `CI / verify` on attempt 3 and every applicable platform workflow. H001 is closed. Two earlier exact-head attempts failed in unrelated hosted checks; the final successful attempt is the acceptance evidence.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | State |
|---|---|---|
| Reconcile the continuation on current main | Candidate was based on `09dc761c7663316066e0f7f427dab89e71600e98`; the integration diff reconciled both branches' ownership and tests | Complete |
| Preserve current main behavior | Compact `repo_search` array shape restored after its exact failing test identified a regression; tool identifier collision behavior and schemas retained | Complete |
| Reviewable integration | [PR #205](https://github.com/dbowm91/codegg/pull/205), merged 2026-10-27 | Complete |
| Exact resulting-main CI | [CI run 40509488918, attempt 3](https://github.com/dbowm91/codegg/actions/runs/40509488918/attempts/3), head `9185b9c34ce30fd52a41aacd0aaae75f39e54895`, conclusion `success` | Passed |
| Applicable platform gates | [Desktop E2E 40509488651](https://github.com/dbowm91/codegg/actions/runs/40509488651), [macOS shared-checkout 40509488936](https://github.com/dbowm91/codegg/actions/runs/40509488936), [Windows native probe 40509488664](https://github.com/dbowm91/codegg/actions/runs/40509488664), [Linux M004 40509488996](https://github.com/dbowm91/codegg/actions/runs/40509488996), [Windows process launcher 40509488658](https://github.com/dbowm91/codegg/actions/runs/40509488658) | Passed on the same SHA |
| Exact main merge verification | Merge SHA above is the head for every listed workflow | Complete |

## 3. Production implementation evidence

The merged integration contains the continuation reconciliation, H002 versioned recovery controls, H003 Rust/job fixtures, ACP typed terminal and event-ordering corrections, CI/SQLite corrections, and associated regression coverage and documentation. The historical continuation branch remains available for traceability. H001 itself did not exercise a live-provider code path.

## 4. Verification executed

Hosted verification is the exact resulting-main evidence above. On the integration candidate, local `scripts/verify.sh full` passed with formatting, generated assets, the guard suite, Harness Lab Python tests, workspace check, strict workspace Clippy, 16,549 workspace tests (9 skipped), and 7,925 feature-gated tests (4 skipped). The exact merge SHA's required hosted checks independently passed afterward.

Exact resulting-main CI attempt 1 failed the hosted Eggwork remote-execution test because its helper remained alive but never became ready. Attempt 2 failed the M005 holdout timing threshold at 5.280 ms against 5 ms; the focused test passed locally. No unrelated threshold or code was changed to mask either result. Attempt 3 passed.

## 5. Invariant review

The integration retained typed ACP tool failures, ordered tool lifecycle publication, unmeasured event gaps when evidence is unavailable, existing denial and path boundaries, the compact repository-search wire shape, and strict lint gates. H002 v1 fixtures and receipts remain immutable; v2 controls are additive.

## 6. Failure and recovery review

The candidate's initial full verification exposed a `repo_search` shape regression. The implementation restored the mainline array contract and the exact failing test passed before merge. Hosted failures on attempts 1 and 2 were investigated as independent readiness/timing failures; the successful exact-head attempt followed without changing the unrelated microbenchmark threshold.

## 7. Migration and compatibility review

No storage migration or ACP protocol version change was introduced. H002 preserves v1 fixtures and manifest version. H003 fixtures use private lanes. The exact main workflows passed on the merge SHA.

## 8. Security review

The local guards and hosted required gates passed. No provider credential or live request receipt was included in H001. H004's separately authorized live measurement and request evidence are recorded in `004-status.md`.

## 9. Documentation and operations

H001–H003 implementation and closure records are registered. PR #205's merge commit is `9185b9c34ce30fd52a41aacd0aaae75f39e54895`. `origin/main` later advanced to `80171f45295bebb9ab0b521262c0b295acb93074` with a documentation-only PR #206; that later head does not change the SHA qualification recorded here.

## 10. Unresolved findings

No H001 acceptance requirement remains unresolved. H004 is conditionally closed at its own recorded scope because its one live run did not meet the planned 30–60 minute duration; this does not reopen H001.

## 11. Roadmap disposition

H001 is closed. H002 and H003 remain closed at their recorded scopes. H004's live measurement is recorded separately in `plans/closure/harness-lab-mainline-qualification/004-status.md`.

## 12. Registry updates

`plans/registry.md` records PR #205 as merged, the exact merge SHA and green hosted gates. The subsystem roadmap records H001 closed and H004 conditionally closed with its named sustained-horizon limitation.
