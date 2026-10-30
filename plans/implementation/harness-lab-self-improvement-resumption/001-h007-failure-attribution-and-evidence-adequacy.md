# Harness Lab Self-Improvement S001 — H007 Failure Attribution and Evidence Adequacy

Status: **ready** for offline handoff. No provider-backed execution authorized.

Repository baseline: `main@99cecc050ed1ae418af62d2d10e6d34d791513e1` (H007 PR #217 merged); verify newer refs and exact-mainline CI before integration.

Source roadmap: `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md`, S001.
Predecessor: `plans/implementation/harness-lab-mainline-qualification/007-deep-workload-and-separate-horizon-measures.md`; original receipts at `plans/closure/harness-lab-mainline-qualification/evidence/007-live-attempt*.json`; closure `plans/closure/harness-lab-mainline-qualification/007-status.md`.

Long-term: `plans/000-long-term-specification.md` §§23–24, 26–29; `plans/001-terminology-and-domain-model.md`; `plans/003-planning-process.md`. Applicable ADR: `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md` (Option B). Primary class: **infrastructure**.

## 1. Objective

Turn H007's seven immutable live receipts and any independently available, appropriately redacted companion ledgers into a **reproducible, evidence-limited diagnosis**: distinguish observed task/test failure, route timeout, missing job/repair observations, measurement inadequacy and a demonstrable CodeGG harness defect. Return an owner-oriented candidate-fix queue, or an explicit finding that no CodeGG defect has yet been established. Do not infer model intent or root cause from a summary terminal alone.

## 2. Dependency readiness

M001–M005, C001–C007, D001–D003 and H001–H006 are closed at their documented scopes; H007 has frozen summary evidence for all seven authorized runs on merged main. S001 can work offline against that pinned source. Require a **frozen source SHA** and exact receipt content digests before interpretation. S002 must separately establish exact-resulting-mainline verification; no additional H007 live attempt is required.

## 3. Current implementation evidence

- `dev/harness_lab/autonomy_run.py` owns trial observation and H007 task/horizon result projection; `dev/harness_lab/h007_results.py` separates those outcomes on merged main; existing `ledger.py`, `scoring.py`, `autonomy.py`, and `event_observation.py` supply relevant event and immutable result contracts.
- Receipt 7 reports `task_outcome=failed`, `horizon_reason=repair_cycles_insufficient`, `repair_cycles=1`, exact-ID running/completed and 53/53 tool terminals; it does **not** establish why the second repair was omitted. Receipt 4 has `independent_verifier.tests_run=0` with an error, not a completed failed verifier. Receipts 1 and 3 have typed `provider_timeout` and cannot be scored as solved task failures. Every H007 receipt reports zero ACP event gaps; this is bounded observation, not proof of zero underlying error probability.
- The closure's seven-run table is the authoritative historical result. Some H007 plan/registry lines still report the earlier two-run state. Do not rewrite frozen receipts to tidy statuses.
- The H007 prompt combines many independent requirements (two explicit repair checks, job overlap, continuation, five crates) and a 30-minute horizon. Failure to obey the prompt is not by itself a defect in CodeGG tool disclosure, ACP or the scheduler.

## 4. Non-regression invariants

Keep history append-only. Preserve task correctness separate from horizon observation; `timeout`, `inconclusive`, `not_exercised`, `measurement_incomplete` and `failed verifier` are not interchangeable. The subject's complete event pairing does not imply a tool was used correctly. Never expose credentials, full provider responses, hidden prompts/reasoning, private user path data or unbounded tool bodies. Do not reinterpret the obsolete stall-terminal requirement contrary to ADR-0040.

## 5. Scope

In: deterministic offline read of frozen H007 receipts, per-run evidence matrix, narrowly scoped optional ledger adapter, typed classification and producer/consumer audit of already exposed evidence, exact reproduction of any *actual CodeGG* failure with a failing-first native/ACP fixture, ownership and severity recommendations.

Out: re-running paid models, expanding the H007 long-horizon task, adding a general transcript warehouse, changing recovery policy, repairing provider timeouts without a reproducible CodeGG-layer defect, new multi-provider adapters, writing candidate patches, modifying immutable verifiers, or claiming a model capability ranking from seven heterogeneous outcomes.

## 6. Required production/tooling changes

**Domain and output:** prefer a dev-only `dev/harness_lab` reporting module or fixture-backed analysis; no daemon/protocol/storage changes unless a new reproducible defect proves a real observation gap. A versioned report row should contain `trial_id`, pinned receipt digest, `observed_outcome`, `evidence_state` (observed/contradicted/unknown), `suspected_owner` (agent-task, fixture, CodeGG tool/ACP, provider route, evaluator, unknown), `cause_assertion` with evidence pointers, and `next_action`. Distinguish *symptom* from *attributed cause*. Unknown fields remain null/unavailable.

**Evidence sources:** read only committed sanitized receipts and available saved tool/projection events; first inventory whether the full event/turn/diagnostic ledgers actually survived. A fabricated replay does not upgrade original live evidence. If event provenance is unavailable, record exactly which causal questions cannot be answered and define the minimum future **redacted** capture contract as a proposal, not a new production collector.

**Interfaces/migration:** no new RPC/API. Keep H005–H007 historical schemas readable and immutable; optional diagnostic schema v1 is additive, local and credential-free.

**Security/runtime:** no credential discovery, provider key, external network or default daemon attachment; cap report size and scrub filenames/prompts. If reproducing a true defect use loopback deterministic fake provider, a private CodeGG daemon/home, isolated worktree and fixed build target.

## 7. Ordered work packages

**A — Freeze and inventory.** Enumerate attempts 1–7, pin their digests, H007 source/fixture/verifier digests and per-run terminal/verifier/request/job/repair/horizon fields. Identify absent companion traces rather than assuming a full trajectory exists. Check counts and truth tables against H007 closure; preserve contradictions explicitly.

**B — Implement typed, conservative attribution.** Create a deterministic H007 offline report or one-shot script, with rows for observed behavior and separately annotated inferences; no heuristic that maps `stop_reason=end_turn` or missing job-status to `model_defect` by itself. Map both provider timeouts to route-boundary observations, unresolved underlying causes. Label apparent lack of second repair as an observed task requirement miss, not evidence of CodeGG recovery failure. Missing verifier execution stays unmeasured.

**C — Independent artifact/contract cross-check.** Check public task contract, two repairs, job overlap and frozen verifier against reference and starter calibration. Identify any evaluator/fixture ambiguity and replay it deterministically; produce a nonvacuous negative test that fails before proposed *tooling* correction. Do not change H007 hidden verifier to make failed attempts pass. Route verified daemon/job/ACP defects to their owners with exact failing-first evidence, or write a zero-defect disposition.

**D — Handoff.** Produce a small prioritized candidate list: concrete source-level behavior, expected observed effect, reproduction command, scoped module, risk and confidence; separate model/fixture/proxy issues with no CodeGG patch. If evidence cannot support a causal claim, report this as the outcome and include only a bounded future capture need. S002 consumes this list without requiring a manufactured defect.

## 8. Failure, cancellation, restart and contention

The analyzer is offline, idempotent and read-only; interrupted generation creates no mutation to original receipts. Validate input digest on restart; refuse changed/missing source and duplicate trial IDs rather than merge incompatible runs. Multi-reader use is safe; no parallel Cargo build unless reproducing one pinned failing-first case. Refuse missing or nonsensical request admission provenance as an evidence-quality failure, not a model failure.

## 9. Compatibility and migration

Read H007 schema v1 and available earlier receipts without modifying them. If implementing a new schema, version it and keep prior fields faithfully null where unavailable. No daemon/protocol migration. Existing long-horizon qualifiers retain their historic outcomes and authorizations. Update only roadmap/registry status after closure.

## 10. Required tests

- Unit truth table: typed provider timeout, task failed with completed verifier, verifier setup error, one repair vs two, no job status vs mismatched ID, and complete start/terminal observation with unknown causal root.
- Negative assertions: a `provider_timeout` cannot become `agent_task_failed`; a null tool observation cannot become `completed`; zero event gaps cannot be used as an absolute reliability claim; historical receipts never change.
- Deterministic fixtures: rerunning the diagnosis yields same ordering and digests; edited receipt or changed case/verifier digest is refused.
- If real CodeGG flaw is asserted, reproduce on current source with a failing-first ACP/tool test and verify its ownership; otherwise state **none reproduced** without manufacturing source changes.
- Full Python Harness Lab unit tests and existing H007 projection tests on the appropriate branch.

## 11. Verification commands

```bash
python3 -m unittest dev.harness_lab.tests.test_h007 dev.harness_lab.tests.test_autonomy_run
python3 -m unittest discover -s dev/harness_lab/tests -v
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
git diff --check
# Run any new deterministic report twice and compare canonical output/digests.
# If native reproduction added:
scripts/verify.sh scoped origin/main
```
Record actual results in closure; never claim a command ran solely because it is prescribed.

## 12. Documentation and static guards

Add analyzer/operator readme (or extend `dev/harness_lab/README.md`) with summary-vs-causal limitations, known sanitization, schema and commands. Add guard/tests for input immutability and forbidden assumption that `0 gaps` implies a globally reliable event stream. Leave original H007 closure and authorizations untouched.

## 13. Acceptance criteria

Every seven-run record is represented once with provenance; contradictions and unknowns are visible. A reviewer can identify evidence for two typed timeouts, the one-of-two repair failures and the cases with verifier setup errors, **without a speculative CodeGG architecture regression claim**. Any proposed CodeGG fix includes a true failing-first reproduction; otherwise the report explicitly states no verified harness defect and allows S002's instrument qualification to proceed.

## 14. Stop conditions

Stop if receipt authenticity cannot be established, verification requires altering a historical receipt, the evaluator/daemon boundary must change without an ADR, user authorization would be needed for new live calls, or source changes outside dev-only reporting are proposed without a failing-first defect. Hand off deficiencies with the smallest independently reproducible case instead of rebuilding the harness.

## 15. Closure evidence

Create `plans/closure/harness-lab-self-improvement-resumption/001-status.md` with baseline and input digests, run-by-run diagnosis + evidence pointers, schema/guard tests, actually executed verification, null/unknown limitations, defects routed (or explicit none), severity, privacy and follow-up disposition. Do not mark S002 ready until S001 closes and actual mainline source/CI evidence is checked; PR #217 has already merged.

## 16. Handoff

Prefer no Rust compilation. Keep test-thread limits/worktree targets modest on the development workstation. Avoid committing raw provider bodies or copied secrets. This milestone is about *evidence integrity*, not a promise that H007's failed attempts can all be explained.
