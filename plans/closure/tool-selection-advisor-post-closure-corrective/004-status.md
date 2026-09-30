# Tool-Selection Advisor Post-Closure Corrective M004 — Closure Status

Status: blocked

Source implementation plan:

- `plans/implementation/tool-selection-advisor-post-closure-corrective/004-small-model-trajectory-qualification.md`

Source subsystem roadmap:

- `plans/subsystems/tool-selection-advisor-post-closure-corrective-addendum.md#m004--live-small-model-trajectory-qualification-and-corrective-closure`

Repository baseline reviewed: `7259292ef371ecd116e8911f9c2ddca7032e2a2e`

Implementation commits or pull requests:

- None — live study did not begin; positive offline gate plus
  operator/provider prerequisites remain unsatisfied (see §1).

## 1. Executive finding

M004 closes **blocked** without a trajectory suite, live provider runs, or
mode disposition, by the plan's own §§2/16 stop logic. The live study is
meaningful only after a qualified learned model exists plus explicit
operator/provider configuration, and neither exists:

1. **No qualified learned model.** C004 closed with disposition B —
   mechanically correct but no useful gain
   (`plans/closure/tool-selection-advisor-evidence-integrity-corrective/004-status.md`):
   contextual MRR 0.46–0.60 vs linear 0.71, no context-sensitive gain
   without regression, no-tool F1 far below baselines, preselector recall
   0.83 vs 0.98. The contextual scorer was demoted to a research/observe
   baseline. Every successor architecture line remains negative or
   evaluation-blocked: order-invariance M004 negative (best 0.9444 vs
   0.99/0.98/0.95, no operating point), retrieval-architecture M003 negative
   (ceiling-proven 68/72), sequence/v2/v3 qualifications D, retrieval-signal
   M001 blocked on its §4 evaluation hard stop. No positive new-architecture
   experiment exists to re-qualify.
2. **Original provider/operator/trajectory/resource prerequisites were never
   satisfied.** No operator-configured small-model/strong-reference matrix,
   no frozen held-out trajectory suite fingerprint, no pre-registered live
   experiment, no explicit provider credentials configured for qualification,
   and no live calls were authorized or spent. Running the off/reactive/
   proactive arms now would measure an unqualified research baseline against
   live spend with no interpretable product question.

Per plan §15, spending live-provider budget on an unqualified model or
claiming a disposition from offline smoke evidence would be a stop
violation. The correct action is blocked closure with the defect owned here,
not a negative effectiveness verdict (no trajectories ran) and not a silent
unblock. Advisor remains default-off; M001-M003 closures stand.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| M001+M002+M003 accepted closure (hard dep) | M001 closed (`001-status.md`, `66bf63e`), M002 closed (`002-status.md`, `aa91080`), M003 closed (`003-status.md`, `3b6c68a`+`84e6db9`) | pass | Hard dep 1 satisfied |
| Positive offline disposition (C004-A or successor fresh-v4 A) | C004 disposition B (`004-status.md` §1-§3); order-invariance M005 blocked with no result (`005-status.md` this batch); retrieval-signal M006 ready but undecided | fail (blocker holds) | No qualified model to take live |
| Held-out trajectory suite >=32 task groups, deferred-tool design, frozen fingerprint (plan §5) | No `trajectory-suite` asset frozen; `assets/tool-advisor/downstream-suite.jsonl` is the offline downstream fixture, not a live trajectory suite; no suite fingerprint preregistered | not run | Suite construction is post-qualification work |
| Primary-model matrix: small/tool-fragile + stronger reference, operator-configured, no commercial-model repo dependency (plan §6) | No model IDs/dates/config recorded; no operator configuration supplied at handoff; no live calls authorized | not run | Explicit operator action required by invariant |
| Off / reactive-rerank / proactive-promote (+ optional full-palette) arms with identical config (plan §7) | No harness arms ran; `project_discovery` (reactive) and pre-turn promotion (M003 seam) code retained but not exercised live | not run | Would confound wiring with unqualified scores |
| Primary/cost/safety metrics incl. failed-trajectory accounting (plan §8) | No per-arm tables; zero live trajectories to account | not run | No silent discarding occurred because nothing ran |
| Pre-registered metrics/thresholds/stopping/retry policy; no threshold tuning on live outcomes (plan §9) | No live pre-registration; M001/M002 dev thresholds exist but are not qualified for live | not run | Tuning on live outcomes correctly avoided by not running |
| Local resource matrix (Apple Silicon / x86_64 Linux / ARM64-SBC) for the selected model (plan §10) | Offline resource evidence exists for C004 variants (small 20.97 MB/928 ms cold, medium 62.92 MB/2804 ms, compact 1.31 MB/58 ms) but the selected live candidate is undefined; no live-model matrix recorded | not run (offline reference only) | SBC viability cannot be judged without a qualified candidate |
| Dry-run harness validating fixtures/model/config without provider calls (plan §13) | No M004 live harness was built; existing offline `eval`/`requalify` CLIs are not trajectory harnesses | not run | Building a harness for an unqualified model would be premature infra |
| Positive-qualify or honest-negative close with mode disposition (plan §12) | Neither disposition is recordable without trajectories | blocked | This blocked closure is the honest record, not an effectiveness verdict |

