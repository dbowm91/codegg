# ADR-0040: Evidence-based agent recovery and terminal-stall authority

Status: proposed

Date: 2026-10-18

Decision owners: project maintainers

Related specification sections:
- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#7-agent-terms`

Affected subsystem roadmaps:
- `plans/subsystems/agent-loop-operational-reliability-roadmap.md`
- `plans/subsystems/agent-recovery-escalation-authority-corrective-addendum.md` (closed C001 must remain historically true)
- `plans/subsystems/long-horizon-work-execution-roadmap.md` (closed foundation; no reimplementation)

## Context

The already-closed recovery C001 correctly removed tool-name-only ShortCycle and argument-blind EquivalentError classification. Its accepted *operational* rule nevertheless treats the identical repeated call/result as a terminal-stall candidate under a four-rung ladder. In `src/agent/progress_recovery.rs`, `AutonomyState::MAX_TRANSITIONS=4`; `src/agent/loop.rs` turns `RecoveryDecision::Stalled` into an error and breaks the turn. Repeating a read or test with identical arguments may be legitimate if the previous output was projected, the filesystem revision changed, a child/job advanced, or the model is investigating a real failure. The current observation lacks sufficient evidence for those distinctions. The source's "not progress" classification for successful process-class calls is uncertainty, not proof of no progress.

The user has explicitly requested greatly relaxed stall behavior in support of long autonomous coding runs. The specific terminal authority is a cross-subsystem architectural decision, not a threshold-only refactor. Existing budgets, permission ceilings and cancellation must not be weakened.

## Decision drivers

- Avoid killing legitimate work because identical commands or tool classes recur.
- Do not allow unbounded empty retry loops or infinite provider consumption.
- Preserve typed errors, idempotency, immutable authority, and measurable operational behavior.
- Keep interactive turn limits distinct from durable autonomous job budgets.
- Reuse existing WorkPlan/Goal, durable AgentRun, scheduler, context artifacts and projection stores; no second execution owner.

## Considered options

### A — Raise repetition/recovery thresholds

Keep the terminal ladder and enlarge constants. Lowest migration cost, but merely delays false stalls and still cannot distinguish changed state from no state evidence.

### B — Evidence-gated, scope-aware recovery (recommended)

Keep bounded diagnostic incidents and graduated model guidance. A repeated call is not by itself terminal evidence. Correlate call identity with actual pre/post workspace revision, relevant job/child lifecycle, tool-output completeness, structured test diagnostics and typed result changes. Only a confirmed stagnant cycle can exhaust its local recovery budget; unrelated incidents cannot accumulate toward termination. When evidence is missing, classify progress as unknown, not stalled. Explicit run budgets, cancellation, policy denial, unrecoverable infrastructure failure, and an independently bounded confirmed-stagnation limit remain terminating authorities. Provide policy-configurable behavior, conservative caps, and structured reasons. Keep the current hard terminal behavior as an explicit compatibility mode if required, never silently enabled for all models.

### C — Disable stall detection

Makes false stalls disappear but permits unlimited retries until a budget burns. Rejected for the default.

## Proposed decision

**Propose Option B; not yet accepted.** No implementation may declare the changed terminal semantics architecturally approved until the maintainer accepts this ADR or records a different decision. Under Option B:
1. Recovery is an advisory and adaptive mechanism until there is *affirmative*, correlated evidence of stagnation for the same work identity and state revision.
2. Tool novelty is evidence, but inability to observe a write, external transition, or progress is *unknown*, not no-progress proof.
3. Confirmed, consecutive no-progress cycles remain bounded. Budget exhaustion remains a separate typed termination condition, not reported as a "stalled" tool.
4. Denied, timeout, cancelled, invalid-schema and infrastructure failures remain typed and are not recast as equivalent results. An agent may correct an error without absorbing prior, unrelated recovery counts.
5. Recovery and projection must not conceal requested evidence; a projected result with a recoverable continuation is not "complete" for stall inference.
6. No automated provider retry may replay acknowledged mutating operations or bypass run-control, permission or sandbox authorization.

## Consequences

Positive: materially fewer false terminal stalls; actionable evidence and explicit lifecycle ownership for unattended work.

Costs: additional bounded progress metadata, clearer test fixtures, more state-machine complexity, risk that unknown progress defers a genuine stall to the hard budget. The hard budgets and bounded confirmed-stagnation cap control that risk.

Neutral: no new persistent state store or wire protocol required initially; diagnostic DTO additions, if needed, must be additive and privacy-safe.

## Compatibility and migration

Preserve old `IncidentKind` and `RecoveryAction` names where possible and preserve existing incident-keying regression tests. Expose versioned opt-in/opt-out behavior if the semantics change, with defaults explicitly documented. Do not retroactively rewrite C001 closure records. If durable configuration fields are needed, validate old TOML fallbacks; new storage migrations require separate justification.

## Security and reliability implications

Do not move trust from daemon to model. Never credit volatile timestamp output or unverified prose as workspace progress. Bound observations by counts/bytes and redact secrets. Distinguish user cancellation, transport abort, hung process, denied operation and budget exhaustion. Check restart/resume for loss of progress evidence and maintain exact run identities.

## Verification and acceptance

Acceptance requires deterministic replay fixtures for repeated read/tool output, identical `cargo test` before/after edits, stable no-op retries, volatile output, external job/child progression, denied/timeout/cancelled errors, context truncation, multi-call batches and unrelated incidents; a confirmed endless cycle must still stop under a documented bound. Existing C001 regression tests must remain green. Do not add a new permanent CI lane or live-provider requirement; optional manual model-backed qualification is explicitly separate.

## Status / decision needed

**Proposed.** Maintainer decision required between Options A/B (B recommended) and the confirmed-stagnation/compatibility defaults before Milestone M002 modifies termination authority.
