# Agent Loop Operational Reliability M007 — Sustained Autonomy Trajectory Qualification

Status: blocked: M001–M006 closure and ADR-0040 disposition

Repository baseline: `f37840ef5fca36d44b35daec5da6b26cd049a3ee` (main; verify actual starting head)

Source roadmap: `plans/subsystems/agent-loop-operational-reliability-roadmap.md#7-milestones`

Long-term requirements: `plans/000-long-term-specification.md#26-reliability-and-recovery`; `plans/000-long-term-specification.md#28-observability`; `plans/000-long-term-specification.md#29-system-invariants`; `plans/001-terminology-and-domain-model.md#7-agent-terms`.

Applicable ADRs: ADR-0040 accepted/superseded disposition required; no scope to change the architectural decision in qualification.

Primary class: capability

## 1. Objective

Establish source-grounded, repeatable evidence that a complete Codegg coding agent can run a long read/edit/build/test/delegate cycle without false stalls or context-retrieval traps, and handles real provider/storage failures without losing control or crossing authorization ceilings.

## 2. Why this milestone is blocked

M001–M006 must close with verified production evidence and ADR-0040 must have an explicit accepted/superseded decision. A synthetic qualification fixture cannot replace implementation or waive pending decision gates.

## 3. Current implementation evidence

- The reported user test completed basic file editing but saw natural-language tool discovery failures, relative glob misses, confusing preflight output, SQLite 517 on child admission and a queued child failing on deferred tool compatibility. Source inspection identifies projection and progress/recovery interactions that can amplify those failures.
- C001 already corrected `ShortCycle` and `EquivalentError`; those tests must stay unchanged. The current default `ExecutionLimits` is 100 turns, 1,000,000 tokens, 600 seconds for an ordinary loop; durable work/goals own longer budgets.
- `tests/agent_loop_harness.rs`, context projection tests, provider mock streams, durable scheduler and SQLite stores give reusable deterministic fixtures. No new test framework or server is needed.

## 4. Invariants

No hanging background worker or endless polling; no hidden tool denial or authorization escalation; no loss of canonical worktree/run/project/session identity; no fabricated success from a partial result or child failure; no unbounded tool output, tests or provider retries; user cancellation and explicit budgets terminate promptly; checkpoints preserve progress across restart.

## 5. Scope

In: a composed deterministic scenario suite over **real production wiring** (bounded multi-stage harness scenarios plus focused store/provider/projection fixtures); optional manually initiated model-backed smoke with exact model/provider/version; failure taxonomy and retry/rollback audit; operator guidance for making long runs observable; concise closure evidence. The bounded scenario equivalent may be several tightly scoped fixtures when a single 100-step transcript would duplicate existing independent failure-injection owners.

Out: a permanent CI stress lane, live-model nightly, benchmark competition, requiring 30–60-minute CI, SBC compilation, release gate, new agent scheduler or optimization ungrounded in measured runs.

## 6. Required production changes

Do not introduce feature changes here except tiny instrumentation or regression fixes within established owners. Qualify the existing deterministic fixtures that exercise `AgentLoop` + `ToolBroker` + projection + recovery, tool_search/glob, SQLite AgentRunStore + task notifications, and provider wire validation. Reuse existing `Goal`/`WorkPlan`/context-epoch controls and scheduler to test long job vs interactive turn limits separately.

Expose a bounded redacted event timeline: sequence, model ID and version/provenance, call identity, output requested/visible/recovered bytes, state/progress signal, recovery action, provider HTTP failure class, child run status, budget/cancellation, checkpoint ID. Do not log secrets or full hidden reasoning.

## 7. Ordered work packages

A. Freeze a deterministic scripted trajectory with a small Rust fixture repository (no extra dependency compile): discovery -> glob -> read 1,000 lines -> edit -> run repeatedly failing then passing build/test -> spawn two fake children -> wait via notifications -> handle one recoverable storage/provider error -> complete.

