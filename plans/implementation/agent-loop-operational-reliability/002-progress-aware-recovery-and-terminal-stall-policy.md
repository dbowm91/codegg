# Agent Loop Operational Reliability M002 — Progress-aware Recovery and Terminal Stall Policy

Status: blocked: proposed ADR-0040 decision on terminal authority

Repository baseline: `f37840ef5fca36d44b35daec5da6b26cd049a3ee` (main; verify at implementation start)

Source roadmap: `plans/subsystems/agent-loop-operational-reliability-roadmap.md#7-milestones`

Long-term requirements: `plans/000-long-term-specification.md#26-reliability-and-recovery`; `plans/000-long-term-specification.md#28-observability`; `plans/000-long-term-specification.md#29-system-invariants`; `plans/001-terminology-and-domain-model.md#7-agent-terms`.

Applicable ADRs: `plans/adrs/ADR-0040-evidence-based-agent-recovery-termination.md` (proposed; ACCEPTANCE REQUIRED before behavior change); prior C001 closure remains authoritative until then.

Primary class: invariant

## 1. Objective

Remove false terminal stalls from ordinary repeated reads, shell builds/tests, artifact retrieval, and child/job observation while retaining independently bounded handling of *confirmed* no-progress cycles. Separate incident classification, progress evidence, model nudges, and true run termination.

## 2. Why this milestone is blocked

The maintainer must accept or replace ADR-0040's recovery terminal-authority decision. Do not simply delete `Stall` or weaken budgets under a planning-only approval. Preparatory failing tests and evidence collection may proceed independently, but altered semantics cannot be asserted complete.

## 3. Current implementation evidence

- `src/agent/progress_recovery.rs`: whole `action_key()` and per-incident reset correctly fixed by closed C001; removed `ShortCycle` must not reappear.
- `DEFAULT_MAX_RECOVERIES=4`, `AutonomyState::MAX_TRANSITIONS=4`, `MAX_POST_TOOL_CONTINUATIONS=1`; a non-progressing repeated action climbs `Nudge/Correct/RestoreBasePalette/Replan/Stall`.
- `src/agent/loop.rs` computes `state_changed` from a batch-drained file-change bus for recognized writers; successful `bash`, `git`, job submission and child progress are *not* credited. No per-tool post-action oracle exists.
- `RecoveryDecision::Stalled` publishes `AppEvent::Error` and breaks; `src/agent/turn_completion.rs` separately enforces `ExecutionLimits`/goal/agent limits.
- Typed status already distinguishes success/denial/timeout/cancel/protocol error. Closed C001 and agent-loop harness tests are mandatory non-regressions.

## 4. Invariants

No tool-name-only incidents; no reuse of unrelated recovery count; no fabricated progress; volatile timestamp changes alone do not reset stagnation; typed errors not parsed from prose; no retry of acknowledged mutation; immutable tool/permission ceiling; budget, cancellation and goal stop conditions remain authoritative and observable; bounded per-run recovery footprint.

## 5. Scope

In: evidence-based recovery observation and terminal decision; per-state action identity/revision tracking; output-completeness semantics (from M001, interface only until landed); transcript-safe diagnostics, distinct operator reasons; config/test migration for approved ADR.

Out: provider transport retry rewrite, new goal/workflow scheduler, model mind-reading, unbounded continuation, automatic child merges, restoration of old false-positive branches.

## 6. Required production changes

**Progress observations:** record typed execution status, canonical call/arg digest, relevant workspace revision/checkpoint, tool completion/consumed-page state, optional structured test verdict/diagnostic fingerprint, and known child/job status transitions. Separate `ConfirmedStagnation`, `Unknown`, `Progress` in internal signal semantics. Preserve bounded fingerprints, not secrets or full arguments/results.

**Recovery owner:** an identical command *after a workspace edit*, and a repeated artifact page that advances offset, must not count as the same stagnant state; a repeated read of a still-incomplete projected artifact is not terminal progress failure. Treat unrelated failures independently. Use per-incident recoveries with evidence horizon; apply the accepted ADR's bounded cap only to confirmed no-op cycles. Make `MAX_POST_TOOL_CONTINUATIONS` behavior explicit and tested for different provider stop reasons. Do not claim a successful process inherently mutated anything.

