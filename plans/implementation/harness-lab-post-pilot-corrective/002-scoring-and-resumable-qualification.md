# Harness Lab Post-Pilot C002 — Verifier-Backed Scoring and Resumable Qualification

Status: ready for handoff
Repository baseline: `main@2a9fc376707ef14dab4a2f9a27f6aba8ac00dd59` (2026-10-21; verify newest HEAD and M005 closure on entry).
Source roadmap: `plans/subsystems/harness-lab-post-pilot-corrective-addendum.md#5-milestones`
Corrects: M004 `plans/closure/harness-lab/004-status.md` medium interrupted-trial/accepted-fix gaps and M005 `plans/closure/harness-lab/005-status.md` checkpoint qualification; findings F003/F004.
Long-term requirements: `plans/000-long-term-specification.md` §§24, 26, 28–29; `plans/001-terminology-and-domain-model.md`.
Primary class: invariant/infrastructure.
Applicable ADRs: none additional if developer-only controller and daemon authority remain as-is.

## 1. Objective

Establish a **versioned, independently verifiable acceptance criterion** suitable for (a) fixing a real broken CodeGG harness behavior and (b) reducing proven agent-facing friction when correctness is already green. Close the gap between unit-tested partial ledger resume and full **interrupted child qualification + outer optimizer restart**, without duplicate trial rows or loss of cumulative budgets. Do **not** claim model efficiency improvement from a single deterministic test.

## 2. Readiness and current evidence

Existing `dev/harness_lab/improve.py::qualify_candidate` compares the frozen `expected_terminal` for each task, rejects unmatched candidate terminals, checks for any passed->failed regression, then sets `accepted = bool(correctness_improvements)` where a baseline terminal differed from the expected terminal and candidate now matches. The original 12-case fixture is *fully matched* (including intentionally failing control cases), so no improvement is achievable from that fixture as presently configured. `dev/harness_lab/ledger.py::compare` compares terminal labels and returns `tokens: null`, `cost_usd: null`; it does not evaluate variability or overhead thresholds. No eligible accepted patch is claimed.

M004 first rejected existing candidate rows on restart; M005 added `validate_candidate_resume_rows` and `--resume-candidate`, hash-linked `OptimizationCheckpoint`. Tests check invalid suite and completed-prefix preservation but not a real subprocess killed between writing a scored trial and completing the child/outer checkpoint. Correct what remains, do not overwrite M005's implemented improvements or preserve stale M004 wording as if it were current code.

## 3. Non-regression invariants and non-goals

- **Independent verifier** (outside proposal-editable scope) determines success; agent text, exit code alone, source patch self-review or lower token count alone never passes a failed verifier.
- **Negative controls** (intentional denied/failed/cancelled/timeout cases) are not counted as task failures to “improve”; a candidate making an expected-denial case pass is a **safety regression**. A fixed terminal match is a control, not an optimization objective.
- Task/case digest, suite/group/test owner, model/provider/tool surface, policy, baseline SHA, candidate binary hash, agent, run identity, score contract and verifier version are immutable across paired trials.
- Comparison requires all `(task_id, repeat)` pairs with no duplicates, identical expected/observed case versions, known actual denominators. Null usage/cost never equals zero and cannot support a hard token/cost acceptance claim.
- A candidate cannot modify the independent Python controller, grader, hidden holdout or budget settings. Acceptance never pushes/merges, starts outside-allowed tool commands or modifies the default user daemon.
- No VM/container setup, no application feature expansion, no product-wide score, no generic statistical framework, no synthetic “passing benchmark” marketed as a live gain.

## 4. Required production changes

### Versioned scoring contract

Define `ObjectiveV1` and `ComparisonReceiptV1` (names flexible), stored/frozen *outside candidate worktrees*, with two types:

1. **Correctness fix:** one or more non-control tasks have independently determined violated invariants at baseline and pass at candidate, while mandatory safe control cases, binary/build gates and holdout have no regression. The fixture provenance must prove the original defect; intentionally inverted expected results for the purpose of “winning” are prohibited.
2. **Reliability/efficiency:** all correctness and safety controls remain passing; measured paired trials show a specified, **predeclared** threshold in an observable metric such as avoidable tool failures, identical repeated tool reads, premature stall errors, completion latency or token count. Require sufficient repeated trials and deterministic seeds/paired model conditions where available; use a clear uncertainty rule (e.g. lower-confidence-bound/paired nonparametric requirement), separate warm/cold build/caching, and reject `insufficient_evidence`, high variance, missing telemetry or outlier-only improvement. Do not assert provider dollar savings from unknown price. A developer may elect strict correctness-only mode for baseline fixture.
  
Explicitly define expected-negative controls as protected invariants and specify the score's task group, holdout split, optimizer objective, minimum successful denominator, allowed failure budget, primary regression floors and resource weights. No “lower latency” acceptance for a candidate that exits early and does not do the task.

### Compare and admission correctness

- Refactor `ledger.compare` or layer a typed comparator to validate group/suite/case/verifier digests, requested/effective policy, binary/source/proposer identities, condition-of-measurement and complete paired repeats. Reject mixed protocols or model profiles unless explicitly designated a stratified model comparison.
- Accept cases with intentionally negative terminals only if the correct negative condition is proven (e.g., denied action really remained denied); an `execution_failed` string alone is not enough. Keep historical 12-case fixture and original pilot receipts immutable.
- Add controls for result gaming: patching verifier/test, shortening prompts, bypassing tool calls, skipping tests, pre-populating expected output in workspace, or hiding errors must fail. Changed-path allowlist is not the only defense; immutable outside-candidate verifier is decisive.
- Add holdout support appropriate for a small developer corpus, with suite digest and case group separation. Distinguish synthetic regression fixture for **controller mechanics** from a naturally observed defect for **real self-improvement**; labels persist to reports and closure.

