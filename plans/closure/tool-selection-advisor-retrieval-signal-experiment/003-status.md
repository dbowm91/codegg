# Tool-Selection Advisor Retrieval-Signal Experiment M003 — Closure Status

Status: closed (negative; no projection qualifies, experiment stops before v4)

Source implementation plan:
`plans/implementation/tool-selection-advisor-retrieval-signal-experiment/003-frozen-encoder-retrieval-projection.md`

Source subsystem roadmap:
`plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md#m003--frozen-encoder-learned-retrieval-projection`

Repository baseline reviewed: `2df9a5f7`
Implementation commit: `2df9a5f7` — tool-advisor M003 frozen-encoder retrieval projection sweep

## 1. Executive finding

M003 closes negatively with a complete preregistered sweep. All 288
projection arms (3 architectures x 2 LR x 2 epochs x 2 temperatures x 3
seeds x 2 hard-negative counts x 2 batch sizes) trained on train-only
inferable pairs over frozen MiniLM embeddings and were evaluated on the
dev 64/128/256 x K16/24/32 frontier plus the required generalization
slices. Zero arms clear the retrieval gates at K<=32 with zero
violations, and the apparent ties disappear under the generalization
guards, so no retrieval-projection artifact is frozen per §10 stop
conditions.

Headline numbers (dev inferable 53, violations 0 throughout):

- Best projected u64/K16 0.9623 (51/53, ties deterministic union 51/53,
  gate 0.99 needs 53/53).
- Best projected u128/K16 0.9623 (gate 0.98 needs 52/53).
- Best projected u256/K16 0.9057 (48/53, below deterministic union
  0.9623; gate 0.95 needs 51/53).
- Unknown/renamed u64/K16 ~0.40-0.49 versus deterministic canonical
  0.9623 (regresses far beyond the 0.02 guard).
- Name-masked u64/K16 0.66-0.77 versus frozen lexical 0.8302 on the same
  masked universes (projection retains less than the description signal
  alone; fails the §7 diagnostic).
- Verification tool-family slice 0.667 versus lexical 1.0 (delta -0.333,
  fails the 0.03 guard); `coverage` is fully lost under the best arm
  (0/1 at K16/24/32).
- Asymmetric 2-layer MLP collapses (best 0.34 recall) while linear
  variants at best tie the deterministic ceiling.

M004/M005 and all live trajectory work stay blocked. No transformer
fine-tuning was attempted (it would require a new plan per §10); the
projection latency envelope is not a factor (query p95 0.022ms,
descriptor build 6ms).

## 2. Requirement-to-evidence matrix

| Requirement (M003 plan) | Evidence | Result | Notes |
|---|---|---|---|
| Only M001R-preregistered projection families (§2) | `M003_ARCHITECTURES` 3 options; `trainable_params` exact 49,280 / 98,560 / 131,584; `verify_param_caps` + `param_counts_match_preregistration` + `arm_grid_is_complete_and_bounded` | pass | All <=500k; no fourth family |
| Normalized scoring as preregistered (§2) | `project_normalized` (L2) + `cosine_normalized`; `projection_output_is_128_and_normalized`, `cosine_is_bounded_and_self_is_one` | pass | Cosine/dot on normalized vectors |
| Artifact records (§2) | `ProjectionArtifactRecord` (arch/dims/params/MiniLM hashes/schema hash/train fp/objective/temp/weights hash/bytes); receipt `artifact: null` because nothing selected | pass (negative) | Record shape proven by `artifact_record` constructor; no artifact frozen |
| Train partition only, inferable labels, no dev/test/v2/v3 in optimization (§3) | 113 `TrainingPair`s from train inferable only (`train_inferable` gate); no-tool omitted (no auxiliary preregistered); unknown variants omitted (none preregistered); mined map train-only | pass | 113 mined entries == 113 train inferable labels |
| M001-frozen InfoNCE shape: contrastive/in-batch, graded weighting, hard-negative term, no margin (§4) | `train_arm`/`sgd_step` (in-batch + H hard negatives, grade/3 weights, temp scaling); `grade_weights_follow_relevance`; `infonce_training_reduces_loss_on_synthetic_pairs` (10-epoch < 1-epoch); no margin (no value preregistered) | pass | Loss 2.07-4.37 across arms, training converges |
| Hard-negative mining on train only with frozen scorers, frozen before optimizer (§5) | `mine_train_negatives` (BM25 flat-v2 + frozen semantic flat-mean + same-family, pool = train tools, 15-list frozen, H=7 is prefix); `mined_negatives_fingerprint` fp `a92ea16d…`; pairs built from frozen lists only | pass | Never touches dev/test/v2/v3 |
| Dev selection on every arm: 64/128/256 x K16/24/32 + slices + latency/RSS (§6) | 288 `EvaluatedArm`s x 9 points (2592); per-tool/family/grade/persistent/no-tool/unknown/masked slices; `resource_evidence` (artifact/RSS/latency, MiniLM excluded) | pass | Full grid, §6 tiebreak ready (unused on negative) |
| Primary gates 64>=0.99, 128>=0.98, 256>=0.95, K<=32, 0 violations (§6) | Best 0.9623/0.9623/0.9057; 0 arms with `clears_gates`; max violations 0 | fail (negative) | Gates enforced literally; no relaxation |
| Generalization guards: unknown<=0.02, family<=0.03, canonical remap, name-memorization (§7) | Unknown ~0.45 vs 0.96; verification family -0.333; masked 0.75 vs lexical 0.83; persistent 4/4 in K16 but 2 new misses (`coverage` lost, `git_diff` K32-only) | fail (negative) | Guards independently fail even where recall ties |
| Name-masking diagnostic (§7) | `mask_canonical_names` (stable `masked_<sha6>`, descriptions kept) + lexical baseline on identical masked universes | pass (negative) | Projection underperforms lexical on masked signal |
| V3 evaluated at most once diagnostically, no tuning follows (§8) | `v3_diagnostic_once` on frozen best arm: `v3-diagnostic-n146-mrr0.7945-r10.6301-nongating`; no grid/gate/K change after | pass | Native 4-candidate universes; rank quality only |
| Resource contract, MiniLM not double-counted (§9) | `ProjectionResourceEvidence`: best-arm artifact 394,240 B, RSS +544,256 B est, query p50/p95 0.020/0.022ms, descriptor build 5.996ms, cache 293 entries/186,256 B | pass | Latency far inside envelope; not a stop factor |
| Stop conditions (§10) | No arm clears gates; gains vanish under unknown/masked; MLP fails outright; latency fine; no transformer tuning attempted | negative close | Valid negative per plan |

