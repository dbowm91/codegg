# Tool-Selection Advisor Late-Interaction Retriever M004 — Retrieval, Ranker, and Promotion Operating Point

Status: blocked on eligible M001/M002/M003 retrieval candidate — TERMINAL:
no candidate qualified (M001 D), so M004 never opens. No operating point
is frozen in this workstream.

Repository baseline: `c4cc6c5b3c9154428b8560a60aa0129624435dfe`

Source roadmap:

- `plans/subsystems/tool-selection-advisor-late-interaction-retriever-experiment-roadmap.md#m004--qualified-retrieval--promotion-operating-point`

Hard dependencies:

- one retrieval candidate qualified by M001 A, M002 A, or M003 A;
- frozen span-packed ranker from order-invariance M003;
- retrieval-signal closure corrective C001 closed.

Primary class: integration/operating-point qualification.

## 1. Objective

Freeze one production-shaped local advisor path:

```text
ResolvedToolSurface
  -> qualified late-interaction retrieval
  -> frozen span-packed ranker
  -> separate abstention + candidate-relevance calibration
  -> bounded proactive promotion
```

No new retrieval architecture is selected in M004.

## 2. Retrieval operating point

Use exactly one qualified predecessor artifact/mode.

Select the smallest K already shown to satisfy:

- u64 >=0.99;
- u128 >=0.98;
- u256 >=0.95;
- zero authority violations.

Do not retune projection/encoder weights, text construction, MaxSim, or fusion.

## 3. Descriptor token cache

Production-shaped cache requirements:

- keyed by resolved-surface fingerprint + descriptor fingerprint + encoder/tokenizer hash + late-interaction artifact hash + token-contract version;
- stores descriptor token vectors only;
- never stores user query/context;
- invalidates on any authority/descriptor/artifact change;
- deterministic size accounting;
- hard cap derived from qualified 256-tool evidence.

If the eligible artifact uses 128-d token projections, cache projected vectors rather than raw 384-d vectors.

## 4. Catalog bound

The experiment qualifies through 256 eligible deferred tools.

For catalogs above the qualified bound:

- do not silently claim late-interaction qualification;
- do not widen K;
- default to existing authority-safe discovery behavior and suppress late-interaction proactive promotion unless a separately qualified bounded prefilter exists.

ANN/indexing is out of scope.

## 5. Downstream span-packed ranker

Pass only retrieved authority-eligible candidates to the frozen selected span-packed ranker.

Verify:

- ranker artifact/hash unchanged;
- retrieval ordering does not reintroduce the old ordinal shortcut;
- mapped top-1 identity consistency remains >=0.95;
- candidate truncation is deterministic and authority-safe.

Report end-to-end Recall@1/3/5, MRR, nDCG, no-tool behavior, and family slices.

## 6. Promotion

Keep distinct:

- abstention calibration;
- candidate-relevance calibration;
- promotion threshold.

Promotion may occur only when:

- ranker does not abstain;
- candidate relevance exceeds its own calibrated threshold;
- candidate is deferred and authority-eligible;
- max promotion/schema caps hold.

Dev targets:

- relevant promotion recall >=0.60;
- no-tool promotion <=0.10;
- irrelevant promotion <=0.15;
- max promotions <=2;
- schema p95 <=16 KiB;
- authority violations =0.

## 7. Resources

Release mode on recorded reference hardware:

- encoder weights/artifact bytes;
- descriptor cache at 64/128/256;
- process-cold p50/p95;
- query encoding p50/p95;
- MaxSim arithmetic p50/p95;
- ranker p50/p95;
- total advisor p50/p95/max;
- RSS.

Hard gates:

- process-cold p95 <=10 s;
- descriptor cache <=64 MiB at 256;
- total warm retrieval p95 <=1.0 s;
- total advisor p95 <=1.5 s.

If a predecessor already froze stricter bounds, retain the stricter values.

## 8. Operating-point artifact

Freeze:

- retrieval architecture/mode;
- token contract;
- projection/encoder hashes;
- K;
- cache contract;
- span-packed ranker hash;
- abstention calibration;
- candidate-relevance calibration;
- promotion threshold;
- dev fingerprints;
- resource evidence.

No v3/fresh-v4 parameter enters this artifact.

## 9. V3 diagnostic

After the operating point is frozen, run v3 once diagnostically for continuity.

No change may follow from v3.

## 10. Verification

```bash
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor
scripts/verify.sh quick
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Require green hosted default-feature CI on the freeze commit before M005 preregistration.

## 11. Acceptance

M004 closes positively only if retrieval, downstream ranker, promotion, authority, cache, and resource gates all pass.

Positive M004 makes M005 ready. Otherwise close the workstream negative without consuming v4.
