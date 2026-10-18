# Milestone C001 — Progress-Recovery Escalation Authority: Closure Record

Status: closed

Milestone: `plans/implementation/agent-recovery-escalation-authority/001-progress-recovery-escalation-authority.md`

Source roadmap: `plans/subsystems/agent-recovery-escalation-authority-corrective-addendum.md#milestone-c001--progress-recovery-escalation-authority`

Class: invariant

Repository baseline: `b77c8156b594494b7f59e0348d93a73ca2c1fd1c`
(branch `fork/b86dd575608c-7c7d222ee32c45d5b9232bfc57ead177`)

Post-closure corrective against: `plans/closure/agent-runtime-model-adaptation-acp/006-status.md`

ADRs: none required. Incident keying, ladder rungs, and the run budget are unchanged;
this repairs the implementation of an already-accepted contract.

## 1. Outcome

The diagnosis was correct and is now fixed. The over-sensitivity of "short cycle
observed" was **not** the removed `ShortCycle` variant — that one was already gone
and already regression-tested. It was two other defects:

1. `EquivalentError` was keyed on `canonical_tool` + `error_class` alone, so any
   second failure of the same tool counted as equivalent work regardless of the
   command or the message.
2. Ladder state (`recoveries`, `incident`) was reset only by a progress signal,
   never by a change of incident identity, so unrelated failures accumulated
   into a terminal stall.

A third finding — that `ProgressObservation::new_evidence` and `child_advanced`
were permanently `false` in production — is fixed by deleting them, not by
inventing producers.

## 2. Requirement-to-evidence matrix

| # | Requirement (source) | Evidence | Result |
|---|---|---|---|
| R1 | Every incident keys on the whole `action_key()`; no tool-name-only check (`architecture/agent.md`) | `detect`'s `EquivalentError` now compares `action_key()`; `distinct_failing_commands_do_not_climb_the_ladder`; `a_batch_of_distinct_failing_commands_does_not_stall_the_turn` | met |
| R2 | A call the turn has never made is not a repeat and must never be escalated | `a_novel_call_is_never_an_incident` (pre-existing, unchanged); `a_retry_with_different_arguments_is_not_equivalent_work` | met |
| R3 | The identical repeated call still escalates and still terminates the turn | `first_repeat_nudges_and_eventually_stalls` (pre-existing); `an_identical_failing_call_is_still_an_incident`; `the_ladder_still_terminates_a_genuinely_repeating_turn`; `autonomy_state_still_bounds_and_breaks_a_repeating_turn`; `an_identical_repeated_failure_still_stalls_the_turn` | met |
| R4 | Every observation field is populated from production evidence or removed | `new_evidence` and `child_advanced` deleted; `ProgressSignal::ChildAdvanced` deleted; no remaining field is constant-`false` at the sole construction site in `src/agent/loop.rs` | met |
| R5 | Ladder state is per turn and per incident identity | `RecoveryController::incident_key`; reset in `observe` when `action_key()` changes; `an_unrelated_incident_does_not_inherit_an_earlier_incidents_ladder_count` | met |
| R6 | Reading many files / alternating read and shell never escalates | `reaching_for_the_same_tool_with_new_arguments_is_not_an_incident`, `interleaving_reads_and_shell_is_never_a_stall` (pre-existing); `read_heavy_exploration_does_not_stall`; `a_build_fix_loop_does_not_stall`; `a_bisecting_failure_loop_does_not_stall`; `exploring_with_alternating_reads_and_shell_never_stalls` | met |
| R7 | Typed execution status stays authoritative; denied/timeout/cancelled are not stalls | `typed_status_remains_authoritative_through_recovery`, `typed_status_is_authoritative_over_display_text`, `typed_tool_errors_map_to_non_success_recovery_statuses`, `successful_failure_like_text_is_not_a_failure` — all pre-existing, unchanged | met |
| R8 | The module stores fingerprints and classifications only, never arguments, output, or reasoning | Unchanged. No new field carries raw text; the `ProgressObservation` doc comment restates the constraint and the added `incident_key` is a SHA-256-class `action_key()` string | met |
| R9 | `ExecutionLimits::max_turns` remains the outer bound on a run | Unchanged. The ladder is strictly *less* likely to terminate a turn than at baseline, so the outer bound is unchanged in force; `architecture/agent.md` and `AGENTS.md` restate it | met |
| R10 | The removed-`ShortCycle` regressions must keep passing | All three pre-existing tests pass unchanged | met |

## 3. W1 failing evidence, shown failing at baseline

Against `b77c8156` with the new tests added and no production change:

```
running 20 tests
test ...::distinct_failing_commands_do_not_climb_the_ladder ... FAILED
test ...::a_successful_mutating_shell_call_does_not_carry_a_failure_ladder_forward ... FAILED
test ...::an_unrelated_incident_does_not_inherit_an_earlier_incidents_ladder_count ... FAILED

---- distinct_failing_commands_do_not_climb_the_ladder stdout ----
five distinct failing commands must never escalate the ladder, but these did:
  ["cargo build -p crate-1", "cargo build -p crate-2", "cargo build -p crate-3", "cargo build -p crate-4"]

---- a_successful_mutating_shell_call_does_not_carry_a_failure_ladder_forward stdout ----
the post-recovery failure must start at Nudge, not inherit the earlier ladder:
  Recover { action: Correct, incident: RecoveryIncident { kind: EquivalentError,
  occurrences: 2, last_action: Correct, ... } }

---- an_unrelated_incident_does_not_inherit_an_earlier_incidents_ladder_count stdout ----
an unrelated failure must not climb the earlier incident's ladder:
  Recover { action: Correct, incident: RecoveryIncident { kind: EquivalentError, ... } }

test result: FAILED. 17 passed; 3 failed
```

Note the asymmetry that makes the first failure readable: `crate-0` continued and
`crate-1` through `crate-4` escalated. The threshold is the *second* failure of
the tool, which is exactly the tool-name-only keying.

The integration-level regression was also shown failing. With only the
`EquivalentError` keying reverted to its baseline form, everything else in the fix
still applied:

```
test a_batch_of_distinct_failing_commands_does_not_stall_the_turn ... FAILED
the turn must reach its second provider call; it stopped at 1 because the ten
distinct failures were classified as one repeat
```

This is the load-bearing result: it proves the defect reaches the real
`AgentLoop` observation path, not just the controller in isolation.

## 4. W1 scope deviation from the plan, recorded

The plan's W1 named three shapes and described them with approximate arithmetic.
Two corrections, both resolved toward the plan's *intent* and recorded here
rather than quietly:

- **"five failing calls … reach `Stalled`"** is six calls, not five, through
  `RecoveryController` (first is `Continue`, then Nudge, Correct,
  RestoreBasePalette, Replan, and only the sixth returns `Stalled`). The written
  test asserts the desired behaviour instead — that none of five distinct
  failures escalates — and the baseline failure message above shows the ladder
  reaching `Correct` on the second.
- **"two repeats followed by three unrelated errors reach `Stalled`"** does not
  reach `Stalled`; it reaches `Replan`. The written test asserts the desired
  behaviour instead — that the three unrelated failures add nothing to the
  ladder, which at baseline they do (it climbs to 4).

## 5. Decisions recorded during the milestone

### A successful mutating or process-class call is not progress

