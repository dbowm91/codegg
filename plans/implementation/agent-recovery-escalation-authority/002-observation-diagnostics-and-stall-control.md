# Agent Recovery Escalation Authority C002 — Progress Diagnostics and Stall-Control Contract

Status: ready (C001 strictly closed)
Baseline: `main@957fcbf00e17bf108429f0d75dcbf760736ae511`, re-check before implementation.
Canonical subsystem owner: `plans/subsystems/agent-recovery-escalation-authority-corrective-addendum.md` §7 C002 (previously eligible but unplanned). Cross-consumer: `plans/subsystems/harness-lab-continuation-corrective-addendum.md` (D004 reference only).
Decision: **accepted ADR-0040 Option B — evidence-gated, scope-aware recovery**. Long-term: `plans/000-long-term-specification.md` §§26 and 29. Primary class: invariant, secondary polish.

## 1. Objective

Close existing Agent Recovery C002: make progress and recovery diagnostics **meaningful, bounded and consistent with production**, identify dead knobs/fields, and provide a truthful controllable test for confirmed stagnation *without* making legitimate long-running agent work stall. Reconcile Harness Lab C005's unreachable `RecoveryDecision::Stalled` and live-unexercised repeated-failure control with ADR-0040, rather than manufacturing an impossible terminal outcome.

## 2. Evidence / why ready

Agent Recovery C001 removed tool-name-only and argument-blind false stall classification and is strictly closed. Its roadmap lists C002 “diagnostics and guard coverage” as eligible but not started. Current `src/agent/progress_recovery.rs` exposes `RecoveryDecision::Stalled`, while C005's `plans/closure/harness-lab-post-pilot-corrective/005-status.md` found the controller no longer *constructs* it: actual termination is by run/turn budgets after advisory recovery. Five induced missing-file live trials saw only one consecutive identical failure each, so the test's premise was never exercised. ADR-0040 Option B governs: unknown progress is **not** confirmed stagnation; budgets remain separate typed termination.

C005 also reports failed-tool ACP notification truth separately; that is owned by Harness Lab continuation D002. **Do not** solve it inside the recovery controller.

## 3. Invariants and non-goals

- Repeated tool name or even full-call identity alone is not proof of no progress; a projected result, changed workspace revision, child/job advance, context recovery or differing typed error resets/reclassifies evidence as appropriate.
- Missing observation is **unknown**, never fabricated confirmation of stagnation. Volatile timestamp/echo output does not count as mutating process progress.
- Consecutive, affirmative same-work-identity/same-state stagnant cycles can terminate only within ADR-0040's explicitly bounded policy; if that production terminal path does not exist yet, this milestone must *not* fabricate it. Scope a separate architecture-approval successor if new authority required.
- Deny/cancel/timeout/provider transport errors remain distinct from `stalled`; outer budgets are enforced independently of recovery ladder.
- Diagnostics never expose tool input/output, model private reasoning, credentials or full workspace paths; only bounded action hash, incident category, counts and known observation state.
- No new provider retry or replay of acknowledged mutating commands, no TCP/perf watchdog, no harness-specific recovery fork, no automatic escalation to a more privileged permission mode.

## 4. Dependency and interfaces

Hard dependency C001 discharged. C002 is independently ready. Interface (soft): Harness Lab continuation D002 provides typed real tool failure outcomes; until then, the controller must treat unknown structured status conservatively, not parse rendered error text to grant stall authority. A new terminal status/API or change to ADR-0040 needs maintainer decision and separate plan, not a local typo fix.

## 5. Ordered work packages

A. **Observation inventory:** map every `ProgressObservation` field to actual production writers in `src/agent/loop.rs` and progress-recovery tests. Classify always-false/unused fields, confirm whether any rung or `RecoveryDecision::Stalled` producer exists. Produce a short source-and-guards table and include the “mutating identical output does not prove progress” case.

B. **Bounded diagnostics:** instrument the actual recovery decision path with incident identity (hash/key), consecutive attempts, rung, known vs unknown progress, exact termination owner (`budget`, `cancel`, `confirmed_stagnation`, `provider_failure`, etc.), without raw args or outputs. Surface a namespaced redacted receipt only if already permitted by native runtime/event disclosure. Keep low overhead for SBC/long runs.

C. **Remove or wire dead fields:** remove any permanently false observation or unavailable state that is misleading; retain deprecated `Stalled` only as a clearly marked compatibility enum if no reachable producer. Do not make `Stalled` reachable by incrementing counters without affirmative stagnation evidence. Add a non-vacuous static guard asserting every observation field is written in production, and a test that fails if a deleted knob is reintroduced.

D. **Actual contract fixtures:** local provider/tool fixture induces (1) same-action same-result *and verified unchanged state* for bounded stagnant cycles, (2) identical read after workspace change, (3) truncated/projected read followed by `context_read`, (4) slowly progressing child/job, (5) distinct repeated tool error, (6) cancellation/Deny/timeout, (7) an unobservable state. Assert the **actual** terminal owner on every case. A known stagnation may escalate guidance and stop only by a supported typed authority; budget stop is not a stall stop. Record whether a live agent ever exercises the predicate, instead of requiring it to repeat artificially.

E. **Taxonomy alignment:** coordinate a Harness Lab benchmark contract follow-up to replace an impossible “terminal stall” expectation with ADR-0040's real observational and budget result, **versioning** the fixture and preserving the frozen historic original. A control not exercised must remain `not_exercised` and not count as held or violated. Do not change historical evidence or loosen benchmark assertions.

## 6. Failure/restart/concurrency

Recovery observations are turn-local and incident-local; restart does not accumulate unverified prior progress, and a new agent/turn cannot inherit prior attempt counts. An external cancelled tool never becomes a repeated-tool stall; multibatch parallel tool outcomes have distinct keys and clean race semantics. The model cannot declare its own progress without tool evidence. A crash during observation produces unknown, not success.

## 7. Compatibility and migration

Internal type removals must fix all consumers and tests; avoid wire/database schema change. If introducing a bounded runtime DTO, it must be optional and privacy-safe. Historical C001 and C005 closure statuses are preserved; new C002 closure may explicitly disagree with the old expected control when accepted ADR-0040 requires it.

## 8. Tests

```bash
cargo test -p codegg --lib progress_recovery
cargo test -p codegg --lib agent::loop
python3 -m unittest discover -s dev/harness_lab/tests -v
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
```
Use actual owning Rust test targets. Required tests include one genuine confirmed-stagnation trajectory bounded by **an existing authority**, one legitimate 30+ tool-call exploration with repeated reads, unknown-progress no-stall, dead-field guard self-test and no secret diagnostics. No provider-backed call required.

## 9. Acceptance / stop / closure

Accept if the reported reason is truthful for **every** trajectory, no false stall on legitimate exploration, no false claim of a stall-terminal producer, repeatable confirmed-nonprogress behavior under documented authority, and non-vacuous guards. If ADR-0040 explicitly authorizes a typed terminal not yet implemented, record the gap and propose a **separate** product milestone rather than changing the decision in this plan.

Close at `plans/closure/agent-recovery-escalation-authority/002-status.md` with before/after source traces, test counts, retained/removed fields, privacy audit, ADR compliance, exact source SHA and hosted CI evidence or blockers. Advance only the existing recovery roadmap and registry; Harness Lab D004 is cross-reference and is not a new duplicated agent-loop milestone.
