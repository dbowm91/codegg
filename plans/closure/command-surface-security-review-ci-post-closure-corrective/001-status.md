# Command Surface / Security Review Post-Closure Corrective C001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/command-surface-security-review-ci-post-closure-corrective/001-security-review-show-contract-and-ci-closure.md`

Source subsystem roadmap:

- `plans/subsystems/command-surface-security-review-ci-post-closure-corrective-addendum.md`

Repository baseline reviewed: `c9c37cca`

Implementation commits:

- `4ec46a9b` — test: align security review show contract
- `5c4e2966` — test: reconcile migration version closure (separate hosted-CI corrective)
- `c9c37cca` — fix: publish managed keys atomically (separate hosted-CI corrective and closure revision)

## 1. Executive finding

C001's production contract is intact and its stale test contract is corrected.
`/security-review-show` remains the typed
`BuiltinSlashAction::SecurityReviewShow` action established by command-surface
reconciliation M001. With a receipt it opens the existing Security Review
panel; without a receipt it leaves the panel closed, starts no task, fabricates
no receipt, and emits the bounded instruction to run `/security-review` first.
No Security Review production code changed in this corrective.

The initial hosted attempts exposed unrelated stale migration assertions and a
real managed-key publication race. Those findings were corrected under their
own plans, and hosted [CI / verify run 35483642396](https://github.com/dbowm91/codegg/actions/runs/35483642396)
passed every guard, formatting, Clippy, workspace test, and cleanup step on
the closure revision. The Security Review corrective is therefore formally
closed without reopening or changing its production command path.

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
| Full workspace suite | `cargo test --workspace --locked -- --test-threads=1` | pass | 11,540 passed, 3 ignored locally; no Security Review, WorkOrder, migration-fixture, or managed-key failures |
| Hosted canonical CI | [run 35483642396](https://github.com/dbowm91/codegg/actions/runs/35483642396) | pass | Guards, formatting, Clippy, workspace tests, post-job cleanup, and job completion all passed on `c9c37cca` |

## 3. Production implementation evidence

No Security Review production implementation changed. Direct inspection
confirms the accepted path remains:

- `src/tui/command.rs` registers `/security-review-show` as
  `CommandAction::Builtin(BuiltinSlashAction::SecurityReviewShow)`, deriving
  no `dialog` metadata.
- `src/tui/app/mod.rs` opens `Dialog::SecurityReview` only when
  `latest_security_review` is present; otherwise it emits the prescribed
  warning.
- `src/tui/components/dialogs/security_review.rs` continues to receive the
  same receipt instance when the panel is opened.

The Security Review change is limited to
`tests/security_review_receipt.rs` and planning/closure records. The separate
managed-key and migration-test fixes are recorded in their own closure files.

## 4. Verification executed

The focused Security Review receipt, panel, and command suites passed with 36,
23, and 18 tests respectively. `scripts/verify.sh quick`, workspace Clippy,
the Git static guard, and `git diff --check` passed. The full local workspace
suite passed 11,540 tests with 3 ignored. Hosted run 35483642396 completed all
guards, formatting, Clippy, workspace tests, cleanup, and job completion in
34m26s.

## 5. Invariant, migration, and security review

- Typed `CommandAction` remains the sole execution authority.
- `dialog` metadata remains derived only from `CommandAction::Dialog`.
- No-receipt `/security-review-show` remains non-mutating and bounded.
- Receipt-present show remains a reopen-only operation and does not start a
  background review.
- There is no Security Review schema, protocol, configuration, or migration
  change.
- The corrective does not widen execution authority or introduce new input,
  secret, filesystem, or network handling.
- Historical M001 closure text was not rewritten.

## 6. Unresolved findings

None. The WorkOrder assertion, managed-key publication race, and stale
layout-62 migration fixtures were resolved by separately scoped corrective
plans and verified by the green hosted closure run.

## 7. Roadmap disposition

C001 implementation is complete and formally closed. The separately
registered WorkOrder, managed-key concurrency, and migration-test correctives
are also closed on the same hosted evidence. The registry unblock audit found
no registered plan blocked on this Security Review correction. Identity/audit
M001 remains independently ready, while its M002, M003, and M004 dependencies
remain unchanged.

## 8. Registry updates

The implementation plan, subsystem roadmap, registry, and closure record now
mark C001 `closed`. No future plan is unblocked by this test-only correction;
Identity/audit M001 remains ready and its dependent milestones remain blocked
as before.