B. Run baseline/after comparisons using exact old and new heads where available. Record false terminal stalls, valid completion, token/tool overhead, request retries, artifact round-trips, SQLite retries, provider schema failures, child error visibility and authorization violations.

C. Inject structured fault matrix: `BUSY_SNAPSHOT`, rejected deferral 400, missing context artifact, truncated content, repeated no-op shell, permission Deny, cancellation during subprocess, provider timeout, crash/restart before and after durable child admission. Confirm durable state and recovery exactly once.

D. Add minimal diagnostics/fixtures to existing test targets and docs; run broad checks. Optional manual 30–60-minute model-backed operation may supplement but does **not** substitute deterministic suite, and is not a permanent CI prerequisite.

## 8. Failure, cancellation, restart, and contention

Every injected failure has a typed terminal or recoverable outcome and bounded retry. Failed child is attributable and retrievable after restart. Cancelled parent or expired budget stops new work; existing child behavior follows canonical policy. Parallel children obey scheduler/worktree limits; no raw shell polling when completion notification is available. Failed artifact persistence cannot claim full retrieval; partial output/corrupted continuation triggers explicit recovery.

## 9. Compatibility and migration

No new schema planned. If M001–M006 introduced new optional fields, test old config and old clients. Existing users keep interactive default budgets while durable autonomous jobs are configured explicitly with appropriate goal/run budgets and checkpoint/continuation. No silently increased global timeout.

## 10. Required tests

Focused: select regression filters from every closed M001–M006. End-to-end: deterministic fake-provider ~100+ meaningful steps or bounded scenario equivalent with multiple edits/tests/retries/pages, verifying complete task; separately test true endless no-op stops. Restart: checkpoint/child/job resume. Contention: 8+ concurrent child admissions with SQLite WAL and cancellation. Security: unauthorized paths, denied tools, sandbox unavailable, secret-bearing logs, child ceiling, provider malformed responses. Manual: optional 30–60-minute one-model smoke with exact build SHA/environment, separate observations from deterministic proof; no SBC startup.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

First run focused test filters for the named modules (`cargo test -p codegg --lib <filter> --locked`) and existing integration targets; confirm actual crate/target names at execution time. Record exact commands, stdout/result and head in closure. Hosted checks only where the existing closure policy requires them. No live-provider CI or new scheduled test lane.

## 12. Documentation updates

Update only affected `architecture/` and `docs/` surfaces; record any compatibility/config changes in `docs/tools.md` or `architecture/provider.md` where applicable. Keep existing predecessor closure history immutable.

## 13. Acceptance criteria

1. Scripted multi-stage agent completes without false `ShortCycle`/`EquivalentResult` stall, missing tool description, lost paged source or undisclosed child failure.
2. Repeated outcomes with unknown progress remain recoverable until an existing hard limit ends the run with its budget reason; no unsupported `stagnated` claim is emitted. Cancellation and policy denial remain independent.
3. Simulated provider/schema and transient SQLite failures recover where safe, never duplicate a side effect and always leave the parent informed.
4. Exact-head focused and ordinary broad checks are recorded truthfully; any unavailable platform/model test is listed, not called passed.
5. All M001–M006 closure records and ADR-0040 disposition exist before the roadmap is marked closed.

## 14. Stop conditions

Stop and record a blocker if implementation would need a new authority owner, broad sandbox changes, unsafe command execution in tests, an undocumented provider wire workaround, a nonadditive migration, or an unaccepted architectural decision. A test-only mitigation with a production gap is not closure.

## 15. Handoff

At start reconcile this plan against actual code and exact head. Preserve unrelated changes. Commit scoped production/tests/docs and add `plans/closure/agent-loop-operational-reliability/NNN-status.md` with requirement/evidence table, exact commands and outcomes, restart/contention/security proof, and residuals. Update registry and roadmap to reflect actual closure or blocker only.
