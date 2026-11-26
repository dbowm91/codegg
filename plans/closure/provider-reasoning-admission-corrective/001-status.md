# Provider Reasoning Admission C001 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/provider-reasoning-admission-corrective/001-a-preference-never-blocks-a-call.md`

Source subsystem roadmap: `plans/subsystems/provider-reasoning-admission-corrective-addendum.md`

Repository baseline reviewed: `b554ed7bc`

Implementation state: uncommitted working-tree changes in this worktree. This
record does not claim a commit or a verified commit head.

## 1. Executive finding

The daemon applied the kernel's fail-closed reasoning-admission policy while
only three of the seven adapter families publish a capability record. On the
other four every dimension resolves to `Unknown`, so **any** resolved
selection — including a persisted `Off` — refused the turn with
`reasoning_capability_unsupported` before provider resolution. On this machine
the preference store held `{"mode":"off"}`, so no turn could dispatch a
provider request on those families at all, and re-selecting `off` could not
clear it because the disable was judged too. The same policy refused a
`disable` on MiniMax, whose baseline correctly declares that the model always
reasons.

The shipped policy is now `unknown → Allow`, `unsupported → WarnDrop`,
`mixed → WarnDrop`, and a declared `Unsupported` dimension is applied per
dimension ahead of the kernel's overall-status read. A reasoning preference can
choose what is sent; it can no longer decide whether a call happens.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| A selection on an all-`Unknown` record is admitted and forwarded unchanged | `core::daemon_turns::tests::a_selection_on_an_unknown_dimension_is_forwarded_unchanged`; `the_shipped_policy_never_refuses_a_preference` | pass | The daemon test drives a real turn request for a `generalcompute` model, whose family declares no record, and asserts the recorded turn selection is the one supplied. |
| An inexpressible disable is dropped with a notice and the turn dispatches | `an_explicit_disable_on_an_always_reasons_model_is_dropped_and_runs` | pass | MiniMax baseline: `disable_status = "unsupported"`, `always_reasons = true`. The test asserts dispatch, `None` reaching the request, and a `reasoning_capability_notice` naming the dimension. |
| An unaccepted effort label is dropped with a notice and the turn dispatches | `an_unaccepted_reasoning_label_is_dropped_and_the_turn_still_dispatches` | pass | `turbo` is outside MiniMax's published ladder. |
| The daemon uses the shipped policy, not the kernel default | source: `admit_reasoning_capability` passes `capability::local_admission_policy()` | pass | `ReasoningCapabilityPolicy::default()` is no longer referenced from `src/`. |
| A partial record keeps its one declared fact | `the_shipped_policy_never_refuses_a_preference` | pass | The record declares only effort `unsupported`; the decision is `WarnDrop` with dimension `effort`, not `Allow` from the overall `Unknown` status. |
| No test asserts that a selection refuses a turn | `grep` over the daemon tests; the three refusal tests were rewritten | pass | `error_of` remains used by the vocabulary-refusal test, which is a different gate. |
| The kernel still supplies the decision | source: the policy value is a `ReasoningCapabilityPolicy`; the decision still comes from `reasoning_capability_notices_neutral` | pass | The only pre-kernel rule added is the declared-`Unsupported` short-circuit, whose disposition is `policy.unsupported`. |
| Read-only | `the_capability_check_does_not_mutate_the_profile_or_the_config` | pass | Unchanged, still passing. |

## 3. Production implementation evidence

- `crates/codegg-core/src/model_profile/capability.rs`
  - new `local_admission_policy()` returns the shipped disposition set and
    documents the two failure modes it removes;
  - `evaluate_reasoning_selection` applies a declared `Unsupported` dimension
    per dimension, before the kernel call, because the kernel reads the model's
    overall status first.
- `src/core/daemon_turns.rs`
  - `admit_reasoning_capability` uses `capability::local_admission_policy()`;
  - its doc comment records why the kernel default is not used;
  - the three tests that asserted the refusal now assert the drop or the
    forward, each with dispatch evidence.
- `architecture/model-adapters.md` §"The Local Admission Policy" states the
  shipped table, the reason for each disposition, and that `Reject` is not on
  the shipped path.
- `plans/adrs/ADR-0050-per-model-reasoning-capability-authority.md` carries a
  corrective amendment note that states the decision **changes**. The authority
  order, the narrowing-only rule, the `Unknown` semantics, the always-reasons
  treatment and the kernel/CodeGG division are unchanged.

## 4. Verification executed

- `cargo test -p codegg-core --lib model_profile::capability` — 12 passed.
- `cargo test -p codegg --lib core::daemon_turns` — 22 passed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — exit 0.
- `python3 scripts/check_comment_budget.py --bank` — banked 1545 files.
- `scripts/verify.sh quick` — passed (exit 0).

## 5. Non-claims

- No live provider request was made. Whether a given provider accepts the
  surface's disable spelling (`reasoning_effort: "none"` on the chat surface,
  `thinking.type = "disabled"` on Anthropic) for a family that publishes no
  contract is the provider's decision and is **not verified** here. The local
  gate no longer refuses the request before the provider can answer.
- The four families without a record still carry none. This milestone does not
  invent a contract, and the discovery overlay is still without a live source.
- The MiniMax connection on this machine logs
  `resolve_provider_credential: failed to decrypt encrypted_api_key ... aead::Error`.
  That is an independent credential-store failure, is not caused by this
  admission gate, and is not repaired here.
