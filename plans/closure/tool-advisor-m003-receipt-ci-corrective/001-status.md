# Tool-Advisor M003 Receipt CI Corrective C001 — Closure Status

Status: **closed**

Source implementation plan: `plans/implementation/tool-advisor-m003-receipt-ci-corrective/001-receipt-identity-reproduction-and-hosted-gate-restoration.md`

Source subsystem roadmap: `plans/subsystems/tool-advisor-m003-receipt-ci-corrective-addendum.md`

Repository baseline reviewed: async PR #119 head `247edec5f37851ca3f67db5943675c896d19e9d9`; `main` before repair `90ab44775a36ebe30466a0d6e340f584324de114`.

Implementation commits:

- `7d56935340710268bb533f5d9e3a3ae84fecc723` — reconstruct the historical test-tool schema identity for frozen M003 receipt verification and pin fail-closed fallback behavior.
- `9de8a5452dbf4070b15aa78801bb0e703eec2447` — merged repair PR #121 into `main`.
- `5540644` — initial focused repair attempt on the PR #119 qualification branch. During C007's merge of latest main, the `causal_frontier.rs` conflict was resolved with main's more complete version of the same historical-schema repair (including compatibility with schemas that contain neither or both approved fields). The final integrated implementation therefore comes from `9de8a545`, not the superseded file contents of `5540644`.

## 1. Executive finding

C001 is closed. The red test compared a frozen M003 effect-catalog fingerprint against the current live tool registry after the `test` tool schema had legitimately gained `execution_mode` and `on_complete`. The receipt is historical evidence bound to the schema present when it was qualified. The repair reconstructs that historical schema for receipt recomputation while preserving the current live fingerprint, so genuine runtime catalog drift continues to fall back to M002.

The checked-in M003 result asset was not rewritten. Its disposition remains negative **D**, M002 remains the selected runtime frontier, the regression remains in the default hosted suite, and no CI skip or waiver was introduced. The repair passed all 13,637 workspace tests on its exact hosted head and merged to main through PR #121.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Reproduce and classify the hosted failure | PR #119 workflow `38650792114`, job `116148778906`, head `247edec5` | Pass | Stored fingerprint `94ef4d...`; fresh fingerprint `a9c5af...`; 4,267 passed, this test failed and 9,522 did not run. |
| Identify the semantic identity drift | `m003_checked_in_receipt_matches_recomputation` in `src/tool_advisor/causal_frontier.rs` | Pass | Reconstructs the historical `test` schema by removing only `execution_mode` and `on_complete`. |
| Preserve live fail-closed behavior | Same regression test | Pass | Current fingerprint differs; `StaleCatalogFallback` returns the same frontier as direct M002 evaluation. |
| Preserve M003 disposition and receipt | Frozen-receipt regression and unchanged result asset | Pass | Disposition remains D; M002 remains selected. The asset was not modified. |
| Targeted repair-branch verification | Receipt regression; causal-frontier suite | Pass | Receipt test passed; suite: 78 passed, 0 failed, 2 ignored; formatting and `git diff --check` passed. |
| Hosted exact-head root CI | [Run 38676411915](https://github.com/dbowm91/codegg/actions/runs/38676411915), head `7d569353` | Pass | `CI / verify` and GitGuardian succeeded. Nextest ran 13,637 tests: 13,637 passed, 0 failed, 5 configured skips. Repaired test passed at 4,251/13,637. |
| Integration into current PR candidate | Merge commit `5b9f761`; final `causal_frontier.rs` matches main's repair | Pass for dependency; C007 active | The initial `5540644` patch remains in history, but its file contents were superseded by the mainline conflict resolution. C007 must qualify the final integrated SHA. |

## 3. Production implementation evidence

`effect_catalog_fingerprint_with_schema_overrides` deterministically reconstructs the historical bound input schema when validating the immutable receipt. The normal live fingerprint still uses the current registry. The regression matches the receipt with the historical schema, proves current schema drift remains visible, and verifies the stale identity fails closed to M002.

No model-selection policy, causal contract, receipt asset, or runtime M002/M003 selection was changed. The historical M003 closure remains unchanged.

## 4. Verification executed

```bash
cargo fmt --all -- --check
cargo test -p codegg --lib tool_advisor::causal_frontier::tests::m003_checked_in_receipt_matches_recomputation -- --exact --nocapture
cargo test -p codegg --lib tool_advisor::causal_frontier -- --nocapture
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
```

- Targeted receipt regression passed on the repair branch.
- Causal-frontier suite: **78 passed, 0 failed, 2 ignored**.
- Hosted `CI / verify`: run `38676411915`, job `116230927235`, commit `7d56935340710268bb533f5d9e3a3ae84fecc723`. Formatting, Clippy, guards and all 13,637 executed tests passed; 5 tests were skipped as configured by nextest. GitGuardian passed.
- PR #121 merged at `9de8a5452dbf4070b15aa78801bb0e703eec2447` on 2026-10-15 03:55:04 UTC.
- The original red run's 9,522 unexecuted tests were exercised by the later complete hosted run.

## 5. Invariant review

- M003 negative disposition D, frozen receipt and benchmark provenance are preserved.
- Current live effect-catalog identity remains schema-bound; current drift is observable and fails closed.
- M002 is still the only selected runtime frontier.
- No test skip, exclusion, `continue-on-error`, or CI policy weakening was added.

## 6. Failure and recovery review

This corrective changes historical identity reconstruction in the receipt regression; runtime persistence, retry, provider calls and admission are unchanged. The fallback assertion compares stale identity behavior directly with M002 evaluation. Default hosted nextest completed on the exact repair head, closing the earlier fail-fast test coverage gap.

## 7. Migration and compatibility review

No schema or protocol migration is introduced. Only the historical receipt test supplies the previous `test` input schema; the live catalog remains current. The checked-in result JSON is unchanged.

## 8. Security review

No authority, credential, sandbox, network, or model-selection privilege changes were made. Fingerprint inputs are schema data; assertions disclose no secrets. M003 continues to abstain from its unsafe negative frontier.

## 9. Documentation and operations

- The corrective regression now names the two post-M003 schema fields and pins stale-catalog behavior.
- The historical M003 closure and frozen result asset remain intact.
- Repair PR #121 merged to main. C007 merged latest main and retained its more complete historical-schema regression implementation; the exact integrated head remains subject to C007 qualification.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| Low | PR #119 has not yet completed exact-head hosted CI and Desktop E2E for the final integrated feature SHA. | Green repair CI cannot substitute for final async integration evidence. | Complete under C007; keep PR #119 draft until gates finish. |

No C001 high or medium finding remains.

## 11. Roadmap disposition

C001 is closed. The independent blocker on async C007 is resolved: C006 has closed, and the receipt repair merged to main with a green full hosted CI. The initial repair commit remains in PR #119 history; the final implementation comes from the already-green mainline repair. C007 is now eligible and has completed latest-main reconciliation, with final hosted qualification in progress.

## 12. Registry updates

- Mark C001 implemented/closed and record this closure under recently closed work.
- Remove C001 from active and dependency-ready sections.
- Unblock async C007 in the same commit; it transitions directly to active as WP-A begins. Exact-head CI, Desktop E2E, migration review, PR metadata and maintainer decision remain outstanding.
- No other registered plan depends on C001.