## 3. Production implementation evidence

- `src/tool_advisor/retrieval_projection.rs` (new, ~3180 lines):
  deterministic RNG, `DenseHead`/tower math with seeded Xavier init,
  L2/cosine scoring, graded InfoNCE with deterministic SGD (frozen
  encoder, gradients into heads only), frozen mining (BM25 + semantic +
  same-family, 15-lists, fp-sealed before training), one-time embedding
  precomputation shared across all 288 arms (encoder never runs inside
  the per-arm loop), cached dev/renamed evaluation, slice diagnostics
  (per-tool/family/grade/persistent/no-tool/unknown/masked),
  projection-only resource evidence, one-shot v3 diagnostic, receipt
  types with deterministic fingerprints, 11 fast unit tests + ignored
  `generate_m003_receipt` + committed-structure gates.
- `src/tool_advisor/mod.rs`: `pub mod retrieval_projection` behind
  `tool-advisor-encoder-training` only (default graph unchanged).
- `assets/tool-advisor/retrieval-signal-m003-projection.json` (1.4MB,
  protocol `m003-frozen-encoder-retrieval-projection-v1`, prereg
  `eda1f6b6…`, derived `23edef17…`, train `c67df3ca…`, mined fp
  `a92ea16d…`, 288 arms x 9 points, slices, resources, v3 diagnostic,
  fp `67acca39…`).
- No `ToolCatalog` change; no v1 semantic change; no per-tool alias;
  span-packed ranker untouched; MiniLM frozen (loads only, never
  trains); v3/v4 select nothing.

Per-architecture best (dev u64/u128/u256 at K16):

- asymmetric-linear: 0.9623 / 0.9623 / 0.8868 (ties union at 64/128).
- shared-linear: 0.9245 / 0.9057 / 0.9057.
- asymmetric-2layer: 0.3396 / 0.3208 / 0.2453 (collapse).

## 4. Verification executed (commands + results; local vs CI truthfully)

Local (implementation `2df9a5f7`, toolchain 1.98.1 for encoder work):

- `cargo test --locked --features tool-advisor-encoder-training -p codegg
  --lib -- tool_advisor::retrieval_projection`: 11 passed, 1 ignored
  (generator). Covers param caps, grid completeness, init determinism,
  128-dim normalization, cosine bounds, grade weights, InfoNCE
  improvement, fingerprint determinism, committed field-wise contract.
- Full sweep generator (ignored, release profile, ~292s):
  `cargo test --release --locked --features
  tool-advisor-encoder-training -p codegg --lib --
  tool_advisor::retrieval_projection::tests::generate_m003_receipt --
  --ignored --nocapture` wrote fp `67acca39…`, 288/288 arms, 0 clears.
  Release profile was chosen for the one-time sweep only (identical
  deterministic algorithm; debug-profile training is ~10x slower and
  would exceed the wall-clock budget). Committed structure tests run in
  the normal suite in both profiles.
- `cargo test --locked --features tool-advisor-encoder-training -p
  codegg --lib -- tool_advisor`: 233 passed, 12 ignored. No new failure
  in related modules.
- `scripts/verify.sh quick`: passed (agents, core-boundary, sandbox,
  execution-ownership, tui-authority, http-route, audit,
  scheduler-bypass, eggwork target-routing, workspace check).
- `cargo fmt --all -- --check`, `git diff --check`: clean.
- `RUSTUP_TOOLCHAIN=1.98.1 cargo clippy --workspace --all-targets
  --all-features -- -D warnings`: clean.
