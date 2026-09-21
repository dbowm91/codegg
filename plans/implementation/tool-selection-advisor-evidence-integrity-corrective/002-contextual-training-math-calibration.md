# Tool-Selection Advisor Evidence Corrective C002 — Contextual Training Math, Calibration, and Capacity Truthfulness

Status: ready for handoff (unblocked by C001 closure)

Repository baseline: `71460c0cb1421f33a62d57123ac562c8a7c4bf1c`

Source roadmap:

- `plans/subsystems/tool-selection-advisor-evidence-integrity-corrective-addendum.md#c002--contextual-training-math-calibration-and-capacity-truthfulness`

Predecessor closure:

- `plans/closure/tool-selection-advisor-post-closure-corrective/002-status.md`

Hard dependency: C001 accepted closure and frozen clean partition fingerprints.

Primary class: invariant/model correctness.

## 1. Objective

Correct the contextual scorer's optimization/calibration path and make reported model capacity reflect what was actually trained.

C002 is not a mandate to keep `contextual-embedding-v1`. It first makes that implementation mathematically correct and measurable. C004 will decide whether it is good enough to qualify or should be retained only as a research baseline.

## 2. Discovered defects

Current score:

```text
s = dot(mean(E(context_tokens)), mean(E(candidate_tokens))) / dim + bias
p = sigmoid(s)
```

Training computes:

```text
error = p - target
scale = -learning_rate * error
update_pair(..., scale)
update_pair: weight -= scale * other_vector / dim
```

For binary cross entropy, `dL/ds = p - target`. With the current call/subtraction combination, the sign is reversed relative to gradient descent.

The derivative for a token participating in a mean also includes `1 / token_count`. The current update omits this factor.

Additional evidence defects:

- no numerical-gradient regression;
- no explicit tiny-overfit loss-decrease gate;
- `calibration_temperature` is present in config/report but contextual runtime does not serialize/use learned temperature;
- abstention is hard-coded from `sigmoid(-top_score)` with no dev-set calibration;
- contextual evaluation can label all supplied cases as test;
- trainer falls back to all cases when no train split exists, which is unacceptable for qualification;
- physical parameter counts (5.2M/15.7M) hide that the current corpus touches only a very small subset of 65,536 embedding buckets.

## 3. Invariants

- Optimizer updates use train partition only.
- Calibration/threshold selection uses dev partition only.
- Final test/family holdout is never touched before C004 evaluation.
- Empty train/dev partitions are hard errors in qualification mode.
- Gradient implementation is validated against finite differences on a tiny deterministic model.
- Runtime abstention uses serialized calibration values actually learned from dev data.
- Calibration must never be silently read from the final test set.
- Artifact versioning prevents old uncalibrated contextual artifacts from being mistaken for corrected qualified artifacts.
- Default/no-feature behavior remains unchanged.
- Existing v1 artifacts may remain inspectable/loadable for compatibility but must be identifiable as legacy/unqualified.

## 4. Correct gradient

For one pair with mean-pooled context vector `c` and candidate vector `t`:

```text
s = dot(c, t) / dim + b
g = dL/ds = sigmoid(s) - y

dL/dE(context_token_i) =
    g * t / (dim * n_context)

dL/dE(candidate_token_j) =
    g * c / (dim * n_candidate)

dL/db = g
```

Implement the update so sign and normalization are obvious at the call site. Prefer an API such as `apply_pair_gradient(..., learning_rate, error)` rather than passing a pre-negated scale.

Compute vectors/gradients from the same pre-update parameter state. Accumulate contributions correctly when a hashed token occurs repeatedly or appears on both sides.

## 5. Numerical correctness tests

Introduce a tiny configurable test model (small bucket count/dimension) or pure loss helper so finite-difference checks are cheap.

Required tests:

- analytic gradient agrees with centered finite differences within a declared tolerance;
- positive-target update increases score on a simple pair;
- negative-target update decreases score;
- repeated token normalization is correct;
- shared context/candidate token receives the sum of both derivative paths;
- bias derivative is correct;
- deterministic tiny corpus loss decreases over several steps;
- tiny corpus can overfit a separable context-dependent task.

