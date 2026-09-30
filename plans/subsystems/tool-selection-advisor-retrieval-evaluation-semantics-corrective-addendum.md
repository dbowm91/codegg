# Tool-Selection Advisor Retrieval-Evaluation Semantics Corrective Addendum

Status: active

Repository planning baseline: `ce088e9153b821d8473372c7c04786a4d90ab6ae`

Controlling architecture/process:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`
- `plans/003-planning-process.md#7-corrective-passes`

Predecessor evidence:

- `plans/closure/tool-selection-advisor-retrieval-signal-experiment/001-status.md`
- `plans/closure/tool-selection-advisor-retrieval-architecture-experiment/003-status.md`
- `plans/closure/tool-selection-advisor-order-invariance-experiment/003-status.md`

## 1. Purpose

Repair the retrieval evaluation target after M001 proved that the frozen corpus mixes current-step tool relevance with implicit future workflow relevance.

This corrective does **not** improve retrieval scoring. It defines what retrieval is supposed to recover, constructs a derived immutable evaluation view, and re-runs the already-existing baseline frontier against that corrected target.

## 2. Defect statement

The existing corpus `relevance` field is suitable as broad tool-use supervision but is not a precise coarse-retrieval target.

Examples include current requests that label an unmentioned downstream tool as relevant:

- summarize + `lsp_rename`;
- plugin enable + `write`;
- plugin enable + `table_filter`;
- coverage + `write`;
- read/public-export task + `table_filter`.

A retriever that only sees bounded current state cannot infer every such secondary label.

Scoring all of them as retrieval false negatives conflates:

- **retrieval quality**, and
- **workflow prediction**.

## 3. Corrective scope

One milestone:

- `plans/implementation/tool-selection-advisor-retrieval-evaluation-semantics-corrective/001-inferable-relevance-target-and-rebaseline.md`

Status: ready.

It must:

1. define authoritative retrieval-relevance semantics;
2. audit all relevant labels in the dev partition, not only the four predecessor misses;
3. generate a versioned derived evaluation view without modifying historical corpus files;
4. validate inferability against allowed `AdvisorContextV2` state;
5. quantify train/dev/test impact for the frozen span-packed ranker;
6. recompute the existing BM25/semantic/fusion frontier against the corrected target;
7. choose the correct successor branch.

## 4. Retrieval relevance classes

The corrective must define at least:

### A. current-step

The tool is directly justified by the current objective/task/unresolved signal/capability cue.

Examples:

- "Locate every test fixture matching ..." → `glob`;
- "Filter the tabular export ..." → `table_filter`;
- "Rename the symbol ... everywhere" → `lsp_rename`.

### B. explicit-next-step

The tool is justified by a next step explicitly present in allowed `AdvisorContextV2.next_steps` or equivalent bounded current-state plan field.

This class may be included in retrieval relevance because the retriever is allowed to see that field.

### C. implicit-future

The tool is merely a plausible downstream/supporting workflow step and is not supported by any allowed current-state field.

This class is excluded from coarse retrieval recall.

### D. evidence-defect

The label/candidate/fixture relationship is internally inconsistent or cannot be justified even as a plausible workflow continuation.

This class fails closed and requires fixture/evidence correction.

## 5. Derived-view rule

Historical corpus JSONL is immutable.

Create a separate repository-owned mapping/manifest keyed by stable case id + candidate identity.

Suggested asset:

- `assets/tool-advisor/retrieval-relevance-v1.json`

It records:

- case id;
- candidate name;
- original relevance grade;
- retrieval relevance class;
- retrieval-eligible boolean;
- supporting `AdvisorContextV2` field;
- bounded supporting text/rationale;
- adjudication provenance/version.

The derived view must be deterministic and fingerprinted.

## 6. Corrected retrieval target

Primary coarse-retrieval recall includes:

- current-step;
- explicit-next-step.

It excludes:

- implicit-future.

Evidence-defect entries are not silently excluded; they block closure until corrected through an additive evidence plan.

Original graded relevance remains available to downstream ranking experiments as historical data.

## 7. Consumer impact audit

Because the selected span-packed ranker was trained on the same historical relevance labels, C001 must report for train/dev/test:

- count and fraction of relevant labels in each class;
- count of cases containing any implicit-future label;
- whether highest-grade/preferred labels are affected;
- whether ranker dev/test metrics materially depend on implicit-future labels.

No retraining occurs in C001.

If implicit-future supervision materially changes the ranker's accepted dev selection, register a separate ranker-label-semantics corrective before fresh-v4 qualification.

## 8. Rebaseline

Using unchanged existing retrievers and scorer implementations, recompute:

- universes 64/128/256;
- K 16/24/32;
- BM25;
- current semantic MiniLM retrieval;
- existing normalized-union / selected fusion modes needed to reproduce the predecessor frontier.

Report:

- corrected eligible relevance count;
- recovered count/recall;
- per-tool misses;
- current-step vs explicit-next-step recall;
- authority violations;
- whether the old four-tool miss set remains.

Do not tune new weights, add aliases, add schema fields, train projections, or change K in this corrective.

## 9. Successor dispositions

### Disposition A — existing retrieval clears corrected target

If an unchanged existing retrieval mode clears:

- 64 >=0.99;
- 128 >=0.98;
- 256 >=0.95;
- K<=32;
- zero authority violations;

then Retrieval Signal V2 representation work is unnecessary.

The corrective closure may make retrieval-signal M004 eligible for replanning/activation using the existing retrieval mode, subject to the ranker-impact audit.

### Disposition B — genuine inferable signal gap remains

If corrected labels are valid but the unchanged frontier still misses inferable current/explicit-next-step tools, unblock:

- `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/001r-signal-preregistration-after-evaluation-corrective.md`

M001R then owns Signal V2/projection preregistration against the corrected target.

### Disposition C — ranker-target semantics materially invalidated

If retrieval semantics correction reveals that the selected span-packed ranker's positive dev selection materially depends on implicit-future labels, register a separate ranker-label-semantics corrective before operating-point/final qualification work.

### Disposition E — evidence correctness failure

If adjudication cannot be made deterministic, supporting text is absent, or derived-view validation fails, stop without unblocking model work.

## 10. Invariants

- frozen corpus unchanged;
- no v3/v4 tuning;
- no retrieval/model algorithm changes;
- no gate relaxation;
- no tool authority widening;
- no implicit workflow label treated as current-step merely to improve recall;
- no user/private runtime context persisted in derived assets.

## 11. Completion definition

C001 closes only with:

- explicit retrieval relevance semantics;
- complete dev-label adjudication;
- deterministic derived-view fingerprint;
- train/dev/test consumer impact report;
- unchanged-baseline remeasurement;
- one explicit A/B/C/E disposition;
- registry/roadmap reconciliation.

Until then M001R and M002-M005 remain blocked.
