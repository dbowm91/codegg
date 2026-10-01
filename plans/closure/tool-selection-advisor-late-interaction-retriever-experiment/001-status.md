# Tool-Selection Advisor Late-Interaction Retriever M001 — Closure Status

Status: closed (negative — disposition D, no architectural signal)

Source implementation plan:
`plans/implementation/tool-selection-advisor-late-interaction-retriever-experiment/001-frozen-exact-maxsim-baseline.md`

Source subsystem roadmap:
`plans/subsystems/tool-selection-advisor-late-interaction-retriever-experiment-roadmap.md#m001--frozen-exact-late-interaction-baseline`

Repository baseline reviewed: `87a7da18`

Implementation commits:
`f169b4f1` — token API + frozen preregistration (Commit A, no dev measurement);
`87a7da18` — one frozen dev frontier run (Commit B).

## 1. Executive finding

The late-interaction hypothesis fails at the frozen baseline with zero
learned parameters: exact token-level MaxSim over pinned MiniLM recovers
29/53 inferable labels at u64/K16 (0.547), far below the B-gate floor of
51/53 and below the frozen lexical baseline (47/53). The lexical+MaxSim
union ties lexical exactly (47/53) — token interaction contributes nothing
beyond lexical matching. The two M002 residual misses rank 63–64/64 under
MaxSim (dead last). Per the preregistered disposition rules this is
unambiguously **D — no architectural signal**: no token projections are
trained (M002 stays closed), no encoder tuning occurs (M003 stays closed),
and the workstream terminates without consuming v4.

## 2. Requirement-to-evidence matrix

| Plan requirement | Evidence |
|---|---|
| Token-level API, same weights/tokenizer, no CLS/SEP/pad, aligned ids, deterministic | `CandleBertSequenceEncoder::encode_token_sequence` (`sequence_encoder.rs`); 4 tiny-fixture tests (alignment, tail truncation, fail-closed empty, forward-row parity); all pass |
| Two-commit discipline (caps/scoring frozen before dev) | Commit A `f169b4f1` (code + prereg `04bea8a8…`); Commit B `87a7da18` (receipt only); no cap/scoring change between |
| Caps ≤128 from train-only/static evidence | Query 96 (train n=132, p100=65); descriptor 96 (static n=114688 occ., p100=39); post-hoc `queries_truncated=0`, `descriptors_truncated=0`, max counts 74/48 |
| Exact modes only (no sweeps) | `exact-maxsim-v1` + fixed `lexical-maxsim-union-v1` (alpha 0.5); receipt `modes` matches prereg |
| Dev frontier u64/u128/u256 × K16/24/32, inferable-only, zero widening | 18 points in receipt; derived view fp `23edef17…` checked at runtime; violations=0 everywhere |
| Slices | Per-tool table; 2 M002 residuals ranked; current/explicit split; unknown/renamed + masked + lexical baselines; family table; schema 0/53 present (frozen cases carry no schema); token distributions |
| Resources at u256 | Cache 20,388,060 B @369 entries (≤64 MiB ✓); warm p95 86.8 ms (≤1.0 s ✓); exactly 1 query forward/turn; cold load 367 s (release test, weights load + first forward) |
| A/B/D disposition | D (below; only A opens M004, only B opens M002) |

## 3. Production implementation evidence

- `src/tool_advisor/sequence_encoder.rs`: `TokenEmbeddingSequence` +
  `encode_token_sequence` (single `[CLS]/[SEP]` forward, tail truncation,
  empty→empty fail-closed, `IndexOp` row extraction).
- `src/tool_advisor/late_interaction.rs` (new, experiment-gated):
  L2/maxsim/order/fusion math with hand-worked unit tests;
  descriptor-only cache (contract-keyed, byte-accounted, never query
  data); frozen prereg loader with fingerprint + contract drift checks;
  one-shot frontier measurement; ignored receipt generator.
- `assets/tool-advisor/late-interaction-m001-preregistration.json`
  (fp `04bea8a8…`): caps, truncation, epsilon, aggregation, tiebreak,
  modes, cache version, limits, gates, corpus/view/Signal-V2/MiniLM
  bindings, cap evidence.
- `assets/tool-advisor/late-interaction-m001-frontier.json`
  (fp `755d555d…`): 18 points, 53 label ranks, slices, resources, D.

## 4. Verification executed (commands + results; label local vs CI truthfully)

Local (darwin aarch64; measurement toolchain 1.98.1 — the 1.89 floor
cannot build candle's `stdarch_neon_f16` use, as in M003; default
workspace builds are unaffected):

