# Tool-Selection Advisor Qualification Evidence Corrective C001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-selection-advisor-qualification-evidence-corrective/001-semantic-holdout-and-retrieval-gate-correctness.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-qualification-evidence-corrective-addendum.md#c001--semantic-holdout-and-retrieval-gate-correctness`

Repository baseline reviewed: `d24cff92`

Implementation commit:

- `4ef947c` — advisor: implement qualification evidence corrective c001
  (retrieval-identity repair, v3 semantic holdout, v3 validators, v3 CLI
  contract; selected model never evaluated on v3).

## 1. Executive finding

C001 is complete. Both post-v2 defects are repaired and regression-tested:
the retrieval frontier now identifies the candidate-universe size separately
from the shortlist K, and a genuinely new 170-case v3 holdout carries
behaviorally valid slice semantics with zero leakage into the historical
corpus and the observed v2 holdout. The selected MiniLM packed-marker
artifact (`01b5c368…`) was never evaluated on v3 during C001, and no
model, threshold, retrieval-algorithm, or runtime behavior changed. C002 is
ready for its separate preregistration freeze and one final release run.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Retrieval frontier schema correction | `RetrievalFrontierPoint` gains `candidate_universe_size`, `shortlist_k`, `eligible_relevant_tools`, `recovered_relevant_tools` in `src/tool_advisor/sequence_retrieval.rs`; `k` retained only for historical deserialization | pass |
| Universe/K gate identity | `select_retrieval_point()` selects by universe + shortlist K + mode; missing/duplicate points fail closed; `qualify` and `qualify_v2` no longer search `k == 64/128` or default to zero | pass |
| Fixture invariants | `validate_expanded_fixture()` enforces per-case universe size, deferred eligibility, canonical-name uniqueness, relevant-tool preservation, and K-fits-universe; `fixture_cases()` rewritten to preserve every labeled relevant tool instead of truncating a shared pool | pass |
| Known-answer regression | `expanded_v3_fixture_preserves_every_known_relevant_tool` proves all 146 labeled v3 cases keep their relevant tools at universes 64/128 with exact universe sizes; identity selection covered by `retrieval_point_identity_uses_universe_not_shortlist_k` plus missing/duplicate fail-closed tests | pass |
| New v3 holdout | `assets/tool-advisor/qualification-v3-holdout.jsonl` (170 cases), manifest, and `scripts/generate_tool_advisor_qualification_v3_holdout.py` | pass |
| v3 floors | 170 cases; 157 leakage groups; 157 skeletons (max share 1.2%); 13 counterfactual pairs; no-tool 24; hard-negative 28; multi-tool 24; long-session 28; unknown-renamed 24; plugin/MCP 22; LSP 26; research/search 34; structured/data 38; 12 ordinary git/filesystem/shell hard negatives | pass |
| No-tool semantics | `validate_no_tool_semantics()` rejects imperative candidate requests and rationale-free cases; all 24 v3 no-tool cases pass | pass |
| True counterfactuals | `validate_counterfactual_pairs()` requires exact pairs with shared universe, high wording overlap, and flipped top labels; 13 pairs pass | pass |
| Unknown/renamed identity | `validate_unknown_semantics()` requires synthetic renamed relevant identities novel against all prior corpora, with both `tool_xNN` obscured and meaningful new-name variants | pass |
| AdvisorContextV2 shape | `validate_context_v2_semantics()` requires Current-objective/task/stale-topic markers, unresolved-signal variants, and stale-conflict wording in the full slice | pass |
| Hard-negative/multi-tool | Name-absence plus distractor-rationale checks; graded two-tool relevance checks | pass |
| Diversity | Skeleton floor/ceiling, 25% family ceiling, 20% name-hidden floor enforced in `validate_v3_holdout()` | pass |
| Leakage | Zero exact/normalized/template/explicit-family/copied-context overlap against historical corpus plus v2 holdout | pass |
| Construction isolation | Generator has no model/tokenizer/inference/scoring/remote path (unit-asserted); no `sequence-qualification-v3-result.json` exists in the tree or history | pass |
| v3 CLI contract | `sequence-encoder-qualify-v3 --prereg --prereg-commit --json` wired to `qualify_v3()` with preregistration SHA passed separately, never hashed | pass |
| Frozen identity | `FROZEN_V3_SEQUENCE_ARTIFACT_SHA256` enforced by `load_preregistration_v3()` and the `selected_artifact_hash_remains_frozen` test | pass |

## 3. Production implementation evidence

All changes are experiment-gated (`tool-advisor-encoder-training` /
`tool-advisor-encoder-experiment`) and qualification-scoped:

- `src/tool_advisor/sequence_retrieval.rs`: frontier points carry explicit
  universe/K identity; ranking, fusion, caching, and authority filtering are
  byte-for-byte the old algorithm.
