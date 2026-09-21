# Tool-Selection Advisor Sequence-Encoder Experiment M002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-selection-advisor-sequence-encoder-experiment/003-current-state-advisor-context-v2.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-sequence-encoder-experiment-roadmap.md#m002--current-state-advisor-context-projection-v2`

Repository baseline reviewed: `a095dfa`

Implementation commits:

- `a095dfa` — advisor: project current-state context v2

## 1. Executive finding

M002 is closed. Runtime proactive disclosure and offline fixtures now share a
versioned, deterministic `AdvisorContextV2` contract. Current durable Goal,
Todo, WorkPlan, current-turn prompt, and bounded unresolved-error signals are
projected before advisor scoring; stale origin prompts no longer dominate an
active current objective. M003 remains blocked only by M001's missing real
pretrained reference assets.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Versioned canonical serializer | `src/tool_advisor/context_v2.rs` | pass | Explicit labels and schema marker. |
| Current objective precedence | `active_goal_and_work_plan_task_outrank_origin_prompt` | pass | Durable frame goal outranks current/origin fallback; WorkPlan task is preferred for task field. |
| Current task and next steps | request-preparation integration + context tests | pass | WorkPlan actionable/pending items are bounded to two. |
| Unresolved signal | `AdvisorContextV2::from_context_frame` | pass | Only bounded ledger error summaries are included. |
| No transcript/tool payloads/reasoning | projection field allowlist | pass | Touched files, commands, results, arguments, and audit records are excluded. |
| Deterministic valid-boundary bounds | `fallback_and_unresolved_projection_are_bounded` | pass | Total output is capped at 8 KiB and remains UTF-8 valid. |
| Secret-negative behavior | `secret_like_fields_never_enter_projection` | pass | Secret-like assignments are replaced as whole fields. |
| Offline compatibility adapter | `from_benchmark_context` test | pass | Frozen labels are not changed. |
| Runtime wiring and default/off behavior | request-preparation seam + `scripts/verify.sh quick` | pass | Projection is advisory input only; authority remains resolved surface. |

## 3. Production implementation evidence

`AdvisorContextV2` is a standalone host-owned DTO under `tool_advisor` with
field-level bounds, deterministic truncation, conservative secret-like field
redaction, and a frozen-case adapter. `AgentLoop` captures the current user
prompt before tool-definition construction. Request preparation builds the
current frame, reads the active WorkPlan when available, and serializes the
v2 context at the existing proactive disclosure seam.

The advisor still receives only a string and can only affect ranking/promotion
of already-resolved definitions. No persistence or history service was added.

## 4. Verification executed

### Commands run

```bash
rtk cargo fmt --all
rtk cargo check --locked
rtk cargo test --locked -p codegg --lib tool_advisor::context_v2
rtk scripts/verify.sh quick
rtk cargo clippy --workspace --all-targets --all-features -- -D warnings
rtk git diff --check
```

### Results

- Four focused context-v2 tests passed.
- Locked default check passed.
- `scripts/verify.sh quick` passed.
- All-feature Clippy passed with `-D warnings`.
- Diff check passed.

## 5. Invariant review

- No full transcript, secrets, credentials, raw tool data, arbitrary audit data, or chain-of-thought enters the projection.
- Projection is deterministic and bounded before tokenizer input.
- Optional storage lookup failures degrade to the smaller frame/origin fallback; they do not fail the turn.
- Existing `ResolvedToolSurface` remains the authority source.

## 6. Failure and recovery review

Missing Goal/WorkPlan storage, absent Todo state, empty prompts, oversized
Unicode, and secret-like content all have deterministic bounded fallbacks. No
new durable state, cancellation path, restart behavior, or contention owner
was introduced.

## 7. Migration and compatibility review

The change is additive and uses no schema migration. Frozen C001 labels remain
untouched. The existing original-prompt path is replaced only at the advisor
input seam; normal/off advisor behavior retains its previous authority and
tool palette semantics.

## 8. Security review

The projection is built after policy-owned state is available and before
advisor scoring. Secret-like values are redacted, and no credential-bearing
tool or result payload is queried. WorkPlan/Goal reads are observational and
do not grant mutation rights.

## 9. Documentation and operations

The implementation and tests document the v2 field contract. The existing
advisor feature remains opt-in/default-off, and no new operator service or
network dependency is required.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Capability cue is currently omitted when no trusted coarse capability projection exists | No correctness/security impact; optional field is not required for v2 | Add only from an existing trusted coarse state source in a future bounded plan. |

## 11. Roadmap disposition

M002 is closed. Its hard dependency is satisfied, but M003 remains blocked on
the independently recorded M001 condition: real local pretrained reference
assets with exact provenance/license hashes. M004 and M005 remain blocked.

## 12. Registry updates

- M002 moved from active implementation to closed history.
- The sequence-encoder roadmap records M002 closed and M003 blocked only on M001 reference-asset evidence.
- No future plan was unblocked past the named M001 condition.
