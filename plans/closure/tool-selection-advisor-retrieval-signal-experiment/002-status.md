# Tool-Selection Advisor Retrieval-Signal Experiment M002 — Closure Status

Status: closed (negative-but-valid; conditional M003 unblocked)

Source implementation plan:
`plans/implementation/tool-selection-advisor-retrieval-signal-experiment/002-deterministic-retrieval-signal-v2.md`

Source subsystem roadmap:
`plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md#m002--deterministic-retrieval-signal-v2`

Repository baseline reviewed: `eff890ec`
Implementation commit: `eff890ec` — deterministic Signal V2 full frontier

## 1. Executive finding

M002 closes as insufficient deterministic signal (negative-but-valid). The
preregistered 8-mode dev frontier was measured without training against the
corrected inferable target (derived `23edef17…`, dev 53/0/19). Best single
mode `normalized-union-v2` recovers 51/53 (0.9623) at all universes/Ks,
passing u256 (≥0.95) but failing u64 (≥0.99, needs 53/53) and u128 (≥0.98,
needs 52/53). Lexical v2 improves v1 46/53→47/53 (glob rank 63→2 fixed);
semantic-flat-mean (40/53 u64) and field-labelled-mean (34/53) are worse
alone; RRF-K60 collapses to 7/53 at K16 (rank-fusion blur, recovers to
47/53 only at K32 u64). Zero authority violations throughout. Labels are
valid/inferable, persistent misses materially improve (all four historical
misses enter K16), no correctness defect exists. Per §9, conditional M003 is
unblocked; M004/M005 and live work stay blocked. No representation fields or
synonyms were added after measurement.

## 2. Requirement-to-evidence matrix

| Requirement (M002 plan) | Evidence | Result | Notes |
|---|---|---|---|
| Versioned representation without touching v1 (§2) | `RetrievalDescriptorV2`/`RetrievalQueryV2`, `RETRIEVAL_SIGNAL_SCHEMA_VERSION=2` in `retrieval_signal_v2.rs`; v1 baseline reproduced exactly (`lexical_modes_reproduce_v1_baseline_exactly`) | pass | M001R field/cap contract followed; derived fp bound |
| Exact M001R field contract + corrected view fp (§2) | `from_candidate`/`from_advisor_context` use frozen `CANDIDATE_FIELD_CAPS`/`QUERY_FIELD_CAPS`, `normalize_identifier`, `cap_field`; `EXPECTED_DERIVED_VIEW_FINGERPRINT` enforced in both frontiers; prereg fp `eda1f6b6…` bound in receipt | pass | No silent v1 change |
| Field-aware lexical scorer, frozen weights, no tuning (§3) | `LEXICAL_MODES` 4 variants, `LEXICAL_FIELD_WEIGHTS` frozen, `order_field_weighted_v2` per-field IDF; weights untouched post-measurement | pass | Catalog unchanged |
| Semantic variants with frozen MiniLM, no training (§4) | `measure_full_frontier` (encoder-training gate): flat vs field-labelled texts, mean pooling only, `encode_context`/`encode_with_pooling`, descriptor-only cache with 5-factor `SemanticCacheKey`; MiniLM frozen, no projection trained | pass | Toolchain 1.98.1, assets SHA-verified (config `953f9c0d…`, vocab `07eced37…`, weights `53aa5117…`); manifest recreated locally (`a3b06f82…` vs recorded `a74faa95…` due to license file formatting; weights identical) |
| Built-in vs synthetic + schema split (§5) | Offline candidates carry no JSON schema by construction; receipt `schema_present 0 / absent 53`; per-tool recall reported for all 25 tools; synthetic distractors via `expand_universe_local` (universes 64/128/256) | pass | No schema invented; synthetic vs built-in via universe expansion |
| Dev frontier 64/128/256 × 16/24/32, zero violations (§6) | Full receipt `retrieval-signal-m002-frontier.json` fp `f5f76a99…`, 72 points (8×3×3), all `violations==0`; latencies p50/p95/max, descriptor bytes, cache size via descriptor bytes + cache entries (in closure §4) | pass | Aggregate, per-tool, primary/secondary (inferable only), built-in/external (via universe), latencies all reported |
| Persistent-miss closure (§7) | Receipt `persistent_misses`: glob 63→2 (descriptor-flat), lsp_rename 1→0 (semantic-flat), write 1→0 (both files); all `in_k16==true`; no family rendered unreachable (per-tool union 52/53, only `read` 3/4 misses one) | pass | Aggregate gain does not hide family regression |
| Positive path (§8) or negative-but-valid (§9) | Best `normalized-union-v2` 51/53 fails u64/u128 gates; §9 conditions hold (valid labels, material miss improvement, no defect) → negative-but-valid, M003 ready | pass (negative) | No field/synonym expansion post-results |
| Stop conditions (§10) | Live/offline descriptor equivalence holds (same `from_candidate`, offline degrades to empty schema, never invented); schema extraction leaks no defaults/examples (tested); authority surface unchanged (deferred-only); preregistration reproduced (fp match) | pass | No corrective needed |

