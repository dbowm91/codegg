# Tool-Selection Advisor Sequence-Encoder Experiment M001A — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/tool-selection-advisor-sequence-encoder-experiment/007-reference-checkpoint-materialization-and-qualification.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-sequence-encoder-experiment-roadmap.md#m001a--reference-checkpoint-materialization-and-qualification`

Repository baseline reviewed: `0dbbd359`

Implementation commits:

- `ab3e9dfd` — advisor: qualify MiniLM reference checkpoint and probe staged scoping

## 1. Executive finding

M001A is conditionally closed. One real pretrained MiniLM-class checkpoint
(`sentence-transformers/all-MiniLM-L6-v2` at immutable revision
`1110a243fdf4706b3f48f1d95db1a4f5529b4d41`) is locally materialized outside
Git, provenance/license/hashes are recorded, Candle 0.11 loads every
required encoder variable (101/101, zero missing), forward execution is
exactly deterministic, head-only training is finite and correctly scoped,
and CLS/mean pooling separation is characterized on a train/dev-only
sanity set. Default CodeGG remains model-free with no download path.

The condition: Candle 0.11 cannot train any encoder weight. Its fused
`layer_norm` kernel records `BackpropOp::none`, so no gradient reaches the
encoder and the top-one-layer stage changes nothing. This is a framework
limitation, not an asset defect. M003 stage 1 (frozen encoder + ranking
head) is unblocked; encoder unfreezing (M003 stages 2–3) is gated on the
new narrow framework corrective M001B. This record supplements, and does
not rewrite, `002-status.md`.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Pinned MiniLM asset outside Git | `target/tool-advisor/reference-assets/all-minilm-l6-v2/` (ignored) | pass | config, weights, vocab, license/provenance, manifest; tokenizer provenance + upstream README alongside. |
| No weights committed | `git status` + receipt metadata only | pass | 91,102,259-byte asset set is ignored; the repo owns only hashes. |
| No model-download path | `src/tool_advisor/sequence_encoder.rs` (fs reads only) + `verify.sh quick` | pass | Acquisition was operator `curl` outside CodeGG. |
| Receipt metadata | `assets/tool-advisor/reference-models/all-minilm-l6-v2.json` | pass | Repo, revision, URLs, Apache-2.0, config facts, SHA-256 per file, total bytes, date, manifest fingerprint `a74faa95…`. |
| Config compatibility (§6) | probe `reference_config.matches_pinned_contract: true` | pass | bert/384/6L/12H/30522/2/512/gelu all match. |
| Complete Candle load, zero missing | probe `load_coverage`: 101/101, missing `[]` | pass | Unexpected source tensors only: `position_ids`, `pooler.dense.*` (147,840 + 512 params correctly ignored by `VarMap::load`). |
| Parameter class ~22M | loaded 22,565,376 / source 22,713,728 | pass | MiniLM range confirmed. |
| Deterministic forward | `deterministic_max_delta: 0.0` across runs | pass | Exact bitwise repeatability on CPU. |
| Finite embeddings | `embeddings_finite: true` | pass | — |
| Head-only scoped training | finite loss; encoder unchanged; head changed | pass | Losses 1.5157/0.4543 across runs (random head init). |
| Top-layer staged training | finite loss; head changed; top layer unchanged | conditional | Loss finite (0.7242/0.0) but no encoder weight can change on Candle 0.11 (see §10). |
| Semantic separation | CLS margin +0.1120, mean margin +0.2578, distinct | pass | Hand-written pairs; no frozen labels touched. |
| Pooling characterized | `PoolingStrategy::{Cls, Mean}` + separation report | pass | Mean materially more coherent; M003 selects explicitly. |
| Timings/resources | load ~17s, forward ~180ms, steps ~130ms, max RSS ~478 MiB | pass | macOS CPU dev profile; artifact 90,868,376 B. |
| Default-build isolation | `cargo check --locked` + `verify.sh quick` | pass | Module stays behind `tool-advisor-encoder-experiment`. |
| TinyBERT second point | §10 disposition | pass | Not pursued per plan §9 (no native safetensors, unclear license). |

Probe evidence fingerprint (SHA-256 of the captured `--json` output):
`82cc5f45a7c6066890d26b5ffc4da27f0797c32244c639905646b0c03c3cfecb`.

## 3. Production implementation evidence

`ab3e9dfd` extends only the experiment-gated `sequence_encoder.rs` (plus
probe text output in `main.rs`):

- `PoolingStrategy::{Cls, Mean}` with `encode_with_pooling` /
  `encode_mean_pooled`; `encode` keeps its CLS contract. M003 consumes the
  strategy as an explicit experiment parameter.
- `validate_reference_checkpoint_config` enforcing the pinned bert/384/6/
  12/30522/2/512 contract with exhaustive activation matching; surfaced in
  every probe run as `reference_config`.
- `load_coverage` parsing the safetensors header and diffing source
  tensors against Candle model variables (expected/loaded/missing/
  unexpected + parameter counts). The probe fails closed on any missing
  encoder variable.
- `probe_local_assets` additionally records staged-optimizer scoping
  facts from variable snapshots (which sets each stage changed),
  pooling separation, and load/forward/backward timings plus asset sizes.
- `candle_fused_layer_norm_has_no_backward` locks the framework premise
  (fused kernel: no grads; composite `layer_norm_slow`, softmax,
  index-select, gelu: grads flow).

No Hub/HTTP/fetch path was added; the loader still resolves only
operator-supplied local manifest paths and checks all four hashes first.

## 4. Verification executed

### Commands run

