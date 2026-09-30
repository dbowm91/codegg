# Tool-Selection Advisor Retrieval-Signal Experiment M001R — Closure Status

Status: closed (positive)

Source implementation plan:

- `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/001r-signal-preregistration-after-evaluation-corrective.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md#m001r--signal-preregistration-after-evaluation-corrective`

Repository baseline reviewed: `43a95f64`

Implementation commits:

- `43a95f64` — tool-advisor M001R: freeze Signal V2 preregistration against inferable target

## 1. Executive finding

M001R closes positively. The corrected inferable retrieval target from C001
(disposition B) is bound by fingerprint, and every Signal V2 plus conditional
learned-projection degree of freedom M002/M003 may consume is frozen with no
new evidence defect.

- Receipt `assets/tool-advisor/retrieval-signal-m001r-preregistration.json`,
  fp `eda1f6b6ce6637fa4911268871a385b68300924a3069743503406a754423401b`.
- Target binding: corpus `06da7e53…`, derived view `23edef17…`
  (`retrieval-relevance-v1`, 280 entries), dev `b804b7d8…`, train
  `c67df3ca…`, test `1765ad09…`; eligible train 113/0/25, dev 53/0/19, test
  53/0/17; C001 closure/disposition B/implementation `8613575d`.
- Gates unchanged: 64≥0.99, 128≥0.98, 256≥0.95 at K≤32, universes 64/128/256,
  primary Ks 16/24/32, zero authority violations.
- Signal V2 frozen: 8 candidate fields with byte caps (total 4096), 5 query
  fields with byte caps (total 8192, max 2 next-steps), generic identifier
  normalization, frozen lexical weights, 8-mode deterministic grid, mean/CLS
  pooling, RRF K=60, union alpha=0.5. No per-tool aliases.
- Conditional projection frozen (M003 only): shared-linear (49,280 params),
  asymmetric-linear (98,560), asymmetric-2-layer (131,584), all ≤500k; grid
  LR {1e-4,2e-4}, epochs {5,10}, temp {0.05,0.07}, InfoNCE, seeds {7,42,123},
  hard negatives {7,15}, batch {64,128}. MiniLM stays frozen; span-packed
  ranker stays frozen.
- No ranker blocker (C001 §4 holds by binding); M001 remains historical
  blocked evidence and was not reopened; v3/future-v4 selected nothing.

Positive M001R makes M002 ready. M003 stays conditional on a valid negative
M002; M004/M005 and live-primary-model work stay blocked.

## 2. Requirement-to-evidence matrix

| Requirement (M001R plan) | Evidence | Result | Notes |
|---|---|---|---|
| Use only frozen corpus + C001 view/baseline + pinned MiniLM + advisor surface (§2) | `retrieval_signal.rs`: corpus/dev/derived constants; encoder manifest + ranker selection paths bound; no v3/v4 read | pass | v3 never selects |
| Freeze representation contract: candidate/query fields, caps, normalization, weighting, mode grid (§3) | `CANDIDATE_FIELDS/CAPS`, `QUERY_FIELDS/CAPS`, `normalize_identifier`, `LEXICAL_FIELD_WEIGHTS`, `DETERMINISTIC_MODES`, receipt fields | pass | 8 candidate fields, 5 query fields, generic normalizer |
| No per-tool aliases (§3) | `no_per_tool_aliases=true`, no alias map in receipt, `no_per_tool_aliases_allowed` + tamper test | pass | Generic normalizer only |
| Freeze conditional projection grid only if justified (§4) | 3 `ProjectionOption`s with hand-checked param counts + `ProjectionTrainingGrid`; C001-B justification recorded | pass | All ≤500k; MiniLM frozen |
| Bind corpus/derived/counts/C001/gates in every receipt (§5) | Receipt carries all five + `validate_preregistration` live-binding check | pass | Any derived-view change invalidates |
| Ranker-impact dependency: stay blocked on disposition C (§6) | C001 disposition B bound; no ranker corrective exists | pass | No ranker blocker |
| Commit compact receipt superseding M001 (§7) | `assets/tool-advisor/retrieval-signal-m001r-preregistration.json` (fp `eda1f6b6…`) | pass | Replaces unproduced M001 receipt |
| Positive close only with no new evidence defect (§8) | 10 prereg tests + 73 `tool_advisor` tests; no defect found | pass | M002 ready |

## 3. Production implementation evidence

- `src/tool_advisor/retrieval_signal.rs` (new, ~870 lines): frozen constants,
  `normalize_identifier`/`cap_field`, `preregistration()` built from live
  corpus + derived view, `prereg_fingerprint`, atomic load/write,
  `validate_preregistration` (schema/protocol/dataset/view/counts/gates/
  fields/modes/alias prohibition/param caps/fingerprint/live rebinding),
  10 regression tests plus ignored `generate_prereg_asset`.
- `src/tool_advisor/mod.rs`: `pub mod retrieval_signal;` only.
- Receipt (181 lines JSON): protocol `m001r-preregistered-signal-v2-v1`,
  schema 1, dataset `assets/tool-advisor/corpus.jsonl`, derived view
  `assets/tool-advisor/retrieval-relevance-v1.json`, adjudication
  `retrieval-relevance-v1`, train fp `c67df3ca…`, dev `b804b7d8…`, test
  `1765ad09…`, eligible as §1, C001 closure path/disposition
  B/implementation `8613575d`, gates/universes/Ks, full representation +
  projection grids, `no_per_tool_aliases=true`, fp `eda1f6b6…`.
- Live-computed train/test fps match the historically known values
  (`c67df3ca…`/`1765ad09…`), confirming partition stability; no corpus file
  changed (`git diff` shows only `mod.rs` + new files).

