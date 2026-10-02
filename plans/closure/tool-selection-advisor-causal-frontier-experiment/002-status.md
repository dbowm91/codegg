# Tool-Selection Advisor Causal Frontier Experiment M002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-selection-advisor-causal-frontier-experiment/002-offline-causal-admissibility-frontier.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-causal-frontier-experiment-roadmap.md#m002--offline-causal-admissibility-frontier`

Repository baseline reviewed: `a2ec2c62`

Implementation commits or pull requests:

- `85f26afd` — tool-advisor causal-frontier M002: offline causal-admissibility frontier

## 1. Executive finding

M002 is complete and closes with disposition **A — positive
causal-admissibility architecture**. The milestone evaluated deterministic
precondition filtering offline over the already-resolved eligible surface,
with no runtime visibility change: a typed `CausalFrontier` result
(admissible promotion vs. reasoned inadmissibility, uncontracted fallback,
required bypass, withheld fail-closed), frozen dev baselines (full eligible
universe + `CORE_PALETTE` projection), a diagnostic-only historical-label
classification, one scoring of the untouched qualification partition, and a
machine-readable receipt verified by test against live recomputation. Every
frozen M001 gate passes on qualification (preservation 1.00/1.00, 0
violations, reduction 1.00, median deferred promotion 3, p95 0.03 ms against
a 5 ms budget). Positive M002 unblocks M004 observe-mode integration and
opens the optional M003 effect-path experiment.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Frontier semantics (§2) | `CausalFrontier::evaluate` (`src/tool_advisor/causal_frontier.rs`): contracted tools check `requires_all` + each `requires_any` group + `forbids`; uncontracted tools enter fallback only; required/never-reduce bypass | pass | BTreeSet-ordered; surface order cannot affect the result (`m002_surface_order_does_not_affect_frontier`) |
| Fail-safe result type (§3) | `CausalFrontier` (admissible/inadmissible+`CausalInadmissibilityReason`/fallback/required/fingerprints); withheld→`Err`, missing required→`Err` | pass | `m002_withheld_tool_fails_closed`, `m002_required_tool_bypasses_causal_suppression`, `m002_fingerprint_drift_invalidates_frontier` |
| Baselines on frozen dev (§4) | `score_partition` dev arm: full-eligible premature baseline 328; `CORE_PALETTE` projection p50/max 21 visible, 101 premature in core; no retraining or model change | pass | `m002_dev_baselines_report`; Signal V2 relevance labels classified diagnostically only |
| Metrics (§5) | `CausalPartitionReport` + per-family/per-case rows: preservation, premature baseline/remaining/reduction, contracted frontier p50/p95/max via sorted sizes, core projection, uncontracted retention, mutating exposure via premature accounting, required/never-reduce preservation, authority/abstention violations, p95 latency | pass | Receipt `assets/tool-advisor/causal-frontier-m002-result.json` carries dev + qual reports with all rows |
| Historical 53-label diagnostic (§5) | `historical_label_candidates` + `classify_historical_label` over frozen `retrieval-relevance-v1.json`: 38 distinct current-step labels → 4 admissible-any-state / 2 state-gated / 1 uncontracted-known / 31 unavailable-unknown | pass | Diagnostic only; causal benchmark owns selection |
| Qualification discipline (§6) | Qual partition (56) resolved from frozen prereg ids and scored once by the same deterministic evaluation; no contract edits before/after (contracts frozen in M001, catalog fp `0ac06de8…` re-verified by `verify_against`) | pass | `m002_qualification_gates_freeze_disposition_a`, `m002_qualify_reports_disposition_a` |
| Disposition A (§7) | All frozen gates pass on qual; `decide_disposition` returns Positive; receipt records `disposition: "A"` with all 7 `gate_results` true | pass | M004 unblocks; M003 may run as optional |
| Regression tests (§8) | 14 new tests: required bypass, uncontracted fallback, missing-fact determinism, contradictory state, surface order, fingerprint drift, authority fail-closed, no provider definitions, dev baselines, qual gates, p95 budget, label diagnostic, end-to-end disposition, receipt recomputation | pass | 62/62 module tests pass (48 M001 + 14 M002) |
| No provider-definition mutation (§8) | Implementation touches only `src/tool_advisor/causal_frontier.rs`, `architecture/tool-advisor.md`, and the new receipt asset; frontier serializes to names + fingerprints only | pass | `m002_frontier_carries_no_provider_definitions`, `git show --stat 85f26afd` |

## 3. Production implementation evidence

Implementation `85f26afd` (3 files, +4469/−6):

