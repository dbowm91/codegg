# Tool-Selection Advisor Sequence Qualification Corrective C002 — Preregistered Release Qualification

Status: implemented

Repository baseline: `c5fa1a0850d88744984a2aba445a1e4ba2b9958f`

Hard dependency:

- C001 qualification harness completeness and fresh holdout — must be closed positively.

Source corrective roadmap:

- `plans/subsystems/tool-selection-advisor-sequence-qualification-corrective-addendum.md#c002--preregistered-release-mode-requalification`

Controlling ADR:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Primary class: qualification/closure.

## 1. Objective

Perform one clean, separately preregistered qualification of the already-frozen MiniLM packed-marker model using:

- the fresh C001 evaluation holdout as the primary final evidence;
- historical C001 test/family data as diagnostic continuity evidence only;
- release-mode resource measurement;
- the complete quality/calibration/promotion/safety gate set.

No model or threshold tuning occurs in C002.

## 2. Preconditions

Before activation, verify:

- C001 closure exists;
- fresh holdout and manifest fingerprints are committed;
- selected sequence artifact SHA-256 remains `01b5c368b4e762dbe6ca284694b290b72e17ec64606e1f26a21510721f9a10e8`;
- MiniLM encoder/tokenizer/source hashes match historical M005;
- dev-selected pooling/calibration/threshold configuration is unchanged;
- no selected-model result exists for the fresh holdout in repository history prior to the preregistration freeze.

If any precondition fails, stop and register the reason. Do not regenerate the holdout opportunistically.

## 3. Commit A — preregistration freeze

Create a commit containing only the finalized qualification-v2 protocol and any documentation/state change required to mark C002 active.

The protocol freezes:

- fresh-holdout fingerprint;
- historical diagnostic fingerprints;
- selected artifact/model/tokenizer hashes;
- exact release binary command;
- target hardware/OS metadata;
- baseline artifacts;
- slice definitions;
- thresholds/calibration provenance;
- retrieval K/mode;
- promotion limits;
- all numeric gates;
- resource limits;
- output path.

The selected model artifact and holdout MUST NOT change in Commit A.

Hosted CI for Commit A must conclude successfully before the final run when CI is available.

Record Commit A SHA outside the protocol hash and pass/echo it into the final result.

## 4. Primary quality gates

Unless C001 closes with stricter values, freeze these minimum gates before evaluation:

- aggregate fresh-holdout MRR >= linear MRR - 0.01;
- sequence improves MRR or Recall@1 by >=0.02 over `hashed-linear-v1` on at least one predeclared contextual slice;
- no predeclared contextual/generalization slice regresses versus linear by >0.02;
- unknown/renamed slice MRR not worse than linear by >0.02;
- each true family-holdout MRR not worse than linear by >0.02;
- no-tool F1 not worse than the best nontrivial baseline by >0.02.

Required predeclared contextual/generalization slices:

- hard-negative;
- counterfactual;
- unknown/renamed;
- context-v2/long-session;
- plugin/MCP;
- LSP;
- research/search;
- structured/data.

No slice may be silently omitted. Missing slice evidence fails the quality gate.

## 5. Calibration gates

Using the artifact's pre-existing dev calibration only:

- calibrated Brier <= raw Brier + 0.01;
- calibrated ECE <= raw ECE + 0.01;
- report NLL raw/calibrated;
- report no-tool precision/recall/F1 raw/calibrated.

No recalibration after seeing C002 results.

## 6. Retrieval and promotion gates

Retrieval:

- 64-tool candidate recall >=0.98;
- 128-tool candidate recall >=0.95;
- authority violations = 0.

Promotion, unless C001 freezes stricter gates:

- relevant-tool promotion recall >=0.50;
- no-tool promotion rate <=0.10;
- irrelevant-tool promotion rate <=0.15;
- max promotions <=2;
- schema budget <=16 KiB;
- authority violations = 0.

Report promotion metrics per slice as well as aggregate.

## 7. Resource gates

Use only release-mode measurements produced by the C001 resource path.

Preserve:

- process-cold load p95 <=10,000 ms;
- total qualification rank time <=600,000 ms;
- encoder weights <=128 MiB.

Also report, even when non-gating unless C001 preregisters stricter limits:

- process-cold first/median;
- rank p50/p95/max;
- retrieval p50/p95/max;
- peak RSS;
- release binary size and feature delta;
- tokenizer/head bytes;
- encoder forwards per case;
- cache entries.

Do not substitute development-profile `cargo run` timings for release evidence.

## 8. Final run discipline

After Commit A CI is green:

1. verify working tree clean;
2. verify all protocol/artifact/holdout hashes;
3. build exact release binary;
4. run the exact preregistered qualification command once;
5. write machine-readable result;
6. do not change gates/model/holdout after reading results.

A rerun is allowed only for an execution failure that produced no usable result and did not alter inputs. Record the failure and reason.

## 9. Disposition

Compute exactly:

- **A — qualify for live M004:** all quality, calibration, retrieval, promotion, authority, leakage, and resource gates pass.
- **B — quality gain but deployment cost too high:** quality/calibration/retrieval/promotion/safety pass; resource gate fails.
- **C — retrieval useful, ranker not qualified:** retrieval/safety pass; ranking/generalization quality fails.
- **D — no useful quality gain:** selected sequence ranker fails the preregistered quality/gain requirements.
- **E — correctness/framework failure:** protocol/hash/leakage/authority/harness correctness fails.

The historical M005 disposition D remains historical regardless of C002 result.

Only A may update the existing live-primary-model M004 from blocked to ready, and even then its original operator/provider/live-trajectory prerequisites still apply.

## 10. Commit B — result and closure

Commit:

- machine-readable qualification-v2 result;
- C002 closure record;
- corrective roadmap state;
- registry state;
- supplemental architecture note if warranted.

The closure must state:

- preregistration commit SHA and CI result;
- exact final command;
- target hardware/OS;
- fresh holdout fingerprint;
- all slice metrics;
- raw/calibrated metrics;
- promotion metrics;
- release resource metrics;
- gate-by-gate outcome;
- explicit A/B/C/D/E disposition.

Do not rewrite historical M005 closure/result.

## 11. Verification

Before Commit A:

```bash
cargo test --workspace --locked
cargo test --locked --features tool-advisor-encoder-training
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

Final measurement path:

```bash
cargo build --release --locked --features tool-advisor-encoder-training --bin codegg
./target/release/codegg tool-advisor sequence-encoder-qualify-v2 \
  --prereg <frozen-v2-preregistration.json> \
  --prereg-commit <commit-a-sha> \
  --json
```

## 12. Acceptance

C002 closes with a reproducible machine-readable result and one explicit disposition. A is the only result that may unblock live M004. B/C/D/E close the corrective honestly and keep live M004 blocked.
