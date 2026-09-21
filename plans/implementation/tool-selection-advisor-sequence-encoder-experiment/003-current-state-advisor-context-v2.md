# Tool-Selection Advisor Sequence-Encoder Experiment M002 — Current-State Advisor Context v2

Status: closed

Repository baseline: `8487967d9cc2605c398d3c608e353c8871289a7d`

Source roadmap:

- `plans/subsystems/tool-selection-advisor-sequence-encoder-experiment-roadmap.md#m002--current-state-advisor-context-projection-v2`

Primary class: capability/invariant polish.

## Objective

Replace the current original-session-prompt-only advisor input with a small versioned projection of the task state that is relevant **now**.

At the baseline, `bounded_advisor_context(self.original_user_prompt.as_deref())` is used for proactive disclosure even though the same agent loop already has access to active Goal/WorkPlan/Todo state and unresolved-error context.

This weakens long-session tool selection and would unfairly handicap a better model.

## Invariants

- No full transcript is supplied to the advisor.
- No secrets, credentials, raw tool arguments/results, or arbitrary audit records are included.
- No new persistence/history service is created.
- The projection is deterministic and bounded before tokenization.
- Failure to gather optional state falls back to a safe smaller context rather than failing the turn.
- The same semantic context schema can be constructed for offline fixtures and runtime turns.

## Proposed `AdvisorContextV2`

Versioned fields, serialized with explicit labels:

- current objective:
  - active durable Goal if present;
  - otherwise current/latest user objective;
  - original session prompt only as final fallback;
- current task:
  - in-progress Todo/WorkPlan item if present;
- next step:
  - at most one or two pending items;
- unresolved signal:
  - bounded recent unresolved error/test failure summary, not raw command output;
- optional workspace/repository capability cue:
  - coarse category only when already known in trusted state;
- no chain-of-thought.

Use an explicit field-level byte/token budget and total budget.

## Ownership

Do not independently re-query every subsystem from request preparation if an existing `ContextFrame`/continuation projection already resolves precedence. Extract a narrow advisor projection from existing authoritative state where possible.

The new projection should have:

- schema/version constant;
- canonical serializer;
- deterministic truncation;
- test fixture constructor;
- redaction/content classification.

## Regression cases

1. original prompt says "inspect architecture", active Goal now says "fix failing Rust tests": advisor context must prioritize current Goal/task;
2. long session with stale origin prompt and in-progress WorkPlan item;
3. unresolved compiler error should provide a bounded error cue;
4. no Goal/Todo/errors falls back cleanly;
5. oversized Unicode input truncates on valid boundaries;
6. content marked secret/credential never enters projection;
7. two identical states serialize identically.

## Dataset compatibility

Do not mutate the frozen C001 test labels.

Add a deterministic adapter that maps existing benchmark case context into `AdvisorContextV2` fixture form for continuity, and permit new context-v2-specific **training/dev** cases only if their lineage is isolated from frozen test/family holdouts.

## Verification

- focused request-preparation/context-frame tests;
- advisor context snapshot tests;
- default/off equivalence;
- secret-negative tests;
- `scripts/verify.sh quick`;
- full Clippy/format/diff checks.

## Acceptance

M002 closes when runtime proactive disclosure and experiment fixtures share one bounded current-state context contract and stale original-prompt-only behavior is covered by a failing-before/passing-after regression.
