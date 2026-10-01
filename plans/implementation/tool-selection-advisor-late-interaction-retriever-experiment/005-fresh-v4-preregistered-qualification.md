# Tool-Selection Advisor Late-Interaction Retriever M005 — Fresh V4 Preregistered Qualification

Status: blocked on positive M004

Repository baseline: `c4cc6c5b3c9154428b8560a60aa0129624435dfe`

Source roadmap:

- `plans/subsystems/tool-selection-advisor-late-interaction-retriever-experiment-roadmap.md#m005--fresh-v4-qualification`

Hard dependency:

- M004 positive frozen operating point.

Primary class: final qualification/closure.

## 1. Objective

Run one fresh, preregistered, release-mode qualification of the complete late-interaction retrieval + span-packed ranker + promotion stack.

No further architecture or threshold selection occurs in M005.

## 2. Fresh v4 holdout

Construct only after M004 freezes.

Minimum:

- >=224 cases;
- >=180 semantic/leakage groups;
- >=40 no-tool;
- >=40 hard-negative;
- >=40 unknown/renamed;
- >=40 multi-tool;
- >=32 long-session/AdvisorContextV2;
- >=20 true counterfactual pairs;
- >=24 each plugin/MCP, LSP, research/search, structured/data, filesystem/git/shell/code-navigation.

Every positive retrieval label must be inferable from allowed current-state fields under the retrieval-relevance-v1 semantics.

## 3. Late-interaction stress slices

Include explicit slices for:

- semantic paraphrase with no exact descriptor verb;
- canonical tool-name masking;
- renamed/unknown tools with unchanged descriptions;
- short generic descriptions;
- schema-absent external candidates;
- distractors sharing many lexical tokens;
- multi-token operation names;
- long descriptors near the frozen token cap;
- reordered descriptor fields/tokens where semantics are preserved.

No stress slice may be constructed by copying dev examples.

## 4. Functional equivalence

If CodeGG contains truly substitutable tools, functional-equivalence sets may be annotated **before preregistration** as an additional diagnostic.

Primary qualification remains against the frozen inferable relevance target unless an equivalence relation is:

- explicit;
- symmetric where appropriate;
- behaviorally justified;
- authority-equivalent for the case.

Do not post-hoc declare a missed tool "equivalent" to rescue a result.

## 5. Leakage

Require zero overlap against:

- historical train/dev/test;
- v2;
- v3;
- retrieval-signal M001R/M002/M003 generated variants;
- late-interaction train/dev augmentation;
- external OOD diagnostic data.

Check exact, normalized, template, family, copied-context, and renamed-identity leakage.

## 6. Separate preregistration commit

Commit A freezes:

- v4 fingerprints/manifest;
- retrieval artifact/mode hashes;
- token contract;
- K;
- cache contract;
- span-packed ranker hash;
- abstention/candidate-relevance calibration;
- promotion threshold;
- all gates;
- exact release command;
- hardware description.

Hosted CI must be green before the final run.

## 7. Retrieval gates

Fresh v4:

- 64-tool recall >=0.98;
- 128-tool recall >=0.95;
- 256-tool recall >=0.92;
- authority violations =0.

Report:

- current-step;
- explicit-next-step;
- unknown/renamed;
- name-masked;
- paraphrase;
- schema-present/absent;
- per-family;
- token-cap/truncation slice.

## 8. End-to-end ranker gates

At the frozen retrieval point:

- aggregate MRR >= frozen linear baseline -0.01;
- Recall@1 >= linear -0.02;
- >=0.02 MRR or R1 gain on at least one contextual/hard-negative slice;
- no required semantic slice >0.02 worse MRR;
- no-tool F1 >= best nontrivial baseline -0.02;
- multi-tool nDCG >= linear -0.02;
- unknown/renamed MRR >= linear -0.02.

## 9. Order/generalization gates

- top-1 identity consistency >=0.95;
- max target-position R1 spread <=0.05;
- worst-permutation MRR >= canonical -0.02;
- name-masked retrieval must remain above the preregistered minimum;
- unknown/renamed retrieval must remain above the preregistered minimum;
- no family collapse.

## 10. Promotion gates

At frozen M004 threshold:

- relevant promotion recall >=0.50;
- no-tool promotion <=0.10;
- irrelevant promotion <=0.15;
- max promotions <=2;
- schema p95 <=16 KiB;
- promotion identity consistency >=0.95;
- authority violations =0.

## 11. Resources

Release mode:

- process-cold p95 <=10 s;
- descriptor cache <=64 MiB at 256;
- total warm retrieval p95 <=1.0 s;
- total advisor p95 <=1.5 s;
- total qualification <=600 s unless preregistration records a bounded split with equivalent one-run semantics.

Report RSS, artifact bytes, encoder forwards, cache hit/miss behavior, and MaxSim arithmetic separately from encoder time.

## 12. One-run discipline

After preregistration and green CI:

1. verify clean tree and artifact hashes;
2. validate v4 semantics/leakage;
3. build exact release binary;
4. execute one final qualification;
5. commit result/closure without tuning.

Retry only a run that produced no usable result due to an execution failure, and record why.

## 13. Disposition

- **A — qualify for live trajectory:** all correctness/retrieval/ranker/order/promotion/resource gates pass.
- **B — quality passes, deployment/resource gates fail.**
- **C — retrieval qualifies but downstream advisor does not.**
- **D — no useful generalizing gain.**
- **E — correctness/evidence/framework failure.**

Only A may make the historical live-primary-model trajectory plan dependency-ready, subject to its original provider/operator prerequisites.

## 14. Verification

```bash
cargo test --workspace --locked -- --test-threads=1
cargo test --locked --features tool-advisor-encoder-training -- --test-threads=1
scripts/verify.sh quick
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
```

## 15. Acceptance

M005 closes with one machine-readable v4 result and explicit A/B/C/D/E disposition.

Historical retrieval-signal and qualification records remain immutable.
