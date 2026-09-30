# Tool-Selection Advisor Retrieval-Signal Experiment M001R — Signal Preregistration After Evaluation Corrective

Status: ready for handoff (C001 closed disposition B with no ranker blocker; see `plans/closure/tool-selection-advisor-retrieval-evaluation-semantics-corrective/001-status.md`)

Repository baseline: `ce088e9153b821d8473372c7c04786a4d90ab6ae`

Source roadmap:

- `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md`

Hard dependency:

- `plans/closure/tool-selection-advisor-retrieval-evaluation-semantics-corrective/001-status.md`
- disposition **B — real inferable retrieval gap**

Predecessor:

- M001 is blocked/closed as historical evidence and MUST NOT be reopened.

Primary class: evidence/infrastructure.

## 1. Objective

Resume only the preregistration half of the original M001 after retrieval relevance semantics are corrected.

M001R freezes Retrieval Signal V2 and conditional learned-projection degrees of freedom against the corrected inferable target.

## 2. Inputs

Use only:

- frozen historical corpus;
- corrected derived retrieval relevance view from C001;
- C001 rebaseline evidence;
- existing pinned MiniLM;
- existing advisor authority/candidate surface.

Do not use v3/future-v4 to select fields or hyperparameters.

## 3. Representation contract

Freeze:

- candidate fields;
- query fields;
- schema byte caps;
- identifier normalization;
- field weighting;
- deterministic mode grid.

No per-tool aliases added to repair observed misses.

## 4. Conditional projection grid

Freeze only if still justified by C001:

- shared linear 384→128;
- asymmetric linear 384→128;
- asymmetric 2-layer 384→128→128;
- <=500k trainable params;
- learning rate/epoch/temperature/loss/seeds/hard negatives/batch size.

MiniLM remains frozen.

## 5. Corrected target binding

Every experiment receipt/config must bind:

- historical corpus fingerprint;
- derived retrieval relevance fingerprint;
- corrected eligible counts per split;
- C001 closure/disposition;
- retrieval gate constants.

Any derived-view change invalidates the preregistration.

## 6. Ranker-impact dependency

If C001 recorded disposition C or a required ranker-label corrective, M001R remains blocked even if a retrieval gap exists.

Do not create a retrieval stack that assumes a downstream model already invalidated by corrected semantics.

## 7. Outputs

Commit a compact preregistration receipt superseding the unproduced original M001 receipt.

Suggested:

- `assets/tool-advisor/retrieval-signal-m001r-preregistration.json`

## 8. Acceptance

M001R closes positively only when the corrected target and all Signal V2/conditional projection degrees of freedom are frozen with no new evidence defect.

Positive M001R makes M002 ready.

M001 remains historical blocked evidence.
