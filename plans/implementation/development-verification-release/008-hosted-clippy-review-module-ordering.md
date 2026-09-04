# Development Verification and Release Milestone 008 — Hosted Clippy Review Module Ordering

Status: implemented — closed; see `plans/closure/development-verification-release/008-status.md`

Repository baseline: `8c8febedb1d941e1a90a2ff3d02264e3127441fb`

Source subsystem roadmap:

- `plans/subsystems/development-verification-release-final-evidence-closure-addendum.md#milestone-008--hosted-clippy-review-module-ordering`

Long-term requirements:

- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/003-planning-process.md#7-corrective-passes`

Applicable ADRs:

- None required; this is a local test-module ordering correction.

Primary class: polish / verification

## 1. Objective

Move the existing `ReviewTool` test module after the production items in
`src/tool/review.rs` so the current hosted stable Clippy gate can complete
without changing runtime behavior or suppressing the lint.

## 2. Why this milestone is ready

Hosted CI run `33838526507` on exact candidate `8c8febed` passed formatting and
all static guards, then failed Workspace Clippy on the pre-existing test-module
ordering in `src/tool/review.rs`. The finding is independent of the
memory-to-skill M005 production changes and has no architecture dependency.

## 3. Scope

In scope:

- relocating the existing `#[cfg(test)] mod tests` block to the end of the
  module;
- focused ReviewTool tests and the normal hosted verification lane.

Out of scope:

- ReviewTool behavior, provider context, API, tests, or production logic;
- Clippy policy, CI configuration, lint suppressions, or toolchain changes.

## 4. Required verification

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --test habit_skill_promotion --test skill_publication --locked
scripts/verify.sh quick
```

The closure record must cite the hosted run on the exact accepted SHA and
record the downstream dependency audit.

## 5. Acceptance criteria

- Workspace Clippy no longer reports `items_after_test_module`.
- ReviewTool tests remain unchanged in behavior and pass.
- No lint suppression, CI weakening, or production behavior change is made.
- The normal hosted `CI / verify` lane reaches and passes Workspace tests.

## 6. Stop conditions

Stop and report if correcting the warning requires changing ReviewTool behavior,
test semantics, lint policy, CI, or another subsystem boundary.

## 7. Closure evidence required

Create `plans/closure/development-verification-release/008-status.md` with the
exact implementation commit, focused/local verification, hosted run/job,
invariant and compatibility review, and registry/dependency audit.
