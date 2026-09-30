# Tool-Selection Advisor Retrieval-Signal Experiment M006 — Retrieval Evaluation Corrective

Status: implemented — decision `current-step-only` recorded, M002 unblocked (receipt `assets/tool-advisor/retrieval-signal-m006-decision.json`; closure `plans/closure/tool-selection-advisor-retrieval-signal-experiment/006-status.md`)

Repository baseline: `88ebbb9685bc0af497bd5b2caac0819b9542d106`

Source roadmap:

- `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md#m006--retrieval-evaluation-corrective`

Controlling architecture:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Triggering evidence:

- `plans/closure/tool-selection-advisor-retrieval-signal-experiment/001-status.md` (blocked: three gate-critical implicit-secondary labels)
- `assets/tool-advisor/retrieval-signal-m001-preregistration.json` (`m002_ready=false`)

Primary class: evidence/infrastructure.

## 1. Objective

Decide the intended retrieval relevance target and re-baseline the dev
frontier so the experiment measures model signal, not hidden workflow
knowledge. Exactly one outcome: a recorded relevance-target decision plus
a re-derived gate-critical label set under which M002 is either unblocked
(positive re-audit) or the workstream closes negatively with the
evaluation defect owned here, not by the scorer.

## 2. Why this milestone is ready

M001 closed blocked with complete evidence: the live BM25 audit
reproduces 52/72, the four persistent misses are attributed case-by-case,
and the three supporting-step labels are named with sibling-explicit
proof (`read` explains filesystem-semantic-013, `coverage` explains
verification-semantic-105, `summarize` explains research-semantic-125).
No model evidence is missing; only the product/evaluation decision the
experiment is forbidden from making implicitly.

## 3. Current implementation evidence

- `src/tool_advisor/retrieval_signal.rs`: audit engine, taxonomy, frozen
  Signal V2 contract, frozen conditional M003 grid.
- `assets/tool-advisor/retrieval-signal-m001-preregistration.json`:
  4 occurrence rows, `stop_condition` hard stop, `m002_ready=false`.
- Frozen gates 0.99/0.98/0.95 at K<=32 and frozen span-packed ranker are
  untouched.

## 4. Invariants that must not regress

- Historical train/dev/test/v2/v3 corpora stay immutable; v3 stays
  diagnostic-only and must not select the target, thresholds, or K.
- The relevance-target decision is recorded explicitly (decision log in
  the receipt); gates are re-derived from the decision, never silently
  lowered to fit observed recall.
- No projection is trained to memorize hidden labels before the decision.
- Advisor locality/authority (ADR-0009) is unchanged.

## 5. Scope

In:

- relevance-target decision among: (a) all plausible workflow tools,
  (b) current-step tools only, (c) graded recall with secondary-weight
  rules;
- mechanical re-derivation of the gate-critical set under the decision
  (explicit include/exclude rules applied to frozen labels, no label
  rewriting);
- re-computation of the dev frontier denominators and the M001
  classifications under the decided target;
- verdict: M002 unblocked (fully inferable gate-critical set) or
  workstream negative close owned here.

Out:

- any retriever/scorer change (M002's job);
- any learned projection (M003's job);
- any threshold/K/gate-constant tuning beyond what the recorded decision
  entails;
- relabeling frozen corpus files;
- v3 or fresh-v4 construction.

## 6. Required production changes

Code: small, additive, inside `src/tool_advisor/retrieval_signal.rs`
(or a sibling corrective module): a `RelevanceTarget` enum plus
`gate_critical_set(target)` filtering over the frozen M001 occurrence
rows, with the decision recorded in an extended receipt. No catalog,
ranker, promotion, or protocol changes. Docs: decision log + updated
frontier denominators in the M006 receipt.

## 7. Ordered work packages

1. Intent: record the relevance-target decision with rationale.
   Changes: `RelevanceTarget::{AllWorkflow, CurrentStepOnly,
   GradedRecall}` + decision constructor requiring a non-empty rationale.
   Acceptance: decision serializes into the receipt; variants are
   exhaustive and documented.
2. Intent: derive the gate-critical set mechanically.
   Changes: pure filter over frozen M001 rows (no corpus edits): e.g.
   under CurrentStepOnly, the three implicit-secondary rows leave the
   gate-critical set; under GradedRecall, they enter with reduced weight
   per preregistered weights.
   Acceptance: per-universe eligible counts recomputed; every exclusion
   cites its occurrence row + rule.
3. Intent: re-verify gate-critical inferability.
   Changes: re-run `run_miss_audit` classifications restricted to the
   derived set.
   Acceptance: either zero implicit-secondary/other-evidence-defect rows
   (M002 unblocked) or a named residual that closes the workstream
   negatively here.

## 8. Failure, cancellation, restart, contention semantics

Pure offline computation over frozen inputs; no daemon, persistence, or
concurrency surface. Corpus/partition drift fails closed on the existing
tripwires. A decision made without recorded rationale is invalid output.

## 9. Compatibility and migration

None: additive code + additive receipt. No artifact schema change
(the M001 receipt format may gain optional decision fields; readers must
tolerate absence).

## 10. Required tests

- decision variants are exhaustive and serialize round-trip;
- CurrentStepOnly excludes exactly the three M001 implicit-secondary
  rows and no inferable row;
- re-derived u256 denominator and required-hit counts are consistent
  with the frozen 0.99/0.98/0.95 gates;
- no frozen corpus byte changes (dataset fingerprint unchanged);
- v3 absent from decision inputs.

## 11. Required verification commands

```bash
cargo test --locked -p codegg --lib -- tool_advisor
python3 scripts/check_execution_ownership.py
scripts/verify.sh quick
cargo fmt --all -- --check
git diff --check
```

(On hosts where `tool-advisor-encoder-training` compiles, also run the
plan-literal feature-gated suite; record the host class truthfully either
way.)

## 12. Documentation updates

M006 receipt (new asset), closure record `001`-style under
`plans/closure/tool-selection-advisor-retrieval-signal-experiment/006-status.md`,
registry + roadmap disposition (unblock M002 iff positive).

## 13. Acceptance criteria

- relevance target decided and recorded with rationale;
- gate-critical set re-derived mechanically with per-row citations;
- M002 unblocked iff the re-derived set is fully inferable; otherwise
  the workstream closes negatively with the defect owned by M006.

## 14. Stop conditions

Stop and report rather than improvise if: the decision requires
relabeling frozen history (forbidden — decide the target, not the
labels); the re-derived set still contains uninferable gate-critical
rows (close negatively instead); or v3 is proposed as a decision input.

## 15. Closure evidence required

Decision log, re-derived denominators, re-audit classifications,
regression tests, verification outputs, and the explicit M002
unblock/no-unblock verdict.

## 16. Handoff notes

The decision input this plan needs is a product judgment (what retrieval
is *for*), not a model judgment: it should arrive from the operator,
not be inferred from recall numbers. Suggested default to evaluate
first: current-step-only, because every supporting-step label's query is
already fully explained by its sibling tool, which makes
all-plausible-workflow recall unmeasurable without hidden-state leakage.
The frozen Signal V2 contract and M003 grid from M001 are reused as-is;
this plan must not alter them.
