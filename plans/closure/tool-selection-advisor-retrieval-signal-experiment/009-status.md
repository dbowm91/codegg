# Tool-Selection Advisor Retrieval-Signal Experiment M009 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/009-semantic-cache-arm-identity-corrective.md`

Source subsystem roadmap: `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md#m009--semantic-cache-arm-identity-corrective`

Repository baseline reviewed: `5288d23`

Implementation commit: `7a9e88fc` — isolate semantic retrieval cache arms

Receipt: `assets/tool-advisor/retrieval-signal-m002-preregistration.json`

Receipt SHA-256: `4e90a05434fc839ba5f0191b590bf80c2bf796f0b0bfb9aed21b0a939ceff36e`

## 1. Executive finding

M009 closes. Semantic arm identity is now frozen in the M002 receipt and is part of every descriptor embedding cache key. Flat and field-labelled descriptor embeddings cannot alias. No M002 frontier measurement occurred during this corrective.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Receipt specifies semantic-arm cache identity | `spec.cache_key_fields` includes `semantic_arm`; receipt SHA matches canonical recursively sorted JSON | Complete |
| Runtime cache keys include semantic arm | `RetrievalCacheKeyV2.semantic_arm`; `semantic_rank` sets it from the selected arm | Complete |
| Distinct arms cannot hit one another's descriptor cache entries | `semantic_cache_identity_distinguishes_descriptor_arms` inserts a flat-arm key and confirms the field-labelled key misses | Complete |
| Committed receipt matches typed spec and fingerprint | `m007_preregistration_is_complete_stable_and_outcome_independent` | Complete |
| No M002 result inspection | Frontier sweep remained ignored and was not run | Complete |

## 3. Verification executed

- `rustup run 1.98.1 cargo test --locked -p codegg --lib tool_advisor::retrieval_signal::tests::m007_preregistration_is_complete_stable_and_outcome_independent` — 1 passed.
- `rustup run 1.98.1 cargo test --locked -p codegg --features tool-advisor-encoder-training --lib tool_advisor::retrieval_signal_m002::tests::semantic_cache_identity_distinguishes_descriptor_arms` — 1 passed.
- `scripts/verify.sh quick` — passed.
- `cargo fmt --check --all` (inside quick verification) — passed.
- `git diff --check` — passed.
- M002 frontier sweep — not run.

Rust 1.89 cannot compile Candle's Apple NEON `float16x8_t` path in this local toolchain; the focused feature-gated test ran successfully on installed Rust 1.98.1. This is a local toolchain limitation, not a test failure in the M009 change.

## 4. Invariant and compatibility review

Only experiment cache identity and its preregistration changed. Lexical scoring, semantic text, model revision, pooling, labels, denominators, gates, catalog behavior, and runtime advisor behavior remain unchanged. The cache still stores descriptor embeddings only and excludes query context.

## 5. Unresolved findings

None in M009 scope.

## 6. Roadmap and registry disposition

M009 is closed. M002 is ready with receipt fingerprint `4e90a05434fc839ba5f0191b590bf80c2bf796f0b0bfb9aed21b0a939ceff36e`. M003 remains conditional on a valid negative M002 result; M004/M005 remain gated on positive retrieval outcomes. The dependency audit found no other plan newly unblocked.