```bash
cargo check --locked
cargo check --locked --features tool-advisor-encoder-experiment
cargo check --locked --features tool-advisor-encoder-training
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib sequence_encoder
cargo run --locked --bin codegg --features tool-advisor-encoder-experiment -- \
  tool-advisor sequence-encoder-probe \
  --manifest target/tool-advisor/reference-assets/all-minilm-l6-v2/manifest.json \
  --json
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

(`--bin codegg` is required because the workspace exposes three binaries;
the plan's probe command is otherwise unchanged.)

### Results

- Default, experiment, and training feature checks passed.
- Seven sequence-encoder tests passed (tiny fixture determinism, hash
  rejection, coverage, reference-contract accept/reject, pooling
  determinism, staged scoping facts, fused-LN premise).
- Real-checkpoint probe succeeded twice (JSON run + RSS run); pooling
  margins reproduced to four decimals across runs (encoder deterministic;
  only the random head varies).
- `verify.sh quick` passed; all-feature Clippy passed with `-D warnings`
  (one new `manual_is_multiple_of` finding in new code was fixed, not
  allowed).
- `git diff --check` passed.
- One transient `rustpython-parser` build-script failure appeared on a
  first default `cargo check` and passed on clean re-run with no manifest
  changes; unrelated to this work and recorded here rather than hidden.

## 5. Invariant review

- Default CodeGG is independent of model weights, tokenizer assets,
  Candle, and network download; the module compiles only behind
  `tool-advisor-encoder-experiment`.
- Assets remain explicit hashed local inputs with license/provenance
  metadata; nothing is fetched at runtime.
- The encoder only produces embeddings/scores; no policy, permission,
  execution, or argument-construction authority was added.
- No fixed tool classifier IDs; candidates remain textual descriptors.
- Frozen C001 test/family-holdout labels were never inspected; the
  pooling sanity pairs are hand-written in code.

## 6. Failure and recovery review

Missing files, unsupported schema/framework, malformed config, hash drift,
shape-mismatched tensors, and any missing encoder variable fail closed
before or during load. Non-finite training losses fail the probe. The
fused-LayerNorm limitation fails safe: the optimizer silently skips
grad-less variables, lower layers provably never change, and the probe
reports the unchanged fact instead of claiming a scoped update. No
mutable cache, restart protocol, or network retry was introduced.

## 7. Migration and compatibility review

Additive and default-off. Existing `tool-advisor` behavior is unchanged.
`ProbeReport` gained additive JSON fields (coverage, reference-contract
verdict, scoping facts, pooling, timings); no consumer outside the probe
CLI exists. No storage, protocol, or artifact migration is needed.

## 8. Security review

The loader performs no network access and accepts only operator-specified
paths. Hashes and Apache-2.0 license metadata are verified before weights
load. The unexpected-tensor report confirms the pooler head and position
ids cannot silently enter the model. The experiment stays outside
authority and does not widen the resolved tool surface.

## 9. Documentation and operations

- Receipt: `assets/tool-advisor/reference-models/all-minilm-l6-v2.json`.
- Probe usage is unchanged from the M001 spike; `--json` now emits the
  coverage, reference-contract, scoping, pooling, and timing evidence.
- `architecture/tool-advisor-framework-spike.md` gains a dated M001A
  subsection recording the qualification numbers and the LayerNorm
  limitation (supplemental evidence; history untouched).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high (unfreezing only) | Candle 0.11 fused `layer_norm` (`candle-nn 0.11.0 ops.rs`, `apply_op3_no_bwd` → `BackpropOp::none`) severs encoder autograd; `top_layer_changed` is false on real and fixture models alike | M003 stages 2–3 (top-layer/full unfreezing) cannot run on this stack; frozen-encoder stage 1 is unaffected | M001B framework corrective owns the decision (differentiable norm path, scoped re-implementation, or formal frozen-only disposition). |
| medium | M001 `002-status.md` recorded head/top-layer backward as "pass" on finite loss alone; this work proves no encoder weight ever changed | Predecessor claim is weaker than stated, but its selection decision (Candle for forward/head training) still stands | Recorded here as supplemental evidence; `002-status.md` is not rewritten. |
| low | LayerNorm weight/bias params of the top layer cannot even be probed for change magnitude | No impact beyond the high finding | Revisit inside M001B. |

## 11. Roadmap disposition

- M001A is conditionally closed by this record: the asset half is fully
  qualified; the named condition is encoder fine-tuning on Candle 0.11.
- M001 moves from conditionally closed to closed: its named condition
  (real pretrained reference-asset evidence) is satisfied. The
  newly discovered staged-backward weakness is owned by M001B, not M001.
- M003 moves from blocked to ready, constrained to frozen-encoder +
  ranking/abstention-head stages; top-layer unfreezing additionally
  requires M001B closure. Its plan status line records the constraint.
- M001B (`008-encoder-finetuning-framework-corrective.md`) is registered
  ready in the same commit.
- M004/M005 remain blocked downstream of M003. TinyBERT disposition
  follows plan §9 (deferred until MiniLM shows signal and provenance
  clears).

## 12. Registry updates

- M001A moves from dependency-ready to recently closed (conditionally
  closed).
- M001 moves from conditionally closed history to closed (condition met).
- M003 (`004-sequence-encoder-ranking-experiment.md`) is registered
  ready with the frozen-encoder constraint noted in its handoff entry.
- M001B (`008-encoder-finetuning-framework-corrective.md`) is registered
  ready with the LayerNorm finding as its handoff note.
- No other registered plan lists M001A as a dependency; M004/M005 stay
  downstream-gated and no plan was silently unblocked past the M001B
  condition.