- `src/tool_advisor/causal_frontier.rs` (+1384): `CausalFrontier` +
  `FrontierInputs` + `CausalFrontierError` + `CausalInadmissibilityReason`
  (evaluation order: missing-required, unsatisfied-any-group,
  forbidden-present), `deferred_promotion` (admissible ∩ non-`CORE_PALETTE`
  ∩ non-required), `promotion_for_use` (empty without structured signal),
  `visible_union`, `is_fresh_against`, benchmark-case evaluation,
  `score_partition` with per-case/per-family rows, nearest-rank latency
  measurement, `decide_disposition` (integer-exact correctness, recorded
  ratio for reduction), historical-label diagnostic, `qualify_m002`
  end-to-end runner, 14 regression tests plus an ignored receipt
  regenerator.
- `assets/tool-advisor/causal-frontier-m002-result.json` (new): frozen
  receipt — benchmark fp `f60dad17…`, catalog fp `0ac06de8…`, palette fp
  `3bc5516f…`, dev fp `09915ce1…`, qual fp `010c66be…`, dev + qual
  reports, latency (1001 iters/surface, 56056 samples, p50 0.024 / p95
  0.027 / max 0.22 ms), 7 gate results, disposition A, label diagnostic.
- `architecture/tool-advisor.md`: M002 offline-admissibility subsection.

Measured outcome (qualification, 56 cases / 41 structured):

- gold current-step preservation 162/162 = 1.00 (gate 1.00);
- authority violations 0 (gate 0); abstention violations 0;
- uncontracted fallback preservation 4/4 = 1.00 (gate 1.00);
- premature exposure: baseline 164, remaining 0, reduction 1.00 (gate ≥ 0.50);
- median deferred promotion (structured) 3, max 5 (gate ≤ 4);
- pure-evaluation p95 0.027 ms (gate ≤ 5.0 ms).

Dev (112 cases / 81 structured): gold 319/319, premature baseline 328 /
remaining 0, median promotion 3, core-projection p50/max 21 visible with
101 premature in core (baseline arm 1 measures a real exposed count that
causal filtering removes).

Planned but absent by design (owned by later milestones): M003 effect-path
refinement, M004 observe-mode runtime integration, M005 active disclosure.
No provider-definition or palette behavior change.

## 4. Verification executed (commands + results; label local vs CI truthfully)

Local (repo HEAD at implementation commit `85f26afd`):

- `cargo test -p codegg --lib tool_advisor::causal_frontier`: 62 passed /
  0 failed / 1 ignored (the ignored test is the receipt regenerator).
- `cargo test -p codegg --lib tool::contract`: existing contract suite
  unaffected (no `src/tool/` edits).
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: clean
  (two new lints self-caught and fixed during implementation:
  `needless_lifetimes`, `bool_assert_comparison`).
- `cargo fmt --all -- --check`: clean. `git diff --check`: clean.
- `scripts/verify.sh quick`: passed (agent schema, core-boundary,
  client/desktop boundaries, sandbox, execution-ownership, TUI authority,
  HTTP-route disposition, audit coverage, scheduler bypass, provider-wire
  guards, workspace check).
- Receipt recomputation:
  `m002_checked_in_receipt_matches_recomputation` re-runs `qualify_m002`
  over the frozen assets and asserts byte-identical deterministic fields;
  latency is compared by gate (environment-sensitive), both stored and
  fresh p95 within budget.

Hosted CI: implementation commit `85f26afd` pushed to `main`; the ordinary
`verify` run is pending at closure-write time (see §10 low finding and §12).

## 5. Invariant review

- `ResolvedToolSurface` remains the only per-turn capability ceiling: M002
  consumes post-authority eligible sets and withheld lists; it grants
  nothing and restores nothing.
- Causal filtering changes visibility only: the frontier is computed
  offline; no disclosure, broker, permission, or request-preparation code
  was touched.
- Required/never-reduce tools remain visible: bypass is structural in
  `evaluate` (missing required is an error, never a suppression), and
  bypassed tools are excluded from reduction accounting per tie-breaking.
- Uncontracted tools stay discoverable: fallback membership is asserted
  per case; the 4 uncontracted qual gold appearances are all retained.
- Hidden/denied/disabled tools can never be promoted: withheld input fails
  closed and output containment is double-checked in `score_partition`.
- No remote telemetry, no model weights, no new inference runtime, no
  chain-of-thought parsing: the frontier uses only typed facts and static
  contracts.
- State projection unchanged from M001: bounded facts/counts/ids only.
- Contract integrity unchanged: catalog fp re-verified against the prereg
  on every `qualify_m002` run; drift fails the run.
- Qual discipline: contracts frozen in M001 were not edited; dev and qual
  are scored by one deterministic pass with no tunable parameters.

