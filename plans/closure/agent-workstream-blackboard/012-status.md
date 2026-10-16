# Agent Workstream Blackboard M012 — Session-Scoped Run Group Continuation

Status: closed
Source implementation plan: `plans/implementation/agent-workstream-blackboard/012-session-scoped-run-group-continuation-corrective.md`
Source subsystem roadmap: `plans/subsystems/agent-workstream-blackboard-roadmap.md`
Implementation commit: `b3829057530eedeb0dc32240c1b15152564f9363`

## Finding and correction

M008's S1 parent turn ended while its group remained active. The retained `session_events` rows show later `wait_group` calls failed with `group actor is not authorized`, because turn-owned groups required the exact creating `turn_id`. Group waits are bounded to 30 seconds, so a later user turn had no supported way to continue observing a still-running group.

Turn-owned group lifecycle authorization now recognizes a top-level turn from the same durable session. The creating turn remains persisted as origin provenance. Child `RunId` actors and different session IDs remain denied; run-owned groups remain exact-run scoped.

## Evidence

| Requirement | Evidence | Result |
|---|---|---|
| Original turn retains group access | `agent_run_group::tests::turn_owned_group_allows_same_session_later_turn_but_rejects_other_actors` | pass |
| A later top-level turn in the same session can continue lifecycle access | same test, second `turn_id` and session ID unchanged | pass |
| Different session cannot inspect/control the group | same test, changed session ID | denied |
| Child run cannot acquire top-level group authority | same test, run actor | denied |
| Run-owned groups preserve exact run ownership | `agent_run_group::tests::sqlite_group_reloads_after_service_recreation_without_respawn` | pass |

## Verification

- Focused `codegg-core` AgentRunGroup tests: 6 passed, 0 failed.
- `cargo fmt --check` — passed.
- `python3 scripts/check_execution_ownership.py` — passed.
- `python3 scripts/check_scheduler_bypass.py` — passed.
- `bash scripts/check-core-boundary.sh` — passed.
- `git diff --check` — passed.

The build emitted the local Rust 1.98.1 `rust-objcopy` warning because its `libLLVM.dylib` is unavailable; the test binary linked and all tests passed.

## Limits and downstream audit

M012 supplies a continuation path for M008 groups but does not prove that its stalled children complete. After restarting the isolated trial daemon on the corrected build, a later headless turn in the same session successfully inspected a recovered terminal group. A fresh S1 group was then launched; subsequent headless turns in the same session completed `wait_group` calls and observed both child runs still running, with no authorization error. This verifies daemon-backed continuation for the live-group wait path; it does not prove child completion. M008 remains active until a provider-backed S1 arm completes and the remaining acceptance strata are resolved. M005 remains gated; M012 changes no checkout placement or writer authority. No other plan is newly eligible.
