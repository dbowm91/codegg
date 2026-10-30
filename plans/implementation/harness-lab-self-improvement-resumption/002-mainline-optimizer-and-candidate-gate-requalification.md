# Harness Lab Self-Improvement S002 — Mainline Optimizer and Candidate-Gate Requalification

Status: **blocked** until S001 closes and H007's resulting-mainline CI/source is qualified. PR #217 is merged. Offline plan ready; no provider-backed calls authorized.

Repository baseline: `main@99cecc050ed1ae418af62d2d10e6d34d791513e1` (PR #217 merge). Re-read exact main and hosted CI before execution.
Source roadmap: `plans/subsystems/harness-lab-self-improvement-resumption-roadmap.md`, S002.
Long-term references: `plans/000-long-term-specification.md` §§23–24, 26–29; `plans/001-terminology-and-domain-model.md`; `plans/003-planning-process.md`. Accepted ADR: `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md` (do not synthesize terminal stalls). Primary class: **invariant**.

Historical authorities: `plans/closure/harness-lab/005-status.md` (M005 optimizer), `plans/closure/harness-lab-post-pilot-corrective/002-status.md` (score/restore), `003-status.md` (authenticated accepted and rejected candidates), `plans/closure/harness-lab-continuation/001-status.md` (headless workspace parity), `plans/closure/harness-lab-mainline-qualification/001-status.md` and H007 closure (PR #217).

## 1. Objective

Prove that the **existing** `dev.harness_lab optimize` and `qualify` pipeline can still perform deterministic, trustworthy candidate acceptance on the *integrated mainline*: two private CodeGG subject daemons, frozen baseline, independent evaluator/holdouts, optional separately bounded proposer, isolated candidate builds, correctly paired scored runs, negative controls, hard stop and interrupted resume. Fix only reproduced regressions. Complete a **credential-free rehearsal** first; live capability remains separately gated.

## 2. Why blocked and what is already complete

M005 implemented an outer `run_optimizer` and hash-linked `OptimizationCheckpoint` with bounded iterations and resumable candidate qualification. C002 corrected acceptance over-relaxation, incomplete/negative controls and ledger replay. C003 ran an authenticated OpenCode proposal correcting headless workspace binding: 13/13 pairs, one correctness improvement, zero regressions, and a real killed/restarted replay. D001 established the fix on mainline. H006 proved native job status and provider cap in a live test; H007 introduced separate task/horizon outcomes. **Do not rebuild any of these components** unless a current-main failing-first reproduction demonstrates regression.

Hard dependencies: S001 closed with an evidence-qualified target/none disposition; independently verify H007's **actual resulting-mainline** CI and score/fixture tests. The PR #217 merge is complete; branch CI alone remains insufficient. The 30-minute H007 sustained-success target is **not** a dependency.

## 3. Current implementation and risk inventory

Likely consumers: `dev/harness_lab/__main__.py` CLI, `supervisor.py` private daemon/ACP lanes and identity, `improve.py` candidate/proposer/build gates/optimization, `ledger.py` hash-linked receipts, `scoring.py` paired comparator, `budget.py` admission, `autonomy_run.py` native task/horizon result. Rust production seams are existing `src/acp.rs`, `src/exec.rs`, `src/tool`, and `src/core`; only inspect or modify if a failing-first proof reaches them.

The original `optimize` command, manual proposer and qualification paths are documented in `dev/harness_lab/README.md`. H007's deeper fixture is *not automatically a valid candidate qualification suite*: it measures task/horizon behavior and its holdout remains outside the candidate write scope. Select and freeze an appropriate deterministic, correct-expected baseline suite before allowing proposals.

Important budget distinction: CodeGG subject `opencode_go` requests can be bounded at CodeGG's provider boundary; an external `opencode run --auto --format json` **proposer** makes its own requests. A CodeGG request cap does not cover the proposer. Existing `--budget` and `--live-provider` options must be audited for actual pretransport enforcement on *both* actors before any live attempt is admissible. No guess based on CLI flags, endpoint selection or provider subscription quota.

## 4. Invariants and non-goals

- The stable controller/evaluator, baseline source, hidden verifier, case manifest, run identity, acceptance criteria and historic ledger cannot be modified by the candidate. A candidate may add scoped regression tests but cannot rewrite the independent verifier or raise its own score.
- A candidate only improves if a preregistered correctness predicate changes from failed to passed on complete comparable cases while all protected controls and holdouts pass. Latency, terminal relabeling, route timeout, or more observed activity cannot cause acceptance. A zero-improvement and incomplete-evidence candidate must be rejected or unscorable.
- Candidate worktree, Cargo target, daemon home, mutable data/config and private test artifacts do not collide with the operator's main or other lanes. `yolo` is explicit for opted-in test sessions; `full-host` and `workspace-write` are distinct, explicit profiles, and Deny remains Deny.
- Interrupted runs do not reset cumulative time/request budgets, duplicate mutations, or silently count an unmeasured pair. A skipped/missing event cannot pass.
- No new production scheduler, model router, external browser, VM/container, automatic merge/push or automatic live provider trial. A mock proposer/seeded historical patch may prove mechanics but must be labelled **synthetic**, not real AI self-improvement.

## 5. Scope

In: exact-main audit, deterministically replayable candidate/negative-control suite, isolated two-lane smoke, rebuild/qualification, score and ledger compatibility with H007 result separation, bounded manual/fixture proposer, kill/resume/digest guards, explicit preparatory audit of proposer-side budget admission, operator handoff commands.

Out: live model requests, new provider adapter, fixing model's Rust ability, broad autonomous-search strategies, polishing generic CLI, new holdout corpus without a need, changes to product defaults, advancing H007's duration gate, unaudited patch merge.

## 6. Required production changes

**Prefer no production changes.** If the current production binary fails the same deterministic tool contract on a properly rooted live ACP/exec path, write a failing-first regression and change the owning Rust module only. Otherwise changes stay within `dev/harness_lab`, fixture tests, guards and documentation. No migration unless a demonstrably incompatible H007 schema requires a versioned additive reader.

**Evaluation/score contract:** audit old `qualify_candidate` and `run_optimizer` with H007 task/horizon schemas: task correctness is primary; a quick correct completion is not a task failure just because horizon is `not_observed`. Horizon can be an optional independent metric, never a free acceptance improvement. Preserve old score-schema digest semantics and refuse mixing incomparable cohorts.

**Two-lane supervisor:** require exact binary hash, sourced Git SHA, independently measured daemon ID and distinct private resources; reject ambiguous `CODEGG_CORE_ENDPOINT` attachment to production daemon. Explicit provider fixture secrets are private dummy credentials with loopback only. Build candidate binary outside source checkout and retain sufficient disk headroom.

**Proposer and admission:** test mock/manual path with zero credential/network. For a future real OpenCode proposer, demonstrate its own request count/timeout admission or define an enforceable separately supervised launch envelope with the known limitation when request-level hard admission is impossible. If the proposer cannot be hard-capped at the required provider boundary, declare live execution **blocked** and stop; do not claim the subject's cap or `--max-iterations` bounds spending. Treat provider-unavailable billed USD as `unknown`.

**Security and static guards:** refuse candidate modifications outside the frozen allow-prefix set; protect controller, test/verifier code and ledger root by construction (including symlinks). Test real fail-closed denial and cancellation. Keep original headless/user-session policy, broker authority and ADR-0040 recovery semantics.

## 7. Ordered work packages

**A — Exact-main audit and scope selection.** With H007 merged, pin resulting main, evaluate S001 findings and list already implemented contracts. Build matching `codegg` and trusted `codegg-sandbox-helper`; run `codegg doctor installation`, two-subject smoke and tool-path fixture in separate private roots. Freeze one deterministic defect-candidate pair and separate protected controls. If no current harness defect is available, seed an explicitly labelled *historical* regression solely to prove controller mechanics.

**B — Offline candidate acceptance and rejection.** Run manual/mocked proposal with isolated worktree and target, full build/fmt/focused native test, frozen ACP suite, hidden/negative controls and complete ledger comparator. Prove a known-correct candidate accepted on frozen synthetic evidence, and a no-op, incomplete patch and changed-expected-terminal candidate rejected. Prove a fast task pass stays a task pass under the H007 reader. Capture source/candidate binary hashes and all trial pairs.

**C — Crash/restart and contention.** Kill the controller after >=1 persisted task/repeat pair and resume under identical args; prove no duplicate pair/provider admission and unchanged wallclock deadline. Refuse changed source, suite digest, case digest, candidate patch/binary, provider profile, policy or budget. Run two subjects concurrently and show distinct daemon generations/roots/Cargo targets plus cleanup of one while the other continues.

**D — Budget/proposer seam audit.** Prove subject request N accepted then N+1 refused *before HTTP* with a local fixture. Investigate OpenCode CLI proposer request authority separately; write a credential-free fake adapter that exercises admission/timeout and refusal paths. If the actual authenticated proposer lacks a provable request-level pretransport cap, report a hard live-qualification blocker and a narrowly scoped follow-up. No real provider call under this milestone.

**E — Operator guide and closure.** Publish one documented command sequence for `doctor`, `smoke`, `tool-trial`, `evaluate`, `qualify` and `optimize` against current main, plus expected negative exits and cleanup. Record whether live S003 can proceed or why it cannot, with no speculative fixes.

## 8. Failure, cancellation, restart and contention

Malformed/changing manifests are refusals, not re-runnable failed tasks. Persist parent-owned ledger before any child/candidate result can be considered scored. On cancellation stop child processes/daemons and preserve immutable receipts; on replay verify exact checkpoint tail and original deadline. No retry of a provider-unknown request under a consumed budget. Two candidates never write the same source, daemon state, credential config, worktree or target. Out-of-workspace commands remain denied under `workspace-write`; `full-host` is only used when explicitly selected by the operator.

## 9. Compatibility and migration

Read older M/C/D receipts and H007 schema without rewriting them. Only add versioned adapters/guards if the current schema cannot represent task-vs-horizon honestly. No ACP/protocol/storage migration by default. Existing CLI names and manual/no-OpenCode workflow must remain functional. Changes affecting production policy, daemon ownership or explicit provider budget contracts require an ADR or separate owning-subsystem plan.

## 10. Tests

- Focused Python `test_improve`, `test_scoring`, `test_ledger`, `test_budget`, `test_supervisor`, `test_autonomy_run`, `test_h007`, including non-vacuous model-independent positive and negative controls.
- Native composed ACP/policy test on actual binary: tool available in rooted workspace; explicit Deny respected; a background `test`/job report not invented.
- Acceptance falsifiers: no-op candidate, advisory-only latency delta, relabeled failure, unpaired case, failed/absent verifier, fixture mutation, missing ACP terminal and a faster correct task with horizon not_observed.
- Resume: SIGKILL after scored row, checkpoint tamper, baseline/candidate digest mismatch, changed budget and original deadline; no duplicate admission.
- Security: symlinked paths escaping candidate root, candidate edits to external evaluator/holdout, private daemon attachment, insufficient disk, malformed provider admission receipts.
- Provider-cap boundary test including refusal of N+1 and separate proposer admission audit. Run **no live provider**.

## 11. Verification commands

```bash
cargo build --locked --bin codegg --bin codegg-sandbox-helper
python3 -m dev.harness_lab doctor --binary target/debug/codegg
python3 -m dev.harness_lab smoke --binary target/debug/codegg --subjects 2
python3 -m dev.harness_lab tool-trial --binary target/debug/codegg --profile workspace-write
python3 -m unittest dev.harness_lab.tests.test_improve dev.harness_lab.tests.test_scoring dev.harness_lab.tests.test_ledger
python3 -m unittest discover -s dev/harness_lab/tests -v
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
git diff --check
scripts/verify.sh quick
# On a native production-code correction or to close mainline gate:
scripts/verify.sh full
```

Commands depending on credentials must not be run here. Record actual commands, skip reasons, environment, disk floor and hosted exact-head results honestly.

## 12. Documentation/static guards

Update `dev/harness_lab/README.md` with current-main operator procedure, proposer-vs-subject budget distinction, local fixture-only recipe, successful/negative control definitions and safe cleanup. Do not overwrite historic C003 or H007 closures. Add guards for verifier path/candidate patch scope and cohort identity only if missing current guards are proven.

## 13. Acceptance criteria

A clean worktree and documented tools reproduce the *existing* optimizer's lifecycle end to end on current main without credentials. Two private subjects remain distinct, frozen test and evaluator are outside candidate authority, complete comparable pairs correctly accept one labelled synthetic correction and reject no-op/bogus cases, restart preserves identity/budget, all negative controls hold and cleanup leaves main untouched. The output explicitly states whether real proposer pretransport request admission is proven or S003 live remains blocked; no auth or live attempt is inferred.

## 14. Stop conditions

Stop for a missing resulting-mainline verification, unresolved S001 finding, inability to freeze an independent verifier, inability to isolate two private subjects, failure to enforce the proposed spending boundary, missing usable local binary/helper, or required changes that would weaken native permissions or allow candidate-modified scoring. Escalate a true unrelated runtime defect to its owning subsystem.

## 15. Closure evidence

`plans/closure/harness-lab-self-improvement-resumption/002-status.md`: exact merge/source/binary/fixture/verifier hashes, two-private-lane proof, positive+negative paired receipts, budget/proposer admission proof or named blocker, actual SIGKILL/resume trace, verification commands and hosted status, candidate scope and security evidence, known limitations and S003 readiness decision. No claimed real AI improvement from synthetic patches.

## 16. Handoff

This is a **requalification of an implemented controller**, not an invitation to replace `improve.py`. Use resource-bounded Cargo jobs and isolated target directories. The operator's existing CodeGG daemon, repository and provider credentials must never become a subject lane by fallback.
