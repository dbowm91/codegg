# Developer Harness Lab M004 — Optional OpenCode Proposer and Candidate Qualification Loop

Status: blocked — hard dependency on Harness Lab M003 closure; design handoff prepared

Repository baseline: `f37840ef5fca36d44b35daec5da6b26cd049a3ee` (`main`, reviewed 2026-10-18). Rebase reasoning on M003 closure before implementation.

Source roadmap: `plans/subsystems/harness-lab-roadmap.md#7-milestones`

Long-term requirements: `plans/000-long-term-specification.md#23-acp-boundary`, `#24-protocol-and-storage-requirements`, `#26-reliability-and-recovery`, `#28-observability`, `#29-system-invariants`; `plans/001-terminology-and-domain-model.md`.

Primary class: capability; supporting controller infrastructure

Research: OpenCode CLI https://docs.opencode.ai/docs/cli/ ; OpenCode permissions https://docs.opencode.ai/docs/permissions/ ; OpenCode ACP https://docs.opencode.ai/docs/acp/ ; server https://docs.opencode.ai/docs/server/ ; DGM https://arxiv.org/abs/2505.22954 ; SEAGym https://arxiv.org/abs/2606.17546 .

## 1. Objective

Add a budgeted and resumable development-only loop: **assess -> report -> propose -> patch -> compile -> focused tests -> live ACP retest -> compare -> accept/reject -> changelog/checkpoint**. Use external **OpenCode** as the first optional stable *proposer*, never as the experiment supervisor or evaluator. The Python controller must also work with manual patches/no proposer.

## 2. Dependency readiness

Blocked until M003 has an auditable baseline, failure corpus, independent task verifiers and a reliable candidate comparison. One may probe OpenCode's installed CLI/API version before that milestone, but autonomous patch acceptance cannot start early.

## 3. Current evidence

OpenCode officially supports `opencode run --format json` for noninteractive execution, `opencode run --auto` for non-Ask approval (explicit Deny survives), `opencode acp` over stdio, and `opencode serve` plus JS/TS SDK/OpenAPI. A version-pinned **CLI runner** is preferable for the first Python implementation: it is subprocess-oriented, straightforward to supervise and does not require a JS sidecar. `opencode acp` is a viable alternative if actual protocol/version and session compatibility are qualified; `serve` is optional when startup/continuation overhead warrants an authenticated, loopback-only persistent proposer. Do not assume changing OpenCode versions preserve JSON event fields without contract tests.

CodeGG's proposer target is **its agent-facing harness**: tool discovery/palette, tool contracts/output projection, context handling, recovery, background job invocation, subagent workstream behavior, permission usability and ACP transport. General feature work, arbitrary dependency refreshes and product UI expansion are not evaluation objectives.

## 4. Invariants

1. The Python controller, task registry, hidden verifiers, accepted baselines and ledger are not edited by the proposer; immutable snapshots/checksums protect their recorded provenance. On a trusted workstation, this is an experiment-integrity measure **not an OS containment guarantee** against deliberately malicious full-host code.
2. The proposer works in a candidate-only Git worktree; tests run against the compiled candidate binary with per-candidate `CODEGG_DAEMON_HOME`/data/workspace. Controller never invokes a candidate as its own parent/controller.
3. OpenCode is optional and version-pinned; it never decides whether its patch passes, whether additional budget exists, whether verifiers should change, or whether acceptance/revert happens.
4. The only automatically accepted edits belong to an explicit scope allowlist, verified from `git diff` after execution. Changes to tests are allowed only under reviewed restrictions; benchmark verifiers/hidden cases/controller changes fail eligibility. Do not execute arbitrary patches that try to modify external home/config or controller records.
5. The loop never modifies the accepted `main` branch, force-updates remote branches, pushes, merges, creates releases, or modifies user worktrees. A candidate can be preserved on an explicitly named local branch after acceptance.
6. Budget exhaustion, a hung tool/build/agent, failed compile, no meaningful progress or a verification regression must **stop or reject**, not resume indefinitely. Changelog and provenance records are mandatory for acceptance.

## 5. Scope