These tests are closure-critical.

## 6. Calibration contract

Replace hard-coded contextual abstention with artifact-backed calibration.

A minimal acceptable calibrated form:

```text
P(abstain) = sigmoid((abstain_bias - max_score) / temperature)
```

where `temperature > 0` and `abstain_bias` are selected using dev data only.

Calibration may use bounded deterministic grid search or another pure-Rust optimizer. Record at minimum:

- dev NLL;
- Brier score;
- ECE;
- no-tool precision/recall/F1;
- selected temperature;
- selected bias/threshold.

Serialize these values in a new contextual artifact version. Runtime and `inspect` must report them.

## 7. Partition/evaluation semantics

Training must consume C001's explicit partitions.

Remove qualification fallback that trains on all cases when train is empty. If a separate smoke-test helper needs all-case fitting, keep it test-only or require an explicit non-qualification flag whose output cannot be mistaken for a qualified artifact.

Update eval CLI/API so qualification requires an explicit partition or defaults to frozen `test`. `all` may exist for diagnostics but must be labeled diagnostic and cannot populate closure metrics.

Training report should include train/dev metrics separately; test metrics should be absent during architecture/calibration tuning unless an explicit final-eval command is invoked.

## 8. Capacity truthfulness

For each run report:

- allocated parameter count;
- artifact bytes;
- embedding bucket count/dimension;
- unique normalized tokens in train;
- distinct buckets touched in train;
- hash collision count/rate;
- percentage of rows receiving data-derived gradient;
- estimated trained/touched parameter count;
- cold-load/RSS/latency.

Do not claim model quality from allocated parameter count.

Permit a `contextual-embedding-v2` artifact with variable bucket count if evidence shows a much smaller table provides equivalent quality and materially better footprint. Keep v1 readable but unqualified.

Compare at least:

- hashed-linear-v1;
- corrected current small embedding configuration;
- corrected current medium configuration;
- one compact embedding-table configuration if technically straightforward.

This comparison is about efficient capacity, not forcing a larger architecture.

## 9. Ordered work

A. Parameterize loss/gradient sufficiently for numerical tests.
B. Correct sign and mean-pooling derivative.
C. Remove qualification all-case training fallback.
D. Add dev-only calibration and artifact schema/version.
E. Update runtime abstention to use serialized calibration.
F. Add partition-aware train/eval reporting.
G. Add active-row/collision/effective-capacity reporting.
H. Retrain only after C001 fingerprints are frozen.
I. Update architecture/framework docs to call the model a hashed embedding interaction scorer unless/until C004 supports a stronger qualification claim.

## 10. Verification

Expected minimum:

```bash
cargo test --locked --features tool-advisor-training -p codegg --lib tool_advisor::contextual
cargo test --locked --features tool-advisor-training -p codegg --lib tool_advisor::training
cargo test --locked --features tool-advisor -p codegg --lib tool_advisor
cargo check --locked
cargo check --locked --features tool-advisor
cargo check --locked --features tool-advisor-training
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

Retraining commands must reference the frozen C001 dataset/partition fingerprints.

## 11. Acceptance criteria

C002 closes when:

1. numerical gradients agree with finite differences;
2. tiny training loss/score-direction fixtures prove descent;
3. train/dev/test roles are enforced;
4. dev-only calibration is serialized and used at runtime;
5. old uncalibrated artifacts are clearly legacy/unqualified;
6. corrected artifacts train deterministically from C001 partitions;
7. active-row/collision/effective-capacity data is recorded;
8. no effectiveness claim beyond corrected offline measurements is made.

## 12. Stop conditions

Stop if fixing the current scorer requires reading test labels for tuning, silently changing artifact interpretation, or using a non-Rust/remote trainer.

If corrected clean evidence shows the architecture cannot learn the contextual slices at all, close C002 as a correctness repair and let C004 demote the architecture rather than hiding the negative result.

## 13. Closure evidence

- gradient derivation and finite-difference results;
- before/after tiny-loss trajectory;
- partition-use matrix;
- calibration metrics and serialized values;
- artifact compatibility behavior;
- active-row/collision report;
- corrected training configs/fingerprints;
- exact verification output.
