# Command Surface / Security Review Post-Closure Corrective C001 — Closure Status

Status: closing

Source implementation plan:

- `plans/implementation/command-surface-security-review-ci-post-closure-corrective/001-security-review-show-contract-and-ci-closure.md`

Source subsystem roadmap:

- `plans/subsystems/command-surface-security-review-ci-post-closure-corrective-addendum.md`

Repository baseline reviewed: `db90730e`

Implementation commits or pull requests:

- Pending commit for the test-contract correction and planning closure evidence.

## 1. Executive finding

C001's production contract is intact and its stale test contract is corrected.
`/security-review-show` remains the typed
`BuiltinSlashAction::SecurityReviewShow` action established by command-surface
reconciliation M001. With a receipt it opens the existing Security Review
panel; without a receipt it leaves the panel closed, starts no task, fabricates
no receipt, and emits the bounded instruction to run `/security-review` first.
No production code changed in this corrective.

The first hosted `CI / verify` result for the pushed closure candidate was
green through all guards, formatting, and Clippy, then failed in an unrelated
WorkOrder remigration assertion (`tests/work_orders_m001_foundation.rs`:
expected 63, current canonical layout 65). That finding is now owned by the
narrow follow-up `plans/implementation/project-work-orders-task-view-ci-corrective/001-migration-version-test-contract.md`;
the corrected candidate is the remaining strict-closure item.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Registry uses typed Security Review show action | `tests/security_review_receipt.rs::security_review_show_command_is_registered` asserts canonical name, `Builtin(SecurityReviewShow)`, and `dialog.is_none()` | pass | Matches accepted M001 closure; no dialog metadata authority is reintroduced |
| No-receipt show behavior is bounded | `security_review_show_without_receipt_warns` proves no dialog, no running task, no receipt, and warning text containing `No security review result available yet` and `/security-review` | pass | User-visible behavior is asserted semantically |
| Receipt-present show behavior remains intact | `security_review_show_with_receipt_opens_dialog` proves Security Review dialog and receipt `sr-test-1`, with no running task | pass | No rerun is triggered |
| Focused Security Review tests | `cargo test --test security_review_receipt -- --test-threads=1`; `cargo test --test security_review_panel -- --test-threads=1` | pass | 36 + 23 tests |
| Command registry unit tests | `cargo test -p codegg --lib tui::command::tests -- --test-threads=1` | pass | 18 passed, 4,763 filtered |
| Canonical quick verification | `scripts/verify.sh quick` | pass | All guards and workspace check passed |
| Clippy and Git policy guard | `cargo clippy --workspace --all-targets --locked -- -D warnings`; `python3 scripts/check_git_forbidden_patterns.py` | pass | No warnings/findings |
| Full workspace suite | `cargo test --workspace --locked -- --test-threads=1` | partial | 5,909 tests passed before the completed run reported 9 unrelated pre-existing failures: one MCP local fixture timeout and eight Python AST assertions. Exact MCP test and all 39 Python analyzer tests pass in isolation; no C001 test failed. |
| Hosted canonical CI | Run [35467200089](https://github.com/dbowm91/codegg/actions/runs/35467200089) | corrective pass required | Guards, formatting, and Clippy passed; workspace tests exposed the unrelated stale v63 WorkOrder assertion. A separate follow-up owns that correction and a new hosted run is required. |

## 3. Production implementation evidence

No production implementation changed. Direct inspection confirms the accepted
path remains:

- `src/tui/command.rs` registers `/security-review-show` as
  `CommandAction::Builtin(BuiltinSlashAction::SecurityReviewShow)`, deriving
  no `dialog` metadata.
- `src/tui/app/mod.rs` opens `Dialog::SecurityReview` only when
  `latest_security_review` is present; otherwise it emits the prescribed
  warning.
- `src/tui/components/dialogs/security_review.rs` continues to receive the
  same receipt instance when the panel is opened.

The change is limited to `tests/security_review_receipt.rs` and planning/
closure records.

## 4. Verification executed

### Commands run

```bash
git pull --rebase
cargo test --test security_review_receipt -- --test-threads=1
cargo test --test security_review_panel -- --test-threads=1
cargo test -p codegg --lib tui::command::tests -- --test-threads=1
cargo test -p codegg --lib mcp::local::tests::noisy_stderr_does_not_block_initialize -- --test-threads=1
cargo test -p codegg --lib python_script::analyze -- --test-threads=1
python3 scripts/check_git_forbidden_patterns.py
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked -- --test-threads=1
git diff --check
```

### Results

- Rebase completed from `origin/main` at `db90730e`.
- Focused Security Review receipt/panel/command suites passed: 36, 23, and
  18 tests respectively.
- The exact MCP noisy-stderr test passed in isolation; the Python analyzer
  suite passed 39 tests in isolation.
- `scripts/verify.sh quick`, Clippy, the Git static guard, and `git diff
  --check` passed.
- The first hosted workspace run completed its guards, formatting, and Clippy
  successfully but failed only at the unrelated WorkOrder remigration test;
  run 35467200089 is recorded above. A focused local rerun reproduced that
  failure as 11 passed and 1 failed, confirming the separate stale assertion.

## 5. Invariant review

- Typed `CommandAction` remains the sole execution authority.
- `dialog` metadata remains derived only from `CommandAction::Dialog`.
- No-receipt `/security-review-show` remains non-mutating and bounded.
- Receipt-present show remains a reopen-only operation and does not start a
  background review.
- Historical M001 closure text was not rewritten.

## 6. Failure and recovery review

The no-receipt test covers the relevant failure path: the command clears
command mode, emits a warning, leaves the dialog state unmounted, and does not
create a running task or receipt. The receipt-present path is read-only and
does not rerun the review. No new persistence, cancellation, restart, or
contention behavior was introduced.

## 7. Migration and compatibility review

There is no schema, protocol, configuration, or migration change. Existing
typed command registrations and receipt data remain compatible.

## 8. Security review

The corrective preserves the existing non-mutating show path, does not widen
execution authority, and does not introduce new input, secret, filesystem, or
network handling.

## 9. Documentation and operations

The stale test comment was corrected to describe the typed-action/no-receipt
contract. The implementation plan, subsystem roadmap, registry, and this
closure record now identify the corrective and its evidence. No architecture
document required a semantic update.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Hosted CI exposed a stale WorkOrder remigration assertion expecting layout 63 while the canonical layout is 65. | Unrelated to C001; it prevented the first full hosted gate from passing. | Owned by `plans/implementation/project-work-orders-task-view-ci-corrective/001-migration-version-test-contract.md`; do not change Security Review production code. |

## 11. Roadmap disposition

C001 implementation is complete. Strict closure is pending on the hosted
`CI / verify` result for the corrected candidate after the separate WorkOrder
test-contract follow-up. The registry unblock audit found no registered plan
blocked on this Security Review correction; Identity/audit M001 remains
independently ready, while its M002, M003, and M004 dependencies remain
unchanged.

## 12. Registry updates

The implementation plan is marked `implemented`, the subsystem roadmap and
registry move C001 to `closing`, and the closure record is discoverable under
the required path. The unrelated hosted failure is registered as a separate
WorkOrder CI corrective; after the corrected hosted run passes, this record,
the roadmap, registry, recently-closed table, and post-closure cleanup gate
will be changed together to `closed`. No future plan is unblocked by this
test-only correction.
