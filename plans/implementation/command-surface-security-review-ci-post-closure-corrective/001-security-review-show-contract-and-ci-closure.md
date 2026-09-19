# Command Surface / Security Review Post-Closure Corrective C001 — Security Review Show Contract and CI Closure

Status: conditionally closed — local implementation complete; hosted workspace-test closure is transferred to a separate corrective.

Repository baseline: `a996e20060a0103a152a80fb62463241c1fd1162`

Source roadmap: `plans/subsystems/command-surface-security-review-ci-post-closure-corrective-addendum.md`

Original corrective source:

- `plans/implementation/command-surface-reconciliation-corrective/001-tui-command-action-convergence.md`
- `plans/closure/command-surface-reconciliation-corrective/001-status.md`

Discovery source:

- `plans/closure/team-collaboration-post-closure-corrective/003-status.md`
- hosted CI run 1856 on `a996e200`

Primary class: corrective / verification closure.

## 1. Objective

Restore green canonical CI by reconciling two stale `security_review_receipt` assertions with the already accepted `Builtin(SecurityReviewShow)` command contract. Preserve current production behavior unless direct inspection proves it has regressed from the M001 closure.

## 2. Current evidence

Production:

- `src/tui/command.rs` registers `/security-review-show` as `CommandAction::Builtin(BuiltinSlashAction::SecurityReviewShow)`, so `Command::new` intentionally leaves `cmd.dialog == None`.
- `src/tui/app/mod.rs` dispatches `BuiltinSlashAction::SecurityReviewShow`:
  - receipt present -> `open_dialog(Dialog::SecurityReview)`;
  - no receipt -> warning toast and no dialog.
- `security_review_show_with_receipt_opens_dialog` passes.

Stale tests:

- `security_review_show_command_is_registered` still asserts `cmd.dialog == Some(Dialog::SecurityReview)`.
- `security_review_show_without_receipt_warns` still documents and asserts the pre-M001 empty-dialog behavior.
- CI run 1856 passes every guard, formatting, and Clippy step, then fails exactly these two tests in the workspace test step.

Accepted predecessor closure explicitly states that `/security-review-show` moved from dead dialog metadata to a typed Builtin action reaching the guarded handler.

## 3. Invariants

- Typed `CommandAction` is the execution authority.
- `dialog` metadata is derived only for `CommandAction::Dialog` and must not be used as a second dispatch authority.
- No-receipt `/security-review-show` is bounded and non-mutating.
- A stale test must not force production back to a previously removed no-op path.
- Tests must verify user-visible behavior in addition to registry metadata.

## 4. Required changes

### A. Registry assertion

Update the registration test to assert:

- canonical name is `/security-review-show`;
- `cmd.action == CommandAction::Builtin(BuiltinSlashAction::SecurityReviewShow)`;
- `cmd.dialog.is_none()`, because Builtin actions do not derive dialog metadata.

Keep the cancel-command registration assertion and any exhaustive action-coherence tests.

### B. No-receipt behavior

Replace the stale empty-dialog assertion with the accepted behavior:

- submit `/security-review-show` with no latest receipt;
- prove `Dialog::SecurityReview` is not mounted / `security_review_dialog` remains absent;
- prove a warning toast communicates that no result exists and suggests running `/security-review` first;
- prove no background Security Review task starts and no receipt is fabricated.

Prefer asserting a stable semantic substring rather than the entire toast if the test harness exposes toast text.

### C. Receipt-present behavior

Retain and, if useful, strengthen the passing test:

- set a latest receipt;
- submit `/security-review-show`;
- assert the Security Review dialog mounts with the same receipt id;
- assert no rerun/background task starts.

### D. Documentation drift

Only update comments/docs that still describe `/security-review-show` as a dialog action. Do not edit accepted historical closure text except to add a new corrective reference where registry governance requires it.

## 5. Required tests

At minimum:

```bash
cargo test --test security_review_receipt -- --test-threads=1
cargo test -p codegg --lib tui::command::tests -- --test-threads=1
cargo test --test security_review_panel -- --test-threads=1
cargo test --workspace --locked -- --test-threads=1
scripts/verify.sh quick
cargo clippy --workspace --all-targets --locked -- -D warnings
git diff --check
```

Then require hosted `CI / verify` on the implementation/closure revision. Because this corrective exists to restore canonical CI truth, local green evidence alone is insufficient for final closure when hosted CI is available.

## 6. Acceptance criteria

- The two currently failing tests pass for the accepted typed-action semantics.
- No production command-dispatch change is required unless a newly discovered regression is proven.
- Security Review with receipt still opens the panel.
- Security Review without receipt warns and leaves the dialog closed.
- Workspace test suite passes with zero failures.
- Hosted CI `verify` passes.
- No unrelated failure is hidden or marked ignored.

## 7. Stop conditions

Stop and register a broader corrective if:

- `Builtin(SecurityReviewShow)` no longer reaches the production handler;
- no-receipt behavior is inconsistent between direct typed input and palette execution;
- fixing the tests requires changing command registry architecture;
- full CI exposes an unrelated product failure that is not already owned.

## 8. Closure evidence

Create `plans/closure/command-surface-security-review-ci-post-closure-corrective/001-status.md` with:

- baseline CI failure excerpts;
- accepted M001 behavior reference;
- before/after test matrix;
- full workspace result;
- hosted CI run URL/id and result;
- explicit production-delta statement; and
- registry unblock audit.
