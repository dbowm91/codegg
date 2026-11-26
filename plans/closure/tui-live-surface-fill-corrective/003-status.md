# TUI Live Surface Fill Corrective M003 — Closure Status

Status: **conditionally closed — live publication and surface receipt outstanding**
Date: 2026-11-22
Class: **capability**
Source implementation plan: plans/implementation/tui-live-surface-fill-corrective/003-live-subagents-reach-named-surfaces.md
Source subsystem roadmap: plans/subsystems/tui-live-surface-fill-corrective-addendum.md
Repository baseline reviewed: d319afec15b8b8b33ced2c0af7a3ee50eb1a82c3
Implementation commit: 4a5e4b670 on feat/auto-20261109-f8abad0d-artifact-safe

## 1. Executive finding

The hiding filter was real and its rule contradicted the producer's vocabulary.
The daemon's durable run statuses are created, queued, preparing, running,
waiting, cancelling (live) and completed, failed, interrupted, cancelled
(terminal). SubagentRow::is_running listed a handful of "live" tokens and called
every other value finished, while the job predicate called every value it did
not recognise live — and its comment claimed the two matched. Four live states
(created, preparing, waiting, cancelling) were therefore hidden from Ctrl+T and
the sessions dialog while the Session panel still listed them. Because the
durable run status overwrites the child's tree status before the filter, a child
that had been dispatched but not started, or one waiting for an answer,
disappeared exactly when the user was looking for it.

The fix is one rule: SubagentRow::is_running delegates to is_live_state, which
now also knows that cancelling is live (a requested cancellation is not a
cancellation that happened) and that interrupted is terminal. Ctrl+T also stops
treating a missing snapshot as emptiness: it falls back to the rows adopted from
the last snapshot, which are the same rows the Session panel is showing.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| A live child in the snapshot appears on the Session panel, the sessions dialog, and Ctrl+T | a_created_child_reaches_all_three_named_surfaces (one selector result fed to SessionDialog, build_task_rows, and the sidebar) | pass; fails on baseline |
| An unrecognized/live producer status does not hide a child | a_dispatched_or_waiting_child_is_not_mistaken_for_finished; a_created_child_reaches_the_overlay | pass; fail on baseline |
| A terminal status does not claim to be live | same tests (completed, failed, interrupted, cancelled) | pass |
| A missing snapshot is not authoritative emptiness | the_overlay_keeps_adopted_rows_when_no_snapshot_is_available | pass; fails on baseline |
| The three surfaces still share one selector | session_work::select_session_work remains the only derivation; the three-surface test starts from it | pass |
| No synthesized child and no ADR-0043 amendment | no row is created from anything but the snapshot; no protocol or ADR change in the diff | pass |

## 3. Production implementation evidence

- crates/codegg-tui/src/app/state/subagents.rs
  - SubagentRow::is_running delegates to is_live_state. The doc records why the
    two rules had to merge and which producer states were being hidden.
- crates/codegg-tui/src/app/state/tasks.rs
  - is_live_state returns true for cancelling before the terminal-token scan,
    and the terminal list now includes interrupt. Both subagent and job rows use
    this one rule.
- crates/codegg-tui/src/app/mod.rs
  - task_rows falls back to ui_state.subagents.rows when no projection snapshot
    is available, instead of an empty vector. Jobs have no fallback: they are
    only carried by the snapshot.
- crates/codegg-tui/src/app/state/session_work.rs
  - The three-surface test drives a session-owned child whose durable run is
    created through the selector, the overlay builder, the sessions dialog, and
    the sidebar.

No daemon, protocol, storage, or ADR change.

## 4. Verification executed