- `python3 scripts/check_execution_ownership.py`,
  `bash scripts/check-core-boundary.sh` (via verify.sh): passed (no
  execution surface touched; advisor-only module).

Hosted: no hosted CI run is claimed by this closure; the next main-push
`verify` run will carry the canonical hosted signal. Local
encoder-training tests carry the semantic signal (routine CI uses
default features; encoder tests are local-only per plan, as in M002).

Float-seal note (not a finding): the stored receipt fingerprint
`67acca39…` is the generation-time seal. Committed tests verify the
receipt field-wise (M002 pattern) rather than recomputing the full hash
because one JSON float repr (21/53) does not round-trip bit-identically
through serde_json parse/serialize in this toolchain (1-ulp shift
`...3965` -> `...397` observed; std `str::parse` is exact, serde_json's
decimal path shifts). Generation-time hashing is single-process
deterministic; the file seal stands as written.

## 5. Invariant review

Preserved: advisor optional/local/default-off/advisory-only;
`ResolvedToolSurface` authority (deferred-only universes, 0 violations
in all 2592 points plus renamed/no-tool slices); no tool execution by
retriever or projection; span-packed ranker frozen (untouched);
historical train/dev/test/v2/v3 immutable (corpus/dev/derived/train
fingerprints held; v3 diagnostic-only, selects nothing); no
remote/telemetry/download (operator-acquired MiniLM assets only);
descriptor caches hold frozen embeddings/projections only, never user
context beyond the scored query; gates unrelaxed (0.99/0.98/0.95
enforced); no per-tool alias (generic normalizer + masking only);
no-tool abstention left to the ranker (no auxiliary trained).

## 6. Failure and recovery review

Training converges (loss falls with epochs on synthetic and real pairs)
but does not generalize: the best linear arm merely ties the
deterministic ceiling while trading misses (`coverage` lost entirely,
`git_diff` degrades to K32-only), and the MLP collapses. Unknown and
name-masked diagnostics show the learned heads memorize canonical-name
signal rather than description signal (unknown 0.45, masked below
lexical). No live/offline divergence (same Signal V2 construction;
offline empty schema, never invented). No authority widening. No
checkpoint/resume needed (precomputation ~1 min, sweep 292s release,
deterministic; reruns reproduce the verdict).

## 7. Migration and compatibility review

Additive experiment code + asset only; no storage/protocol/config
migration, no artifact format change for production (no artifact
frozen), no rollback concern. Receipt protocol
`m003-frozen-encoder-retrieval-projection-v1` is forward-only; any
future representation change needs a new version. Encoder assets stay
gitignored (`target/`); the repo owns hashes only.

## 8. Security review

No authorization, secret, network, or privilege surface touched.
Receipt contains fingerprints, counts, recalls, latencies, bytes, and
tool/case ids only — no prompts, contexts, paths, or credentials.
Encoder loader checks hashes before use; fails closed on mismatch.

## 9. Documentation and operations

No operator action. M004/M005 remain blocked, so no downstream consumer
exists. Roadmap/registry updates below are the only planning docs.
Architecture docs unchanged (experiment output, not production
contract).

## 10. Unresolved findings

None (no open high/medium/low against M003 scope).

Interpretive note (not a finding): the deterministic union ceiling
(51/53) plus projection memorization (unknown collapse, masked
regression, MLP failure) jointly suggest the remaining misses are a
representation-coverage problem rather than an alignment problem solvable
by a <=500k head over frozen MiniLM. Transformer fine-tuning was
explicitly out of scope and would require a new plan; this closure makes
no claim about it.

## 11. Roadmap disposition

Negative close. M003
(`003-frozen-encoder-retrieval-projection.md`) is closed; no projection
artifact is frozen and none may be consumed by M004. M004 (requires
positive M002 or M003) stays blocked; M005 (requires positive M004)
stays blocked; live-primary-model work stays blocked (requires M005 A +
operator/provider prerequisites); order-invariance M005 stays blocked as
historical evidence. No v4 plan is registered by this close. M001 stays
blocked/closed historical; v3 stays diagnostic.

## 12. Registry updates

- `plans/registry.md`: M003 ready→closed (implementation `2df9a5f7`,
  closure this record, receipt fp `67acca39…`, 0/288 clear, negative);
  M004/M005 rows unchanged (still blocked); retrieval-signal gate
  paragraph updated; blocked-work row updated.
- `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md`:
  Status active with M003 closed (negative); M004/M005 remain blocked.
- `plans/implementation/.../003-...md`: ready→implemented (this closure).
- Unblock audit: M004 lists positive M002/M003 as hard dependencies;
  M002 closed negative-but-valid and M003 closes negative, so M004 stays
  blocked. M005 requires positive M004, so it stays blocked. No other
  registered plan lists M003 as a hard or interface dependency, so
  nothing else is unblocked. No corrective pass is registered (the
  negative result is the planned terminal outcome for this arm of the
  experiment, not a defect).
