# Provider Reasoning Admission C001 — A Preference Never Blocks a Call

Status: closed
Repository baseline: `b554ed7bc`

Source roadmap: `plans/subsystems/provider-reasoning-admission-corrective-addendum.md`
Long-term requirements: `plans/000-long-term-specification.md#29-system-invariants`
Applicable ADRs: `plans/adrs/ADR-0050-per-model-reasoning-capability-authority.md`,
amended by this milestone.
Primary class: invariant

## 1. Objective

A resolved reasoning selection never refuses a turn. An `Unknown` dimension is
forwarded unchanged; a declared `Unsupported` dimension is dropped with a
notice; the model call happens in both cases.

## 2. Why this milestone is ready

The provider-reasoning-control line is closed and this is a post-closure
corrective against a live defect in its M003 admission policy. Every seam it
needs exists: the capability record and its resolution, the kernel's decision
function, the daemon's single admission point, the notice channel, and the
tests that drive the daemon's turn entry point.

## 3. Current implementation evidence

Read at the baseline above. Line numbers are read locations; re-read before
editing.

- `crates/codegg-core/assets/model-adapters/` has seven families; only
  `anthropic`, `openai`, and `minimax` carry
  `[profile.reasoning_capability]`.
- `crates/codegg-core/src/model_profile/capability.rs`
  `resolve_reasoning_capability` defaults to
  `ReasoningCapabilityRecord::default()`, whose every dimension is
  `Unknown`.
- `crates/codegg-core/src/model_profile/capability.rs`
  `evaluate_reasoning_selection` owns the accepted-label and budget-bound
  checks, then delegates to `apply_reasoning_capability_policy`.
- `to_neutral_thinking_capability` sets the kernel's overall `status` from
  `overall_status(record)`, and the kernel consults that status before the
  per-dimension one.
- `src/core/daemon_turns.rs` `admit_reasoning_capability` is the single
  admission point; it passed `ReasoningCapabilityPolicy::default()` and
  mapped `Reject` to a `reasoning_capability_unsupported` response.
- The same file's turn path calls it after `resolve_reasoning_selection`, so a
  persisted principal preference reaches the gate on every turn.

## 4. Invariants that must not regress

- The kernel remains the decision function. The shipped policy is a
  `ReasoningCapabilityPolicy` value, not a second decision table.
- The open, provider-scoped label vocabulary is unchanged, as is the
  `reasoning_vocabulary_unsupported` refusal for a label a daemon has not
  advertised.
- No reasoning field is invented: an unset selection still encodes no field on
  every surface.
- `Reject` stays expressible; this milestone removes it from the shipped path
  rather than from the vocabulary.
- The capability resolution order, its read-only property, and the
  narrowing-only override and overlay rules are untouched.

## 5. Scope

In:

- Add `local_admission_policy()` to `codegg_core::model_profile::capability`
  with `unknown → Allow`, `unsupported → WarnDrop`, `mixed → WarnDrop`.
- Use it at the daemon's admission point.
- Apply a declared `Unsupported` dimension per dimension, before the kernel
  call, so a partial record keeps its declared fact.
- Rewrite the tests that asserted the refusal, and add a policy test covering
  all three selections against an all-`Unknown` record.
- Amend ADR-0050 and `architecture/model-adapters.md`.

Out:

- New adapter baselines, discovery-overlay wiring, a config knob for the
  policy, a protocol variant, a storage change, encoder changes, and
  frontend changes.

## 6. Acceptance

1. A selection on an all-`Unknown` record is admitted and forwarded unchanged.
2. An explicit disable that cannot be expressed is dropped with a notice and
   the turn dispatches.
3. An unaccepted effort label is dropped with a notice and the turn dispatches.
4. The daemon's admission point uses the shipped policy, not
   `ReasoningCapabilityPolicy::default()`.
5. No test asserts that a reasoning selection refuses a turn.
