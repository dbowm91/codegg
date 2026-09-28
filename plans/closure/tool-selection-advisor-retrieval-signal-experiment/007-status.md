# Tool-Selection Advisor Retrieval-Signal Experiment M007 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/007-m002-preregistration-reproducibility-corrective.md`

Source subsystem roadmap: `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md#m007--m002-preregistration-reproducibility-corrective`

Repository baseline reviewed: `0c663820`

Implementation commit: `ff4b5544` — freeze deterministic retrieval signal contract

Receipt: `assets/tool-advisor/retrieval-signal-m002-preregistration.json`

Receipt SHA-256: `798a6e4847f93d5cf5039ba365f1535ced769fa9e328be1b3f902ec37eb1c7c0`

## 1. Executive finding

M007 closes. The exact M002 lexical and semantic arms are now frozen in a versioned receipt with a canonical, recursively key-sorted fingerprint. The specification was made without inspecting M002 dev outcomes. M002 is ready on M006's current-step-only denominators.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Exact four-arm lexical specification | Receipt `spec.lexical_arms`, fixed BM25/BM25F constants, query/descriptor field weights, formula, IDF, tokenization, and ties | Complete |
| Exact three-arm semantic specification | Receipt `spec.semantic_arms` and `spec.pooling_variants`; all 3 × 2 combinations, independent text encoding, 256-token budget, normalized cosine, tie-break | Complete |
| Frozen inputs and relevance denominator | M006 receipt pointer, current-step-only, 69 relevant per universe, required hits 69/68/66, K 16/24/32 and unchanged gates | Complete |
| Model/cache identity | Pinned MiniLM revision `1110a243fdf4706b3f48f1d95db1a4f5529b4d41`; verified local manifest hashes; descriptor-only cache key and explicit surface fingerprint | Complete |
| No post-hoc degrees of freedom | No synonym list, added variants, training, or M002 dev measurement; fingerprint changes on contract mutation | Complete |
| Receipt reproducibility | `m007_preregistration_is_complete_stable_and_outcome_independent` compares generated and committed receipts and checks hash mutation | Complete |

## 3. Production implementation evidence

This is experiment infrastructure only. `src/tool_advisor/retrieval_signal.rs` now exposes the typed M007 spec builder, canonical fingerprint, and receipt builder. No runtime advisor behavior, catalog scoring, model weights, thresholds, corpus, or tool authority changed.

## 4. Verification executed

- `cargo test --locked -p codegg --lib -- tool_advisor::retrieval_signal::tests::m007_preregistration_is_complete_stable_and_outcome_independent` — 1 passed.
- `scripts/verify.sh quick` — passed (formatting, agent schema, architecture guards, workspace check).
- Independent Python SHA check over recursively key-sorted compact JSON — matched receipt fingerprint.
- `git diff --check` — clean.
- The M002 dev sweep was not run, as required by M007.

## 5. Invariant review

- Current-step-only relevance decision, frozen labels, denominators, and gates preserved.
- Descriptor construction remains bounded and value-safe under the M001 schema extraction contract.
- Descriptor cache excludes query/context; query embeddings are not cached.
- No tool-specific aliases or candidate authority changes.
- MiniLM remains a pinned local asset with hash verification and no download path.

## 6. Failure and recovery review

The receipt builder and focused test fail on serialization/spec drift. M002 must consume the frozen receipt and may not select alternate parameters after observing dev results. There is no persistent runtime state or recovery path.

## 7. Migration and compatibility review

Additive experiment receipt and functions only. No schema migration, protocol consumer, configuration, or production runtime behavior changed.

## 8. Security review

No secrets, user context, or network data enter the receipt or descriptor cache key. No authorization or execution surface changed.

## 9. Documentation and operations

Registry and roadmap now mark M007 closed and M002 ready. M002 retains its prior blocked closure as historical evidence. The preregistration receipt is the controlling M002 parameter source.

## 10. Unresolved findings

None in M007 scope.

## 11. Roadmap disposition

M002 is ready and may begin the frozen sweep. M003 remains conditional on a valid negative M002; M004/M005 remain blocked on their stated positive dependencies. The blocked-work audit found no other plans newly unblocked.

## 12. Registry updates

M007 moved from active to closed. M002 moved from blocked to ready because both its relevance target (M006) and scoring contract (M007) are now fixed. No other plan's blocker was cleared by this closure.
