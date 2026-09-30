# Tool-Selection Advisor Retrieval-Evaluation Semantics Corrective C001 — Inferable Relevance Target and Rebaseline

Status: implemented (evidence gathered; disposition B — see `plans/closure/tool-selection-advisor-retrieval-evaluation-semantics-corrective/001-status.md`; implementation `8613575d`)

Repository baseline: `ce088e9153b821d8473372c7c04786a4d90ab6ae`

Source corrective:

- `plans/subsystems/tool-selection-advisor-retrieval-evaluation-semantics-corrective-addendum.md`

Predecessor hard stop:

- `plans/closure/tool-selection-advisor-retrieval-signal-experiment/001-status.md`

Controlling architecture/process:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`
- `plans/003-planning-process.md#7-corrective-passes`

Primary class: correctness/evidence.

## 1. Objective

Define a retrieval target that contains only tools inferable from bounded current-state evidence, derive that target without rewriting historical corpus labels, and remeasure the existing retrieval frontier before any new retrieval representation/model is authorized.

## 2. Non-goals

Do not:

- change historical corpus JSONL;
- alter BM25, semantic retrieval, fusion weights, K, or pooling;
- add aliases/synonyms/schema fields to descriptors;
- train any projection/model;
- retrain the span-packed ranker;
- create v4;
- change recall gates;
- change authority/disclosure behavior.

## 3. Work package A — authoritative semantics

Introduce a versioned retrieval-evaluation contract.

Recommended types:

```rust
enum RetrievalRelevanceClass {
    CurrentStep,
    ExplicitNextStep,
    ImplicitFuture,
    EvidenceDefect,
}
```

The exact type location is implementation-defined, but one authoritative serializer/validator must exist.

### CurrentStep

Must cite an allowed field containing direct support for using the tool now.

### ExplicitNextStep

Must cite an allowed structured next-step field. Generic "this might be useful later" reasoning is insufficient.

### ImplicitFuture

No allowed current-state field directly supports the tool, but it is a plausible later workflow action.

### EvidenceDefect

The original relevance label is unsupported/inconsistent even under broad workflow interpretation.

## 4. Work package B — complete dev adjudication

Audit **every positive relevance label in the dev partition**.

Do not adjudicate only misses.

For each label record:

- case id;
- candidate;
- original relevance grade;
- preferred-order index if any;
- semantic/tool family;
- class;
- supporting allowed field;
- bounded supporting text;
- rationale.

Require deterministic stable ordering.

At least two static checks should prevent:

- `ImplicitFuture` with a non-empty claimed direct-support field;
- `CurrentStep`/`ExplicitNextStep` without supporting evidence.

## 5. Work package C — derived evaluation view

Create a repository-owned asset such as:

- `assets/tool-advisor/retrieval-relevance-v1.json`

and a manifest/fingerprint if useful.

Do not copy full private/runtime context; this asset is derived only from the repository corpus.

Validation:

- every dev positive label appears exactly once;
- no unknown case/candidate;
- original grade matches corpus;
- no duplicate case+candidate;
- all required rationales present;
- corpus fingerprint bound;
- adjudication schema version bound.

## 6. Work package D — corpus-wide consumer-impact audit

Apply the same deterministic classifier/adjudication rules to train/test labels or produce an explicit audited mapping for them.

Report:

- positive labels by class per split;
- cases containing implicit-future labels;
- highest-grade labels by class;
- preferred-first labels by class;
- per-family distribution.

Then recompute frozen span-packed dev/test metrics in two diagnostic views:

1. historical broad relevance;
2. retrieval-inferable relevance.

This is diagnostic only. Do not retrain/select a new ranker.

A material delta is one that would have changed a prior selection/gate decision, not merely any numeric movement.

## 7. Work package E — unchanged retrieval rebaseline

Use the corrected retrieval target while keeping retrieval code/config unchanged.

Required frontier:

- universes: 64, 128, 256;
- K: 16, 24, 32;
- BM25;
- current semantic mode;
- predecessor normalized-union/fusion mode(s) sufficient to reproduce the old best point.

For each point report:

- eligible inferable relevant tools;
- recovered inferable tools;
- recall;
- current-step recall;
- explicit-next-step recall;
- misses by tool;
- authority violations.

Also report the predecessor broad-label recall alongside it for traceability.

## 8. Stop conditions

Stop with no downstream unblock if:

- any dev positive is `EvidenceDefect`;
- classification cannot be made deterministic from allowed evidence;
- a derived entry cannot be traced to the frozen corpus;
- retrieval implementation/config changed accidentally;
- authority violations >0.

## 9. Disposition logic

### A — existing retrieval sufficient

All 64/128/256 gates clear at K<=32 with unchanged retrieval.

Record exact mode/K and make retrieval-signal M004 eligible for dependency review.

M001R/M002/M003 remain unnecessary/blocked unless future evidence reopens signal quality.

### B — real inferable retrieval gap

At least one gate still fails because one or more `CurrentStep`/`ExplicitNextStep` tools remain outside K<=32.

Unblock M001R only.

### C — selected ranker semantics materially affected

Corrected relevance would invalidate the prior positive span-packed selection or required ranker gates.

Register a separate ranker-label-semantics corrective; do not proceed to M004/fresh-v4.

### E — evidence failure

No model/retrieval successor is unblocked.

## 10. Regression tests

At minimum:

- all dev positives covered exactly once;
- class/support invariants;
- deterministic asset fingerprint;
- corpus fingerprint mismatch fails;
- historical corpus files unchanged;
- broad versus inferable targets remain distinguishable;
- implicit-future labels excluded from retrieval recall;
- current-step labels included;
- explicit-next-step only included when supported by allowed next-step field;
- unchanged retriever mode identity/fingerprints;
- authority remains zero-violation.

## 11. Verification

```bash
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor
python3 scripts/check_execution_ownership.py
scripts/verify.sh quick
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
git diff --check
```

If the full all-feature Clippy is blocked by unrelated repository state, record the exact unrelated failure and require focused advisor Clippy plus canonical hosted CI before closure.

## 12. Closure evidence

Required closure record:

- `plans/closure/tool-selection-advisor-retrieval-evaluation-semantics-corrective/001-status.md`

It must include:

- implementation/evidence commits;
- derived-view fingerprint;
- class counts by split;
- ranker impact result;
- old versus corrected frontier table;
- A/B/C/E disposition;
- exact downstream plan status changes.

## 13. Acceptance

C001 is complete only when the evaluation target is explicit, deterministic, auditable, frozen-corpus-preserving, and rebaselined with unchanged retrievers.

No model work may begin before that closure.
