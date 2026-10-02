# Tool-Selection Advisor Causal Frontier Experiment M003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tool-selection-advisor-causal-frontier-experiment/003-structured-effect-path-frontier.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-causal-frontier-experiment-roadmap.md#m003--structured-effect-path-frontier`

Repository baseline reviewed: `82fc5b40`

Implementation commits or pull requests:

- `82fc5b40` — tool-advisor causal-frontier M003: structured effect-path frontier (negative, D)

## 1. Executive finding

M003 is complete and closes with disposition **D — negative: effect-path
narrowing is unsafe; M002 remains the selected frontier**. The milestone
implemented the full bounded effect-path refinement specified in the plan
(typed demand derivation with no free-text inference, breadth-first
minimal-path planner with depth cap 3 / no repeats / no cycle expansion /
lexicographic tie-break / no scores, implementation-identity-bound effect
catalog with fail-closed staleness, provenance-carrying advisory results)
and scored it against the M002 reference arm on the 54 frozen
structured-demand benchmark cases (dev 36 / qual 18).

Measurement is unambiguous: every structured demand finds a length-1 path,
the caller-visible median drops from 3 to 1 (reduction gate passes),
premature exposure stays at 0, authority/fallback invariants hold, and
p95 effect-path computation is 0.05 ms against a 5 ms budget. But
narrowing to demand producers hides current-step gold tools the agent
genuinely needs — work-plan/goal-state transitions plus co-producers such
as `verify` and `git` — driving qualification preservation to 0.41 with
32 false causal exclusions across 4 of 5 families. Only
`artifact_recovery` preserves fully. Per the plan's positive criterion
(§6) and acceptance (§9), M003 closes negative and M004 observe-mode
integration selects the positive M002 frontier.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Structured demand sources, no free-text inference (§2) | `derive_structured_demand` (`src/tool_advisor/causal_frontier.rs`): reads only the frozen typed `desired_outcome`; TestJob→TestEvidenceProduced, Commit→CommitEvidenceProduced, Artifact→ArtifactExpanded, DelegatedRun/AgentRun→DelegatedRunProduced; SchedulerJob/absent evidence abstain; kind/evidence mismatch fails closed; plus `derive_unmet_demands` (live typed mapping) and `derive_preview_demand` (armed-only workspace mutation) | pass | `m003_demand_mapping_covers_typed_sources`; `m003_demand_ignores_free_text` proves a rationale mentioning "test" without typed demand abstains |
| NoStructuredDemand fallback (§2) | `EffectPathStatus::NoStructuredDemand`; `used_size` returns the M002 promotion size; `NoAdmissiblePath` abstains when every producer is inadmissible rather than hiding M002 | pass | `m003_no_admissible_producer_falls_back` (commit demand + unresolved error → abstain) |
| Effect graph bounds (§3) | `find_minimal_effect_path`: BFS levels shortest-first, global lexicographic order within a level, `CAUSAL_M003_MAX_PATH_DEPTH = 3`, no repeated tool, no probabilistic score; `plan_effect_path` admits only admissible contracted tools (unknown/uncontracted outside the graph, fallback discoverable via M002) | pass | `m003_planner_bounds_depth_no_repeat_lexicographic` (synthetic enablement pins depth cap, termination without repeats, lex tie-break); `m003_unknown_tools_outside_graph` (incl. all 54 frozen cases stay inside the native catalog) |
| Advisory-only paths (§3) | `EffectPathFrontier` carries names + provenance + fingerprints only; no execution, broker, permission, or disclosure call sites | pass | No runtime files touched (`git show --stat 82fc5b40`: module + receipt + arch doc) |
| Effect catalog binds implementation/schema identity (§4) | `effect_catalog_fingerprint`: per-tool `bind_causal_contract` over live `implementation_id`/`implementation_version`/`input_schema` (default registry plus lazily constructed session-gated tools, no I/O); differs from the payload-only `causal_catalog_fingerprint` | pass | `m003_effect_catalog_binds_implementation_identity` (fp stable, payload-fp differs, impl/version/schema drift each move the fp); `m003_all_pilot_tools_resolve_live` |
| Stale fingerprint fails closed to M002 (§4) | `StaleCatalogFallback` status when live ≠ expected effect fp; `is_fresh_against` on results | pass | `m003_stale_catalog_fails_closed` |
| No external/MCP contract participates (§4) | Planner consults only the native `causal_catalog`; `native_causal_contract` returns `None` for multiplexed/external identities | pass | `m003_unknown_tools_outside_graph`; provenance assertion below |
| No tool output rewrites contracts (§4) | Contracts are `&str`-keyed statics evaluated against host facts; evaluation inputs are names + fingerprints | pass | Result carries no payloads; provenance sources pinned to `static:M001-pilot-native` |
| Provenance in diagnostics (§4) | `EffectPathFrontier.tool_provenance` per path tool | pass | `m003_result_carries_provenance_and_fingerprints` |
| Evaluation on structured-demand cases (§5) | `score_effect_partition` + `qualify_m003` over the 54 frozen `desired_outcome` cases (dev 36 / qual 18); M002 reference arm on identical inputs; metrics: preservation, path existence/length, used size, premature exposure, false exclusions, coverage, latency | pass | Receipt `assets/tool-advisor/causal-frontier-m003-result.json`; `m003_frozen_demands_all_validate` (54/54 validate) |
| Positive criterion (§6) | Frozen `CausalM003Gates::m003_frozen` (preservation 1.00, authority 0, fallback 1.00, median ≤ 2 = 25%-below-M002-3, p95 ≤ 5 ms) plus exact-integer 25% check, premature non-increasing, no family loss, contract coverage | fail (honest negative) | Qual: preservation 0.41, 4 families lose gold → disposition D; reduction/latency/premature/authority/fallback/coverage gates pass |
| Non-goals (§7) | No NL goal compiler, no GNN, no evolving graph, no learned inference, no persisted mutable procedure graph | pass | Planner is a static BFS over frozen contracts; no storage, no learning |