In: proposer adapter interface, OpenCode CLI implementation, version/credential preflight, bounded prompts and structured result capture, candidate worktrees, separated Cargo build targets, compile/test/retest gates, retries/rollback/checkpoint, changelog, generated reports and model comparison.

Out: OpenCode installation/credential enrollment automation, live background autonomous service, mutation of harness source by subject CodeGG during a benchmark case, VM/container construction, optimizer training, use of a second model to replace deterministic grading, automatic remote push, large-scale candidate DAG/genetic search.

## 6. Required changes

### Controller state machine and budgets

- Define explicit states `BASELINED`, `DIAGNOSED`, `PROPOSING`, `PATCHED`, `BUILDING`, `QUALIFYING`, `EVALUATING`, `COMPARING`, `ACCEPTED`/`REJECTED`/`BLOCKED`. Persist transitions atomically, with first-valid terminal state and monotonic attempt IDs.
- Config includes `max_iterations`, `max_candidate_retries`, `max_proposer_calls`, `max_model_tokens`/`max_total_usd` if observable, `max_wallclock`, `max_parallel_subjects`, `max_parallel_builds`, `max_no_improvement_iterations`, `require_changelog`, `accepted_regression_floor`, `stop_on_unverifiable_cost` and `preserve_rejected`.
- Distinguish a **compile-correction retry** from another independent candidate and from **repeating a scored trial**. A failed trial remains in the denominator; an infrastructure retry gets its own identity and is never hidden. Hard budgets include every attempt and teardown time.
- Resuming from a checkpoint validates Git refs, worktree dirtiness, binary SHA, tests/verifiers/manifest digests and ledger hashes. If state cannot be safely reconciled, halt with a diagnostic.

### OpenCode proposer adapter

- Minimal interface `Proposer.propose(assessment, allowed_scope, budget, candidate_worktree)->ProposalReceipt`; implement `ManualProposer` (no-model baseline) and `OpenCodeCliProposer`.
- For OpenCode default to `opencode run --auto --format json --dir <candidate-worktree> ...` (pin tested OpenCode version and accepted flags). Ensure no interactive Ask; explicit Deny surfaces as failure, not an automatic override. Sanitize environment output; preserve provider config without embedding keys in reports.
- Use a bounded instruction template with target failure, native ACP/tool evidence, reproduction instructions, allowed changed paths and mandatory **candidate-local** explanation/changelog. The proposer is **not** told hidden holdout outputs or arbitrary external workstation paths. Do not assume textual promises enforce scope: verify changed files and forbidden paths afterwards.
- Parse OpenCode JSON events/version safely; if schema unsupported, fail closed and provide a CLI diagnostics report. Optional future `opencode acp` or `serve` integration must meet the same receipt/timeout/cancellation guarantees, with authenticated loopback-only server if used.
- Caller can substitute another external proposer without changing controller/evaluator architecture. Record model/provider and provenance separately for proposer and CodeGG subject.

### Build and candidate admission

- Base Git revision and task suite are immutable for an experiment. Create candidate branch/worktree using Git APIs/CLI with exact baseline SHA; use **separate target directories per candidate** or a validated compiler cache. Avoid simultaneous `cargo build` calls stomping on shared targets.
- Two-stage gates: (1) compile + `cargo fmt --check`, focused tests and relevant source guards; (2) real CodeGG ACP evaluation using M001/M002 private candidate daemon, then M003 baseline/held-out comparisons.
- Verify no controller/fixture/verifier or out-of-scope edits and no unexplained dirty untracked files; refuse to adopt if scope or provenance mismatches. Run changelog requirement as a machine-verifiable artifact containing root cause, touched contracts, validation results, accepted risks and candidate SHA.
- Report multiobjective correctness/reliability, cost/time/avoidable tool-friction rather than a single manipulable overall score. Use primary correctness/reliability nonregression thresholds, paired trials and holdout; reject inconclusive results rather than assuming a model self-review is proof.
- Cleanup only owned processes/temporary subjects; preserve rejected patch on explicit option, otherwise discard candidate worktree after evidence is persisted. No automatic integration into the default development worktree.

## 7. Work packages