## 6. Failure and recovery review

- Withheld tool in surface → `Err(WithheldInSurface)`; scoring aborts
  rather than promoting. Covered by test.
- Required tool missing from surface → `Err(RequiredNotEligible)`.
  Benchmark validation already forbids this; the constructor fails closed
  independently.
- Snapshot/catalog drift → `is_fresh_against` false; receipt recomputation
  fails loudly on palette/disclosure drift via the palette fingerprint.
- Benchmark bytes ≠ prereg fp, or split/catalog/gate drift → `qualify_m002`
  errors before scoring.
- Contradictory fact sets (all 17 facts) evaluate without panic.
- Disposition E path (`ContractFailure`) is implemented and unit-decided
  but not taken: correctness is exact.

## 7. Migration and compatibility review

Additive only: new types/functions/tests/asset; no existing API changed
(`Tool::causal_contract` seam untouched). The receipt is a new file with a
versioned schema (`CAUSAL_M002_REPORT_SCHEMA_VERSION = 1`). No storage,
protocol, config, or migration impact.

## 8. Security review

No new trust boundary: contracts remain static native metadata; no
external/MCP contract participates; no tool output can rewrite a contract
(contracts are `&str`-keyed statics evaluated against host facts). The
frontier cannot widen authority (fail-closed + output audit) and carries no
definitions or payloads that could leak into provider requests — it
serializes to names + fingerprints only. No secrets in snapshots (M001
property, unchanged).

## 9. Documentation and operations

- `architecture/tool-advisor.md`: M002 subsection (semantics, baselines,
  receipt, measured outcome, unblock consequences).
- Receipt regeneration is documented and reproducible: ignored test
  `m002_regenerate_checked_in_receipt` reruns `qualify_m002` over the
  frozen assets; the always-run recomputation test pins every
  deterministic field.
- Operator action: none. M004 implementors consume the receipt (disposition
  A + frozen promotion semantics); the qualification partition must not be
  used for contract tuning (contracts remain frozen; any future contract
  change is a new experiment version).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low (resolved) | Hosted CI result pending at closure-write time (local evidence only). | Positive closure assumed ordinary hosted CI green per plan §11. | Reconciled: hosted `CI / verify` run `37017387365` success on implementation commit `85f26afd` (includes all M002 code + receipt). Assumption holds. |

No medium or higher findings. No corrective pass required. One
implementation-time defect was found and fixed before commit (reduction
metric counted removed instead of remaining tools, inverting the gate;
caught by the dev-baseline test, corrected to the frozen formula
semantics).

## 11. Roadmap disposition

M002 closes with disposition A (positive). Consequences:

- M004 (observe-mode runtime integration) is dependency-ready: its hard
  dependency (positive M002) is satisfied. M004 moves blocked → ready in
  the same commit as this record.
- M003 (structured effect-path frontier) is dependency-ready as the
  optional experiment: its dependency (M002 A) is satisfied. M003 moves
  blocked/optional → ready (optional) in the same commit.
- M005 stays blocked on positive M004. No other registered plan lists
  M002 as a dependency; nothing else is unblocked.
- The subsystem roadmap M002 row moves active → closed (A); M003 row moves
  blocked/optional → ready (optional); M004 row moves blocked → ready.

## 12. Registry updates

- `plans/registry.md` subsystem row: causal-frontier experiment
  `M001 closed (positive); M002 ready; M003-M005 dependency-gated` →
  `M001 closed (positive); M002 closed (A); M003 ready (optional); M004
  ready; M005 dependency-gated`.
- `plans/registry.md` plan rows: M002 `active` → `closed` (this record,
  implementation `85f26afd`, disposition A); M003 `blocked/optional` →
  `ready` (optional; M002 A satisfied); M004 `blocked` → `ready`
  (positive M002 satisfied). M005 row unchanged.
- `plans/subsystems/tool-selection-advisor-causal-frontier-experiment-roadmap.md`:
  status line, M002 status active → closed (A) with receipt pointer; M003
  status blocked/optional → ready (optional); M004 status blocked → ready.
- `plans/implementation/tool-selection-advisor-causal-frontier-experiment/002-*.md`:
  status active → closed (pointer to this record).
- `plans/implementation/tool-selection-advisor-causal-frontier-experiment/003-*.md`:
  status blocked/optional on M002 A → ready (optional; dependency met).
- `plans/implementation/tool-selection-advisor-causal-frontier-experiment/004-*.md`:
  status blocked on positive M002 → ready for handoff.
- Hosted CI: ordinary `verify` run `37017387365` success (on `85f26afd`,
  includes all M002 code + receipt). Positive-closure assumption holds;
  no downgrade.
