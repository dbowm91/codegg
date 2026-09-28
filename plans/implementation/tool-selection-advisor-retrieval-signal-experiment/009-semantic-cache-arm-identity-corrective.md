# Tool-Selection Advisor Retrieval-Signal Experiment M009 — Semantic Cache Arm Identity Corrective

Status: closing

Repository baseline: `5288d23`

Source roadmap: `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md#m009--semantic-cache-arm-identity-corrective`

Corrective references:

- M002: `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/002-deterministic-retrieval-signal-v2.md`
- M002 blocked closure: `plans/closure/tool-selection-advisor-retrieval-signal-experiment/002-status.md`
- M008: `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/008-lexical-arm-tokenization-corrective.md`
- M008 closure: `plans/closure/tool-selection-advisor-retrieval-signal-experiment/008-status.md`

Primary class: evidence / experimental infrastructure

## 1. Objective

Prevent semantic descriptor embeddings from being reused across preregistered semantic arms whose text representations differ.

## 2. Finding

Pre-sweep review of M002 found that the cache key froze the descriptor fingerprint and surface but omitted semantic arm identity. The flat and field-labelled descriptor arms can therefore map to the same key and reuse embeddings generated from different text. No M002 frontier measurement has run.

## 3. Scope

In scope:

- Add semantic-arm identity to the typed cache key and M002 receipt contract.
- Update the committed canonical receipt and fingerprint.
- Include arm identity in semantic descriptor cache construction.
- Add a regression test proving two arms cannot alias a cache entry.
- Update M002/registry/roadmap status only after receipt and cache-key tests pass.

Out of scope:

- Running M002 scoring or inspecting dev results.
- Changing semantic text, lexical scoring, labels, denominators, gates, model revision, pooling, or selection rules.

## 4. Acceptance evidence

1. The receipt enumerates `semantic_arm` among descriptor embedding cache key fields.
2. The `RetrievalCacheKeyV2` type and semantic encoder use the arm as part of cache identity.
3. A regression test demonstrates distinct keys for distinct semantic arms with otherwise identical inputs.
4. Run focused cache/receipt tests, `scripts/verify.sh quick`, formatting, and diff checks. Do not run the M002 sweep.
5. Close M009 and restore M002 to ready only after checks pass.

## 5. Closure evidence

Record the updated receipt SHA, focused test results, verification, and dependency audit in `plans/closure/tool-selection-advisor-retrieval-signal-experiment/009-status.md`.