## 3. Production implementation evidence

Implementation `82fc5b40` (3 files, +3757/−0):

- `src/tool_advisor/causal_frontier.rs` (+~1900): M003 section —
  `CausalM003Gates::m003_frozen`, `NoStructuredDemandReason`,
  `derive_structured_demand`, `derive_unmet_demands`,
  `derive_preview_demand`, `find_minimal_effect_path`,
  `plan_effect_path`, `effect_catalog_fingerprint` (+
  `effect_binding_registry`, `effect_catalog_binding_complete`),
  `EffectPathFrontier` (+ `is_fresh_against`), `EffectPathError`,
  `EffectPathStatus` (+ `code`, `used_size`), `evaluate_effect_path`,
  `EffectPathCaseRow` / `EffectPathFamilyRow` /
  `EffectPathPartitionReport`, `score_effect_partition`,
  `measure_effect_path_latency_ms`, `CausalM003Disposition`
  (Positive→"A" / Negative→"D" / ContractFailure→"E" under the
  `causal-frontier-m003-structured-effect-path` protocol),
  `decide_m003_disposition` (exact-integer correctness + `4·m003 ≤ 3·m002`
  reduction), `CausalM003Report`, `qualify_m003`, 16 regression tests
  plus an ignored receipt regenerator. M001/M002 code paths untouched.
- `assets/tool-advisor/causal-frontier-m003-result.json` (new): frozen
  receipt — benchmark fp `f60dad17…`, contract catalog fp `0ac06de8…`,
  effect catalog fp `94ef4d74…`, dev 36 / qual 18 structured cases,
  latency (1001 iters/surface, 18018 samples, p50 0.038 / p95 0.048 /
  max 0.29 ms), 8 gate results, disposition D.
- `architecture/tool-advisor.md`: M003 subsection (semantics, bounds,
  integrity, measured negative outcome, M004 selects M002).

Measured outcome (qualification, 18 structured-demand cases):

- path existence 18/18 = 1.00; path length median/max 1/1 (the M001
  pilot ontology declares no outcome-to-fact enablement, so every path
  is a single direct-producer step; the depth-3 machinery is implemented
  generically and pinned by synthetic tests);