## 3. Production implementation evidence

No production implementation landed, by design for a blocked closure.

State at review:

- Advisor wiring retained from M001-M003 + evidence-integrity C003: optional
  local advisor path, `hashed-linear-v1` baseline, contextual research
  artifacts, consent-gated training plumbing, pre-turn promotion seam at
  `request_preparation.rs` — all default-off, unchanged.
- No trajectory suite, qualification harness, model-matrix config, live
  experiment pre-registration, operator guide update, or resource-compat
  matrix was added for M004. `git status` shows only this closure batch as
  pending.
- No provider IDs, temperatures, fingerprints, costs, or secrets were
  recorded because no live calls ran; no secret entered the repo.

Distinguished clearly: planned-but-absent is the entire live trajectory
stack (suite → harness → pre-reg → arms → analysis → disposition).
Retained-and-valid is the M001-M003 mechanism plus the C004-B verdict that
correctly gates it from live spend.

## 4. Verification executed

### Commands run

```bash
cargo test --locked -p codegg --lib -- tool_advisor
cargo test --locked -p codegg --lib -- agent::request_preparation agent::tool_surface tool::tool_search
cargo test --locked -p codegg --lib -- tool_surface_minimization
python3 scripts/check_execution_ownership.py
scripts/verify.sh quick
cargo fmt --all -- --check
git diff --check
```

Plan-literal live commands (`cargo test --workspace`, feature-gated
`tool-advisor` suites, `cargo clippy --all-features`, full `verify.sh`,
dry-run validation, live trajectory runs with recorded fingerprints) were
**not run as M004 evidence**: there is no M004 harness to dry-run and no
authorized live configuration to execute. The base-feature suites above
confirm the retained mechanism is unregressed; they are not claimed as live
qualification.

Feature-gated encoder suites were not run on this host for the same
pre-existing `candle-core`/aarch64-darwin toolchain reason recorded in the
sibling closures; Linux CI remains the qualified host for any future
unblocked pass.

### Results

- `cargo test --locked -p codegg --lib -- tool_advisor`: 55 passed /
  0 failed (offline advisor mechanism green).
- Focused authority/wiring suites (`request_preparation`, `tool_surface`,
  `tool_search`, `tool_surface_minimization`): green at this baseline
  (exact counts recorded in the sibling C004 closure for the same code;
  no wiring change landed since).
- `check_execution_ownership.py`: guard ok.
- `scripts/verify.sh quick`: passed (all canonical guards incl.
  scheduler-bypass and eggwork routing).
- `cargo fmt --all -- --check` and `git diff --check`: clean.
- Live/dry-run scope: not run, truthfully labeled; no trajectory, cost,
  latency, or effectiveness claim is made.

## 5. Invariant review

Per plan §4 invariants — all hold because no live study ran:

- Qualification provider calls are explicit operator actions, not ordinary
  startup/use: holds; zero provider calls ran.
- Tasks held out from training/threshold tuning: vacuously holds; no live
  tasks were defined or consumed by training.
- No user/private content auto-uploaded as a benchmark: holds; no benchmark
  collection ran.
- Advisor default-off regardless of result: holds; no default changed.
- Permission/sandbox/broker identical across arms: vacuously holds; no arms
  ran, and the M003 seam preserves authority by construction.
- Identity/settings/fingerprint recording: vacuously holds; nothing to
  record, and no secret was committed.
- Failed/aborted trajectories retained: vacuously holds; zero trajectories
  means zero silent discards.

## 6. Failure and recovery review

No new failure modes introduced. Applicability review:

- Duplicate delivery/idempotency: no live arms, so no cross-arm
  contamination or double-counting hazard.
