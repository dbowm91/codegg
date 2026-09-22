# Tool-Selection Advisor Order-Invariance Experiment M002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-selection-advisor-order-invariance-experiment/002-order-robust-ranker-architectures.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-order-invariance-experiment-roadmap.md#m002--order-robust-ranker-architectures`

Repository baseline reviewed: `722d8e7c874f5ccdb134c8359077004c3f27cd86`

Implementation commits or pull requests:

- `722d8e7c` — order-invariance M002: order-robust ranker architectures

## 1. Executive finding

M002 is complete. Three order-robust ranker variants are implemented
alongside the retained distinct-marker v1 control, all artifact-
versioned and proven against the M001 permutation contract before any
training selection: shared-marker packed, shared-marker span-pooled
packed, and a batched pairwise cross-encoder reference whose
mapped-back scores are invariant within 1e-5 on the real MiniLM
encoder. Packed truncation is a deterministic order-independent
longest-first policy (drops reported, never silent), and the
inference-only cost probe bounds every variant on 4/8/16-candidate
cases. No stop condition triggered. M003 is unblocked.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| A. Distinct-marker v1 retained as control only | `RANKING_ARCHITECTURE_PACKED` path, `packed_encoding`, v1 manifest test; M003 plan forbids selecting it on historical dev metrics | pass | v1 encoding byte-identical; old artifacts load with pre-contract defaults |
| B. Shared-marker packed (one `[unused0]`, positions map identity) | `packed_encoding_with_strategy(..., Shared)`, `shared_marker_uses_one_token_id_for_every_candidate` | pass | Removes marker-token ordinal leakage; absolute positions remain (documented) |
| C. Span-pooled shared-marker packed | `descriptor_spans` in `PackedEncoding`, `packed_span_vectors` (mask-aware mean), `descriptor_spans_map_back_to_correct_candidate` | pass | No candidate-index learned embedding outside BERT |
| D. Batched pairwise cross-encoder, one batch forward | `batch_encode_pairs` (single `DifferentiableBertModel` forward over padded `[B,S]`), `batched_pairwise_scores_are_invariant_under_permutation` (drift <= 1e-5, M001 consistency >= 0.99 on real encoder) | pass | Separate context representation kept for abstention; padding never loses identity |
| Architecture property tests (2/5/16, duplicates, unknown, long, truncation) | Encoder tests on tiny fixture (2/5-candidate, duplicate-like, synthetic, long) + real-encoder 5-candidate invariance + 16-candidate cost probe | pass | 16-candidate property covered by probe + budget tests; full 16-way permutation enumeration is contract-covered by M001 helpers |
| Token-budget semantics (report or reject) | `order_independent_selection`, `strategy_packed_truncation_is_order_independent`, `packed_budget_drops_are_reported_and_deterministic` (same drop set under all tested orders) | pass | Labels never influence truncation; middle drops map names via `candidate_indices` (fixed a take-prefix misalignment during implementation) |
| Artifact versioning (arch/marker/representation/batching/contract/objective) | `RankingArchitecture::contract`, manifest fields with v1-compatible defaults, `new_architectures_are_versioned_distinctly`, `historical_packed_v1_manifest_still_loads` | pass | v1 artifacts load unchanged |
| Runtime isolation (experimental features only) | All changes inside `sequence_encoder`/`sequence_ranking` (feature-gated) + `order_invariance` helpers; no CLI/default-build change | pass | `verify.sh quick` + workspace check green |
| Resource probe (forwards/tokens/p50/p95/RSS/batch) | `probe_architecture_cost` + ignored `architecture_cost_probe_receipt`; `target/tool-advisor/order-invariance/m002-cost-probe.json` | pass | RSS is `None` on macOS (no unsafe-free source; documented); Linux uses procfs VmRSS |

## 3. Production implementation evidence

`src/tool_advisor/sequence_encoder.rs`: `MarkerStrategy`
(Shared/DistinctOrdinal), `PackedEncoding.descriptor_spans` +
`marker_strategy` (serde-defaulted), `order_independent_selection`,
`packed_encoding_with_strategy`, `packed_span_vectors`,
`BatchPairVectors`, `batch_encode_pairs`, lazy ordinal-marker
resolution. `src/tool_advisor/sequence_ranking.rs`: three new arch
ids, `RankingArchitecture` variants + `contract()`, per-arm
`features()` paths with identity-correct name mapping,
`ArchitectureCostProbe` + `probe_architecture_cost`,
manifest contract fields. No training, retrieval, promotion, or
default-surface change.

