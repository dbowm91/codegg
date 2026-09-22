# Tool-Selection Advisor Sequence Qualification Corrective C001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-selection-advisor-sequence-qualification-corrective/001-qualification-harness-and-fresh-holdout.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-sequence-qualification-corrective-addendum.md#c001--qualification-harness-completeness-and-fresh-holdout`

Repository baseline reviewed: `323e3285`

Implementation commit:

- `621032e` — advisor: implement sequence qualification v2 harness

## 1. Executive finding

C001 is complete. The qualification boundary now has a v2 protocol model,
explicit case-membership slices, raw and artifact-threshold calibration
evidence, projector-backed promotion simulation, release resource reporting,
and deterministic A/B/C/D/E disposition semantics. The repository-owned
holdout contains 128 locally authored cases and is not loaded by the selected
sequence model anywhere in the construction path. C002 is ready for its
separate preregistration freeze; no final selected-model holdout result exists
in this C001 commit.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Frozen candidate identity | `FrozenSequenceCandidate` and v2 identity checks in `src/tool_advisor/sequence_qualification.rs`; historical selected artifact hash remains `01b5c368…` | pass |
| Explicit independent slices | `QualificationSlice`, tag/family membership, and per-slice `SliceEvidence` | pass |
| Baseline arms | Aggregate and every declared slice emit keyword, BM25, frozen linear, selected sequence, and no-advisor metrics | pass |
| Calibration | Raw probabilities, threshold-calibrated decisions, Brier/ECE/NLL, and no-tool precision/recall/F1 are emitted | pass |
| Promotion simulation | `project_discovery` is exercised over deferred-only candidates with preregistered threshold, promotion cap, and schema budget | pass |
| A/B/C/D/E disposition | Pure `disposition_for` logic plus unit coverage for all five outcomes | pass |
| Fresh holdout | `assets/tool-advisor/qualification-v2-holdout.jsonl`, 128 cases, 128 content-derived leakage groups, manifest and deterministic generator | pass |
| Historical leakage | C001 validation compares exact, normalized, template-lineage, semantic, and explicit leakage families | pass |
| Release resource path | v2 resource probe performs five independent process launches and records binary, RSS, load, rank, retrieval, cache, and forward evidence | pass |
| Construction isolation | Generator is local-only and has no model/tokenizer/inference path; regression test asserts this | pass |

## 3. Production implementation evidence

The new code remains experiment-gated under `tool-advisor-encoder-training`.
It does not alter model weights, tokenizer, pooling, the selected MiniLM
artifact, live disclosure, permissions, execution, providers, or telemetry.
Promotion qualification delegates to the established `project_discovery`
authority boundary, so the offline simulation cannot widen the authorized
deferred universe.

## 4. Verification executed

| Command | Result | Truth |
|---|---|---|
| `cargo fmt --all -- --check` | pass | local |
| `cargo check --locked --features tool-advisor-encoder-training` | pass | local |
| `cargo test --locked --features tool-advisor-encoder-training -p codegg --lib tool_advisor::sequence_qualification` | 6 passed | local |
| `git diff --check` | pass | local |
| `cargo build --release --locked --features tool-advisor-encoder-training --bin codegg` | required release evidence; final result recorded with the C002 run | local |

The first broader package-filtered test attempt was not a source failure: the
macOS linker crashed while linking an unrelated large integration-test binary
(`__eh_frame section too large`, clang segmentation fault). The narrow library
target completed successfully and is the focused C001 evidence; the broader
workspace/quick verification remains a release qualification prerequisite.

## 5. Invariant review

- Historical M005 inputs, result, preregistration, and closure remain untouched.
- The holdout is not used for tuning and C001 never loads the sequence artifact
  during holdout construction or leakage validation.
- Slice membership is independent of comparison-arm availability.
- The selected artifact identity is checked before evaluation in v2.
- The candidate universe remains deferred-only and authority-filtered.
- No default advisor behavior or live primary-model wiring changed.

## 6. Failure and recovery review

Malformed v2 protocol content, protocol-hash drift, historical partition drift,
holdout fingerprint drift, manifest drift, baseline drift, candidate identity
drift, leakage, missing slices, and floor failures fail closed before final
qualification output. Process resource probes are bounded to five child
launches and do not mutate model or holdout inputs.

## 7. Migration and compatibility review

C001 adds no storage, protocol, provider, or runtime migration. The existing v1
qualification command remains available for immutable historical evidence. The
v2 command and resource probe are feature-gated and additive.

## 8. Security review

The generator is local-only and contains no network, provider, teacher, model
download, or inference dependency. Promotion evidence is produced through the
existing authority-preserving projector, and the report records zero authority
violations by construction of that boundary.

## 9. Documentation and operations

The v2 schema and holdout manifest document provenance, floors, fingerprints,
and the no-inference construction contract. The C002 plan owns the final
numeric preregistration, release build sequence, one-run discipline, and final
result receipt.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Hosted CI conclusion for the C001 implementation is not available in this local closure record | Does not affect local harness correctness; repository policy still requires the final C002 preregistration CI check when available | Check CI before the C002 final run |
| — | No other C001 findings | — | — |

No high- or medium-severity finding remains. The low operational item is owned
by C002's preregistration/CI gate and does not prevent C001 closure.

## 11. Roadmap disposition

C001 is closed positively. C002's only hard dependency, C001 accepted closure,
is now satisfied. The live-primary-model M004 remains blocked because C002 has
not yet produced disposition A and its original prerequisites remain separate.

## 12. Registry updates

- C001 moves from `ready`/active execution to `closed` with this record.
- C002 moves from `blocked on C001` to `ready`; its separate preregistration
  freeze and CI gate remain mandatory.
- No other registered plan lists C001 as a hard or interface dependency.
- No corrective pass is registered: all C001 acceptance requirements are met;
  C002 is the planned final qualification, not a defect repair to C001.
