# Agent Runtime, Model Adaptation, and ACP Milestone 006 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/agent-runtime-model-adaptation-acp/006-progress-loop-and-tool-recovery-controller.md`

Source subsystem roadmap:

- `plans/subsystems/agent-runtime-model-adaptation-acp-roadmap.md#milestone-006--progress-loop-and-tool-recovery-controller`

Repository baseline reviewed: `672479726f1c79bbc931d70f084cd1649e8b2ed4`

Implementation commits:

- `8c2f673` — bounded progress controller, loop integration, permission-authority separation, tests, and planning closure

## 1. Executive finding

Milestone 006 is closed. The agent loop now uses a bounded observable recovery
controller instead of permission checks as the repeated-call termination
authority. It recognizes repeated actions, equivalent errors/results, short
cycles, malformed calls, narration without structured action, and unavailable
tools; it applies bounded nudges/corrections, restores the authorized base
palette or constrains parallelism, and emits a bounded stalled diagnostic after
the recovery budget is exhausted.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Observable-only bounded state | `src/agent/progress_recovery.rs`: typed observations, SHA-256 fingerprints, fixed ring, bounded evidence | pass |
| Graduated recovery | `RecoveryAction` ladder and loop control-message integration | pass |
| Exact/equivalent/cycle/no-action coverage | Six focused controller tests plus loop integration | pass |
| Valid changed-result polling | `changed_result_is_progress` test | pass |
| Authorized palette restoration | restoration derives from `base_request_tools`; no registry/config mutation | pass |
| Temporary execution constraint | per-loop `recovery_parallel_limit`, cleared on progress | pass |
| Stalled outcome and cancellation precedence | typed `StalledReport`, bounded event, ordinary loop break; cancellation checked before provider work | pass |
| Permission and hidden-reasoning boundary | Doom-loop denial removed from permission checks; no raw arguments/results or reasoning in recovery diagnostics | pass |

## 3. Production implementation evidence

- Added `src/agent/progress_recovery.rs` with bounded taxonomy, fingerprints,
  incident tracking, recovery decisions, and stalled reports.
- Integrated observations after structured tool batches and narration-only
  provider responses.
- Routed recovery guidance through the existing model-profile control-message
  placement seam.
- Restored only the captured profile-filtered base tool surface and constrained
  parallelism per active loop; global configuration and permissions are not
  broadened.
- Retired `DoomLoopDetector` as an execution authority while retaining its
  existing compatibility tests/module for unrelated API stability.

## 4. Verification executed

Local verification (all green):

- `cargo fmt --all`
- `cargo test -p codegg --lib agent::progress_recovery` — 6 passed
- `cargo test --test doom_loop` — 7 passed
- `cargo test -p codegg --lib agent::loop` — compile/test gate green
- `cargo test --test agent_loop_harness` — 40 passed
- `cargo test --test subagent` — 22 passed
- `cargo check --workspace`
- `scripts/verify.sh quick`
- `scripts/check-core-boundary.sh`
- `scripts/check_scheduler_bypass.py`
- `scripts/check_execution_ownership.py`
- `git diff --check`

No live-model or hosted verification was required by this bounded milestone.

## 5. Invariant review

Recovery state is in-memory, ring-bounded, and reset per run. Fingerprints are
stable and output summaries are never retained. A successful changed result
clears incidents. Recovery cannot add tools beyond the captured base surface.

## 6. Failure and recovery review

Network/provider retry remains in its existing path. Recovery starts only from
observable model/tool behavior, escalates through bounded actions, and stops at
a typed report. User cancellation is checked before recovery can initiate new
provider work. A tool batch receives one batch identity so parallel calls are
not serialized into a false cycle.

## 7. Migration and compatibility review

Existing provider control placement, text parser fallback, model profile policy,
and tool palette reduction remain in place. The old detector API and tests are
not removed; only its permission-side termination behavior is retired.

## 8. Security review

Recovery messages contain classifications, canonical tool names, and bounded
operator guidance only. They do not include raw arguments, tool output,
secrets, or hidden reasoning. Palette restoration uses already-authorized
definitions and cannot bypass permission checks.

## 9. Documentation and operations

The implementation plan, subsystem roadmap, registry, and this closure record
now agree on M006’s status and evidence. Operational diagnostics are bounded
`tracing` records plus a concise `AppEvent::Error` stalled notification.

## 10. Unresolved findings

None at critical, high, or medium severity. Provider-specific recovery
preferences remain intentionally deferred to M007’s typed adapter seam.

## 11. Roadmap disposition

M006 is closed. M007 remains ready. The blocked-work audit found no newly
unblocked plan: M008 still requires M007; M009 requires M007 and later
reasoning integration; M010 requires M006 plus M009; and M011 requires M004
through M010. No corrective pass is required.

## 12. Registry updates

- Marked the implementation plan `implemented`.
- Added this accepted closure record and moved M006 to recently closed.
- Marked M006 closed in the subsystem roadmap and active registry summary.
- Removed M006 from dependency-ready plans.
- Audited all registered blocked plans; none became ready from M006 alone.