## 4. Verification executed

### Commands run

```bash
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib tool_advisor::sequence_encoder
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib tool_advisor::sequence_ranking
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib tool_advisor
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib tool_advisor::sequence_ranking::tests::architecture_cost_probe_receipt -- --ignored --nocapture
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

### Results

- Encoder suite: 15 passed, 0 failed (6 new M002 property tests).
- Ranking suite: 6 passed, 0 failed, 1 ignored (4 new M002 tests;
  real-encoder invariance + budget tests ran, not skipped).
- Full `tool_advisor` lib suite: 123 passed, 0 failed, 2 ignored.
- Cost probe (ignored, 98 s): ok. Packed arms ~700/1250/1540 ms at
  4/8/16 candidates (1 forward); batched pairwise ~855/1634/3117 ms
  (1 forward, more tokens). Receipt at
  `target/tool-advisor/order-invariance/m002-cost-probe.json`.
- `cargo fmt --all -- --check`: clean. `git diff --check`: clean.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: clean.
- `scripts/verify.sh quick`: passed.

## 5. Invariant review

- v1 artifact behavior frozen: v1 encoding untouched; old manifests
  deserialize with documented defaults; historical test asset still
  loads (`historical_packed_v1_manifest_still_loads`).
- No production/live surface change: experimental modules only.
- Permutation contract v1 consumed, not altered.
- Authority/candidate identity preserved: middle-drop name mapping
  via `candidate_indices`; batch padding maps rows by descriptor.

## 6. Failure and recovery review

- Empty candidate/batch inputs fail closed; span/count mismatches
  fail closed; out-of-bounds spans fail closed.
- Missing `[unused0]`/`[PAD]` vocab entries fail closed with named
  errors.
- Ordinal markers resolve lazily so shared-marker encodings never
  require the full `[unusedN]` range.
- Probe rejects empty candidate/iteration input.

## 7. Migration and compatibility review

Additive only. `PackedEncoding` gains serde-defaulted fields;
`SequenceArtifactManifest` gains serde-defaulted fields; both old
artifacts load. `RankingArchitecture` gains variants; legacy serde
names unchanged. No storage/protocol/config migration.

## 8. Security review

No authorization, secret, network, or privilege surface touched.
`#![deny(unsafe_code)]` holds (RSS uses procfs on Linux, omitted
elsewhere). No label or relevance data influences truncation.

## 9. Documentation and operations

- Implementation plan `002-...md` status moves to implemented.
- Cost-probe receipt command recorded in the ignored test rustdoc.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | RSS delta unavailable on macOS (unsafe-free) | Cost envelope lacks RSS on one dev target | M003/M005 record Linux RSS where available; no gate impact |
| low | 16-candidate permutation invariance exercised via probe/budget tests, not exhaustive 16! enumeration | Bounded coverage; contract default (20) still applies at selection | M003 dev suite uses contract sampling; no action |

No high/medium/critical findings. No stop condition triggered:
pairwise batching is stable (drift <= 1e-5), packed drops are
order-independent, no sidecar needed.

## 11. Roadmap disposition

Milestone closed and next dependency may proceed: M003 becomes ready.
M004-M005 remain blocked per the roadmap dependency graph. Live
primary-model M004 remains blocked.

## 12. Registry updates

- `plans/registry.md`: order-invariance roadmap row M002 active ->
  M002 closed, M003 ready; M002 plan row active -> closed; M003 plan
  row blocked -> ready; execution-order gate paragraph updated;
  this closure added under recently closed work.
- `plans/subsystems/tool-selection-advisor-order-invariance-experiment-roadmap.md`:
  M002 ready -> closed; M003 blocked on M002 -> ready.
- `plans/implementation/.../002-...md`: active -> implemented.
- `plans/implementation/.../003-...md`: blocked on M002 -> ready for handoff.