## 3. Production implementation evidence

- `src/tool_advisor/retrieval_signal_v2.rs` (extended, ~1850 lines):
  representation (`RetrievalDescriptorV2`/`RetrievalQueryV2`,
  `schema_fields_from_parameters`, `operation_terms` generic verb families,
  caps), cache-key contract (`descriptor_fingerprint`,
  `semantic_cache_key` 5 factors), deterministic BM25 (`order_flat_v2`,
  `order_field_weighted_v2`, frozen weights), semantic fusion
  (`cosine_similarity`, `fuse_rrf_v2` K=60, `fuse_normalized_union_v2`
  alpha=0.5), lexical frontier (`measure_lexical_frontier`), full frontier
  (`measure_full_frontier` encoder-gated, 8 modes, descriptor cache,
  latency/bytes), fingerprinting excluding wall-clock, atomic write, 11
  default + 2 encoder-gated tests (representation, schema bounds, query
  fields, generic terms, cache-key 5 factors, v1 parity, determinism,
  committed match lexical-subset, full structure).
- `assets/tool-advisor/retrieval-signal-m002-frontier.json` (full 8-mode,
  protocol `m002-deterministic-signal-v2-frontier-v1`, prereg
  `eda1f6b6…`, derived `23edef17…`, `modes_measured` 8, `modes_deferred`
  [], 72 points, per-tool 25, persistent 4, schema 0/53, fp `f5f76a99…`).
- `src/tool_advisor/mod.rs`: no change needed (module already declared).
- No `ToolCatalog` change; no v1 semantic change; no per-tool alias; v3/v4
  select nothing; MiniLM frozen; no projection trained.

Frontier highlights (K16, violations 0):
- u64: v1 46/53 (0.8679), v2-lexical 47/53 (0.8868) ×3, sem-flat 40/53
  (0.7547), sem-labelled 34/53 (0.6415), rrf 7/53 (0.1321), union 51/53
  (0.9623).
- u128: same lexical 46/47, sem-flat 37/53 (0.6981), labelled 34/53, rrf
  7/53, union 51/53.
- u256: lexical 46/47, sem-flat 36/53 (0.6792), labelled 30/53 (0.5660),
  rrf 7/53, union 51/53 (passes 0.95).
- K24/K32: lexical flat (no K-scaling, same 47/53); sem-flat u64 K32 41/53;
  rrf u64 K24 46/53, K32 47/53 (recovers with K), u128 K32 34/53, u256 K32
  7/53 (stays collapsed); union 51/53 at all Ks (no scaling, already max).
- Per-tool union (any of 8 modes, u64): 24/25 tools 100%, `read` 3/4 (one
  miss); best single (union) 51/53 misses 2 (one `read` + one other,
  non-family-systematic).
- Descriptor bytes (u64): baseline 0, v2 505349; u128 1022732; u256 2062348.
  Semantic p95 ~380–760ms (MiniLM CPU) vs lexical ~2–18ms; first-load ~17s
  (probe), warm query ~215ms, descriptor ~195ms (probe §4).

## 4. Verification executed (commands + results; local vs hosted labeled)

Local (implementation `eff890ec`):
- `cargo test --locked -p codegg --lib -- tool_advisor::retrieval_signal_v2`
  (default): 10 passed, 1 ignored (generator). Covers representation,
  schema bounds, query fields, generic terms, cache-key, v1 parity,
  determinism (36 points), committed lexical-subset match.
- `RUSTUP_TOOLCHAIN=1.98.1 cargo test --locked --features
  tool-advisor-encoder-training -p codegg --lib -- tool_advisor::retrieval_signal_v2`:
  11 passed, 2 ignored. Plus `full_frontier_committed_structure` (8 modes,
  72 points, 53 relevant, 0 violations) green.
- `RUSTUP_TOOLCHAIN=1.98.1 cargo test --locked --features
  tool-advisor-encoder-training -p codegg --lib -- tool_advisor`: 222
  passed, 11 ignored. No new failure in related modules.
- Full frontier generation (ignored, ~310s, toolchain 1.98.1):
  `generate_full_frontier_asset` wrote fp `f5f76a99…`, 8 modes, 72 points,
  per-tool 25, persistent 4, schema 0/53. Determinism: lexical subset
  fingerprint-stable across runs (`lexical_frontier_is_deterministic`);
  semantic determinism via encoder probe (`deterministic_max_delta 0.0`,
  parity 2.3e-06) + committed lexical-subset match.
