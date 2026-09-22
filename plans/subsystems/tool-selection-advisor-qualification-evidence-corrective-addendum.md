# Tool-Selection Advisor Qualification Evidence — Narrow Post-Closure Corrective

Status: active

Repository planning baseline: `739bf5060690fa71dffd25ffeb1b28e444a00681`

Controlling architecture:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Predecessor evidence:

- `plans/subsystems/tool-selection-advisor-sequence-qualification-corrective-addendum.md` — closed.
- `plans/closure/tool-selection-advisor-sequence-qualification-corrective/001-status.md` — v2 harness/holdout closure.
- `plans/closure/tool-selection-advisor-sequence-qualification-corrective/002-status.md` — historical v2 disposition D.
- `assets/tool-advisor/sequence-qualification-v2-result.json` — observed historical result; diagnostic only after this corrective.

Normative process:

- `plans/003-planning-process.md#7-corrective-passes`

## 1. Purpose

The prior qualification corrective fixed the broad qualification architecture and release-resource methodology, but post-closure review found two narrow evidence defects that make its model-quality disposition non-final:

1. the v2 holdout's semantic tags did not correspond to the behavior they claimed to test;
2. the retrieval gate confused shortlist size with candidate-universe size, making the 64/128-tool recall gates fall through to zero.

The v2 result remains immutable historical evidence. This corrective does not reinterpret it as a valid model rejection.

## 2. Concrete defects

### V2 holdout semantic validity

The deterministic v2 generator reused one short family template and assigned semantics largely by index:

- `none=true` cases still told the model to "use the ... tool";
- `counterfactual` was assigned to the same modulo-8 cases as `no-tool`, not to paired prompts whose changed semantic cue changes the correct label;
- `unknown-renamed` was a tag only; relevant tool names were not actually renamed/unseen;
- `context-v2-long-session` was a tag on the same short prompt rather than a realistic AdvisorContextV2-shaped stale-origin/current-goal/task/error scenario;
- unique record IDs made leakage signatures unique without providing sufficient semantic diversity.

Consequently no-tool, counterfactual, unknown-tool, long-session, and several aggregate conclusions are contaminated.

### Retrieval gate identity bug

Qualification v2 evaluated:

- a shortlist of `retrieval_k = 16`;
- against candidate universes expanded to 64 and 128 tools.

The frontier records identify the shortlist size as `point.k == 16`. The gate later searched for `point.k == 64` and `point.k == 128`; both lookups therefore returned no point and defaulted to recall `0.0`.

The reported v2 64/128 retrieval recall of zero is a harness artifact, not measured retrieval evidence.

## 3. Scope

This corrective is deliberately narrow.

It may change:

- qualification-only fixture construction and semantic validation;
- qualification-only retrieval evidence schema/gating;
- regression tests;
- a new holdout and preregistered result;
- planning/closure documentation.

It MUST NOT change:

- MiniLM encoder weights;
- ranking-head weights;
- selected packed-marker architecture;
- pooling;
- abstention threshold/calibration;
- retrieval ranking algorithm;
- runtime authority/promotion implementation;
- live provider behavior;
- normal/default CodeGG behavior.

## 4. Dependency graph

```text
historical v2 qualification (immutable)
             |
             v
C001 semantic-fixture + retrieval-identity repair
             |
             v
C002 fresh-v3 preregistered qualification
             |
          disposition A
             v
existing live-primary-model M004
```

- C001 is closed positively with `plans/closure/tool-selection-advisor-qualification-evidence-corrective/001-status.md`.
- C002 is ready; its separate preregistration freeze and final-run discipline remain mandatory.
- Live M004 remains blocked unless C002 records A.

## 5. Milestones

### C001 — Semantic holdout and retrieval-gate correctness

Plan:

- `plans/implementation/tool-selection-advisor-qualification-evidence-corrective/001-semantic-holdout-and-retrieval-gate-correctness.md`

Status: closed.

Repair the frontier identity bug, add semantic-fixture validators, and freeze a genuinely new v3 holdout whose labels and slice tags are behaviorally true. The selected model MUST NOT be evaluated on v3 during C001.

### C002 — Fresh v3 preregistered requalification

Plan:

- `plans/implementation/tool-selection-advisor-qualification-evidence-corrective/002-fresh-v3-preregistered-qualification.md`

Status: ready.

Freeze the unchanged model plus v3 holdout and corrected gates in a separate commit, require CI, then perform one release-mode final run.

## 6. Exit conditions

The corrective closes when:

- semantic validators prove every special slice represents the claimed behavior;
- retrieval frontier points distinguish shortlist size from candidate-universe size;
- retrieval gates use candidate-universe identity rather than `k`;
- v3 has zero leakage into all historical corpora including v2;
- the selected model remains byte-identical;
- one preregistered release qualification produces A/B/C/D/E;
- only A can unblock live M004.

A negative result from a semantically valid v3 holdout is accepted as substantive model evidence.