- caller-visible median 1 vs M002 reference median 3 on the same subset
  (exact `4·1 ≤ 3·3` holds; frozen cap 2);
- gold current-step preservation 22/54 = 0.41 (M002 reference 1.00);
  32 false causal exclusions; families losing gold:
  `unmet_test_acceptance`, `unmet_commit_acceptance`, `failed_tests`,
  `mixed_state` (`artifact_recovery` preserves fully: gold = producer
  `context_read` + required-bypass `read`);
- premature exposure: baseline 52, M002 remaining 0, M003 remaining 0
  (non-increasing);
- contract coverage 18/18 = 1.00; uncontracted fallback 0/0 → 1.00
  (no uncontracted gold in the structured-demand subset);
  authority violations 0; catalog binding complete;
- pure-computation p95 0.048 ms (gate ≤ 5.0 ms).

Dev (36 cases): paths 36/36, preservation 44/110 = 0.40, false
exclusions 66, premature 104/0/0, same 4 families losing gold.

Mechanism of the negative: narrowing to demand producers excludes the
work-plan/goal-state transitions (`work_plan_update_item`,
`work_plan_get`, `goal_update_progress`, `goal_get`) and co-producers
(`verify` via lexicographic minimality, `git`, `read`-adjacent
inspection) that the frozen gold current steps genuinely need alongside
the demand producer. The reduction is real but unsafe.

Planned but absent by design (owned by later milestones): M004
observe-mode runtime integration (selects M002), M005 active disclosure.
No provider-definition, palette, broker, or disclosure behavior change.

## 4. Verification executed (commands + results; label local vs CI truthfully)

Local (repo HEAD at implementation commit `82fc5b40`):

- `cargo test -p codegg --lib tool_advisor::causal_frontier`: 78 passed /
  0 failed / 2 ignored (the ignored tests are the M002/M003 receipt
  regenerators). Covers 62 pre-existing M001/M002 tests (unaffected) +
  16 new M003 tests.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: clean
  (one new lint self-caught and fixed during implementation:
  `field_reassign_with_default` in a test helper).
- `cargo fmt --all -- --check`: clean. `git diff --check`: clean.
- `scripts/verify.sh quick`: passed (agent schema, core-boundary,
  client/desktop boundaries, sandbox, execution-ownership, TUI authority,
  HTTP-route disposition, audit coverage, scheduler bypass, provider-wire
  guards, workspace check).
- Receipt recomputation:
  `m003_checked_in_receipt_matches_recomputation` re-runs `qualify_m003`
  over the frozen assets and asserts identical deterministic fields;
  latency is compared by gate (environment-sensitive), both stored and
  fresh p95 within budget.
- End-to-end gate assertion: `m003_end_to_end_negative_on_frozen_benchmark`
  pins disposition D with the exact counts above (36/18 structured,
  18/18 qual paths, medians 1 vs 3, preservation < 1.00,
  `artifact_recovery` loss-free, premature 0, p95 within budget,
  18018 latency samples).

Hosted CI: implementation commit `82fc5b40` was pushed on branch
`impl/causal-frontier-m003-effect-path` (branch pushes do not trigger
this workflow — it runs on `pull_request` and `push` to `main`), so the
ordinary `verify` evidence comes from the merge to main: hosted `CI /
verify` run `37051423825` **success in 16m55s** on merge commit
`41513fd3` (contains all M003 code + receipt + the M004/M005 merges).
Reconciled — see §10. Prior main run `37047118019` failed on an upstream
M005 single-sample latency flake (`m005-holdout-109: single evaluation
over budget`), classified in `005-status.md` §10; green on the merge run.

Environment note (operational, no production impact): this host builds
the workspace with an x86_64 (Rosetta) toolchain while MacPorts
`/opt/local` supplies arm64 `libz`/`libiconv`/`liblzma`, which poisons
the final test-binary link (`ld` does not fall through after a
wrong-arch match). Local test evidence above used a pkg-config shim
redirecting `zlib`→SDK stub, `liblzma`/`libiconv`→Intel Homebrew
x86_64 libs, plus matching `RUSTFLAGS -L` search paths. The shim lives
outside the repo; no production code, dependency, or build config was
changed for it.