- `scripts/verify.sh quick`: passed.
- `cargo fmt --all -- --check`, `git diff --check`: clean.
- `RUSTUP_TOOLCHAIN=1.98.1 cargo clippy --workspace --all-targets --locked -- -D warnings`:
  clean; plus `--all-features` clean after doc-list fix (no `retrieval_signal_v2` warning).
- `python3 scripts/check_execution_ownership.py`,
  `bash scripts/check-core-boundary.sh`: passed (no execution surface touched).

Hosted (implementation `eff890ec` on main):
- Run `36782415149` (main push, `verify` job): success, `live_required=true`,
  Clippy 1m51s, prebuild 1.29s+0.49s, build 7m13s
  (`Finished 'test' profile ... in 7m 13s`), exec 337.452s
  (`Summary [337.452s] 11830 tests run: 11830 passed, 5 skipped`), live
  Eggwork included. It carries the production signal; local encoder tests
  carry the semantic signal (routine CI uses default features, encoder tests
  are local-only per plan).

## 5. Invariant review

Preserved: advisor optional/local/default-off/advisory-only; `ResolvedToolSurface`
authority (deferred-only universe, 0 violations); no tool execution by
retriever; span-packed ranker frozen (untouched); historical train/dev/test/
v2/v3 immutable (fingerprints held; v3 diagnostic-only, selects nothing); no
remote/telemetry/download (assets operator-curled, no binary download path);
descriptor cache holds embeddings only, never user context; gates unrelaxed
(0.99/0.98/0.95 enforced); no per-tool alias (generic normalizer only,
`no_per_tool_aliases` upheld).

## 6. Failure and recovery review

No live/offline divergence (same construction; offline empty schema).
No schema leak (defaults/examples/values excluded, tested). No authority
widening. Preregistration reproduced (fp `eda1f6b6…`, derived `23edef17…`,
counts 53/0/19 dev). RRF collapse (7/53) is a measurement finding (rank-fusion
blur on this target), not a correctness defect; union preserves magnitudes
and succeeds best. No checkpoint/resume needed (lexical ~7s, full ~310s
deterministic; cache in-memory per run).

## 7. Migration and compatibility review

Additive experiment code + asset only; no storage/protocol/config migration,
no artifact format change for production, no rollback concern. Frontier
protocol `m002-deterministic-signal-v2-frontier-v1` v1 forward-only; future
representation change needs new version. Encoder assets gitignored
(`target/`), repo owns hashes only.

## 8. Security review

No authorization, secret, network, or privilege surface touched. Receipt
contains fingerprints, counts, field names, recalls, latencies, bytes only —
no prompts, contexts, paths, or credentials. Encoder loader checks all four
hashes before use; fails closed on mismatch.

## 9. Documentation and operations

M003 consumes `ALL_MODES`, `RRF_K`, `UNION_ALPHA`, frozen representation and
`PREREG_ENCODER_MANIFEST` directly; no operator action. Roadmap/registry
updates below are the only planning docs. Architecture docs unchanged (experiment
output, not production contract).

## 10. Unresolved findings

None (no open high/medium/low against M002 scope).

Encoder asset note (not a finding): local `manifest.json` SHA `a3b06f82…`
differs from recorded `a74faa95…` solely due to recreated
`LICENSE.provenance.json` formatting; config/vocab/weights SHAs match upstream
exact-revision (`953f9c0d…`/`07eced37…`/`53aa5117…`); encoder behavior verified
via probe (101/101 load, parity 2.3e-06, mean margin +0.2578). No impact on
measurement validity.

## 11. Roadmap disposition

Negative-but-valid close. M002 (`002-deterministic-retrieval-signal-v2.md`)
closed; conditional M003 (`003-frozen-encoder-retrieval-projection.md`)
becomes ready (frozen MiniLM + ≤500k projection, train-only/dev-selected).
M004/M005, live-primary-model work, order-invariance M005, and v4 stay
blocked (require positive M003→M004→M005 chain). M001 stays blocked/closed
historical; v3 stays diagnostic.

## 12. Registry updates

- `plans/registry.md`: M002 active→closed (implementation `eff890ec`,
  closure this record, receipt fp `f5f76a99…`, best union 51/53 negative);
  M003 blocked/conditional→ready (valid negative M002 satisfies its hard
  dependency); retrieval-signal gate paragraph updated; blocked-work row
  updated.
- `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md`:
  M002 active→closed, M003 blocked/conditional→ready.
- `plans/implementation/.../002-...md`: active→implemented (this closure).
- `plans/implementation/.../003-...md`: blocked/conditional→ready for handoff.
- Unblock audit: M003 is the only plan listing M002 as hard dependency with
  all other deps satisfied (M001R closed, no interface instability); it moves
  to ready. M004 (requires positive M002/M003) stays blocked; M005 (requires
  positive M004) stays blocked; live-primary-model M004 stays blocked (requires
  M005 A + operator/provider prerequisites); no other workstream lists M002.