- `src/tool_advisor/sequence_qualification.rs`: corrected fixture expansion
  and gate selection; new v3 validators, `qualify_v3()`, and counterfactual
  pair-accuracy scoring; `promotion_evidence()` refactored to explicit
  parameters with identical semantics for v2 and v3.
- `src/main.rs`: additive `SequenceEncoderQualifyV3` command; retrieve
  display now prints universe and K.
- No encoder weights, ranking-head weights, architecture, pooling,
  abstention threshold, linear baseline, historical corpus, v2 holdout, v2
  result, or live/provider behavior changed.

## 4. Verification executed

| Command | Result | Truth |
|---|---|---|
| `cargo test --locked --features tool-advisor-encoder-training -p codegg --lib tool_advisor::sequence_qualification` | 19 passed | local |
| `cargo test --locked --features tool-advisor-encoder-training -p codegg --lib tool_advisor::` | 100 passed | local |
| `cargo test --locked -p codegg --lib -- --test-threads=1` | 4,871 passed | local |
| `cargo test --locked -p codegg --tests -- --test-threads=1` | 8,749 passed; 1 ignored | local |
| `cargo test --locked --features tool-advisor-encoder-training -- --test-threads=1` | 8,816 passed; 3 ignored | local |
| member crates (`codegg-core`, `codegg-config`, `codegg-protocol`, `codegg-git`, `egggit`, `eggsentry`, `eggcontext`, `codegg-providers`, `egglsp`) | 2,923 passed | local |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | pass | local |
| `scripts/verify.sh quick` | pass | local |
| `cargo fmt --all -- --check` | pass | local |
| `git diff --check` | pass | local |

Two timing-sensitive socket/cancellation tests flaked once each under
full-workspace load (`codegg-providers` redirect bound,
`core::eggpool` cancellation) and passed in isolation and in every rerun;
the chunked sweeps above are fully green. The defects are unrelated to this
corrective (no provider/executor code changed).

## 5. Invariant review

- Historical v2 holdout, result, preregistration, and both predecessor
  closures remain untouched and immutable.
- The v3 Python fingerprint (`fafdecb5…`) equals the Rust loader
  fingerprint byte-for-byte; historical and partition fingerprints reproduce
  the v2 preregistration values.
- Slice membership is independent of comparison-arm availability.
- The candidate universe remains deferred-only and authority-filtered.
- No default advisor behavior or live primary-model wiring changed.

## 6. Failure and recovery review

Malformed v3 protocol content, protocol-hash drift, historical partition
drift, holdout fingerprint drift, manifest drift, baseline drift, candidate
identity drift, non-`[64, 128]` universe lists, leakage, semantic-slice
violations, missing slices, floor failures, missing/duplicated retrieval
points, and invalid fixtures all fail closed before any model evaluation.
Resource probes reuse the bounded five-launch helper and mutate no inputs.

## 7. Migration and compatibility review

No storage, protocol, provider, or runtime migration. The v1 and v2
qualification commands remain available; old result artifacts deserialize
with defaulted universe fields that can never match an expanded gate. The
v3 command and validators are feature-gated and additive.

## 8. Security review

The v3 generator is local-only with no network, provider, teacher, model
download, or inference dependency. Promotion evidence flows through the
existing authority-preserving projector; retrieval authority checks are now
explicit (`authority-v3`) alongside the preserved zero-violation boundary.

## 9. Documentation and operations

The v3 schema, manifest (provenance, floors, fingerprints,
no-inference construction contract), and this closure record document the
repair. The C002 plan owns final numeric preregistration, Commit A/Commit B
discipline, the release build sequence, and the one-run receipt.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Hosted CI conclusion is not available in this local closure record | Does not affect local harness correctness; C002's preregistration gate still requires CI when available | Check CI before the C002 final run |
| — | No other C001 findings | — | — |

No high- or medium-severity finding remains.

## 11. Roadmap disposition

C001 is closed positively. C002's only hard dependency is satisfied and
C002 moves to ready: it must still freeze Commit A separately, observe CI,
and execute exactly one release-mode v3 run. Live-primary-model M004
remains blocked unless C002 records disposition A.

## 12. Registry updates

- `plans/registry.md`: subsystem row to `C001 closed; C002 ready`; C001
  implementation-plan row to `closed` with this closure record; C002 row to
  `ready`; qualification-evidence gate paragraph updated; recently-closed
  entry added.
- `plans/subsystems/tool-selection-advisor-qualification-evidence-corrective-addendum.md`:
  C001 `closed`, C002 `ready`.
- `plans/implementation/tool-selection-advisor-qualification-evidence-corrective/002-fresh-v3-preregistered-qualification.md`:
  `blocked on C001` to `ready for handoff`.
- Downstream audit: the only registered plan listing C001 as a hard
  dependency is C002; no other plan is unblocked by this closure. Live M004
  still requires C002 disposition A plus its original prerequisites.