- Cancellation races / daemon or node restart / partial persistence: no
  harness, persistence, or concurrency surface added.
- Stale generation/lease, contention, resource release: no scheduler or
  provider resource was acquired.
- Malformed/unauthorized input: no new input surface; the M003
  promotion-seam authority revalidation stands unregressed.
- Denied/hidden/parent-ceiling promotion (§15 stop): no live promotion ran;
  offline authority-negative evidence (C004: 0 violations) stands.
- Telemetry-without-consent (§15 stop): no telemetry path exercised.
- Fixture-into-training leak (§15 stop): no live fixtures created.
- Promotion-vs-`tool_search` indistinguishability (§15 stop): no harness
  exists to confuse; future harness must still prove the distinction.

## 7. Migration and compatibility review

No schema migration, no provider wire change, no config change, no artifact
change. No live evidence store to migrate or roll back. Rollback is a revert
of this planning-only batch. The M001 corpus/splits, M002 artifacts, and M003
palette seam need no migration.

## 8. Security review

No authorization, secret, network, or privilege surface touched. No provider
credentials configured, logged, or committed. No telemetry transmitted.
Promotion authority boundary (filter before advisor, revalidate before
altering visibility, model output untrusted) stands unregressed as verified
by the retained focused suites. `#![deny(unsafe_code)]` holds for the lib.

## 9. Documentation and operations

- This closure record is the sole new artifact for M004 in this batch
  (plus registry/roadmap/plan status updates in §12).
- Final qualification report, operator enable/disable guide, model/resource
  compat matrix, and linear-vs-contextual live comparison (plan §14) are
  **not** produced: there are no live results to report. Existing
  `architecture/tool-advisor.md` (C004-B section) remains the accurate
  operator-facing status: contextual scorer is a research baseline, live use
  is unqualified, default stays off.
- Future unblocked pass must still produce: suite/model/config fingerprints,
  exact model IDs/settings/dates, per-arm tables, false-promotion and
  authority negatives, linear-vs-contextual comparison, resource matrix,
  failed-trajectory accounting, verification commands/results, and an
  explicit experimental mode disposition.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | No qualified learned model exists (C004-B; all successor lines negative/blocked) | Live study has no interpretable candidate; spend would be uninterpretable | Await a positive new-architecture offline disposition (successor fresh-v4 A or equivalent) before re-activating M004 |
| high | Operator/provider/trajectory/resource prerequisites never satisfied (no matrix, suite, pre-reg, credentials, or live calls) | Even with a qualified model, M004 could not run without a new operator handoff | Future activation needs an explicit operator-supplied model matrix + suite freeze + pre-registration as plan §§5-6/9 require |
| medium | No M004 trajectory harness exists (not even a dry-run) | Unblocked pass must build the harness before any live spend | Build harness work packages B-C in the unblocked pass; do not retrofit offline `eval` as a trajectory harness |
| low | Offline resource reference (C004 small/medium/compact) exists but no live candidate to judge | SBC viability remains undecided | Record the full matrix for the future qualified candidate per plan §10 |

No critical findings. No findings indicate a defect in the shipped
advisor/wiring code.

## 11. Roadmap disposition

- M004: **blocked** — positive offline gate plus live prerequisites both
  absent; no downstream plan is unblocked by this closure.
- Corrective workstream
  (`tool-selection-advisor-post-closure-corrective-addendum.md`) stays
  **blocked** at M004 with M001-M003 closed; no roadmap revision beyond
  recording this blocked closure.
- No corrective plan is registered: the blocker is a missing positive
  predecessor plus missing operator inputs, not a new CodeGG defect. The
  retrieval-signal M006 line (ready) remains the only active path that could
  eventually supply a qualified retrieval/ranker stack.

## 12. Registry updates

- `plans/registry.md` Blocked-work row for post-closure M004: blocker text
  retained, closure link added to this record.
- `plans/registry.md` post-closure corrective gate paragraph (advisor
  corrective gate): M004-stays-blocked text retained with this closure as
  the current evidence.
- `plans/subsystems/tool-selection-advisor-post-closure-corrective-addendum.md`:
  M004 row `blocked` retained with closure link; §7 M004 status gains the
  closure reference.
- `plans/implementation/tool-selection-advisor-post-closure-corrective/004-small-model-trajectory-qualification.md`:
  status line `blocked` retained with closure link.
- Unblock audit: no registered plan lists M004 as a satisfied dependency;
  live-primary-model consumers stay blocked. Nothing is moved to `ready` by
  this closure.