## 5. Invariant review

- `ResolvedToolSurface` remains the only per-turn capability ceiling: M003
  consumes the M002 admissible set and grants nothing; withheld input
  fails closed and output containment is double-checked in
  `score_effect_partition`.
- Visibility-only: the frontier is computed offline; no disclosure,
  broker, permission, or request-preparation code was touched.
- Required/never-reduce tools remain visible: the M003 visible set is
  path ∪ uncontracted fallback ∪ required bypass; `read` (never-reduce)
  and `tool_search` (required) are never excluded, which is exactly why
  `artifact_recovery` preserves fully.
- Uncontracted tools stay discoverable: fallback membership is reused
  from the M002 frontier verbatim (0 violations; no uncontracted gold
  exists in the structured-demand subset).
- No remote telemetry, no model weights, no new inference runtime, no
  chain-of-thought parsing: demands are typed enums; the planner is a
  static BFS with no scores.
- State projection unchanged from M001; contracts frozen (catalog fp
  `0ac06de8…` re-verified by `verify_against` on every `qualify_m003`
  run; no contract edits before/after).
- Qual discipline: no tunable parameters exist (no thresholds, no
  weights), so dev inspection cannot leak into qualification; dev and
  qual are scored by one deterministic pass.

## 6. Failure and recovery review

- Withheld tool in surface → `Err(WithheldInSurface)`; scoring aborts
  rather than promoting. Covered by test.
- Required tool missing from surface → `Err(RequiredNotEligible)`.
- Live ≠ expected effect-catalog fingerprint → `StaleCatalogFallback`
  status; the caller keeps M002. Covered by test. In `qualify_m003`,
  expected equals live by construction, so a stale status there would
  indicate registry nondeterminism (did not occur; binding complete).
- Demand present but every producer inadmissible (e.g. commit demand
  with a recorded unresolved error) → `NoAdmissiblePath` abstention, not
  exclusion. Covered by test.
- No structured demand (absent/unsupported/mismatched evidence) →
  `NoStructuredDemand` abstention. Covered by tests, including the
  free-text negative (prose mentioning "test" creates nothing).
- Contradictory or empty fact sets evaluate without panic (planner only
  tests set membership; BFS over ≤20 candidates × depth 3 is finite by
  construction; empty candidates return `None`).
- Disposition E path (`ContractFailure`) is implemented and unit-decided
  but not taken: authority and fallback accounting are exact.

## 7. Migration and compatibility review

Additive only: new types/functions/tests/asset; no existing API changed
(`Tool::causal_contract` seam, `CausalFrontier`, M001/M002 scoring
untouched — all 62 pre-existing module tests pass unmodified). The
receipt is a new file with a versioned schema
(`CAUSAL_M003_REPORT_SCHEMA_VERSION = 1`). No storage, protocol,
config, or migration impact.

## 8. Security review

No new trust boundary: demands are host-owned typed enums, never parsed
prose; contracts remain static native metadata; no external/MCP contract
participates (planner consults only the native catalog; path tools are
asserted inside `NATIVE_PILOT_TOOLS` over all 54 structured cases); no
tool output can rewrite a contract; the stale-catalog fail-closed
prevents planning against drifted implementations. The frontier cannot
widen authority (fail-closed + output audit) and carries no definitions
or payloads — names, wire codes, and fingerprints only. No secrets in
snapshots (M001 property, unchanged).

## 9. Documentation and operations

- `architecture/tool-advisor.md`: M003 subsection (semantics, bounds,
  integrity, measured negative outcome, M004 selects M002).
- Receipt regeneration is documented and reproducible: ignored test
  `m003_regenerate_checked_in_receipt` reruns `qualify_m003` over the
  frozen assets; the always-run recomputation test pins every
  deterministic field.
