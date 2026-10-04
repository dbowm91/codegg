# Tool-Selection Advisor Causal Frontier Timing Corrective Addendum

Status: active

Repository planning baseline: `cdfd6257`

Controlling process:

- `plans/003-planning-process.md#7-corrective-passes`

Controlling architecture:

- `plans/adrs/ADR-0009-local-tool-advisor-boundaries.md`
- `architecture/agent-tool-surface.md`

Triggering evidence:

- Hosted `CI / verify` run `37219495080` on `e6748230` (M006-A head) — the
  `Workspace tests` step failed at
  `causal_observe_replay::live_request_preparation_delta::observe_mode_leaves_live_definitions_byte_identical`
  (`outcome.evaluation_millis < CAUSAL_OBSERVE_P95_BUDGET_MS`). A second
  attempt failed in a *different* test,
  `causal_active_m005::m005_holdout_structural_gates`
  (`m005-pinned-002: single evaluation over budget`); the first failure passed
  on that run. `nextest` fail-fast left ~6,900 of 12,180 tests unrun.
- Root-caused as pre-existing rather than M006-A-caused in
  `plans/closure/desktop-frontend-ide-foundation/005-status.md` §4.4: a
  separate worktree at baseline `main` (`614e983e`, confirmed to contain no
  editor code) failed **4 times in 8 runs**, while the M006-A branch failed
  **2 times in 8**. Neither `tests/causal_*.rs` nor `src/tool_advisor/` appears
  in `git diff main...HEAD`.
- Third occurrence, same defect class, already observed and deferred by the
  causal frontier milestone itself: the M005 close push run `37047118019`
  (`e32dfe53`) failed with `m005-holdout-109: single evaluation over budget`,
  classified in `plans/closure/tool-selection-advisor-causal-frontier-experiment/005-status.md`
  §10 as "the documented load flake … no code or gate change". That record is
  immutable historical evidence and is not rewritten; this corrective is the
  deferred work it pointed at.

## 1. Purpose

Make the causal advisor's latency gates measure the causal advisor. Three
sites assert a single cold wall-clock sample against the frozen 5 ms budget:

- `src/tool_advisor/causal_observe.rs` — `observe_never_mutates_or_suppresses`
- `tests/causal_active_m005.rs` — per-scenario check inside `qualify_scenario`
- `tests/causal_observe_replay.rs` — `observe_mode_leaves_live_definitions_byte_identical`

A single cold sample conflates the algorithm's cost with cold caches, page
faults, and scheduler preemption, so on a shared hosted runner it measures the
machine rather than the code. Each of these three is a methodologically wrong
duplicate of a sound p95 gate that already exists next to it
(`replay_p95_within_budget` over 201 warm samples; the holdout's own
nearest-rank p95). The distributional gates are correct; the single-shot
smoke assertions are what flake.

The fix changes measurement only. The 5.0 ms constants are frozen and stay
frozen: `m005_freeze_record_matches_live_contracts` asserts
`structural_gates.pure_active_eval_p95_ms_max == CAUSAL_ACTIVE_P95_BUDGET_MS`,
and a benchmark fingerprint pins the record. Raising the budget was therefore
never an available option, and is not the fix.

## 2. Corrective scope

One milestone:

- `plans/implementation/tool-selection-advisor-causal-frontier-timing-corrective/001-warm-best-of-n-budget-gates.md`

Status: implemented.

C001 must:

- replace the single-shot assertion at each of the two re-runnable sites with
  a warm best-of-N check (one discarded warmup, then the minimum of 15 warm
  samples) against the *unchanged* constant, keeping an assertion at every
  site;
- convert the third site, whose recorded sample comes from a live agent-loop
  preparation that cannot be re-run with identical inputs, into a
  finite/non-negative sanity check that points at `replay_p95_within_budget` as
  the enforcement point for the same constant;
- keep the distributional p95 gates and the freeze-record pin untouched;
- delete no assertion and drop no site;
- obtain green hosted `CI / verify` on the fix head, including the full
  workspace sweep that fail-fast previously truncated.

## 3. Invariants

This corrective MUST NOT:

- change `CAUSAL_OBSERVE_P95_BUDGET_MS` or `CAUSAL_ACTIVE_P95_BUDGET_MS`, the
  M005 freeze record, the benchmark fingerprint, or any frozen gate threshold;
- change causal-frontier scoring, admissibility, promotion, disclosure, or
  effect-path behavior, or any production code path — this is a test-harness
  measurement change only;
- retrain or fine-tune a model or freeze an advisor artifact;
- alter `ResolvedToolSurface` authority or advisor default-off/local-only
  behavior;
- delete, relax, or skip a budget assertion at any site;
- rewrite any historical closure record, including
  `plans/closure/tool-selection-advisor-causal-frontier-experiment/001`–`005-status.md`;
- widen into a toolchain pin, CI-command policy change, or moving the gates to
  a dedicated non-default CI job.

## 4. Completion definition

C001 closes only when:

- the flake is eliminated by repetition, not by a single green run (20+
  consecutive clean runs of the two affected integration targets);
- the gate is proven still able to fire — a temporarily impossible budget
  makes each gate fail with an explicit message, and restoring the constant
  makes it pass again;
- `cargo fmt --all -- --check` and
  `cargo clippy --workspace --all-targets --locked -- -D warnings` pass;
- the affected suites pass locally;
- hosted `CI / verify` is green on the fix head with the full workspace sweep
  executed;
- the additive closure record is committed and registered.
