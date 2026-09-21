# Tool-Selection Advisor Sequence-Encoder Experiment M001B — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-selection-advisor-sequence-encoder-experiment/008-encoder-finetuning-framework-corrective.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-sequence-encoder-experiment-roadmap.md#m001b--encoder-fine-tuning-framework-corrective`

Repository baseline reviewed: `ab3e9dfd`

Implementation commits:

- `1633bfce` — advisor: restore encoder fine-tuning via composite-norm BERT forward

## 1. Executive finding

M001B closes positively via plan option 1. A CodeGG-owned BERT encoder
forward reuses Candle tensors/ops verbatim for variable creation
(`candle_nn::layer_norm` for `weight`/`bias` names and inits,
`embedding`/`linear`/`softmax`/`gelu` unchanged) and substitutes only the
forward kernel: composite `ops::layer_norm_slow` instead of the fused
`ops::layer_norm` (`apply_op3_no_bwd` → `BackpropOp::none`). On the
unchanged M001A MiniLM manifest the differentiable path loads 101/101
variables with zero missing, matches upstream `BertModel::forward` to max
2.3e-06 / mean 3.9e-07 (tolerance 1e-4), and proves correctly-scoped
top-layer updates (head + top layer change, lower layers unchanged,
head-only leaves the encoder unchanged). Options 2 (custom kernel
backward) and 3 (frozen-only disposition) were not needed; no Candle
crate was forked or patched, no dependency version changed, and no
production advisor behavior changed. M003 stages 2–3 (top-layer/full
unfreeze) are now authorized through this path.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| CodeGG-owned differentiable path, no Candle fork/patch | `src/tool_advisor/sequence_encoder.rs`: `DifferentiableLayerNorm` wraps `candle_nn::LayerNorm` (same `weight`/`bias` vars) with a `layer_norm_slow` forward; full `DiffBert*` stack mirrors upstream `pp` names + `<model_type>.` fallback | pass | Smallest framework fork per plan §3 option 1. |
| Loads qualified MiniLM weights by tensor name, zero missing | Real-checkpoint probe `load_coverage`: 101/101, missing `[]`, unexpected only `position_ids` + `pooler.*` (correctly ignored); 22,565,376 loaded / 22,713,728 source params | pass | Same coverage as M001A; weight-name compatibility proven. |
| Matches `BertModel::forward` within documented tolerance | Probe `differentiable_parity_max_delta` 2.302e-06, mean 3.91e-07, tolerance 1e-4 (`DIFFERENTIABLE_PARITY_TOLERANCE`), `matches_reference: true`; reproduced identically on rerun | pass | Two orders of magnitude headroom; parity checked on pristine weights before any optimizer step. |
| Top-layer updates change head + top, lower stays unchanged | Probe: `head_only_encoder_unchanged` true, `head_vars_changed` true, `top_layer_changed` true, `head_changed_in_top_stage` true, `lower_layers_unchanged` true; losses 0.9918/0.9918 deterministic via fixed probe heads | pass | Exact acceptance fact from plan §5(a). |
| Regression tests lock chosen path | `differentiable_layer_norm_has_backward`, `differentiable_encoder_matches_reference_forward`, updated `tiny_local_bert_proves_forward_and_staged_backward` (now expects `top_layer_changed` true + parity); upstream fused premise still locked by `candle_fused_layer_norm_has_no_backward` | pass | 9/9 `sequence_encoder` tests pass. |
| ADR-0009 authority/isolation preserved | No policy/permission/execution/argument authority added; encoder still produces embeddings/scores only; default build stays model-free; no download path | pass | See §5. |
| Out-of-scope respected | No production advisor changes; no M003 ranking-head work; no TinyBERT/second asset; no version changes; no runtime download; frozen C001 labels untouched | pass | `git show 1633bfce --stat`: one experiment file only. |

Probe evidence fingerprint (SHA-256 of the captured `--json` output for
the run cited in §4): `790da17cf487105b13599059bbe1dcf61a085f32081958dec57df3ec73335dee`.

## 3. Production implementation evidence