**Lifecycle:** distinguish terminal `budget_exhausted`, `cancelled`, `denied`, `provider_unrecoverable`, and affirmed `stagnated` from harmless repeats. Recoveries should steer the model before terminating; if evidence insufficient, prefer normal continuation until hard bounds.

**Compatibility/security:** version or document changes to recovery config and diagnostics; no cross-session visibility; no additional authority to tools.

## 7. Ordered work packages

A. Capture deterministic failing trajectories through actual `AgentLoop` (not controller-only): repeated `cargo test` before/after edit, read pagination, child wait, build-fix loop, repeated no-op output. Reconcile C001 tests rather than deleting them.

B. Define finite typed progress evidence and which producer owns each signal (workspace event/revision, structured job/test state, page completion). Reject hardcoded `false` fields and false "state change" inferred from human prose.

C. Implement ADR-approved bounded recovery and separate terminal reason with explicit configurable limits; keep default safe and consistently applied in foreground, follow-up and durable child loop paths.

D. Add observability/redaction, deterministic provider replay and failure injection; update architecture and close only with an affirmed-stagnation counterexample.

## 8. Failure, cancellation, restart, and contention

Cancellation and budget exhaustion win over model recovery. The next call after restart must not assume missing progress metadata means confirmed stagnation. Two successful concurrent writers must not confer false progress attribution to unrelated calls. Child/job advancement must use canonical durable identities. Retry after a recorded side effect follows existing idempotency fences only.

## 9. Compatibility and migration

No new history store or DB migration anticipated. Preserve historical incident kinds and C001 closure evidence. Any added result DTO/config is optional/backward compatible; old transcript/playback remains intelligible. If ADR rejected, close as blocked/proposed disposition, not as a silently changed policy.

## 10. Required tests

Focused: old C001 `progress_recovery` tests and new revision/state/paging observations; typed denial/timeout/cancel vs `EquivalentError`. Integration: `tests/agent_loop_harness.rs` plus scripted build-fix (same command after edit), 20-file read, shell/noop, error bisection and wait/resume. Restart: evidence missing vs durable child advancement. Concurrency: batch attribution and independent incident identities. Security: volatile outputs cannot prevent bound, permissions not elevated. True negative: identical no-op calls with unchanged revision *eventually stop* under the approved affirmative cap.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
git diff --check
```

Run applicable focused tests from §10 *before* broad verification; use `cargo test -p codegg --lib <module_filter> --locked` and/or the named existing integration target, adjusting command to the actual crate/target at implementation time. Record exact commands and results; never claim verification that was not run. No new CI lane, permanent benchmark, network-dependent provider tests or SBC compile/startup test.

## 12. Documentation updates

Update the relevant `architecture/{agent,tool,context-ledger,provider,permission}.md` sections only where behavior changes; update `docs/tools.md` and `AGENTS.md` if operator/model-visible advice changes. Preserve completed predecessor closure records.

## 13. Acceptance criteria

1. No false terminal `Stall` for bounded fixture trajectories representing legitimate build/test/read/delegate cycles.
2. Deliberate identical no-op infinite cycle still terminates within a documented finite cap, with an evidence-rich `stagnated` reason.
3. Provider adaptation/continuation cannot silently stop after a single soft-stop when useful work is outstanding, but actual `final` remains respected.
4. Run/goal budgets, security, cancelled/denied statuses, and all C001 regression tests remain authoritative.
5. ADR-0040 is explicitly accepted/superseded with decision recorded before implementation is described as landed.

## 14. Stop conditions

Stop and report rather than broaden scope if the implementation demands a new authorization owner, durable-store migration without a reviewed design, an incompatible provider contract, a cross-boundary bypass, or a new ADR. If source differs materially from pinned baseline, record the exact discrepancy and update the smallest possible plan section before implementation. Do not mark closed until a `plans/closure/agent-loop-operational-reliability/NNN-status.md` record contains commits, failing-then-passing evidence, requirement-to-test matrix, cancellation/contention/security results, residual risks and the exact verified head.

## 15. Handoff

Execute only this milestone's scope. Preserve unrelated working changes, pin the initial commit and provider/model fixture, run the required tests, and register the closure or a named blocker in `plans/registry.md` using the planning skill.
