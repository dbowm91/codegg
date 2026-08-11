# Runtime Consolidation, Deletion, and Footprint M002 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/runtime-consolidation-deletion-footprint/002-structured-outcome-recovery-convergence.md`
Source subsystem roadmap: `plans/subsystems/runtime-consolidation-deletion-footprint-roadmap.md`
Repository baseline reviewed: `bd9b3b610af0fa72ce3fe5a8b8f59222659f006d`
Implementation commits: `2789a2122b3b83e23654d15bcb0d58b0a22d6fa1` — structured outcome/recovery convergence

## 1. Executive finding

M002 is strictly closed. Recovery now consumes typed execution status and
contract-derived effect class, equivalent-result detection is scoped to the
canonical action identity, and progress requires a bounded observable effect.
The implementation stays within the existing broker/loop boundary and does not
introduce a protocol, schema, retry, or workflow-framework change.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence |
|---|---|
| Equivalent results use action identity | `RecoveryController::detect()` compares canonical tool plus normalized argument fingerprint and result fingerprint; regression test covers unrelated identical text. |
| Typed status is authoritative | `AutonomyState::observe_tool_result()` maps `Denied`, `Timeout`, `Cancelled`, `ProtocolError`, and `ToolError` directly; loop paths no longer call a prose classifier. |
| Success text remains success | Typed-status regression test uses failure-like words in a successful outcome. |
| Effect-aware progress | Contract `ToolEffectClass` is attached to observations; mutating progress requires an observed file-change event, while only the first changed read result is bounded evidence. |
| Child progress is real | Successful task submission no longer fabricates `child_advanced`; the field is reserved for an actual child transition. |
| Recovery remains bounded | Existing history and recovery limits remain unchanged; volatile mutating output does not reset recovery. |
| Unsafe replay is not introduced | No retry machinery changed; non-idempotent effects cannot gain progress from output novelty. |
| Authority denial remains safe | Denial remains typed and the existing restore-palette guard still excludes denied outcomes. |

## 3. Production implementation evidence

- `src/agent/progress_recovery.rs` removed unused progress/incident variants,
  deleted the ordinary-path prose classifier, added typed status/effect facts,
  scoped equivalence, and implemented bounded read novelty.
- `src/agent/loop.rs` obtains effect class from the registered tool contract,
  preserves the outcome status, consumes the existing file-change event stream
  as the observable mutation fact, and does not infer child advancement from
  successful task text.
- `architecture/agent.md` and `architecture/tool.md` document the final
  structured status/effect boundary and its ownership.

## 4. Verification executed (commands + results)

Local verification:

- `cargo test -p codegg --lib agent::progress_recovery -- --nocapture` — passed, 14 tests.
- `cargo test -p codegg --lib agent::r#loop::tests -- --nocapture` — passed, 39 tests.
- `cargo test --test agent_loop_harness -- --test-threads=1` — passed, 40 tests.
- `scripts/verify.sh quick` — passed, including formatting, generated assets,
  core boundary, sandbox contract, execution ownership, and workspace check.
- `CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --locked -- -D warnings` — passed.
- `git diff --check` — passed.

No hosted CI result is claimed in this local closure record.

## 5. Invariant review

Structured provider calls remain canonical. Model-facing text is preserved but
does not override known status. Fingerprints remain bounded and contain no raw
arguments, outputs, or private reasoning. Adapter repair and semantic recovery
remain separate, with existing bounded budgets and provider transport retries
outside this state.

## 6. Failure and recovery review

Explicit timeout, cancellation, denial, protocol, and tool-error outcomes are
carried as distinct typed statuses into recovery incidents. A failed execution
does not become progress from diagnostic text. Mutating calls require an
observed file-change event; read-only novelty is limited to the first changed
result for one action identity. Parallel completion association and the
existing permission/cancellation branches are unchanged.

## 7. Migration and compatibility review

No durable schema, wire protocol, or public Tool Broker contract changed. The
model-facing result string remains compatible. The removed classifier had no
remaining production typed-path consumer; `ProgressObservation::tool()` now
starts with no inferred error class.

## 8. Security review

Permission denial remains authoritative and cannot restore a broader palette.
No authority, path policy, child authority, or retry boundary was broadened.
Recovery stores only normalized fingerprints and compact bounded summaries.

## 9. Documentation and operations

The two authoritative architecture documents now describe typed outcome and
effect ownership, bounded read evidence, real state-change requirements, and
child-transition semantics. No new operational lane or persistent state was
added.

## 10. Unresolved findings (severity: critical/high/medium/low)

None. Hosted CI evidence is not available from this local pass, but all plan-
required local verification, including workspace Clippy, passed.

## 11. Roadmap disposition

M002 is closed. M003's sole hard dependency is now satisfied and its Tool
Broker/recovery interface is stable for decomposition. M004 and M005 remain
ready. M006 remains blocked on M003-M005, and M007 remains blocked on M003-M006.

## 12. Registry updates

- Marked M002 `implemented` in its implementation plan and `closed` in this
  record and the subsystem roadmap.
- Removed M002 from dependency-ready work and recorded it under recently
  closed work with implementation commit `2789a2122b3b83e23654d15bcb0d58b0a22d6fa1`.
- Promoted M003 from `blocked` to `ready` because M002 was its only hard
  dependency; M001 is already closed and M004 remains only a soft preference.
- Audited all remaining blocked work: M006 still requires M003-M005, M007
  still requires M003-M006, and no other registered plan became ready.