The roadmap recorded this as an open decision point with a suggested fallback
("treat a successful `bash` as progress when its result differs from the
previous identical call"). **That fallback was rejected.** Extending the
changed-result rule past read-only calls makes it fire on the second identical
shell call, which is exactly the volatile-output case
`volatile_mutating_output_does_not_reset_recovery` exists to prevent — a shell
loop that echoes a timestamp would read as productive.

The two requirements cannot both hold, and the bounded-volatility one is the
stronger invariant. So a successful mutating or process-class call carries no
progress signal. It is still protected from false stalls by the two rules it
does get: a call the turn has never made is never escalated, and a change of
incident identity resets the ladder. The decision is stated in a comment at the
computation site so the next reader does not rediscover it, and the roadmap §7
exit condition and §10 decision point were corrected to match.

**Residual, for C002:** a successful mutating call that *is* a repeat leaves
`AutonomyState.transitions` untouched rather than resetting it, so a long turn
can exhaust the transition budget without a matching number of incidents.
Fixing it needs the per-tool state-change observation described in the roadmap
§4, not a heuristic.

### Error wording is not part of any key

The argument fingerprint already identifies which work a call is. Letting the
error text into the key would make classification unstable across two ways of
reporting the same failure. Pinned by
`error_wording_is_not_part_of_the_equivalent_error_key`.

### An unregistered tool name is unknown, not read-only

`effect_class: None` (registry lookup miss) is treated exactly like a mutation,
matching `ToolEffectClass`'s own conservative `#[default] NonIdempotent`. A
lookup miss must never become permission to call a mutation evidence. Pinned by
`an_unregistered_tool_name_is_unknown_not_read_only`, and centralised in
`is_read_effect` so it cannot drift back into an inline `matches!`.

### `state_changed` stays narrow, and its imprecision is documented

`state_changed` still requires a drained file-change bus event **and**
`is_file_modifying_tool` (`write|edit|replace|apply_patch`) **and** success. A
mutating `bash`, a git mutation, and a job submission all change state but
nothing observable separates "changed it" from "ran and changed nothing", so
crediting them would credit every step a looping model takes.

One imprecision is now documented at the site rather than left for a reader to
find: the bus is drained once per batch, so within a multi-call batch every
file-modifying call is credited when *any* of them emitted a change. Correcting
it needs per-path attribution, which is a separate change.

## 6. One existing test encoded the defect and was rewritten

`cosmetic_argument_error_retry_is_detected` asserted that `bash("command-a")`
followed by `bash("command-b")`, both failing with the same `tool_error` class,
was `EquivalentError`. That assertion **was** the defect: it is why five distinct
failing commands climbed the ladder. It was replaced by
`a_retry_with_different_arguments_is_not_equivalent_work` (asserting the new
behaviour) plus `an_identical_failing_call_is_still_an_incident` (asserting the
true positive survives). This is called out because a "no existing test was
changed" claim would be false here.

## 7. Production changes

| File | Change |
|---|---|
| `src/agent/progress_recovery.rs` | Deleted `ProgressObservation::new_evidence` and `child_advanced`; deleted `ProgressSignal::ChildAdvanced`; added `RecoveryController::incident_key` with reset-on-identity-change in `observe`; re-keyed `EquivalentError` on `action_key()` + `error_class`; centralised read-effect classification in `is_read_effect`; documented the mutating-call decision at the computation site; restructured the `state_changed` and `effect_class` doc comments |
| `src/agent/loop.rs` | Sole production construction site: dropped the two removed fields; documented what `state_changed` does and does not cover, and the `None`-effect-class case |
| `crates/codegg-core/src/goal/progress.rs` | Doc prose referenced the deleted `ChildAdvanced` variant; removed. Its own enums were never structurally dependent on it |

No protocol, storage, config, provider, or public-surface change. No new tool. No
change to the ladder's rung order or vocabulary.

## 8. Test changes

`src/agent/progress_recovery.rs`: 17 → 28 unit tests. Eleven net new, one
rewritten (see §6), plus two new helpers (`failing_obs`, `mutating_success_obs`)
and `is_escalation`.

Non-escalation sweeps: `distinct_failing_commands_do_not_climb_the_ladder`,
`an_unrelated_incident_does_not_inherit_an_earlier_incidents_ladder_count`,
`a_successful_mutating_shell_call_does_not_carry_a_failure_ladder_forward`,
`a_retry_with_different_arguments_is_not_equivalent_work`,
`a_bisecting_failure_loop_does_not_stall`, `a_build_fix_loop_does_not_stall`,
`read_heavy_exploration_does_not_stall`.

True positives: `an_identical_failing_call_is_still_an_incident`,
`error_wording_is_not_part_of_the_equivalent_error_key`,
`the_ladder_still_terminates_a_genuinely_repeating_turn`,
`autonomy_state_still_bounds_and_breaks_a_repeating_turn`.

Contract: `an_unregistered_tool_name_is_unknown_not_read_only`.

`tests/agent_loop_harness.rs`: three integration tests driving the real
`AgentLoop` observation path with a scripted provider — the batch-of-distinct-
failures regression, the alternating read/shell explore shape, and the
identical-repeat true positive — plus three fake tools whose contracts declare
`ReadOnly` / `ProcessExec` so the loop's `effect_class` lookup is exercised for
real.

The integration tests assert on **provider turn count**, not on the global event
bus: the bus is process-global, so asserting on it from a parallel test binary
would be racy, and a stalled turn breaks out of the loop so the scripted provider
stops being called.

## 9. Verification

All local. No hosted CI was run for this milestone and none is claimed.

| Command | Result |
|---|---|
| `cargo fmt` | clean |
| `cargo test -p codegg --lib agent::progress_recovery` | **28 passed; 0 failed** (6356 filtered out) |
| `cargo test -p codegg --test agent_loop_harness` | **58 passed; 0 failed; 0 ignored** |
| `cargo clippy --all-targets -- -D warnings` | **clean** (caught one `clippy::int_plus_one` in a new assertion, fixed) |
| `scripts/verify.sh quick` | **"Quick verification passed."** — including the agent schema, core boundary, sandbox, execution-ownership, scheduler-bypass, TUI authority, http-route-disposition, audit-coverage, config-merge-coverage, no-hardcoded-models, provider wire/catalog/resilience, OpenAI endpoint, Eggwork routing, and first-party-skill guards, then `cargo check --workspace --all-targets --locked` |

## 10. Not covered

- No hosted CI run. Native macOS/Windows execution qualification was not
  attempted; this change is platform-neutral classification logic.
- No live-model trajectory. The claim "ordinary agent work stops producing false
  stalls" is proven by unit and integration tests over the shapes ordinary work
  takes, not by a recorded model session.
- `recovery_parallel_limit` remains a dead knob and `RestoreBasePalette` still
  overwrites the request tool list with the full base surface. Both are C002, by
  design: polish is sequenced after the correctness boundary, and wiring the
  limit changes parallelism under recovery, which must not happen incidentally.

## 11. Unblock audit

Performed at closure, as the registry requires.

Nothing newly unblocked. No registered plan depends on C001. The one plan whose
readiness this could have affected is
`plans/subsystems/project-picker-rework-roadmap.md` M005, which is `closing` with
a conditionally-closed walkthrough — C001 touches no TUI, session, or workspace
path and cannot bear on it. The change to unblocking status here is that **C002
becomes eligible for fresh planning**, since its hard dependency was C001.