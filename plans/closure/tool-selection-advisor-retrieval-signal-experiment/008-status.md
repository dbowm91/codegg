# Tool-Selection Advisor Retrieval-Signal Experiment M008 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/008-lexical-arm-tokenization-corrective.md`

Source subsystem roadmap: `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md#m008--lexical-arm-tokenization-corrective`

Repository baseline reviewed: `e62c161e`

Implementation commit: `00533709` — close M008 lexical tokenization correction

Receipt: `assets/tool-advisor/retrieval-signal-m002-preregistration.json`

Receipt SHA-256: `4f9e00a367aa0e8568197deb1d877d1151d9437a9dd0b9ef601151f8047e9712`

## 1. Executive finding

M008 closes. It removes the last lexical ambiguity from the M002 preregistration without inspecting or measuring M002 dev outcomes. The corrected typed spec and committed receipt distinguish flat BM25, field-weighted BM25F, and normalized BM25 tokenization and specify BM25F frequency, IDF, and length accounting. M002 is ready.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Flat and field-weighted lexical tokenization | `spec.lexical_arms`: lowercase Unicode, alphanumeric boundaries, repeated-token preservation, no identifier decomposition | Complete |
| Normalized lexical tokenization | `signal_tokens`: NFKC, full lowercase, identifier decomposition, first-occurrence deduplication | Complete |
| BM25F term accounting | `spec.lexical_arms[2].scorer`: weighted query TF, raw field-local document TF, document-presence IDF, per-field average lengths including empty fields | Complete |
| Receipt and fingerprint parity | `m007_preregistration_is_complete_stable_and_outcome_independent` compares the committed JSON receipt with the typed builder and verifies the canonical fingerprint is mutation-sensitive | Complete |
| No outcome inspection | No M002 dev sweep or score computation was run for this corrective | Complete |

## 3. Implementation evidence

The typed preregistration builder and committed receipt were updated in `src/tool_advisor/retrieval_signal.rs` and `assets/tool-advisor/retrieval-signal-m002-preregistration.json`. No catalog scoring, runtime advisor behavior, model assets, labels, denominators, gates, or M006 relevance decisions changed.

## 4. Verification executed

- `cargo test --locked -p codegg --lib tool_advisor::retrieval_signal::tests::m007_preregistration_is_complete_stable_and_outcome_independent` — 1 passed.
- `scripts/verify.sh quick` — passed.
- `cargo fmt --check` — passed as part of quick verification.
- `git diff --check` — passed.
- Corrected receipt SHA-256 — `4f9e00a367aa0e8568197deb1d877d1151d9437a9dd0b9ef601151f8047e9712`.
- M002 dev sweep — not run.

## 5. Invariant, compatibility, and security review

M006's current-step-only label decision, denominator, and gates remain unchanged. The work only changes experiment preregistration metadata and adds no runtime or persistence contract. No secrets, user context, network access, or candidate authority data enter the receipt.

## 6. Unresolved findings

None in M008 scope.

## 7. Roadmap and registry disposition

M008 is closed. M002 is ready with the corrected receipt fingerprint. M003 remains conditional on a valid negative M002 result; M004 and M005 remain gated on positive retrieval outcomes. The dependency audit found no other plan newly unblocked.