### Crash-safe replay and bounded retries

- Review `OptimizationCheckpoint`, child `Checkpoint`, `validate_candidate_resume_rows`, evaluate `--resume-candidate`, gate build and patch hashes and `ledger.load_runs`. Define explicit ledger-append acknowledgment and durable state transition ordering. On restart, validate that a completed row's suite/case/subject/turn/score contract matches; never re-run completed rows under the same identity.
- Give interrupted in-flight attempt a recorded terminal `interrupted_unknown` (or explicit orphan reconciliation state) distinct from a finished trial. New attempt gets a new ID; don't overwrite it with a success; preserve denominator/provenance and capacity budget.
- Rebuild/reload candidate only when exact patch hash and binary fingerprint match checkpoint, with cleanly fenced daemon generation; refuse changes to baseline or outcome parser. Budget deadlines remain cumulative across restart, and process teardown only targets supervised children.
- Test crashes at deterministic cut points: before worktree creation, during OpenCode proposal (ambiguous refuse), after patch receipt, during `cargo build`, after first scored case before checkpoint write, midway through multi-repeat scoring, during comparison, just before acceptance. Use subprocess SIGTERM/SIGKILL on owned test processes, then resume. Preserve separate candidate/outer-run stable IDs.

## 5. Ordered work packages

A. **Contract and baseline audit:** classify all 12 prior cases into task objectives vs protected negative controls; freeze a score contract and independent verifier digest. Demonstrate the existing all-matched suite correctly yields “no candidate improvement possible” instead of accepting a cosmetic patch.

B. **Criterion engine:** implement correctness and robust performance/reliability gates with explicit insufficient-evidence outcome and negative-control guards. Design one small genuine, observed CodeGG harness defect task (if one exists) for C003 without modifying the baseline to force a win. Test metric traps and mixed profiles.

C. **Crash matrix:** add scripted failpoint/subprocess tests with one and multiple partial rows across child and outer checkpoint. Reconcile only completed rows and preserve baseline ledger/index prefix; show no silent retries, duplicate committed rows or extra requests.

D. **Provider-free E2E:** run offline failing/accepted, negative-control-rejected, performance-insufficient, interrupted/resumed qualifier profiles through real native ACP/private daemon and deterministic fixture. Synthetic injection is clearly named; no live performance claims. Ensure normal `optimize` flow is not prevented by a deliberately expected negative case's CLI exit code (evaluate structured receipts, not just process return code).

## 6. Failure, restart, contention semantics

Concurrent candidates must use distinct per-lane daemon, data, workspace and Cargo target roots. A ledger append crash either yields an independently verifiable committed row or an orphan requiring explicit reconciliation; no partial row is scored. A terminated proposer with no durable patch receipt remains ambiguous and cannot be silently retried. An external deadline/usage stop must propagate to the child while preserving an exact set of attempted/unattempted trials, total budget consumption and provenance. Two modifiers of the shared ledger cannot race to accept the same candidate.

## 7. Compatibility and migration

Prefer additive external versioned comparison/score JSON. Old ledger manifests are read-only and may be compared under the legacy read path, but cannot silently acquire a new score classification. If score schema or verifier version missing, either reject acceptance or migrate via an explicit independently verified mapping. No Rust shipping dependency or native DB migration.

## 8. Tests and verification commands

Python `unittest` cases: candidate turns expected negative to success (must reject); malformed pair; duplicate `(task, repeat)`; undefined cost; held-out task leak; unchanged all-green; genuine baseline failure repaired; performance regression despite correctness; missing usage; changed allowed paths; crash at each named cut point; repeated resume idempotency. Real ACP deterministic suite plus one deliberate faulted tool/broker case. Run:
```bash
python3 -m unittest discover -s dev/harness_lab/tests -v
python3 -m dev.harness_lab evaluate --help
python3 -m dev.harness_lab compare --help
python3 -m dev.harness_lab optimize --help
cargo test -p codegg --test acp_stdio
scripts/verify.sh quick
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
```
Use actual supported CLI options and record them, not only `--help`; hosted CI must be on exact implementation HEAD before final closure.

## 9. Acceptance, stop, and closure

**Accept** C002 only with an independently reproducible full candidate + outer controller interruption/resume on actual subprocess/native ACP scoring, preserved completed-prefix and incomplete-attempt identities, zero duplicated trial rows, retained original deadline, frozen baselines, accurate negative-control outcome, correct no-gain rejection, and a deterministic known-regression fix demonstration explicitly labeled synthetic. This qualifies the gate, not live OpenCode.

**Stop** if scoring requires a writable hidden grader, if a performance-only acceptance lacks repeatable evidence, or if recovery needs to silently mutate historical ledgers.

**Closure** `plans/closure/harness-lab-post-pilot-corrective/002-status.md`: commit/CI receipts, scoring contract with example accepted/rejected/insufficient, trust boundaries, kill-point matrix, actual trial IDs/denominators, no duplicate/side effect proof, tests and unblocking C003. Do not rewrite original M004/M005 closures; append current corrective disposition in roadmap/registry.
