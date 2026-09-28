# Tool-Selection Advisor Retrieval-Signal Experiment M007 — M002 Preregistration Reproducibility Corrective

Status: implemented — receipt `assets/tool-advisor/retrieval-signal-m002-preregistration.json` generated; focused reproducibility test and `scripts/verify.sh quick` passed; closure record follows

Repository baseline: `0c663820`

Source roadmap: `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md#m007--m002-preregistration-reproducibility-corrective`

Related plans and evidence:

- M001: `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/001-signal-sufficiency-audit-and-preregistration.md`
- M001 closure: `plans/closure/tool-selection-advisor-retrieval-signal-experiment/001-status.md`
- M002: `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/002-deterministic-retrieval-signal-v2.md`
- M002 blocked closure: `plans/closure/tool-selection-advisor-retrieval-signal-experiment/002-status.md`
- M006 decision: `plans/closure/tool-selection-advisor-retrieval-signal-experiment/006-status.md`

Primary class: evidence / experimental infrastructure

## 1. Objective

Make every M002 deterministic lexical and semantic arm reproducible from a frozen, reviewable contract before any frontier measurements are run.

## 2. Why this milestone is ready

M002's stop condition is confirmed: M001 freezes field order, byte caps, variant names, and generic normalization categories, but not the exact query/descriptor weighting strategy, numeric field weights, or all semantic encoding/pooling details. M006 resolves the relevance-target question and unblocks M002 only once this separate reproducibility gap is closed.

## 3. Scope

In scope:

- Audit each M002 lexical and semantic arm against M001's receipt and implementation plan.
- Freeze exact query and descriptor fields, field weights, tokenization/normalization behavior, scorer parameters, and semantic input construction for every preregistered arm.
- Specify pooling behavior and cache identity for each encoder-backed variant.
- Record a versioned preregistration receipt and fingerprint, with no dev-result-dependent choices.
- Update M002 and registry/roadmap dependency status only after a reproducibility audit proves each arm can be reconstructed from the receipt.

Out of scope:

- Running retrieval measurements or inspecting M002 dev outcomes.
- Changing the current-step-only target, M006 receipt, frozen labels, corpus, gates, K values, or M001 historic receipt.
- Adding synonyms, tool-specific aliases, new variants, training, learned weights, or production advisor behavior.

## 4. Required changes and acceptance evidence

1. Produce a field-by-field specification for all four lexical variants and three semantic variants, including fixed parameters and deterministic tie-breaking.
2. Verify the specification introduces no tool-specific aliases and uses only the frozen current-step query fields and static candidate metadata/schema.
3. Define the exact encoder/tokenizer asset identity and pooling semantics; descriptor cache keys must include all representation and surface inputs and must exclude query/context data.
4. Add a preregistration receipt and deterministic fingerprint. A change to any variant input or parameter must change the fingerprint.
5. Demonstrate that another implementation can reconstruct each arm from the receipt alone. If a choice requires inspecting dev outcomes, stop and amend the plan before measurement.
6. After all arms are reproducible, set M002 to `ready`, set M007 to `closed`, and record the dependency audit. M003 remains conditional on the M002 result; M004/M005 remain blocked.

## 5. Verification

Run focused tests for serialization/fingerprint stability and field/query/cache boundaries, `scripts/verify.sh quick`, formatting, and diff checks. Do not run the M002 sweep in M007.

## 6. Closure evidence

Create `plans/closure/tool-selection-advisor-retrieval-signal-experiment/007-status.md` with the exact receipt hash, reproduction matrix, verification results, and registry/roadmap disposition.