| Command | Result |
|---|---|
| cargo check -p codegg-tui --all-targets | pass |
| cargo test -p codegg-tui --lib -- app::state --test-threads=1 | pass, 466 |
| cargo test -p codegg-tui --lib -- components::sidebar --test-threads=1 | pass, 22 |
| cargo test -p codegg-tui --lib -- components::dialogs::session --test-threads=1 | pass, 29 |
| cargo test -p codegg-tui --test tui_render | pass, 1239 |
| scripts/verify.sh scoped | guards, fmt, and the scoped selection passed; the first widened pass stopped on the root-crate fixture failure recorded below, which this commit fixes |
| cargo nextest run --workspace --locked --profile ci --no-fail-fast (after the fixture fix) | 17,623 of 17,623 passed (11 skipped); every codegg-tui target, including every test added here, passed |
| CI verify (PR #231, commit 4a5e4b670) | pass, 20m41s — https://github.com/dbowm91/codegg/actions/runs/44979048913 |

scripts/verify.sh scoped needed a two-line harness fix in this change: it piped
the selection JSON into the formatter, which stops reading once it has parsed
the selection, and pipefail turned the resulting SIGPIPE into exit 141 before
the widened pass could start. The fix replaces both pipes with here-strings.

The first sweep exposed one pre-existing failure in the root codegg package's
agent_loop family:
delegated_transcript::a_delegated_run_records_a_bounded_tool_result_with_truncation_marked.
It fails deterministically because path redaction shrinks the capped tool
result below the 8 KiB row bound, so the layered truncation markers never
engage. The fixture now uses long lines that survive redaction, and the test
passes. The leaky test is
search_backend::bootstrap::bootstrap_report_summary_includes_coverage, also in
the root package and untouched by the TUI change.

Baseline failure was measured by restoring the old is_running body and forcing
the fallback branch off for one run. Four tests failed —
a_dispatched_or_waiting_child_is_not_mistaken_for_finished,
a_created_child_reaches_the_overlay, a_created_child_reaches_all_three_named_surfaces,
and the_overlay_keeps_adopted_rows_when_no_snapshot_is_available. The files were
restored byte-for-byte from backups and all four passed again.

## 5. Invariant review

- A frontend paints only what the authorized projection vouches for; no row is
  synthesized.
- The three surfaces read one selector; no second registry of children exists.
- A finished child may still be omitted from Ctrl+T: completed, failed,
  interrupted, and cancelled are terminal.
- A cancelling run is shown as live; the run is still winding down. A reply
  addressed to it may be refused by the daemon with a typed error, which is
  preferable to hiding a child the user can still see working.

## 6. Failure and recovery review

- Missing snapshot: the overlay keeps the last adopted rows; the Session panel
  already behaved this way.
- Projection hiccup: no row is cleared on absence, only on an arriving snapshot
  that says the child is gone.
- Unknown future status: shown rather than hidden, matching the job rule; the
  asymmetry is deliberate and now documented in one place.
- Dialog selection of a non-active session: unchanged, and still reports
  unavailable rather than empty.

## 7. Migration and compatibility review

None. No schema, protocol, or config change. The rule is a pure frontend
presentation decision over status strings the projection already carries.

## 8. Security review

No new data is read: the rows come from the existing authorized snapshot. The
change only decides which statuses are displayed. No authority, path, or process
surface is touched.

## 9. Documentation and operations

No docs/tui.md change: the document already says Ctrl+T shows live subagents, and
that sentence is now backed by this rule. The comment budget was banked for the
added explanation.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | No live snapshot inspection or manual surface receipt was captured | The deterministic tests prove the filter and the wiring, but not that the daemon's live snapshot contains the child the user can see elsewhere | With a real subagent running, open Ctrl+T, the sessions dialog, and the Session panel and record whether the child appears; inspect the snapshot's subagents, active-turn tree, newest recent turn, and agent_runs |
| medium | The M003 stop condition is not yet discharged | If the child is absent from all four snapshot sources, the defect is a publication gap and this milestone must not paint around it | If the receipt shows absence, stop and report the publication gap rather than adding a TUI-side registry |

## 11. Roadmap disposition

Conditionally closed. The hiding rule is fixed and the baseline failure is
measured. Strict closure waits on the receipt that also discharges the stop
condition: a live child present in the snapshot must appear on all three
surfaces. If it does not, this record must be superseded by a publication-gap
report and no TUI-side row may be invented.

## 12. Registry updates

- Roadmap milestone table: M003 conditionally closed, closure record linked.
- Registry milestone row: ready -> conditionally closed with this record.
- The workstream remains conditionally closed until all three receipts exist.
