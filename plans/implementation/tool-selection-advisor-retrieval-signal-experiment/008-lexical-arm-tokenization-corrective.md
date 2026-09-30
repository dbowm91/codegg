# Tool-Selection Advisor Retrieval-Signal Experiment M008 — Lexical-Arm Tokenization Corrective

Status: closed

Repository baseline: `e62c161e`

Source roadmap: `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md#m008--lexical-arm-tokenization-corrective`

Corrective references:

- M002: `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/002-deterministic-retrieval-signal-v2.md`
- M002 blocked closure: `plans/closure/tool-selection-advisor-retrieval-signal-experiment/002-status.md`
- M007: `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/007-m002-preregistration-reproducibility-corrective.md`
- M007 closure: `plans/closure/tool-selection-advisor-retrieval-signal-experiment/007-status.md`

Primary class: evidence / experimental infrastructure

## 1. Objective

Remove an ambiguity discovered in the M007 receipt before M002 measures dev retrieval: define the exact tokenization for each lexical arm and the BM25F term-frequency accounting.

## 2. Finding

M007 froze generic NFKC/identifier normalization and raw term frequency but did not state whether the flat BM25 arm used that deduplicated tokenizer or the catalog's occurrence-preserving tokenizer. Those interpretations can collapse the flat and normalized arms into the same scorer. M002 has not run, so no outcome was inspected.

## 3. Scope

In scope:

- Specify catalog-compatible tokenization for flat BM25 and BM25F, including Unicode lowercasing, alphanumeric boundaries, and repeated-token handling.
- Specify normalized BM25's NFKC, identifier decomposition, and first-occurrence deduplication separately.
- Freeze BM25F query term frequency, document field term frequency, IDF document frequency, and field-average-length rules.
- Update the typed spec, committed receipt, canonical fingerprint, and tests.
- Update M002/registry/roadmap status only after the receipt test proves the old ambiguity is resolved.

Out of scope:

- Any score computation, dev measurement, variant selection, new parameter, or change to gates/labels.
- Changes to the semantic arms, M006 decision, MiniLM pin, candidate authority, or production catalog scorer.

## 4. Acceptance evidence

1. Receipt spells out distinct, implementation-ready tokenizer contracts for flat BM25, BM25F, and normalized BM25.
2. Receipt specifies BM25F IDF presence across descriptor fields, raw field-local term frequency, weighted query-term frequency, and field-average-length calculation.
3. Regression tests assert the three tokenization paths do not silently alias and verify receipt/fingerprint parity.
4. Run the focused receipt test, `scripts/verify.sh quick`, format, and diff checks; do not run the M002 sweep.
5. Close M008 and restore M002 to ready only after the checks pass. No other plan is unblocked by M008.

## 5. Closure evidence

Create `plans/closure/tool-selection-advisor-retrieval-signal-experiment/008-status.md` with the updated receipt SHA, evidence matrix, verification, and registry disposition.