- Operator action: none. M004 implementors consume this record
  (disposition D + frozen M002 promotion semantics); the qualification
  partition must not be used for contract tuning (contracts remain
  frozen; any future contract change is a new experiment version).

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low (resolved) | Hosted CI result pending at closure-write time (local evidence only). | Negative closure assumes ordinary hosted CI green per plan §11. | Reconciled: hosted `CI / verify` run `37051423825` success in 16m55s on merge commit `41513fd3` (all M003 code + receipt + M004/M005 merges). Assumption holds. |
| low (environment) | Local test-binary linking on this host requires a pkg-config/RUSTFLAGS shim (see §4 environment note). | Verification friction only; no production or dependency change. | Future milestones on this host reuse the same shim until the toolchain/sysroot setup is reconciled. |

No medium or higher findings. No corrective pass required. The negative
disposition is a valid experiment outcome, not a defect: the plan
explicitly provides the negative branch, and the evidence (exact counts
above) unambiguously fails the preservation/family gates while passing
correctness.

## 11. Roadmap disposition

M003 closes with disposition D (negative). Consequences:

- M004 (observe-mode runtime integration) keeps its `ready` status with
  the selected frontier now fixed to **M002** (plan §9 acceptance:
  negative leaves positive M002 as the selected frontier; M004 already
  specifies "M003 if positive, otherwise M002"). No M004 plan edit is
  required beyond this recorded selection.
- M005 stays blocked on positive M004. No other registered plan lists
  M003 as a dependency; nothing else is unblocked.
- Dependency audit (registry Blocked-work section + affected roadmap
  graphs): the only plans depending on this workstream are M004
  (already ready; selection now M002) and M005 (still blocked on
  positive M004). No blocked plan becomes ready in this commit; no new
  corrective or follow-up work is registered (a negative optional
  experiment leaves no repair obligation).
- The subsystem roadmap M003 row moves active/optional → closed (D);
  the M004 row records M002 as the selected frontier.

Post-merge reconciliation (factual correction — this record was written
before the concurrent main-side closes were merged in):

- M004 subsequently closed **positive** (implementation `635213bc`;
  `plans/closure/tool-selection-advisor-causal-frontier-experiment/004-status.md`)
  with M002 as the selected frontier — exactly the outcome this record's
  disposition prescribes — so M004 moved ready → closed.
- M005 subsequently closed with **disposition B**
  (implementation `019bf326`; `005-status.md`): structural gates green,
  live trajectories unavailable; it moved blocked → ready → closed. The
  historical live-primary-model study stays blocked (requires A with
  live evidence), so this M003 close still unblocks nothing.
- With M001–M005 all closed, the subsystem roadmap and its registry row
  moved `active` → `closed` during the merge reconciliation.

## 12. Registry updates

- `plans/registry.md` subsystem row: causal-frontier experiment
  `M001 closed (positive); M002 closed (A); M003 active (optional); M004
  ready; M005 dependency-gated` →
  `M001 closed (positive); M002 closed (A); M003 closed (D, negative);
  M004 ready (M002 selected); M005 dependency-gated`.
- `plans/registry.md` plan rows: M003 `active (optional)` → `closed`
  (this record, implementation `82fc5b40`, disposition D); M004 row
  handoff note records M002 as the selected frontier (M003 negative).
  M005 row unchanged.
- `plans/subsystems/tool-selection-advisor-causal-frontier-experiment-roadmap.md`:
  status line, M003 status active/optional → closed (D) with receipt
  pointer; M004 status ready with M002-selected note.
- `plans/implementation/tool-selection-advisor-causal-frontier-experiment/003-*.md`:
  status active → closed (pointer to this record).
- Hosted CI: branch `impl/causal-frontier-m003-effect-path` pushes do not
  trigger the workflow; the green run is `37051423825` on merge commit
  `41513fd3` after the merge to `main` (see §4 and §10 — reconciled).
- Post-merge reconciliation (factual correction, see §11): M004 closed
  positive (`635213bc`, M002 selected), M005 closed B (`019bf326`), the
  subsystem roadmap status line and its registry row moved
  `active` → `closed` (all milestones closed), the registry
  causal-frontier gate paragraph was rewritten to the final M001–M005
  state, and the M004/M005 closure records received the same factual
  M003-closed-D corrections. Nothing additional is unblocked.
