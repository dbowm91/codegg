# Command Surface / Security Review — Post-Closure CI Corrective Addendum

Status: closing

Repository baseline reviewed: `a996e20060a0103a152a80fb62463241c1fd1162`

Predecessor work:

- `plans/subsystems/command-surface-reconciliation-corrective-addendum.md`
- `plans/closure/command-surface-reconciliation-corrective/001-status.md`
- `plans/closure/team-collaboration-post-closure-corrective/003-status.md`

Relevant long-term references:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/003-planning-process.md#7-corrective-passes`

No new ADR is required.

## 1. Corrective trigger

Hosted CI on current `main` fails only in `tests/security_review_receipt.rs`:

- `security_review_show_command_is_registered` expects `Command.dialog == Some(Dialog::SecurityReview)`;
- `security_review_show_without_receipt_warns` expects an empty Security Review dialog to open.

Those expectations predate command-surface reconciliation M001. The accepted M001 implementation deliberately changed `/security-review-show` to `CommandAction::Builtin(BuiltinSlashAction::SecurityReviewShow)` because the old generic dialog metadata path was a documented no-op. The current production handler opens the dialog only when a receipt exists and otherwise emits a bounded warning. `security_review_show_with_receipt_opens_dialog` passes.

The post-closure M003 verification pass surfaced the mismatch because routine CI now runs the whole workspace suite. This is test-contract drift, not evidence that production should revert to the old dialog action.

## 2. Scope

One corrective milestone, C001:

- reconcile the stale security-review command tests with the accepted typed-action contract;
- add a regression that asserts no-receipt behavior without relying on obsolete dialog metadata;
- run the full workspace suite and hosted CI evidence before closure.

No production change is expected. If implementation discovers that the current built-in handler no longer matches the accepted M001 closure behavior, stop and treat that as a production regression rather than editing tests around it.

## 3. Non-goals

- Reopening command action convergence.
- Converting `/security-review-show` back to `CommandAction::Dialog`.
- Redesigning Security Review persistence, panels, or background execution.
- Adding new CI lanes or weakening the workspace test job.
- Changing security-review content or LSP enrichment.

## 4. Milestone

### C001 — Security Review command test-contract and CI closure

Status: closing; hosted CI closure evidence pending.

Plan: `plans/implementation/command-surface-security-review-ci-post-closure-corrective/001-security-review-show-contract-and-ci-closure.md`.

## 5. Completion definition

The workstream closes only when:

- `/security-review-show` remains `Builtin(SecurityReviewShow)`;
- registry metadata tests assert typed action, not obsolete `dialog` metadata;
- with a receipt, the command opens the Security Review dialog;
- without a receipt, the command does not mount a stale/empty dialog and emits the intended warning;
- `cargo test --test security_review_receipt` is green;
- `cargo test --workspace --locked -- --test-threads=1` is green;
- canonical quick verification is green; and
- hosted `CI / verify` on the closure revision is green, or any unrelated failure is recorded as a new corrective before closing this one.

## 6. Milestone status

| Milestone | Status | Dependencies |
|---|---|---|
| C001 | implemented | command-surface M001 closed; typed-action tests and local verification updated; hosted CI closure evidence pending |
