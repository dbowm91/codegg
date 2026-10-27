# Harness Lab Mainline Qualification H001 — Closure Status

Status: **blocked** (integration PR is open; ordinary review, merge, and exact resulting-main verification remain)
Source implementation plan: `plans/implementation/harness-lab-mainline-qualification/001-mainline-integration-and-exact-head-verification.md`
Source subsystem roadmap: `plans/subsystems/harness-lab-mainline-and-long-horizon-qualification-addendum.md`
Repository baseline reviewed: `origin/main` `09dc761c7663316066e0f7f427dab89e71600e98`
Implementation candidate: `7b2e51b5a2547058a319f097743812a9a1e4978f` on `plans/harness-lab-integration-20261027`
Integration PR: [#205](https://github.com/dbowm91/codegg/pull/205)

## 1. Executive finding

The continuation was rebased onto the current main tip, reconciled with the newer mainline behavior, and published as an ordinary review PR. Local full verification passed on candidate `7b2e51b`. H001 is not closed: hosted candidate checks are running, no review decision or merge exists, and the resulting main SHA has not been qualified. H004 must wait for those gates.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | State |
|---|---|---|
| Reconcile continuation on current main | Candidate merge base is `09dc761c7663316066e0f7f427dab89e71600e98`; 90-file integration diff reviewed and rebase conflicts reconciled | Complete on candidate |
| Preserve current main behavior | Restored compact `repo_search` array shape after exact failing test identified a regression; retained stricter tool identifier collision behavior and current schemas | Locally verified |
| Reviewable conventional integration | [PR #205](https://github.com/dbowm91/codegg/pull/205), open against `main`; no auto-merge | Pending review |
| Exact candidate hosted CI | CI run [40456207603](https://github.com/dbowm91/codegg/actions/runs/40456207603); Desktop E2E [40456207767](https://github.com/dbowm91/codegg/actions/runs/40456207767); platform workflows are attached to the PR | In progress at record time |
| Merge and resulting-main SHA verification | No merge SHA exists yet | Blocked on review and merge |
| H004 live qualification | Fresh authorization received for one `opencode_go` Muse Spark 1.3 Contributor session, capped at 60 requests / 60 minutes; no request has been made | Blocked on H001 merge and resulting-main CI |

## 3. Production implementation evidence

The candidate contains the continuation reconciliation, H002 versioned recovery controls, H003 Rust/job fixtures, ACP typed terminal and event ordering correction, CI/SQLite corrections, and associated regression coverage and documentation. The separate integration branch preserves the historical continuation branch. No live-provider code path was exercised during this integration.

## 4. Verification executed

Local command: `ZLIB_NO_PKG_CONFIG=1 LIBLZMA_NO_PKG_CONFIG=1 scripts/verify.sh full`

Passed on candidate `7b2e51b5a2547058a319f097743812a9a1e4978f`:

- `verify.sh quick`: formatting, generated assets, all 82 guards, Harness Lab Python suite (354 passed, 1 skipped), and workspace check.
- Strict workspace Clippy with `-D warnings`.
- Workspace nextest: 16,549 passed, 9 skipped.
- Feature-gated `server,plugins,lsp-test-support` suite: 7,925 passed, 4 skipped.

These are local results. Hosted CI has not completed, and no exact resulting-main verification is claimed.

## 5. Invariant review

The reconciliation retained typed ACP tool failures, ordered tool lifecycle publication, unmeasured event gaps when evidence is unavailable, existing denial/path boundaries, the current compact repository-search wire shape, and the existing strict lint gate. H002 v1 fixtures and receipts remain immutable; v2 controls are additive.

## 6. Failure and recovery review

An initial full verification exposed a compact repository-search shape regression. The implementation was corrected to preserve the mainline array shape and the exact failing test passed; the full local verification then passed. The submitted PR has no hosted result yet.

## 7. Migration and compatibility review

No storage migration or ACP protocol version change is introduced by this integration. H002 uses versioned controls while preserving v1 fixtures and manifest version. H003 fixtures run in private lanes.

## 8. Security review

Local guards and tests passed. No provider credentials or live request receipts were added. PR review and hosted security/platform workflows remain outstanding.

## 9. Documentation and operations

H001–H003 status and evidence are recorded in the registry, roadmap, and closure records. Disk had approximately 107 GiB free after local verification. H004's approved cap is 60 requests and 60 minutes, with the runner required to stop below 10 GiB free disk.

## 10. Unresolved findings

- **Medium / external gate:** PR #205 has no review decision, merge, or exact resulting-main CI yet. Required next evidence is an ordinary review and merge followed by successful required checks on the resulting `main` SHA.
- **Operational gate:** H004 has fresh operator authorization but remains unrun until the H001 gate above passes.

## 11. Roadmap disposition

H001 remains blocked at the review/merge gate. H002 and H003 remain closed at their recorded scope. H004 remains blocked on H001.

## 12. Registry updates

The registry points to PR #205, candidate SHA `7b2e51b`, pending hosted checks, and the H004 authorization state. It does not claim H001 landed or H004 completed.
