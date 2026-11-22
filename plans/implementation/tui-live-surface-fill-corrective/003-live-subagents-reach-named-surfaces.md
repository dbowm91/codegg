# TUI Live Surface Fill Corrective M003 — Live Subagents Reach the Named Surfaces

Status: implemented — conditionally closed
Closure record: `plans/closure/tui-live-surface-fill-corrective/003-status.md`
Repository baseline: `d319afec15b8b8b33ced2c0af7a3ee50eb1a82c3`

Source roadmap: `plans/subsystems/tui-live-surface-fill-corrective-addendum.md#m003--live-subagents-reach-the-named-surfaces`
Long-term requirements:
- `plans/000-long-term-specification.md#44-frontends-render-projections`
- `plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy`
Applicable ADRs:
- `plans/adrs/ADR-0043-session-owned-subagent-lifecycle-projection.md` is the projection contract. Do not amend it. If the live child is absent from the snapshot, stop.
Primary class: capability

## 1. Objective

A live subagent that the authorized session snapshot contains appears on the Session panel, the sessions dialog, and `Ctrl+T`. A snapshot that does not contain that child is reported, not painted around.

## 2. Why this milestone is ready

No hard dependency is open. `tui-subagent-and-job-visibility-convergence` C001–C007 are closed and are not reopened. They fixed the `Ctrl+T` binding, the sessions-dialog sections, and the naming chain. `tui-live-work-visibility-corrective` M006 is conditionally closed and named manual terminal receipt as a re-entry condition. This milestone is that re-entry for the empty-row symptom. It does not reopen M006's other conditions.

## 3. Current implementation evidence

- `sync_subagents_from_snapshot` and `task_rows` both call `select_session_work` on `active_snapshot()`. `task_rows` uses `unwrap_or_default()` when the snapshot is missing.
- `select_session_work` reads `snapshot.subagents`, the active turn's agent tree, and only the newest recent turn. Durable run status overwrites the child status.
- `build_task_rows` keeps subagent rows only when `SubagentRow::is_running` is true. Unrecognized non-empty status is not running. The job helper's comment says unrecognized non-empty state is live and says the subagent rule matches it. The subagent predicate does not.
- The sessions dialog builds its pending rows from `task_rows`, so it inherits that filter. The sidebar does not filter by `is_running`, and it omits the section when the vector is empty.
- A socket integration test can observe `snapshot.subagents`. That test is not a manual TUI with a live child. The projection contents at the reported failure were not captured.

## 4. Invariants that must not regress

- The TUI does not mint a subagent the snapshot does not contain.
- One selector feeds the Session panel, the sessions dialog, and `Ctrl+T`. Do not add a fourth reader.
- Finished rows may remain on the Session panel. `Ctrl+T` may stay a live-work view, but a live child must not be classified finished because its status token is unrecognized.
- No name is resolved from a frontend pool.

## 5. Scope

In: the selector, the liveness predicate, and the three paint surfaces, plus a stop if the snapshot lacks the child.

Out: send-to-subagent, name allocation, tab presentation, and any new projection field unless the stop condition is hit and a later plan owns it.

## 6. Required production changes

- Make the subagent liveness rule match the job rule for an unrecognized non-empty status, or map the producer's actual live token into the known set at the projection boundary. One rule. Record the token that a live child actually carries.
- Do not treat a missing snapshot as an authoritative empty. `Ctrl+T` already has an unavailable label; `task_rows` must not erase that distinction by substituting an empty vector before the label is chosen.
- If a fixture shaped like a live child is dropped by `select_session_work`, fix that drop. If the real snapshot does not contain the child, do not fix it in the TUI.

## 7. Ordered work packages

### W1 — Record the hiding rule
Intent: a snapshot that contains a live child with an unrecognized status produces no `Ctrl+T` or sessions-dialog row on this baseline, and an empty snapshot produces an empty Session section.
Required changes: a test for the unrecognized-status hide, failing on this baseline. A second test that a child present only on an older recent turn is absent, so the limit is explicit.
Acceptance evidence: the status test fails before the production change.

### W2 — One liveness rule
Intent: a live child is not hidden by an unrecognized token.
Required changes: §6. Update the comment that claims the rules already match, in the same change, so it describes the code.
Acceptance evidence: the W1 status test passes, and a known terminal status is still omitted from `Ctrl+T`.

### W3 — Same child, three surfaces
Intent: one snapshot row is painted on the Session panel, the sessions dialog, and `Ctrl+T`.
Required changes: no new reader. Assert the three surfaces from one selector result.
Acceptance evidence: one test builds one snapshot and checks all three. A missing snapshot does not render "Nothing pending" when the client status is unavailable.

### W4 — Publication stop
Intent: do not declare the symptom fixed if the live snapshot lacks the child.
Required changes: none in production if the snapshot is empty. Write the finding: which snapshot fields were empty, which session id was active, and where the child was observed instead.
Acceptance evidence: either a socket-attached or manual note that names the child on all three surfaces, or a stop report. A hand-built snapshot test alone is not the note.

## 8. Failure, cancellation, restart, contention semantics

- Missing snapshot: keep the last painted rows, and do not relabel that as authoritative emptiness. First paint with no snapshot says unavailable, not nothing pending.
- Child completes: `Ctrl+T` may drop it. The Session panel may keep it until the snapshot drops it.
- Snapshot for a different session: do not show that session's children on this one. The existing session binding stays.

## 9. Compatibility and migration

None if the snapshot already carries the child. A missing field is the stop condition, not a silent serde default that hides the gap.

## 10. Required tests

- Unrecognized live status is visible on `Ctrl+T` and the sessions dialog after the change, and hidden before it.
- Known terminal status stays off `Ctrl+T`.
- One snapshot child appears on all three surfaces.
- Missing snapshot is not rendered as authoritative emptiness.
- No test inserts a row the selector did not return.

## 11. Required verification commands

```bash
cargo test -p codegg-tui --lib session_work
cargo test -p codegg-tui --test daemon_socket_integration_tests -- subagent
scripts/verify.sh scoped
```

Use the existing family if those filters match nothing. Do not substitute `cargo check`.

## 12. Documentation updates

If `docs/tui.md` says `Ctrl+T` shows live subagents, leave that sentence only if W4's receipt confirms it. If W4 stops, do not document the capability as present.

## 13. Acceptance criteria

- A live child present in the authorized snapshot is visible on the Session panel, the sessions dialog, and `Ctrl+T` without reopening the surface.
- An unrecognized non-empty status does not hide a live child.
- A child absent from the snapshot is not invented.
- The three surfaces still share one selector.

## 14. Stop conditions

- Stop if the live snapshot's `subagents`, active-turn tree, newest recent turn, and `agent_runs` do not contain the child the user can see elsewhere. Report the publication gap. Do not add a TUI-side registry of children.
- Stop if fixing the hide requires an ADR-0043 amendment. Report the field that is missing.
- Stop if the only failing case is a finished child. `Ctrl+T` is allowed to omit finished work. The reported case is a child that exists now.

## 15. Closure evidence required

- Baseline failure of the status-hide test and its post-change pass.
- The three-surface test.
- Focused commands and `scripts/verify.sh scoped`, labeled local.
- W4's manual or socket-attached receipt, or the stop report. Closure is not accepted on the hand-built snapshot test alone.

## 16. Handoff notes

The convergence closure proved wiring: the key, the dialog sections, and the naming chain. This milestone starts from the opposite observation: those paths exist and a real TUI still shows no row. Do not close by showing the same wiring again.
