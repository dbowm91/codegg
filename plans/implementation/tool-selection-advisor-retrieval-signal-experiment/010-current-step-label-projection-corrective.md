# Tool-Selection Advisor Retrieval-Signal Experiment M010 — Current-Step Label Projection Corrective

Status: closing — implementation landed; regression and repository verification are under closure review before retrying M002

Repository baseline: `0ff67b4`

Source roadmap:

- `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md#m010--current-step-label-projection-corrective`

Related plans and evidence:

- M002 experiment: `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/002-deterministic-retrieval-signal-v2.md`
- M006 relevance target: `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/006-retrieval-evaluation-corrective.md`
- M006 receipt: `assets/tool-advisor/retrieval-signal-m006-decision.json`
- Failed full sweep: test `m002_preregistered_dev_frontier_sweep`, which stopped at `filesystem-semantic-013-variant-1` because `table_filter` remained in `preferred_order` after removal from `relevance`.

Primary class: experimental infrastructure / evidence integrity

## 1. Objective

Apply M006's current-step exclusion consistently to all derived supervision fields used to construct M002 dev cases, without changing frozen corpus bytes, M006's relevance decision, or any M002 scoring contract. Precompute and validate the complete universe fingerprint before model loading so deterministic input/fingerprint errors cannot consume another multi-hour sweep. Add regression guards for both defects.

## 2. Why this milestone is ready

M006-M009 are closed. The first full M002 sweep ran for 47,110 seconds and stopped at case validation before writing a frontier receipt. The failure is deterministic and localized: `current_step_dev_cases()` removes the three M006-excluded occurrences from `ToolAdvisorCase.relevance`, but does not remove those same names from `preferred_order`. `ToolAdvisorCase::validate` correctly rejects a preferred-order entry without a relevance label.

No retrieval outcomes were produced. The frozen corpus and M006 receipt remain valid and unchanged; only the derived current-step evaluation view needs coherent projection.

## 3. Current implementation evidence

- `src/tool_advisor/retrieval_signal_m002.rs`: `M006_EXCLUDED` and `current_step_dev_cases()` define the in-memory M002 target projection.
- `src/tool_advisor/mod.rs`: `ToolAdvisorCase::validate` requires every `preferred_order` entry to have a corresponding relevance label.
- `universe_fingerprint()` currently uses the persisted-case fingerprint helper, which validates a maximum of 128 candidates; M002's expanded 256-candidate view needs deterministic raw serialization after the dedicated universe validator.
- `assets/tool-advisor/corpus.jsonl`: historical `filesystem-semantic-013-variant-1` retains both `read` and `table_filter` labels and preferred order; these bytes are frozen.
- `assets/tool-advisor/retrieval-signal-m006-decision.json`: excludes exactly `table_filter`, `write`, and `lsp_rename` occurrences under `current-step-only`.

## 4. Invariants that must not regress

- Do not edit, regenerate, or relabel historical corpus, partition, M001, or M006 artifacts.
- Apply exactly the three M006 exclusions to the derived M002 evaluation view; keep all other relevance and ordering entries intact.
- Keep the 69 eligible labels, universes, K values, gates, scoring arms, and M007-M009 fingerprints unchanged.
- Never begin the full sweep unless projected dev cases pass case validation, every expanded universe passes its dedicated size/authority validator, and the complete fingerprint is computed before encoder loading.
- Do not inspect or infer retrieval outcomes from the failed partial execution; it emitted none.

## 5. Scope

### In scope

- Make current-step projection remove each excluded tool from both `relevance` and `preferred_order` for its exact case occurrence.
- Validate projected cases before returning them.
- Compute the complete universe fingerprint before loading MiniLM, using stable serialized case bytes after the expanded-fixture size/authority validator. Retain byte-for-byte fingerprint equivalence for valid cases.
- Add regression tests for all three exclusions, unchanged eligible labels/order, stable denominators, and frozen corpus fingerprints.
- Rerun the bounded feature-gated M002 tests and prove the complete sweep input preflight finishes before model loading. The full model-backed sweep remains M002 work after M010 closure unblocks that plan.

### Explicitly out of scope

- Changing relevance grades, M006's decision, preregistration receipts/fingerprints, tokenizers, scoring, descriptor/query surfaces, gates, candidate universes, or K.
- Editing historical corpus artifacts or changing the underlying benchmark supervision.
- Shortening, sampling, checkpointing, or otherwise changing the full M002 sweep.
- Learned retrieval work or any production advisor behavior.

## 6. Required production changes

### Core and data projection

Update only experiment-owned M002 projection code. For every `(case_id, tool)` in `M006_EXCLUDED`, remove that exact tool from both `case.relevance` and `case.preferred_order`. Preserve relative order among remaining preferred entries. Validate each derived case before returning the split.

### Storage, protocol, runtime, and security

No persistent storage, protocol, production runtime, authority, or security behavior changes.

### Documentation and static guards

