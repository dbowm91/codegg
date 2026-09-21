# Tool-Selection Advisor Sequence-Encoder Experiment M001B — Encoder Fine-Tuning Framework Corrective

Status: ready for handoff

Repository baseline: `ab3e9dfd`

Source roadmap:

- `plans/subsystems/tool-selection-advisor-sequence-encoder-experiment-roadmap.md#m001b--encoder-fine-tuning-framework-corrective`

Predecessor closure:

- `plans/closure/tool-selection-advisor-sequence-encoder-experiment/007-status.md` — M001A conditionally closed; Candle 0.11 fused `layer_norm` severs encoder autograd.

Controlling ADR:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Primary class: infrastructure / experiment prerequisite.

## 1. Objective

Decide, with evidence, how the sequence-encoder experiment regains encoder
fine-tuning (top-layer unfreeze and full fine-tune stages) given that
Candle 0.11 cannot train any `BertModel` weight: its fused `layer_norm`
kernel (`candle-nn 0.11.0`, `ops::layer_norm` via `apply_op3_no_bwd` →
`BackpropOp::none`) records no backward graph, so every encoder variable
is silently skipped by the optimizer. Either land a narrow differentiable
path that proves correctly-scoped encoder updates, or record a formal
frozen-encoder-only disposition and rescope M003 stages 2–3 out.

M003 stages 2–3 (any encoder unfreezing) remain blocked until this plan
closes positively. M003 stage 1 (frozen encoder + head) is unaffected and
already ready.

## 2. Why this is separate work

M001A proved everything up to the framework boundary: real MiniLM asset
provenance, complete Candle load (101/101 variables), deterministic
forward, finite head-only training with correct scoping, and pooling
characterization. It also proved, with a committed regression test
(`candle_fused_layer_norm_has_no_backward`), that the staged-unfreeze
half cannot work on this stack: the composite `layer_norm_slow`,
softmax, index-select embeddings, and gelu all propagate gradients, but
the fused kernel `BertModel` actually uses does not. The pinned upstream
is Candle 0.11.0 (newest `candle-core` on crates.io as of 2026-09-21), so
no version bump resolves this.

Do not re-litigate the M001A asset evidence. This plan owns only the
differentiable-encoder decision.

## 3. Options under test

Evaluate in this order, stopping at the first viable narrow path:

1. **Composite-norm BERT forward in experiment code.** A CodeGG-owned
   BERT encoder forward that reuses Candle tensors/ops but substitutes a
   composite differentiable normalization for the fused kernel, loading
   the already-qualified MiniLM weights by tensor name. Smallest
   framework fork; must prove weight-name compatibility and numerical
   parity with `BertModel::forward` on the reference checkpoint.
2. **Custom backward for the fused kernel.** A user-land `CustomOp3`
   implementing the layer-norm backward, wired through a CodeGG-owned
   forward. Heavier and riskier; pursue only if option 1 fails parity or
   shape/name compatibility.
3. **Frozen-encoder-only disposition.** If neither path stays narrow and
   auditable, formally scope M003 to frozen-encoder/head-only work,
   demote unfreezing from the training strategy, and close this plan
   with that disposition recorded.

A Burn comparison from M001 remains a research follow-up, not the
default answer: do not add a second framework contract to dodge this
decision. Do not fork or patch the Candle crates.

## 4. Scope

In: differentiable forward parity evidence on the qualified MiniLM
asset; correctly-scoped top-layer update evidence (the exact facts the
M001A probe already reports); regression tests locking whichever path is
chosen; ADR-0009 authority/isolation preservation.

Out: production advisor changes; M003 ranking-head work; TinyBERT or
second-capacity assets; dependency version changes; any runtime download
path; touching frozen C001 labels.

## 5. Acceptance

This plan closes positively when either (a) a CodeGG-owned experiment
path loads the qualified MiniLM weights, matches `BertModel::forward`
within a documented tolerance, and proves top-layer updates change head
+ selected top-layer variables while lower layers stay unchanged; or
(b) a frozen-encoder-only disposition is recorded with rationale, the
M003 plan is rescoped accordingly, and M003 stages 2–3 are retired
rather than left ambiguously blocked. A vague "unfreezing might work"
is not closure evidence.

## 6. Verification

```bash
cargo check --locked
cargo check --locked --features tool-advisor-encoder-experiment
cargo check --locked --features tool-advisor-encoder-training
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib sequence_encoder
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

Plus the real-checkpoint probe (`sequence-encoder-probe --json`) rerun
against the unchanged M001A manifest for any forward-path change, with
parity numbers captured as closure evidence.
