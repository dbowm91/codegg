# Tool-Selection Advisor Retrieval-Signal Experiment M004 — Retrieval and Promotion Operating Point

Status: blocked on positive M002 or M003

Repository baseline: `1093ad0e3285e8ee66684e8a7f3401a200c0596e`

Dependencies:

- positive deterministic M002 **or** positive learned M003;
- frozen span-packed downstream ranker from order-invariance M003.

Source roadmap:

- `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md#m004--retrieval--promotion-operating-point`

Primary class: operating-point qualification.

## 1. Objective

Freeze one complete dev-only retrieval + downstream promotion operating point before constructing a fresh v4 holdout.

## 2. Retrieval candidate

Input is exactly one selected signal path:

- deterministic Retrieval Signal V2 from M002; or
- frozen projection artifact from M003.

Do not reopen both arms in M004.

## 3. Retrieval gates

Measure from clean checkout:

- universe 64 recall >=0.99;
- universe 128 recall >=0.98;
- universe 256 recall >=0.95;
- K<=32;
- zero authority violations.

Also require:

- persistent four-tool miss set resolved or explicitly reduced without another gate-critical replacement miss;
- unknown/renamed recall no worse than selected-arm dev evidence;
- cache contains no user context.

## 4. Downstream ranker integration

Pass retrieved candidates to the frozen M003 span-packed ranker.

Verify:

- ranker architecture/hash unchanged;
- candidate order does not alter mapped ranking beyond its established permutation tolerance;
- retrieval truncation never changes authority;
- relevant candidate entering the pool can actually be surfaced/ranked.

Report end-to-end:

- retrieval recall;
- ranker Recall@1/3/5 and MRR;
- permutation consistency;
- no-tool behavior.

## 5. Promotion operating point

Reuse the corrected separation between:

- abstention calibration;
- candidate relevance calibration;
- promotion threshold.

Do not reuse abstention threshold as promotion threshold.

Select promotion threshold on dev only with:

- relevant promotion recall >=0.60 target;
- no-tool promotion rate <=0.10;
- irrelevant promotion rate <=0.15;
- max promotions <=2;
- schema p95 <=16 KiB;
- zero authority violations.

If the 0.60 target cannot be met while respecting false-promotion bounds, close negatively; do not weaken thresholds silently.

## 6. Resources

Report release-mode:

- process-cold load p50/p95;
- retrieval p50/p95/max;
- rank p50/p95/max;
- total advisor p50/p95/max;
- RSS;
- encoder weights;
- projection bytes if any;
- cache entries/bytes;
- schema bytes.

Preserve previous hard resource gates unless this plan explicitly preregisters stricter values:

- process-cold p95 <=10 s;
- encoder/shared model weights <=128 MiB.

## 7. Operating-point artifact

Freeze a versioned artifact containing:

- retrieval representation/projection hash;
- MiniLM hashes;
- retrieval mode;
- K;
- query/descriptor schema version;
- downstream ranker hash;
- abstention calibration;
- candidate relevance calibration;
- promotion threshold;
- dev fingerprints;
- resource evidence.

No v3-derived parameter enters this artifact.

## 8. V3 diagnostic

Only after freeze, evaluate v3 once to compare:

- retrieval recall;
- four-tool miss recovery;
- end-to-end rank quality;
- promotion behavior.

Diagnostic only.

## 9. Verification

```bash
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor
scripts/verify.sh quick
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
git diff --check
```

## 10. Acceptance

M004 closes positively only when retrieval, end-to-end ranker integration, promotion, authority, and resource gates all pass. Positive closure makes M005 ready. Otherwise the workstream closes negatively with no fresh holdout consumed.
