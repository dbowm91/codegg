# Agent Runtime Correctness, Autonomy, and Simplification M005 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/agent-runtime-correctness-autonomy-simplification/005-agent-loop-recovery-and-autonomy-state-machine.md`

Source subsystem roadmap:

- `plans/subsystems/agent-runtime-correctness-autonomy-simplification-roadmap.md`

Repository baseline reviewed: `e88d6f4f67ff729c894b228ddb8b5324582f3fbc`

Implementation commits:

- `ddb495a` — simplify agent loop recovery state machine

## 1. Executive finding

M005 is complete. Recovery decisions now have one turn-local bounded owner,
generic synthetic repository bootstrap is disabled, textual repair is limited
to the M002 adapter allowance, and the profile policy no longer exposes
duplicate bootstrap/post-tool recovery switches.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| One bounded recovery owner | `AutonomyState` and `RecoveryController` in `src/agent/progress_recovery.rs`; primary and follow-up loop integration | pass | Provider transport retry remains separate. |
| Repeated no-progress stalls | Existing controller tests plus bounded transition tests | pass | Fingerprints remain compact. |
| One textual adapter repair | `AutonomyState::adapter_repair_allowed()` gates both loop entry points | pass | M002 parser remains the only repair path. |
| No generic bootstrap | `bootstrap_allowed = false`; no profile policy bootstrap field | pass | Strong profiles take the direct final/call path. |
| Denial does not broaden authority | Typed denial classification and guarded base-palette restoration | pass | Profile-filtered and explicitly denied tools are not restored. |
| Typed tool outcome | `ToolExecutionStatus`, `ToolExecutionOutcome`, and `observe_tool_result` | pass | Existing rendered model text is preserved. |
| Documentation | `architecture/agent.md` recovery section updated | pass | Stale policy fields removed. |

## 3. Production implementation evidence

`AutonomyState` owns adapter repair, bounded continuation, bootstrap-use
tracking, phase transitions, and delegation to compact progress fingerprints.
The primary loop uses it for narration and tool-result recovery; follow-up
draining uses the same bounded continuation policy. `RecoveryAction::ConstrainParallelism`
was removed because no reachable transition selected it. The two independent
execution-policy booleans were deleted; model profile data remains compatible
for the prompt/profile consolidation milestone.

Before: separate missing-call, narration, post-tool, bootstrap-repeat, and
bootstrap-use counters, plus duplicate generic continuation instructions.

After: one `AutonomyState` transition budget, one adapter-repair allowance,
one post-tool continuation allowance, and the existing bounded fingerprint
incident classifier. Synthetic `list .` execution is disabled.

Final transition table:

| Input | Transition |
|---|---|
| valid structured calls | execute tools; observe typed result |
| final text with no pending work | finish |
| malformed textual protocol | one M002 adapter repair, then finish/stall |
| soft stop after tool progress | one `ContinueOrReplan` transition |
| repeated equivalent call/result | bounded recovery actions, then `Stall` |
| denied/timeout/cancelled result | preserve authority and report/recover without broadening |

## 4. Verification executed

Commands:

```text
rtk cargo fmt --all
rtk cargo test -p codegg --lib agent::progress_recovery --no-fail-fast
rtk cargo test -p codegg --lib agent::loop --no-fail-fast
rtk cargo check -p codegg --all-targets --locked
rtk scripts/verify.sh quick
```

Results:

- formatting passed;
- recovery tests passed: 9 tests;
- loop-filtered test passed: 0 matching tests, 4172 filtered (compile/test harness success);
- package all-target check passed;
- quick verification passed through generated-agent checks, core boundary,
  sandbox/execution guards, and workspace check; the first invocation was
  interrupted after it waited on a concurrent Cargo build lock, then the
  direct all-target check passed after the lock owner completed.

## 5. Invariant review

Recovery limits are explicit and turn-local. Cancellation/steering checks stay
before autonomous provider work. Final answers without explicit pending work
remain terminal. Profile filtering remains the source of the base palette;
recovery does not add tools. Repeated fingerprints eventually stall. Provider
network retry remains in `stream_with_retry`.

## 6. Failure and recovery review

Malformed and repeated calls retain bounded diagnostics. Denial, timeout, and
cancellation are distinguishable statuses. Recovery state is not persisted,
so restart semantics remain unchanged. Diagnostics contain incident kinds and
fingerprints/compact evidence, not raw arguments or tool output.

## 7. Migration and compatibility review

No storage or protocol migration is required. Removed execution-policy fields
were internal runtime fields. Model profile compatibility fields remain for
M006's prompt/control consolidation and existing profile assets continue to
parse.

## 8. Security review

Permission denial cannot trigger palette restoration. Recovery does not infer
new permissions, and profile-hidden tools remain hidden. Diagnostics preserve
the existing path redaction boundary and add no raw-result logging.

## 9. Documentation and operations

Updated `architecture/agent.md` with the state-machine semantics and removed
stale execution-policy recovery documentation.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Fragile-model profile fields `requires_explicit_tool_contract` and `requires_post_tool_continue_nudge` remain in profile assets | They are compatibility inputs not consumed by the generic loop | M006 may consolidate/remove them after prompt-policy review. |

## 11. Roadmap disposition

M005 is closed. M006 is unblocked and registered `ready`; independently ready
M007 and M008 remain available. M009 remains blocked until M001-M008 have
accepted closure records.

## 12. Registry updates

- M005 moved to `closed` with this record and implementation commit `ddb495a`.
- M006 moved from `blocked` to `ready` because its hard M005 dependency is
  satisfied.
- The subsystem roadmap records M005 closed.
