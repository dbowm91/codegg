# TUI Live Surface Fill Corrective M002 — Closure Status

Status: **conditionally closed — live dirty/clean receipt outstanding**
Date: 2026-11-22
Class: **capability**
Source implementation plan: plans/implementation/tui-live-surface-fill-corrective/002-diff-panel-paints-the-daemon-answer.md
Source subsystem roadmap: plans/subsystems/tui-live-surface-fill-corrective-addendum.md
Repository baseline reviewed: d319afec15b8b8b33ced2c0af7a3ee50eb1a82c3
Implementation commit: 4a5e4b670 on feat/auto-20261109-f8abad0d-artifact-safe

## 1. Executive finding

The Diff panel now paints the answer to the load it started. A completion that
arrives while the panel's placeholder still says Loading is adopted into that
panel as the Diff viewer; an empty answer becomes an explicit empty state; a
daemon refusal, a transport error, or an unexpected response becomes a named
failure state. Every arm ends the Loading state. A /diff typed from the composer
still opens its modal, because no panel was waiting for that read.

Two placeholder states were added for this: Empty (the load finished with
nothing to show) and Failed (the load failed, carrying the daemon's own words).
They are distinct from Idle (nothing was asked) and Cannot (the panel would act
if it could), which is what makes the panel's words match what happened.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| A waiting panel adopts the daemon answer instead of a modal | a_waiting_diff_panel_adopts_the_daemon_answer_instead_of_a_modal | pass; fails on baseline |
| Loading ends on success | same test (panel_is_loading false; adopted content true) | pass |
| A clean tree is an explicit panel state, not only a toast | a_clean_tree_ends_the_panel_load_with_an_explicit_empty_state | pass; fails on baseline |
| A daemon refusal is visible in the panel | a_daemon_refusal_is_painted_in_the_panel | pass; fails on baseline |
| A transport error settles the panel | a_transport_error_settles_the_panel_load | pass; fails on baseline |
| A /diff from the composer keeps its box | a_diff_asked_for_from_the_composer_still_opens_its_box | pass (both trees) |
| The TUI still does not read the worktree | no git process, no filesystem read; the only inputs are CoreResponse::GitDiff bytes | pass |
| The placeholder keeps the host chrome | existing hosted placeholder tests plus the new variants they enumerate | pass |

## 3. Production implementation evidence

- crates/codegg-tui/src/commands/diff.rs
  - apply_diff_finished reads whether the Diff panel is waiting
    (panel_is_loading) and settles every arm. Success adopts a DiffDialog into
    the panel; equal old/new content sets Empty; Error/transport/unexpected set
    Failed with the daemon's code and message. The composer path is unchanged:
    it stores dialog_state.diff_dialog and opens the modal.
- crates/codegg-tui/src/panels/hosted.rs
  - PlaceholderState gained Empty(&'static str) and Failed(String); the type is
    no longer Copy, and lines() matches by reference. HostPanel exposes
    placeholder_state() for the completion path and for tests.
- crates/codegg-tui/src/panels/mod.rs
  - PanelHost gained panel_is_loading(id) and placeholder_state(id). A panel
    that already holds a box is not rewritten by an unrelated answer
    (set_placeholder_state is a no-op once content exists).
- crates/codegg-tui/src/commands/diff.rs tests
  - Five completion tests, including the no-panel composer path.

No git read was added. The bare daemon branch still returns an empty old side
and the unified text as the new side; DiffViewer's default Inline mode renders
that text as the diff, which is the honest rendering the source plan allows and
does not manufacture a second read.

## 4. Verification executed

| Command | Result |
|---|---|
| cargo check -p codegg-tui --all-targets | pass |
| cargo test -p codegg-tui --lib -- commands::diff::tests --test-threads=1 | pass, 5 |
| cargo test -p codegg-tui --lib -- panels::hosted --test-threads=1 | pass, 3 |
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

Baseline failure was measured by forcing the old correlation answer
(panel_waiting = false) for one run. Four tests failed —
a_waiting_diff_panel_adopts_the_daemon_answer_instead_of_a_modal,
a_clean_tree_ends_the_panel_load_with_an_explicit_empty_state,
a_daemon_refusal_is_painted_in_the_panel, and
a_transport_error_settles_the_panel_load — while the composer test still passed.
The file was restored byte-for-byte from a backup and all five passed again.

## 5. Invariant review

- A frontend paints only what an authorized projection or command result
  vouches for: the viewer is built from the response bytes only.
- A panel the user is looking at shows the outcome of the load it started: all
  four completion arms settle it.
- No second diff implementation and no frontend git spawn.
- The hosted chrome contract is intact: adoption uses the same PanelHost::adopt
  path the other Filled arms use, and DiffDialog already implements
  render_in_panel and docks_as_viewer.

## 6. Failure and recovery review

- Daemon refusal: Failed carries code and message in the panel.
- Transport failure: Failed carries the error text; the toast still fires.
- Empty answer: Empty; the toast still says no changes.
- Completion after the user left the panel: adopted into the panel that asked,
  never a modal over another surface, and Loading is cleared.
- Stale request: DiffFinished carries no request id (pre-existing), so two
  in-flight reads are not separable; the first completion settles the panel and
  the second follows the composer path. This is unchanged by the milestone.

## 7. Migration and compatibility review

None. No protocol, storage, or configuration change. PlaceholderState is
crate-internal; its two new variants are additive and every match is exhaustive.

## 8. Security review

The diff remains a daemon read through the existing command route; the frontend
performs no filesystem access and holds no path. Placeholder text is the
daemon's own message, rendered as text.

## 9. Documentation and operations

No canonical documentation change. The empty copy stays in the host's
placeholder vocabulary. The comment budget was banked for the added
explanations.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | No live dirty-tree or clean-tree receipt was captured | The adoption and empty states are proven by deterministic completion tests only | Select Diff on a socket-daemon TUI with one dirty tree and one clean tree; record what the panel showed; attach the note to this record |
| low | The adopted viewer is a snapshot: while the panel holds content, selecting it does not reload, and /diff opens the modal instead | A user who changes files keeps the old diff until another surface replaces it | Out of this milestone's exit; a future corrective can reload on select or on an explicit gesture |

## 11. Roadmap disposition

Conditionally closed. The capability and its empty/refusal arms are implemented
and the baseline failure is measured; the manual receipt remains outstanding.

## 12. Registry updates

- Roadmap milestone table: M002 conditionally closed, closure record linked.
- Registry milestone row: ready -> conditionally closed with this record.
