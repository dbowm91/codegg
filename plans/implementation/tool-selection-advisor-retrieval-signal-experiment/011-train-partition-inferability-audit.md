# Tool-Selection Advisor Retrieval-Signal Experiment M011 — Train-Partition Inferability Audit

Status: active

Repository baseline: `5a14384c`

Source roadmap: `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md#m003--frozen-encoder-learned-retrieval-projection`

Long-term requirements: `plans/000-long-term-specification.md`; `plans/002-long-term-roadmap.md`

Applicable ADRs: `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Primary class: evidence/infrastructure

## 1. Objective

Determine which frozen corpus train relevance labels satisfy M003's explicit requirement for M001-approved inferable optimizer inputs, apply M006's current-step-only target without modifying corpus bytes, and freeze the exact train-only optimizer-input partition before any projection training.

## 2. Why this milestone is ready

M001 preregistered the inferability taxonomy and M003's train-only label contract. M006 chose current-step-only and positively re-audited the dev gate-critical misses, but its audit input was limited to those dev occurrences. M003 therefore cannot yet prove that its separate train optimizer input is valid. The gap is bounded and can be resolved from the frozen corpus, partition code, M001 classifier, and M006 target decision without inspecting dev outcomes or training a model.

## 3. Current implementation evidence

- Frozen corpus: `assets/tool-advisor/corpus.jsonl`.
- Deterministic leakage partition: `partition_cases` in `src/tool_advisor/mod.rs`.
- Frozen inferability taxonomy/classifier: `classify_inferability` in `src/tool_advisor/retrieval_signal.rs`.
- Relevance decision: `assets/tool-advisor/retrieval-signal-m006-decision.json` (`current-step-only`).
- M003 requires clean train relevance labels that passed M001 inferability rules; no train-label audit or optimizer-input receipt currently exists.

## 4. Invariants that must not regress

- Corpus bytes, labels, group identities, and partition assignment stay unchanged.
- Only train-partition labels may enter the receipt or later optimizer; dev/test/v2/v3 labels must not be read as training examples.
- Apply the M006 current-step-only target consistently: exclude only labels classified `implicit-secondary`; any `other-evidence-defect` is a hard stop.
- Every retained label has a frozen M001 inferability class, rationale, allowed query-text support, grade, case ID, and tool identity.
- Do not inspect model outcomes, alter the M003 preregistered grid, or fit projections in M011.

## 5. Scope

In:

- derive the clean train partition using the existing deterministic partition function;
- classify each train positive with the frozen M001 classifier and allowed context/descriptor/schema fields;
- exclude current-step-only `implicit-secondary` labels and preserve all inferable labels;
- fail closed on `other-evidence-defect`, missing support, duplicate pair identity, partition drift, or corpus/preregistration fingerprint mismatch;
- write a compact receipt with raw corpus/train fingerprints, counts by class/grade/family, exact included/excluded pairs, and an optimizer-input fingerprint;
- add regressions for train-only membership, deterministic output, exact current-step filtering, and fail-closed defects.

Out:

- changing corpus labels, train/dev/test assignments, M006's product decision, M003's model grid, or retrieval gates;
- using dev/test/v2/v3 examples for optimization, calibration, negative mining, or receipt-derived tuning;
- encoding with MiniLM or fitting any projection.

## 6. Required production changes

Implement a bounded audit/receipt builder in experiment-owned advisor code and commit the compact train-label receipt at `assets/tool-advisor/retrieval-signal-m003-train-audit.json`. The builder must validate partition and corpus fingerprints against repository-owned frozen values. It may expose the exact filtered train cases for M003 only after all checks pass. No runtime catalog or production advisor changes.

## 7. Ordered work packages

1. Reproduce corpus and deterministic train partition fingerprints from frozen assets.
2. Apply `classify_inferability` to each train relevance pair using `AdvisorContextV2` query text, candidate name/description, and only available bounded schema cues.
3. Apply M006 current-step-only filtering; preserve inferable grades and reject other evidence defects.
4. Serialize and fingerprint the exact optimizer pairs and record excluded implicit-secondary rows with reasons.
5. Add regression tests proving no non-train case or test/dev-derived value enters the optimizer view.

## 8. Failure, cancellation, restart, contention semantics

Offline deterministic calculation only; no persistence or concurrency surface. A fingerprint or validation mismatch fails before emitting an optimizer-input receipt. Partial output must not be accepted. This milestone runs once; corrections require a new receipt version and closure audit before M003 resumes.

## 9. Compatibility and migration

No persisted user data, runtime configuration, catalog schema, or protocol changes. The receipt is experimental metadata; the frozen source corpus remains byte-identical.

## 10. Required tests

- train partition fingerprint and exact membership are stable;
- only train-partition cases enter the optimizer view;
- current-step filtering removes exactly the classifier's implicit-secondary rows;
- other evidence defects and missing support fail closed;
- repeated generation is byte-identical and corpus hashes remain unchanged.

## 11. Required verification commands

```bash
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor::retrieval_signal_m003
scripts/verify.sh quick
cargo clippy --workspace --all-targets --features server,plugins,lsp-test-support -- -D warnings
cargo fmt --all -- --check
git diff --check
```

## 12. Documentation updates

Update the M003 handoff, retrieval-signal roadmap, and registry to link the result and state whether M003 is ready or remains blocked.

## 13. Acceptance criteria

The receipt validates the frozen corpus/partition and records every train relevance pair's inferability evidence. No `other-evidence-defect` remains, retained labels are fully supported under the allowed query state, implicit-secondary labels are excluded under M006, and the frozen optimizer-input hash is reproducible. If any criterion fails, M003 remains blocked and the finding is recorded without training.

## 14. Stop conditions

Stop without emitting a positive optimizer-input receipt if the frozen classifier detects an `other-evidence-defect`, allowed query support is missing, the train partition cannot be fingerprinted as expected, or the corpus/preregistration differs from its frozen hash. Do not relabel, add paraphrases, hand-tune exceptions, or use dev/test evidence to rescue the audit.

## 15. Closure evidence required

Record the audit counts, all fingerprints, exact verification commands/results, corpus immutability, and the dependency audit. Positive closure makes M003 ready; negative closure leaves M003 blocked and requires a separately scoped evaluation corrective before training.

## 16. Handoff notes

M006's positive result is limited to dev gate-critical misses and is not a substitute for this train audit. Use the frozen classifier and decision as written; do not infer optimizer label validity from aggregate M002 recall.