- `cargo +1.98.1 test --locked --features tool-advisor-encoder-training
  -p codegg --lib -- tool_advisor::late_interaction`: 6 passed
  (prereg binding, 4 scoring/cache unit tests, mask determinism).
- `cargo +1.98.1 test ... sequence_encoder`: 19 passed (incl. 4 new
  token-sequence tests).
- Ignored `generate_m001_receipt` run once in release (372 s): exit 0,
  receipt written, fingerprint sealed at generation time.
- `cargo +1.98.1 clippy --locked --features
  tool-advisor-encoder-training -p codegg --all-targets -- -D warnings`:
  exit 0. `cargo fmt --all -- --check`: clean.
- Default-feature guards (`scripts/verify.sh quick`, 1.89 workspace
  Clippy, hosted `CI / verify` run `36912603806` success) cover the
  non-encoder surface; encoder-feature tests are out of hosted scope by
  existing convention.

Harness validity (not tuning): the attribution-only reproduction of the
frozen M002 best mode with frozen M002 arithmetic matches the frozen M002
receipt exactly (51/53 at u64/K16 → `m002_attribution_reproduces_receipt:
true`), proving encoder load, texts, universes, eligibility, and ranking
are all correct. The MaxSim path shares all of it.

## 5. Invariant review

`ResolvedToolSurface` owns authority (violations checked, all zero);
advisor stays optional/default-off/local-only; corpora and derived view
immutable (fingerprints bound); span-packed ranker untouched; no model
download (operator-acquired pinned rev `1110a24…`, SHAs match the
provenance receipt); no ANN/index; no aliases; no training occurred.

## 6. Failure and recovery review

Failure mode considered: a MaxSim implementation bug masquerading as a
negative. Ruled out by (a) hand-worked unit fixtures (mean-of-maxima,
order invariance, tie-break), (b) forward-row parity test against
`forward_ids`, (c) exact M002 reproduction through the same harness, and
(d) the union mode behaving exactly as theory predicts when one arm is
pure noise (ties lexical). The negative is real: frozen MiniLM
token-level cosine genuinely anti-correlates with inferable tool
relevance here (filesystem family 0.0 vs lexical 0.86; both residuals at
rank ≥63).

## 7. Migration and compatibility review

None: additive experiment module + additive assets. Default builds do not
link the encoder path. No storage, protocol, or catalog change.

## 8. Security review

None applicable: no authority, execution, network, or persistence change.
The descriptor cache stores static tool metadata vectors only; probe and
receipt contain no query/context text beyond frozen corpus excerpts
already committed in prior milestones.

## 9. Documentation and operations

Contract, prereg, and receipt are machine-readable under
`assets/tool-advisor/`. Cold load (367 s in the release test process,
dominated by weights load) and per-turn costs are recorded; no operating
point is frozen, so no runbook changes follow.

## 10. Unresolved findings (severity: critical/high/medium/low)

None blocking closure. Observations for any future separate workstream
(not this line): unknown/renamed (0.925) and name-masked (0.943) MaxSim
slices beat lexical (0.830) even while aggregate recall collapses —
token-level matching preserves name-robustness but destroys ranking;
any successor must explain the 29/53 aggregate before claiming the
slice deltas. Explicit-next-step recall is 1.0 on 0 eligible dev labels
(vacuous — the frozen view has no eligible explicit-next-step dev
labels); the current-step split (29/53) carries the verdict.

## 11. Roadmap disposition

The late-interaction workstream is **terminally negative** at M001:

- M001: closed (D). No architectural signal; training forbidden by the
  preregistered rules.
- M002 (token projection training): never opens (requires M001 B).
- M003 (top-layer tuning): never opens (requires M002 B).
- M004 (operating point): never opens (requires M001/M002/M003 A).
- M005 (fresh v4): never opens (requires positive M004).
- Live-primary-model trajectory: unchanged (still blocked; this
  workstream contributes a negative data point, not a candidate).

## 12. Registry updates

- Late-interaction experiment rows move to closed-negative with this
  record (M001 D; M002–M005 never opened, recorded as terminally blocked,
  not silently dropped).
- Blocked-work audit: no registered plan lists late-interaction M001 as a
  hard/interface dependency (its dependents M002–M005 are intra-workstream
  conditionals, resolved to never-open above). Retrieval-signal C001's
  closure already unblocked M001 to run; nothing further is unblocked by
  this D — that is the correct terminal outcome, and no corrective pass
  is registered because no defect was found (a negative result is valid
  closure).