Update this plan's closure and M002's status/dependency note after evidence is available. Keep corpus and receipt hashes unchanged.

## 7. Ordered work packages

### Work package A — Coherent current-step projection

Intent: Ensure the M006 target is applied consistently to derived supervision.

Required changes: Filter excluded occurrence names from `relevance` and `preferred_order` in the same exact-case branch; validate all projected dev cases.

Acceptance evidence: All projected cases validate, the three excluded entries are absent from both fields, and every non-excluded entry matches the frozen source order and relevance grade.

### Work package B — Regression and corpus immutability guards

Intent: Prevent the full sweep from spending hours before detecting this class of defect.

Required changes: Add focused tests over all three excluded occurrences, assert the 69-label count and each 64/128/256 expanded-universe denominator, and assert that the original dataset fingerprint is unchanged by projection. Prove deterministic fingerprinting for the 256-candidate view and that it is computed before model loading. The generic case validator caps cases at 128 candidates, so size-256 expansion is checked by its dedicated `validate_expanded_fixture` contract through `expand_universe`.

Acceptance evidence: Focused tests fail against the old projection and pass with the correction; source corpus hash and partition behavior remain unchanged.

### Work package C — Fail-fast sweep input preflight

Intent: Ensure projection, expansion, and fingerprint errors cannot appear after expensive encoder work has started.

Required changes: Compute the complete universe fingerprint before reading/loading the encoder. Verify the focused test exercises 64/128/256 universe expansion and deterministic fingerprinting. Do not change the frozen sweep inputs or protocol. M002 runs the full ignored sweep after this corrective closes.

Acceptance evidence: The bounded projection/fingerprint test passes, and source order proves `universe_fingerprint()` completes before model manifest loading. M002 can safely return to ready status.

## 8. Failure, cancellation, restart, and contention semantics

Projection is deterministic and in-memory. A failed projected-case validation or universe fingerprint must return an error before model loading or frontier work begins. The full test is a single offline process; do not run duplicate sweeps concurrently. A failed attempt produces no accepted M002 evidence and must not be treated as a partial measurement.

## 9. Compatibility and migration

None. Historical artifacts, runtime schemas, and production behavior remain unchanged. The in-memory dev view becomes a mechanically coherent representation of the already-decided current-step target.

## 10. Required tests

### Focused unit tests

- Exact removal of `table_filter`, `write`, and `lsp_rename` from both projected label fields in their named cases.
- Preservation of every non-excluded relevance value and preferred-order position.
- Every projected case validates.
- The dev and expanded universe denominators remain 69.
- Applying the projection does not mutate loaded corpus cases or their dataset fingerprint.

### Integration and security tests

No production integration/security surface changes. Run the full ignored sweep only after the focused gates pass.

## 11. Required verification commands

```bash
rustup run 1.98.1 cargo test --locked -p codegg --features tool-advisor-encoder-training --lib tool_advisor::retrieval_signal_m002::tests
scripts/verify.sh quick
cargo fmt --all -- --check
git diff --check
```

Use the available Rust 1.98.1 toolchain for Candle feature-gated checks on this Darwin host; report the Rust 1.89 NEON compiler limitation truthfully. Do not use `--all-features` for workspace verification.

## 12. Documentation updates

- Create `plans/closure/tool-selection-advisor-retrieval-signal-experiment/010-status.md`.
- Update M002 and M010 status, roadmap, and registry with evidence and dependency audit.
- Record the failed attempt and its duration as failed verification history; do not publish its nonexistent frontier receipt.

## 13. Acceptance criteria

- Current-step projection is coherent and regression-tested for all three M006 exclusions.
- Frozen corpus and preregistration hashes do not change.
- The complete derived-view and universe-fingerprint preflight succeeds without loading the encoder; the M002 full sweep is then unblocked to run under its own plan.
- Registry and roadmap accurately state which future plans are eligible after M010/M002 disposition.

## 14. Stop conditions

Stop and register a separate corrective if any M006 exclusion cannot be mapped to exactly one frozen case/tool occurrence, a non-excluded label changes, a corpus or receipt fingerprint drifts, a full sweep fails before producing a complete receipt, or any edit would change the frozen target, gates, K values, or retrieval arms.

## 15. Closure evidence required

Record the implementation commit, focused and quick verification, corpus/receipt fingerprint checks, fail-fast ordering evidence, unresolved findings, and the registry audit that returns M002 to ready. Record no M002 outcome here; M002 owns the subsequent full sweep and its downstream audit.

## 16. Handoff notes

The M002 sweep is exceptionally expensive on the current host: its failed first attempt ran 47,110 seconds. M010 closes on deterministic projection/fingerprint preflight evidence; only then may M002 restart and run the full sweep. Run the focused tests before starting it and do not interrupt or duplicate a healthy full run. The pinned model files are local ignored assets under `target/tool-advisor/reference-assets/all-minilm-l6-v2/`.
