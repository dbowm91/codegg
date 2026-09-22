# Tool-Selection Advisor Sequence Qualification Corrective C001 — Qualification Harness and Fresh Holdout

Status: ready for handoff

Repository baseline: `c5fa1a0850d88744984a2aba445a1e4ba2b9958f`

Source corrective roadmap:

- `plans/subsystems/tool-selection-advisor-sequence-qualification-corrective-addendum.md#c001--qualification-harness-completeness-and-fresh-holdout`

Predecessor closure:

- `plans/closure/tool-selection-advisor-sequence-encoder-experiment/006-status.md`

Controlling ADR:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`

Primary class: correctness/evidence corrective.

## 1. Objective

Repair the offline qualification harness so it implements the evidence contract the historical M005 plan actually specified, then freeze a fresh final holdout suitable for a later positive/negative qualification.

C001 does **not** requalify the model and MUST NOT inspect selected-model metrics on the new holdout.

## 2. Why the historical verification was insufficient

Historical M005 correctly enforced hashes/leakage/authority and reported aggregate ranking/retrieval metrics, but the implementation diverged from its plan:

- `contextual-slice-gain` was implemented as aggregate sequence MRR versus aggregate `contextual-embedding-v2` MRR.
- `contextual_artifact: null` meant that gate was guaranteed false.
- no per-slice unknown/renamed, hard-negative, family-holdout, or long-session/context-v2 result was emitted;
- no explicit no-tool F1 gate against the best nontrivial baseline was emitted;
- calibration was reported only after calibration, without raw-vs-calibrated comparison;
- proactive-promotion recall/false-promotion/schema-budget evidence was absent;
- resource evidence came from the dev-profile qualification path and omitted binary delta, RSS, and p50/p95 timings;
- the disposition function mapped every failing gate to D instead of preserving A/B/C/D/E semantics.

Regression tests in this plan must lock those gaps.

## 3. Non-goals

Do not:

- retrain or modify `target/tool-advisor/sequence-ranking-minilm-packed-head.json` or its head weights;
- alter the selected MiniLM checkpoint, tokenizer, mean pooling, packed-marker layout, or dev-selected calibration;
- run top-layer/full fine-tuning;
- modify historical M005 result/preregistration/closure files;
- wire the sequence ranker into production live disclosure;
- add remote telemetry, teacher generation, or provider calls;
- tune any model/threshold on the new holdout.

## 4. Freeze the selected artifact identity

C001 must define one canonical `FrozenSequenceCandidate`/equivalent record containing the historical M005-selected:

- sequence artifact SHA-256 `01b5c368b4e762dbe6ca284694b290b72e17ec64606e1f26a21510721f9a10e8`;
- encoder-manifest hash;
- encoder config hash;
- tokenizer hash;
- source-weight hash;
- architecture id;
- pooling strategy;
- calibration/abstention values;
- training partition fingerprint.

The C002 harness fails closed if any candidate identity differs.

## 5. Correct qualification slice semantics

Implement explicit slice membership independent of comparison-model availability.

At minimum support:

- `aggregate`;
- `counterfactual`;
- `hard-negative`;
- `unknown-renamed`;
- `no-tool`;
- `multi-tool`;
- `context-v2-long-session`;
- true family holdouts:
  - plugin/MCP;
  - LSP;
  - research/search;
  - structured/data.

A case MAY belong to multiple slices.

The historical rejected contextual model may remain an informational comparison arm, but **must not define what "contextual slice" means**.

For each quality arm, emit MRR, Recall@1/3/5, nDCG@5, case count, relevant-tool count, and no-tool metrics where defined.

## 6. Baseline arms

Qualification-v2 supports at least:

- keyword;
- BM25;
- frozen `hashed-linear-v1`;
- selected frozen MiniLM packed-marker ranker;
- no-advisor disclosure.

The historical `contextual-embedding-v2` arm is optional informational evidence only. Its absence MUST NOT automatically fail a contextual-slice gate.

## 7. Calibration evidence

For the selected sequence artifact, emit both:

- raw/uncalibrated abstention probabilities;
- artifact-calibrated probabilities.

For aggregate and no-tool slices report:

- Brier score;
- ECE;
- NLL;
- no-tool precision/recall/F1.

Calibration passes only when calibrated Brier and ECE are each non-inferior to raw within a preregistered tolerance. Do not recalibrate on qualification data.

## 8. Promotion simulation

Implement an offline adapter using the same semantic rules as pre-turn promotion:

- already-authorized deferred universe only;
- same max-candidates, max-promotions, threshold, and schema-byte budget as preregistration;
- no permission/execution effects.

Report:

- relevant-tool promotion recall;
- irrelevant-tool promotion rate;
- no-tool promotion rate;
- mean/p95 number of promoted tools;
- mean/p95 schema bytes added;
- threshold source and dev fingerprint;
- zero authority violations.

C002 preregistration must set numeric gates. Suggested starting contract, subject to C001 closure review before any final run:

- relevant-tool promotion recall >= 0.50;
- no-tool promotion rate <= 0.10;
- irrelevant-tool promotion rate <= 0.15;
- hard schema budget remains 16 KiB and max promotions remains 2.

If C001 changes these values, it must document operational rationale before C002 and may not use new-holdout outcomes.

## 9. Fresh final holdout

Create a repository-owned evaluation-only corpus, for example:

- `assets/tool-advisor/qualification-v2-holdout.jsonl`
- `assets/tool-advisor/qualification-v2-holdout-manifest.json`

Minimum corpus requirements:

- >=128 cases;
- >=96 leakage groups;
- >=24 hard-negative cases;
- >=24 unknown/renamed-tool cases;
- >=16 no-tool cases;
- >=16 multi-tool cases;
- >=16 context-v2/long-session cases;
- >=12 cases in each plugin/MCP, LSP, research/search, and structured/data family slice;
- cases may overlap slice categories but aggregate case floor remains >=128.

Construction requirements:

- local-only;
- explicit provenance per case;
- labels derived from intended tool semantics, not model predictions;
- no remote teacher;
- no selected-model inference during construction.

Run the existing content-derived leakage machinery against **all** historical C001 train/dev/test inputs plus the new holdout.

Required:

- zero exact input overlap;
- zero normalized input overlap;
- zero template-lineage overlap;
- zero explicit leakage-family overlap.

Commit the holdout and manifest in C001. After C001 closes, they are immutable for C002.

## 10. Historical test handling

The original C001 final test may still be evaluated in C002 as a **diagnostic continuity slice**, but because its results have already been observed:

- it cannot be the sole basis for a positive disposition;
- model/gate selection cannot use it;
- any disagreement between fresh holdout and historical test must be reported.

## 11. Release-mode resource methodology

Add a qualification resource command/path that measures the built artifact, not Cargo compilation.

Required reference command shape:

```bash
cargo build --release --locked --features tool-advisor-encoder-training --bin codegg
./target/release/codegg tool-advisor sequence-encoder-qualify-v2 ...
```

Do not include compilation time.

Measure:

- encoder/tokenizer/head bytes;
- release binary bytes with encoder feature;
- release binary bytes for the comparable build without encoder experiment feature;
- binary delta;
- peak RSS;
- >=5 independent process launches for model load;
- process-cold load first/median/p95;
- warmed per-case rank latency p50/p95/max;
- retrieval/query p50/p95/max;
- total qualification time;
- encoder forwards per case;
- cache warm/cold counts.

Document that OS filesystem cache is uncontrolled unless the host provides a safe reproducible mechanism; call the metric `process_cold_load`, not physical-disk cold load.

Preserve the historical 10-second load budget as a preregistered gate in C002 unless C001 finds that the intended production semantics require a stricter interpretation. Do not relax it based on qualification outcomes.

## 12. Correct disposition semantics

Implement deterministic disposition logic:

- **A — qualify for live M004:** all correctness, quality, calibration, promotion, authority, and resource gates pass.
- **B — quality gain but deployment cost too high:** quality/calibration/promotion/safety gates pass, one or more resource gates fail.
- **C — retrieval useful, ranker not qualified:** retrieval gates pass but ranking-quality/generalization gates fail.
- **D — no useful quality gain:** sequence ranker does not clear the preregistered quality/generalization gates.
- **E — correctness/framework failure:** artifact/hash/leakage/authority/harness correctness cannot be established.

Do not map all non-A outcomes to D.

## 13. Qualification-v2 protocol schema

Create a v2 preregistration schema that includes:

- frozen candidate identity;
- old diagnostic dataset fingerprints;
- fresh-holdout fingerprint;
- slice definitions/floors;
- baseline artifact hashes;
- dev-only calibration/threshold provenance;
- promotion gates;
- quality/generalization gates;
- resource gates;
- release command;
- target hardware/OS descriptor fields;
- declared gate IDs.

Avoid a self-referential commit field inside the hashed manifest. Prefer:

- protocol content hash excludes only `protocol_hash`;
- preregistration commit SHA is supplied to the final command/closure and echoed in the result, not embedded into the content that creates that same commit.

## 14. Required regression tests

At minimum:

- missing contextual historical artifact does not erase contextual slices;
- contextual slice gate uses case membership, not another model's aggregate MRR;
- every required slice is present or qualification fails closed;
- raw and calibrated metrics are both emitted;
- no-tool F1 gate executes;
- promotion metrics execute and authority negatives remain zero;
- disposition A/B/C/D/E unit cases;
- candidate artifact hash drift rejects evaluation;
- fresh-holdout historical leakage is rejected;
- release resource report requires binary/RSS/load/p50/p95 fields;
- C001 holdout-construction command cannot invoke sequence inference.

## 15. Verification

```bash
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib tool_advisor
cargo test --workspace --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
cargo build --release --locked --features tool-advisor-encoder-training --bin codegg
```

Do not run the fresh final holdout through the selected model during C001.

## 16. Acceptance

C001 closes when:

- qualification-v2 harness implements every required evidence family;
- the selected model artifact identity is frozen and unchanged;
- the >=128-case fresh holdout is committed and proves zero historical leakage;
- release-mode resource instrumentation is complete;
- regression tests lock the historical M005 gaps;
- no final selected-model metrics on the new holdout have been generated.

Positive C001 closure makes C002 ready.
