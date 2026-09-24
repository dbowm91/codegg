# Tool-Selection Advisor Retrieval-Signal Experiment Corrective C001 — Retrieval Evaluation Target

Status: blocked

Repository baseline: `f351d014`

Source roadmap: `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md`

Related milestone: M001 at `plans/closure/tool-selection-advisor-retrieval-signal-experiment/001-status.md`.

Primary class: evidence/infrastructure.

## 1. Objective

Resolve whether retrieval relevance means current-step tools, all explicitly inferable
tools, or graded workflow recall, and repair only invalid labels in a new immutable
evaluation version. This corrective owns the M001 hard-stop finding; historical corpus,
dev frontier, and predecessor receipts remain unchanged.

## 2. Why this corrective is not ready

The M001 audit found labels that appear unrelated to the allowed current-state query.
For example, `shell-semantic-186-variant-1` asks to list running processes but labels
`table_filter` relevant at grade 2. `filesystem-semantic-013-variant-1` asks to read a
module and report exports but also labels `table_filter` at grade 2. The current
evaluation target does not say whether these are intentional future workflow positives
or erroneous labels. A product/evaluation decision is required before changing labels
or declaring the signal experiment valid. M001-M005 remain blocked meanwhile.

## 3. Required decision and work

1. Define relevance as one explicit contract: current-step recovery, all inferable
   workflow tools, or graded workflow recall with a rule for secondary labels.
2. Adjudicate every gate-critical `table_filter` label and all persistent-miss labels
   against only the permitted `AdvisorContextV2` fields. Each decision must quote the
   supporting text and explain grade and primary/secondary status.
3. Produce a new immutable corpus/evaluation version and fingerprints if any labels
   change. Never edit historical train/dev/test/v2/v3 assets or receipts in place.
4. Recompute the affected dev frontier against the frozen predecessor scorer solely
   to determine whether the evaluation defect materially changed the M001/M002 gates.
5. Register a revised Signal V2 preregistration plan only after the evaluation target,
   labels, split fingerprints, and gate impact are accepted.

## 4. Stop conditions

Stop if the intended relevance target cannot be established from existing canonical
contracts and decision-owner direction. Do not infer the decision from model scores,
delete inconvenient labels, retrain against disputed labels, or relax retrieval gates.

## 5. Closure evidence

The corrective closure must contain the target decision, complete case-level adjudication,
new fingerprints if applicable, leakage review, affected-gate recalculation, verification
commands and results, and an explicit dependency audit for M001-M005.
