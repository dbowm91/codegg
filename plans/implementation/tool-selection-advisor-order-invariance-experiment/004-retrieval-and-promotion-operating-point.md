# Tool-Selection Advisor Order-Invariance Experiment M004 — Retrieval and Promotion Operating Point

Status: ready for handoff

Repository baseline: `c96fbdfa75996187f725289890ccaaf5b140d180`

Hard dependency:

- M003 positive selected order-robust artifact.

Source roadmap:

- `plans/subsystems/tool-selection-advisor-order-invariance-experiment-roadmap.md#m004--retrieval-and-promotion-operating-point`

Controlling ADR:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Primary class: model operating-point/integration experiment.

## 1. Objective

Choose a large-catalog retrieval operating point and a promotion-specific confidence threshold using train/dev-only evidence.

This milestone fixes two independent v3 failures:

- K=16 retrieval recall 0.9294 at 64/128 candidates;
- promotion threshold incorrectly inherited from abstention calibration.

## 2. Retrieval dev fixtures

Build 64/128/256-tool candidate universes from clean train/dev cases plus deterministic authority-safe distractors.

Requirements:

- every labeled relevant candidate is preserved;
- candidate universe size and shortlist K are explicit;
- descriptors use the same schema as runtime;
- include unknown/renamed descriptors where available from dev-only data;
- no v3/v4 cases used for tuning.

## 3. Retrieval frontier

Predeclare and evaluate:

- K = 16, 24, 32;
- BM25;
- semantic;
- RRF/union fusion currently implemented.

Do not add a new learned retriever in this milestone.

Select the smallest/cheapest operating point satisfying:

- 64-tool recall >=0.99 on dev;
- 128-tool recall >=0.98 on dev;
- 256-tool recall >=0.95 on dev;
- zero authority violations.

If no K<=32 clears the frontier, close negatively or register a separate retrieval architecture experiment. Do not silently raise candidate budget without bounded latency/schema analysis.

## 4. Promotion confidence separation

Do not reuse abstention threshold as candidate promotion threshold.

The selected M003 artifact must expose:

- candidate relevance probability or calibrated confidence;
- abstention probability separately.

Define promotion eligibility as:

- advisor not abstaining;
- candidate relevance confidence >= promotion threshold;
- candidate is within authority-filtered deferred set;
- max promotion/schema budgets still apply.

The promotion threshold is selected on dev only.

## 5. Candidate relevance calibration

Using M003 candidate-relevance logits:

- fit temperature/Platt/isotonic only if the method is predeclared and implementable locally;
- prefer a simple temperature+bias transform with explicit artifact parameters;
- report Brier/ECE/NLL before/after calibration.

Do not calibrate from v3.

## 6. Promotion threshold sweep

Predeclare threshold grid/range and select the highest-recall point satisfying:

- no-tool promotion rate <=0.10;
- irrelevant-tool promotion rate <=0.15;
- max promotions <=2;
- p95 schema bytes <=16 KiB.

Target:

- relevant-tool promotion recall >=0.60 on dev.

If multiple points satisfy, choose lower schema/false-promotion cost before extra recall.

## 7. Context/ordering robustness

Promotion evaluation must include candidate permutation suites:

- promoted candidate identity should be invariant when the candidate set is permuted;
- promotion confidence drift must stay within the M003 numeric contract;
- no candidate should cross threshold solely because presentation position changed.

## 8. Resource measurements

Report at selected K/threshold:

- retrieval p50/p95/max;
- ranking p50/p95/max;
- total turn-side advisor latency;
- encoder forwards;
- cache size/hit behavior;
- RSS;
- added schema bytes;
- K scaling 16→24→32.

Preserve the prior resource envelope unless a separate plan explicitly changes it.

## 9. Artifact/config separation

Persist separate fields for:

- abstention calibration;
- candidate relevance calibration;
- promotion threshold;
- retrieval mode;
- retrieval K.

A static/serialization test prevents one threshold field from populating another.

## 10. Regression tests

At minimum:

- promotion threshold != abstention threshold contractually;
- serialized artifact round-trips both independently;
- K24/K32 frontier points identify universe and shortlist separately;
- missing retrieval point fails closed;
- permutation does not change promoted identity for order-equivariant model;
- no-tool examples remain unpromoted at selected threshold;
- denied/hidden tool never enters promotion universe.

## 11. V3 diagnostic

After operating point is frozen on dev, v3 may be rerun once diagnostically to compare:

- retrieval recall at selected K;
- promotion recall/FPR.

No parameter/threshold changes may follow from this diagnostic.

## 12. Acceptance

M004 closes positively only if:

- retrieval clears dev large-catalog gates at bounded K<=32;
- promotion has a separately calibrated relevance threshold;
- promotion clears dev recall/FPR/no-tool/schema constraints;
- order robustness and authority hold;
- operating point is frozen for M005.

Positive closure makes M005 ready.