`1633bfce` extends only the experiment-gated `src/tool_advisor/sequence_encoder.rs`:

- `DIFFERENTIABLE_PARITY_TOLERANCE` (1e-4) with documented rationale
  (measured ~1e-6, two orders headroom).
- `DifferentiableLayerNorm`: variable creation delegated to
  `candle_nn::layer_norm` (identical names/inits), forward via
  `ops::layer_norm_slow` (proven differentiable by the M001A premise
  test). No other kernel changed.
- `DiffBertEmbeddings` / `SelfAttention` / `SelfOutput` / `Attention` /
  `Intermediate` / `Output` / `Layer` / `Encoder` /
  `DifferentiableBertModel`: verbatim upstream structure and `pp`
  paths (`embeddings.*`, `encoder.layer.N.*`, `<model_type>.` fallback),
  same dropout-identity, position-id, extended-mask, softmax, and
  gelu/erf semantics; only LayerNorm uses the differentiable wrapper.
- `CandleBertSequenceEncoder` now holds `DifferentiableBertModel`
  (single model path; frozen inference and training share it, so no
  divergence risk). `top_layer_vars`, optimizer staging, and pooling are
  unchanged.
- `forward_parity_against_reference` + `differentiable_forward_parity`:
  fresh upstream `BertModel` from the same manifest on pristine weights,
  max/mean abs delta; probe fails closed above tolerance.
- Probe ordering fixes found during implementation: parity and pooling
  run on pristine weights before any optimizer step (post-training
  comparison would measure the update, not the forward; post-training
  pooling would vary with the update). Pooling now reproduces M001A to
  four decimals (CLS +0.1120, mean +0.2578).
- Deterministic probe heads (`deterministic_probe_weights`: weight
  0.01, bias 0.0): margin loss ~0.9918 on every run with gradients to
  both head and encoder. A zeroed head would block encoder grads (head
  weight zero kills the chain rule); a random head flakes (lucky init
  separates the probe pair with zero loss and no update, correctly but
  non-deterministically). Fixed small weights give deterministic
  positive evidence.
- `ProbeReport` gains four additive fields (`parity_max/mean`,
  `tolerance`, `matches_reference`); no consumer outside the probe CLI
  exists, so no migration is needed. The `top_layer_changed`
  documentation is updated from "false on Candle 0.11" to "true via the
  composite-norm path".

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
- Nine sequence-encoder tests passed (tokenizer determinism, hash
  rejection, fused-LN premise, tiny forward + staged backward with the
  new `top_layer_changed: true` + parity assertions, reference-contract
  accept/reject, tiny coverage, pooling determinism, new diff-LN
  backward, new diff-vs-reference parity).
- Real-checkpoint probe succeeded repeatedly with identical parity
  (max 2.302e-06, mean 3.91e-07) and identical scoping
  (`head_only_encoder_unchanged` true, `head_vars_changed` true,
  `top_layer_changed` true, `head_changed_in_top_stage` true,
  `lower_layers_unchanged` true). Deterministic forward max delta 0.0,
  embeddings finite, reference contract matches, 101/101 coverage,
  pooling CLS +0.1120 / mean +0.2578 (matches M001A to four decimals),
  losses 0.9918/0.9918. Timings on the dev host: load ~17s, forward
  ~200ms, head step ~920ms, top step ~1120ms (head step slower than
  M001A's ~130ms because backward now traverses the differentiable
  encoder even when the optimizer scopes to the head; correctness
  scoping is unchanged).
- `verify.sh quick` passed; all-feature Clippy passed with `-D warnings`;
  `git diff --check` clean; `cargo fmt --check` clean (one formatting
  nit fixed, not allowed).

## 5. Invariant review

- Default CodeGG is independent of model weights, tokenizer assets,
  Candle, and network download; the module compiles only behind
  `tool-advisor-encoder-experiment`.
- Assets remain explicit hashed local inputs with license/provenance
  metadata; nothing is fetched at runtime; the manifest is unchanged
  from M001A.
- The encoder only produces embeddings/scores; no policy, permission,
  execution, or argument-construction authority was added.
