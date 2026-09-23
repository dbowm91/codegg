# Tool-Selection Advisor Order-Invariance Experiment M004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-selection-advisor-order-invariance-experiment/004-retrieval-and-promotion-operating-point.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-order-invariance-experiment-roadmap.md#m004--retrieval-and-promotion-operating-point`

Repository baseline reviewed: `3a9428a9ea347b1f84f7dd59c0a0f1339ee75a11`

Implementation commits or pull requests:

- `f04ba608` — order-invariance M004: retrieval and promotion operating point
- `3a9428a9` — order-invariance M004: checkpoint/resume per-universe frontier

Disposition: **negative — no retrieval operating point clears the preregistered gates at K<=32; no operating point frozen; M005 stays blocked.**

## 1. Executive finding

M004 closes negatively per the plan-literal stop condition (§3/§12:
"If no K<=32 clears the frontier, close negatively ... Do not
silently raise candidate budget"). The preregistered sweep
(`m004-preregistered-operating-point-v1`) ran once to completion on
dev only (3930.46 s, EXIT=101 by design): all three retrieval
universes (64/128/256) completed with 12/12 frontier points each and
zero authority violations, but the best observed recall is 0.9444 at
every universe — below all three gates (64≥0.99, 128≥0.98,
256≥0.95). `select_retrieval_operating_point` therefore returns the
expected `no retrieval operating point clears 0.99/0.98/0.95 at
K<=32` error; promotion, order-robustness, resource, and v3 stages
are not reached by design (fail-closed, no silent K increase). No
retrieval mode/K, promotion threshold, or artifact is frozen. M005
remains hard-blocked; a separate retrieval-architecture experiment
may be registered but is not created here.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Train/dev-only fixtures; 64/128/256 universes preserve every labeled relevant tool; no v3/v4 tuning | `expand_universe`, `universe_expansion_preserves_relevant_tools`; sweep consumes M003 span-packed artifact + dev corpus only | pass | Checkpoints: 62 cases / 72 relevant tools per universe; sweep fingerprint `0e802d79…` |
| Predeclared frontier K=16/24/32 × BM25/semantic/RRF/union; no new learned retriever | `FRONTIER_UNIVERSES=[64,128,256]`, `FRONTIER_KS=[16,24,32]`; 36/36 points measured | pass | Per-universe checkpoint receipts under `target/tool-advisor/order-invariance/m004/` |
| Smallest/cheapest point with 64≥0.99, 128≥0.98, 256≥0.95, zero violations, K≤32 | `select_retrieval_operating_point` + `retrieval_selection_enforces_gates`; full frontier below | fail (plan-literal negative) | Best 0.9444 everywhere; see §4 table; error is the specified fail-closed outcome |
| Zero authority violations | Checkpoint `authority_violations: 0` at 64/128/256 | pass | Necessary but insufficient given recall miss |
| Promotion threshold separate from abstention; calibrated relevance sweep on dev grid | Implemented (`PromotionCaseView`, `select_promotion_threshold`, field-separation test) but not executed | not run | Correctly gated behind retrieval selection; code covered by fast unit tests only |
| Order robustness / resources / v3 diagnostic at frozen point | Not executed | not run | No frozen point exists; v3 correctly never runs (plan forbids tuning from v3 and forbids diagnostics without a frozen point) |
| K24/K32 universe/shortlist identity; missing point fails closed; no silent budget raise | `RetrievalFrontierPoint` carries `candidate_universe_size` + `shortlist_k`; sweep errors instead of raising K | pass | Matches plan §3 stop condition |

## 3. Production implementation evidence

`src/tool_advisor/operating_point.rs` (+ `mod.rs` gating):
`OPERATING_POINT_PROTOCOL=m004-preregistered-operating-point-v1`,
`OPERATING_POINT_SCHEMA_VERSION=1`,
`FRONTIER_UNIVERSES`/`FRONTIER_KS`, `expand_universe`,
`select_retrieval_operating_point` (K-first, latency-second,
mode-name tiebreak), promotion separation
(`AdvisorOperatingPoint::new` rejects promotion==abstention),
per-universe checkpoint/resume
(`m004-checkpoint-frontier-{size}.json` bound to sweep
fingerprint), fail-closed missing-point behavior. No retrieval,
ranking, authority, storage, protocol, or default-surface change
beyond the experiment module.

## 4. Verification executed

### Commands run

```bash
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor::operating_point::tests::m004_operating_point_sweep --ignored --nocapture
cargo test --locked --features tool-advisor-encoder-training -p codegg --lib -- tool_advisor::operating_point:: --skip m004_operating_point_sweep
cargo fmt --all -- --check
git diff --check
```

### Results

- M004 sweep (ignored, single run, ~65.5 min): **FAILED as the
  plan-literal negative outcome** — `finished in 3930.46s`,
  `test result: FAILED. 0 passed; 1 failed`, panic
  `operating-point selection: no retrieval operating point clears
  0.99/0.98/0.95 at K<=32` (`src/tool_advisor/operating_point.rs:1293`),
  EXIT=101. Log `/tmp/m004b.log`, done flag `/tmp/m004b.done`.
- Frontier evidence (dev, 62 cases / 72 relevant tools per universe,
  authority violations 0):

| universe | best recall | best configs | gate | verdict |
|---|---|---|---|---|
| 64 | 0.9444 | normalized-union K16/K24/K32; rrf K32 | ≥0.99 | fail (−0.0456) |
| 128 | 0.9444 | normalized-union K16/K24/K32 | ≥0.98 | fail (−0.0356) |
| 256 | 0.9444 | normalized-union K24/K32 (K16 0.9306) | ≥0.95 | fail (−0.0056) |

  Full per-mode recall: BM25 0.7222 flat; semantic 0.8889–0.9028;
  RRF 0.6944–0.9444 rising with K; normalized-union 0.9306–0.9444.
  Even the loosest gate (256≥0.95) has no feasible point, so no
  K≤32 clears the conjunction.
- Fast `operating_point` unit tests: 5 passed, 0 failed (gates,
  field separation, promotion selection, universe preservation).
- `cargo fmt --all -- --check` and `git diff --check`: clean.
- No workspace sweep/clippy rerun for this evidence-only closure:
  no production code changed since `3a9428a9` (M003 closure already
  recorded full workspace + clippy + `verify.sh quick` on this
  code).

## 5. Invariant review

- Train/dev/test/v2/v3 corpora immutable: sweep reads assets only;
  no asset modifications.
- v3 never selects or diagnoses: sweep aborts before the v3 stage;
  no v3 output produced.
- Promotion/abstention stay separate fields; K/universe identity
  explicit per point; authority boundary untouched
  (`authority_violations: 0`).
- Default-off/local-only posture unchanged; no telemetry, download,
  or runtime path change.

## 6. Failure and recovery review

- Missing retrieval point fails closed with an explicit error
  (unit-tested); no silent K increase or budget change.
- Per-universe checkpoint/resume survived two SIGTERM/duplicate-launch
  incidents during the run; resumed sweep reused bound checkpoints
  instead of recomputing.
- Authority violations would fail closed (unit-tested); none
  observed.

## 7. Migration and compatibility review

Additive experiment module only; no storage/protocol/config
migration, no artifact format change, no rollback concern.

## 8. Security review

No authorization, secret, network, or privilege surface touched.
`#![deny(unsafe_code)]` holds. Denied/hidden tools never enter the
promotion universe by construction (promotion stage unreached).

## 9. Documentation and operations

- Implementation plan `004-...md` status moves to implemented
  (evidence gathered, negative result).
- Frontier checkpoints retained under
  `target/tool-advisor/order-invariance/m004/`
  (`m004-checkpoint-frontier-{64,128,256}.json`); no frozen
  `m004-operating-point.json` / `advisor-operating-point.json`
  exists by design.
- Sweep command recorded in the ignored-test rustdoc.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | No K≤32 retrieval configuration meets the preregistered recall gates; best 0.9444 misses even the 256≥0.95 bar | No operating point can be frozen; promotion/order/resource/v3 stages have no input | Do not run M005; register a separate retrieval-architecture experiment if pursued (new plan, not a silent K raise) |
| low | Semantic mean-latency outliers (first-embedding load, e.g. 68–174 s max) dominate wall-clock | Sweep cost ~65 min; no gate impact | Any follow-up architecture plan should budget or cache encoder load explicitly |

No stop-condition violation: the plan explicitly anticipates this
outcome and prescribes negative closure.

## 11. Roadmap disposition

M004 is closed (negative). M005 stays hard-blocked on a positive
M004 operating point, which does not exist; no downstream plan is
unblocked. Live primary-model M004 remains blocked. A future
retrieval-architecture experiment (new retriever, larger K with
bounded latency/schema analysis, or revised gates) requires its own
implementation plan and preregistration — out of scope here.

## 12. Registry updates

- `plans/registry.md`: order-invariance roadmap row M004 ready →
  M004 closed (negative), M005 blocked; M004 plan row active →
  closed (negative, implementations `f04ba608` + `3a9428a9`, no
  downstream unblocked); M005 row stays blocked (now explicitly on
  negative M004); execution-order gate paragraph updated; this
  closure added under recently closed work.
- `plans/subsystems/tool-selection-advisor-order-invariance-experiment-roadmap.md`:
  dependency graph M004 ready → closed (negative); M004 section
  ready → closed (negative); M005 stays blocked on positive M004.
- `plans/implementation/.../004-...md`: active → implemented
  (evidence gathered; negative result).
- `plans/implementation/.../005-...md`: unchanged blocked on M004
  (negative M004 keeps it blocked).
