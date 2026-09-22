# Tool-Selection Advisor Qualification Evidence Corrective C002 — Fresh V3 Preregistered Qualification

Status: ready for handoff

Repository baseline: `739bf5060690fa71dffd25ffeb1b28e444a00681`

Hard dependency:

- C001 semantic holdout and retrieval-gate correctness — positive closure required.

Source corrective roadmap:

- `plans/subsystems/tool-selection-advisor-qualification-evidence-corrective-addendum.md#c002--fresh-v3-preregistered-requalification`

Controlling ADR:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Primary class: qualification/closure.

## 1. Objective

Run one final qualification of the unchanged selected MiniLM packed-marker artifact against the semantically valid, previously unevaluated v3 holdout using the corrected retrieval identity semantics.

Historical v1/v2 results remain diagnostic context only.

## 2. Preconditions

Before activation:

- C001 closure accepted;
- v3 holdout/manifest committed;
- no selected-model v3 result exists in Git history;
- selected artifact SHA remains:
  `01b5c368b4e762dbe6ca284694b290b72e17ec64606e1f26a21510721f9a10e8`;
- encoder/tokenizer/source hashes unchanged;
- dev threshold/calibration unchanged;
- retrieval algorithm unchanged;
- C001 semantic validators pass from a clean checkout.

Failure of any item blocks C002.

## 3. Separate preregistration commit

Commit A freezes:

- v3 raw/canonical fingerprint;
- v3 semantic-manifest hash;
- selected artifact/model/tokenizer hashes;
- linear baseline hash;
- exact slice definitions;
- candidate-universe sizes 64/128;
- shortlist K=16 unless C001 has a pre-holdout operational reason to change it;
- retrieval mode;
- promotion threshold/budgets;
- quality/calibration/promotion/resource gates;
- exact release command;
- target hardware description;
- output path.

Do not embed a self-referential commit SHA inside the hashed content. Pass Commit A SHA separately to the final command/result.

Hosted CI must pass before the final model evaluation when available.

## 4. Quality gates

Preserve the prior v2 numeric policy unless C001 documented a reason before model evaluation:

- aggregate sequence MRR >= linear MRR - 0.01;
- >=0.02 gain in MRR or Recall@1 over linear on at least one predeclared contextual/generalization slice;
- no required generalization slice regresses >0.02 MRR versus linear;
- unknown/renamed MRR no worse than linear by >0.02;
- each family-holdout slice no worse than linear by >0.02;
- no-tool F1 no worse than best nontrivial baseline by >0.02.

Required slices:

- hard-negative;
- true counterfactual pairs;
- actual unknown/renamed;
- no-tool;
- multi-tool;
- AdvisorContextV2/long-session;
- plugin/MCP;
- LSP;
- research/search;
- structured/data.

Additionally report pair accuracy for counterfactual pairs: both members are correct only when the ranker chooses the expected changed label for each side.

## 5. Retrieval gates

For each expanded fixture explicitly select the point by:

```text
candidate_universe_size == 64 or 128
shortlist_k == prereg.retrieval_k
mode == prereg.retrieval_mode
```

Gates:

- 64-tool universe recall >=0.98 at K=16;
- 128-tool universe recall >=0.95 at K=16;
- zero authority violations;
- fixture validity checks all pass.

Never default a missing frontier point to zero. Missing/duplicate point is correctness failure E.

## 6. Calibration and promotion

Use frozen dev-selected abstention threshold; no v3 recalibration.

Report raw/calibrated:

- Brier;
- ECE;
- NLL;
- no-tool P/R/F1.

Promotion:

- relevant-tool promotion recall >=0.50;
- no-tool promotion rate <=0.10;
- irrelevant-tool promotion rate <=0.15;
- max promotions <=2;
- schema p95 <=16 KiB;
- authority violations =0.

If v3 no-tool semantics are valid and promotion recall remains zero, that is substantive negative evidence.

## 7. Resources

Reuse the corrected release-mode method from v2.

Gates:

- process-cold load p95 <=10,000 ms;
- total qualification <=600,000 ms;
- encoder weights <=128 MiB.

Report binary delta, RSS, rank/retrieval p50/p95/max, forwards/case, cache counts. Resource evidence may be reused only if binary/artifact hashes are identical and C001 did not modify executable code in a way that changes the measured path; otherwise remeasure.

## 8. One-run discipline

After preregistration CI:

1. clean working tree;
2. validate protocol/model/baseline/v3 hashes;
3. run semantic validators;
4. build exact release binary;
5. execute final v3 qualification once;
6. commit result/closure without changing inputs.

An execution failure that produces no usable model result may be retried only after recording the failure; model/gates/holdout remain unchanged.

## 9. Disposition

- **A — qualify for live M004:** correctness + quality + retrieval + calibration + promotion + resource gates pass.
- **B — quality gain but deployment cost too high:** quality/retrieval/calibration/promotion/safety pass, resource fails.
- **C — retrieval useful, ranker not qualified:** retrieval/safety pass, ranker quality/generalization fails.
- **D — no useful quality gain:** valid harness/fixtures, but ranker quality/generalization fails and retrieval does not provide an independently qualifying path.
- **E — correctness/framework failure:** protocol, artifact, leakage, semantic-fixture, retrieval-identity, or authority correctness fails.

Only A makes live M004 dependency-ready; original live/provider/operator prerequisites still apply.

## 10. Closure evidence

Commit B must include:

- preregistration SHA + CI conclusion;
- v3 semantic/fingerprint receipt;
- exact command/hardware;
- aggregate and every slice metric;
- counterfactual pair accuracy;
- corrected 64/128 universe retrieval evidence showing universe+K;
- calibration;
- promotion;
- resources;
- gate matrix;
- A/B/C/D/E disposition;
- registry/roadmap state.

Historical v2 closure/result MUST NOT be rewritten.

## 11. Verification

```bash
cargo test --workspace --locked -- --test-threads=1
cargo test --locked --features tool-advisor-encoder-training -- --test-threads=1
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
cargo build --release --locked --features tool-advisor-encoder-training --bin codegg
```

Final command follows the C001 v3 CLI contract and must accept preregistration SHA separately.

## 12. Acceptance

C002 closes with one reproducible v3 result and explicit disposition. A negative result on the semantically valid v3 corpus is accepted as substantive evidence about the current MiniLM ranker rather than another harness artifact.