- No fixed tool classifier IDs; candidates remain textual descriptors.
- Frozen C001 test/family-holdout labels were never inspected; pooling
  and training pairs are hand-written in code.
- ADR-0009 locality/authority constraints hold; the change is confined
  to the experiment training path.

## 6. Failure and recovery review

Missing files, unsupported schema/framework, malformed config, hash
drift, shape-mismatched tensors, any missing encoder variable, parity
above tolerance, and non-finite losses all fail closed before or during
the probe. The optimizer still silently skips grad-less variables, but
the probe now proves the top-layer set is not grad-less (change
reported) and the lower set is correctly excluded (unchanged reported).
Deterministic probe heads remove the zero-loss flake without hiding it:
the rationale is documented in code. No mutable cache, restart
protocol, or network retry was introduced.

## 7. Migration and compatibility review

Additive and default-off. Existing `tool-advisor` behavior is
unchanged. `ProbeReport` gains four additive JSON fields; no consumer
outside the probe CLI exists. Frozen-stage M003 work built on the
upstream forward remains valid because parity (2.3e-06) proves the
differentiable forward is numerically the same model. No storage,
protocol, or artifact migration is needed.

## 8. Security review

The loader performs no network access and accepts only operator-specified
paths. Hashes and Apache-2.0 license metadata are verified before weights
load, unchanged from M001A. The unexpected-tensor report still confirms
the pooler head and position ids cannot silently enter the model. The
experiment stays outside authority and does not widen the resolved tool
surface. No secrets, credentials, or telemetry were added.

## 9. Documentation and operations

- Probe usage is unchanged from M001A; `--json` additionally emits the
  four parity fields.
- `architecture/tool-advisor-framework-spike.md` gains a dated M001B
  subsection recording the composite-norm decision, parity numbers, and
  scoping facts (supplemental evidence; history untouched).
- M001A predecessor records are not rewritten; this record supplements
  `007-status.md` with the restored unfreezing path.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Head-only training step is slower (~920ms vs M001A ~130ms) because backward now computes encoder grads even when the optimizer scopes to the head | Dev-loop cost only; scoping correctness holds (encoder unchanged) | None; noted for M003 capacity planning. |
| low | Full fine-tuning (`FineTuneStage::Full`) is wired through the same differentiable path but its weight-change evidence is not separately gated here | Plan §5(a) requires only top-layer scoping, which is proven; full unfreeze inherits the same LN path but M003 must still qualify it with overfitting controls | M003 stage 3 qualifies full fine-tuning per its own exit conditions. |
| info | Burn comparison from M001 remains a research follow-up, not pursued here | No impact; plan explicitly excludes a second framework contract | None. |

No high/medium findings remain. Options 2 and 3 were evaluated in
order and not needed: option 1 succeeded within tolerance on the first
narrow path.

## 11. Roadmap disposition

- M001B is closed by this record (positive option-1 disposition).
- M003 moves from ready (frozen-encoder/head-only; unfreeze gated on
  M001B) to ready (all three training stages authorized; stages 2–3 run
  through the M001B differentiable path with M003's own dev-signal and
  overfitting gates still applying). Its plan status line is updated in
  the same commit.
- M004/M005 remain blocked downstream of M003; no plan is silently
  unblocked past M003.
- Live primary-model M004 remains blocked until the sequence experiment
  ultimately records a positive M005 disposition (unchanged).

## 12. Registry updates

- M001B (`008-encoder-finetuning-framework-corrective.md`) moves from
  dependency-ready `ready` to recently closed (`closed`,
  implementation `1633bfce`).
- M003 (`004-sequence-encoder-ranking-experiment.md`) stays `ready`
  with its handoff note updated: frozen-encoder + ranking/abstention
  head stages plus top-layer/full unfreeze stages authorized via the
  M001B differentiable path.
- Dependency audit: M001B was the sole named blocker for M003 stages
  2–3. No other registered plan lists M001B as a hard or interface
  dependency; M004/M005 stay downstream-gated on M003 and are not newly
  unblocked. No corrective follow-up is registered (no defect remains).