## 4. Verification executed (local truth; no hosted CI required by M001R)

```bash
cargo test --locked -p codegg --lib -- tool_advisor::retrieval_signal
cargo test --locked -p codegg --lib -- tool_advisor
cargo test --locked -p codegg --lib -- tool_advisor::retrieval_signal::tests::generate_prereg_asset --ignored --nocapture
cargo fmt --all -- --check
git diff --check
python3 scripts/check_execution_ownership.py
bash scripts/check-core-boundary.sh
scripts/verify.sh quick
cargo clippy -p codegg --lib --locked -- -D warnings
```

Results:

- `retrieval_signal`: 10 passed, 0 failed (1 ignored generator). Covers
  C001 binding, derived-view match, fingerprint determinism, generic
  normalization, frozen caps/weights, param-cap exact counts, alias
  prohibition, unrelaxed gates, committed-receipt live match, tamper-closed.
- `tool_advisor` (default features): 73 passed, 0 failed (3 ignored).
- Receipt generator (ignored, ~1 s): wrote fp `eda1f6b6…`, derived
  `23edef17…`, dev 53/0/19.
- `cargo fmt --check`, `git diff --check`: clean after `cargo fmt --all`.
- `check_execution_ownership.py`: ok. `check-core-boundary.sh`: passed.
- `scripts/verify.sh quick`: passed (agents, core-boundary, sandbox,
  execution-ownership, tui-authority, http-route, audit, scheduler-bypass,
  eggwork target-routing, workspace check).
- Focused `cargo clippy -p codegg --lib -- -D warnings` is blocked by an
  unrelated pre-existing failure in `codegg-providers`
  (`collapsible_else_if` in anthropic chunk parsing); no `retrieval_signal`
  warning was reported. Full workspace/all-features Clippy remains blocked
  by the same unrelated state plus the known `egglsp`/`candle-core`
  toolchain failures recorded in C001. Canonical hosted CI is required
  before M002 close, not for this preregistration freeze.

## 5. Invariant review

- Frozen corpus/view unchanged (corpus `06da7e53…`, dev `b804b7d8…`,
  derived `23edef17…` tripwires held; `committed_*_matches_*` tests).
- No v3/v4 tuning or reads for selection; v3 never selects.
- No retrieval/model algorithm change: no retriever implemented, no weights
  trained, no ranker retouched.
- No gate relaxation (0.99/0.98/0.95 at K≤32 enforced literally; tamper
  test fails a 0.90 gate).
- No tool authority widening: deferred-only universe unchanged; receipt
  carries no authority path.
- No per-tool repair: aliases forbidden by constant + test + receipt field.
- No user/private runtime context in receipt (corpus fingerprints and
  counts only, no context text).
- Default-off/local-only posture unchanged; no telemetry, download, network,
  or runtime-path change.

## 6. Failure and recovery review

- Corpus/dev/derived drift, count drift, gate relaxation, field/mode-grid
  drift, alias addition, projection over-budget, fingerprint mismatch, live
  rebinding drift: all fail closed in `validate_preregistration`
  (unit-tested via tamper tests).
- Checkpoint/resume not needed: prereg build is ~2 s deterministic; no
  partial state.

## 7. Migration and compatibility review

Additive module + asset only; no storage/protocol/config migration, no
artifact format change, no rollback concern. Prereg schema v1 is
forward-only; any future representation change needs a new protocol version
and invalidates this receipt by fingerprint.

## 8. Security review

No authorization, secret, network, or privilege surface touched. Receipt
contains fingerprints, counts, field names, caps, weights, and grid values
only — no prompts, contexts, paths, or credentials.

## 9. Documentation and operations

M002 consumes `SIGNAL_PREREG_PROTOCOL`, `SIGNAL_PREREG_ASSET_PATH`, and the
frozen constants directly; no operator action is needed. Roadmap/registry
updates (below) are the only planning documentation.

## 10. Unresolved findings

None. Severity: no open high/medium/low findings against M001R scope.

Toolchain note (not a finding against this milestone): full
workspace/all-features Clippy and encoder-feature builds remain blocked by
unrelated `codegg-providers`/`egglsp` lints and `candle-core` nightly
errors; M002 (which needs the MiniLM encoder) must re-measure with encoder
assets and obtain hosted qualification before operating-point selection.

## 11. Roadmap disposition

Positive close. M002 (`002-deterministic-retrieval-signal-v2.md`) becomes
ready: it must implement exactly the frozen representation and measure the
same 64/128/256 × 16/24/32 dev frontier without training. M003 stays
conditional on a valid negative M002. M004/M005, live-primary-model M004,
order-invariance M005, and v4 remain blocked. M001 stays blocked/closed as
historical evidence and MUST NOT be reopened.

## 12. Registry updates

- `plans/registry.md`: M001R ready→closed (implementation `43a95f64`,
  closure this record, receipt fp `eda1f6b6…`); M002 blocked→ready (positive
  M001R satisfies its hard dependency); retrieval-signal gate paragraph
  updated; blocked-work row updated (C001 satisfied, M001R closed, M002
  ready).
- `plans/subsystems/tool-selection-advisor-retrieval-signal-experiment-roadmap.md`:
  M001R ready→closed, M002 blocked→ready.
- `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/001r-signal-preregistration-after-evaluation-corrective.md`:
  active→implemented (this closure).
- `plans/implementation/tool-selection-advisor-retrieval-signal-experiment/002-deterministic-retrieval-signal-v2.md`:
  blocked→ready for handoff.
- No other workstream is unblocked by this closure (Eggwork M003, CI C002,
  and Eggplan M002 were already ready on independent dependencies).