**A — Dry-run controller.** Build candidate state machine, budget arithmetic, receipt/checkpoint and rollback with `ManualProposer`; verify no paid inference or real OpenCode needed.

**B — OpenCode adapter contract.** Pin/run `opencode run --auto --format json` on a scratch codegg worktree; prove exit code, JSON event decoding, no Ask, timeout/cancellation, sensitive-output redaction and forbidden-path rejection. Document installed version and supported interface; a missing OpenCode binary must fail gracefully without affecting manual mode.

**C — Build/test evaluator gates.** Enforce serialized or bounded builds, candidate hashes, immutable test fixtures, changelog, scoped diff and retest against separately pinned ACP lane(s). Baseline never overwritten.

**D — Bounded loop smoke.** Run one controlled fixture that yields a failing candidate patch followed by a corrective retry and a passing/rejected outcome; repeat with max iterations=1, max retries=0, no-progress stops, forced controller interruption and resumed checkpoint. Prove every requested stop occurs exactly once with complete evidence.

## 8. Failure, cancellation and concurrency

OpenCode process hang/crash or malformed JSON fails **proposal attempt**, not the controller; signal and reap owned process groups. A compile failure may consume a bounded retry with no full model assessment. A timed-out/failed CodeGG benchmark may be a real regression and is never silently retried into success. Interruptions before `ACCEPTED` cannot finalize adoption. Multiple candidate evaluations are safe only with separate worktrees, daemons, mutable data and compatible build caches. External Git operations must never touch developer's unrelated worktrees.

## 9. Compatibility and migration

No CodeGG production DB migration, no OpenCode library dependency in shipping Rust, no forced OpenCode installation for other developers. All Python/config/event schemas versioned; `ManualProposer` remains supported. If OpenCode CLI is incompatible with the selected version, pin/downgrade or select optional qualified interface, not ad hoc JSON parsing fallbacks.

## 10. Tests

- Pure Python state/budget tests: zero-iteration, exhaustion at final retry, cumulative wallclock/cost caps, duplicate terminal transition, crash during build, resume against mismatched Git SHA, invalid changelog, scope violation, verifier changes, false pass, stalled proposer.
- Fake executable `opencode` fixtures with JSON frames, stdout pollution, permission Ask, denied permission, subprocess hang, crash, exit success without patch; assert controller exits correctly and continues in manual mode.
- Real local dry-run/no-provider build-candidate fixture plus a **manual opt-in** live OpenCode + CodeGG ACP one-cycle pilot. Real-model outcomes marked optional and never claimed based on mocks.
- Non-regression of `main`, worktree cleanup, binary hash and per-lane daemon attachment, reporter replay after restart.

## 11. Verification commands

```bash
python3 -m unittest discover -s dev/harness_lab/tests -v
# Actual implemented CLI equivalents must exist and be documented:
python3 -m dev.harness_lab improve --proposer manual --max-iterations 1 --dry-run
python3 -m dev.harness_lab improve --proposer opencode --max-iterations 1 --max-candidate-retries 1
cargo fmt --all -- --check
./verify.sh quick
```

The real OpenCode step is opt-in, requires an installed authenticated OpenCode binary, and must not be a routine CI requirement. Include exact build, focused test and candidate ACP commands in closure.

## 12. Acceptance / stop conditions

Close only with: tested `ManualProposer` and OpenCode optional adapter; exact version-validated JSON event handling; correct bounded retry/timeout; required changelog+patch receipts; independent compile/qualification/heldout criteria; accepted and rejected candidate traces; cancel/resume proof; no mutation of the normal user daemon or `main`. If OpenCode needs an interactive response, runtime authority is ambiguous, costs cannot honor a configured **hard** cap, or verified candidate worktree state differs from expectation, stop rather than auto-approve or force acceptance.

## 13. Closure artifact

`plans/closure/harness-lab/004-status.md` with implementation commit, source/pinned version of OpenCode, provider-backed/fixture-backed evidence separately labeled, candidate branch/hash receipts, scope gate outcomes, budget arithmetic audit, retry/cancel/recovery proof, changelog examples and residual recommendations.
