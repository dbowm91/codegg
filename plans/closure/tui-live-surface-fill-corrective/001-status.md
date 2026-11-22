# TUI Live Surface Fill Corrective M001 — Closure Status

Status: **conditionally closed — live keystroke receipt outstanding**
Date: 2026-11-22
Class: **capability**
Source implementation plan: plans/implementation/tui-live-surface-fill-corrective/001-terminal-input-ready-on-load.md
Source subsystem roadmap: plans/subsystems/tui-live-surface-fill-corrective-addendum.md
Repository baseline reviewed: d319afec15b8b8b33ced2c0af7a3ee50eb1a82c3
Implementation: working tree on feat/auto-20261109-f8abad0d-artifact-safe (uncommitted at record time)

## 1. Executive finding

Selecting the Terminal panel is now a load that ends ready for typing. When the
controller holds no handle the select path starts the default shell through the
existing create command; when the shell lands in the panel and the panel is the
surface holding the keyboard, the attach completion focuses the PTY. A live but
detached handle is re-attached through the panel route, and a live attached
handle is focused directly. The gesture that used to be required on load (a
second i) still works as the explicit route back after Esc.

The milestone also removed a state bug that made the new path unretryable: a
create refused before dispatch (no core client, no workspace) left the panel
mid-create bit set, so every later select and i returned through the in-flight
guard. The bit is now set only by a create that actually began.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Selecting the panel with no shell starts one | select_panel -> ready_terminal_panel -> spawn_default_terminal_for_panel; the create -> attach -> panel adoption path is the existing pull fixture panel_rendered_buffer_shows_live_pty_output | pass (code path; real spawn covered by the existing fixture) |
| A successful panel attach focuses the PTY when the panel holds the keyboard | apply_terminal_attach_finished focus arm; a_panel_attach_focuses_the_terminal_the_panel_is_showing | pass |
| The first key after load is queued as PTY input, not navigation | selecting_the_terminal_panel_leaves_a_live_shell_ready_for_typing | pass; fails on baseline (see 4) |
| i after Esc returns to input without opening the box | escaping_a_loaded_terminal_and_pressing_i_returns_to_input; a_docked_terminal_panel_accepts_typing_as_soon_as_it_loads | pass |
| A failed create or attach does not report readiness and is retryable | a_refused_create_does_not_leave_the_panel_mid_create; a_failed_panel_attach_does_not_keep_the_panel_mid_create | pass |
| A second select during an in-flight create does not start a second shell | the existing guard is reached through the new path; the guard test a_second_select_during_an_in_flight_create_does_not_start_another_shell | pass |
| An attach landing after the user left the panel does not steal focus | focus arm requires panel_host.is_focused(), active == Terminal, and an empty focus manager; the_box_opens_in_insert_while_the_docked_panel_still_needs_i (panel half) | pass |
| No new transport and no local PTY | the only create/attach calls are the existing daemon commands; no new variant in the diff | pass |

## 3. Production implementation evidence

- crates/codegg-tui/src/commands/interactive_terminal.rs
  - start_terminal_create returns whether the request began. A refusal before
    dispatch now clears the panel route instead of leaving it stuck.
  - spawn_default_terminal_for_panel records only a create that began.
  - apply_terminal_attach_finished focuses the handle when the panel route is
    active, the Terminal panel holds the keyboard, and no dialog is mounted. A
    completion that arrives after the user left, or behind a dialog, shows the
    shell without taking focus back.
  - focus_terminal_panel sets the panel route before a recovery re-attach, so
    the completion lands in the panel rather than opening the box.
  - ready_terminal_panel is the select-path rule: start when empty, focus when
    live and attached, re-attach when live and detached, and leave an exited or
    gone handle on screen for its post-mortem output (i replaces it).
- crates/codegg-tui/src/commands/panels.rs
  - select_panel calls ready_terminal_panel for the Terminal id. Every route
    into the panel (manager, saved hotkey, dock gesture, reveal) passes here.
- crates/codegg-tui/tests/tui_render/tui_panel_docking.inc
  - The navigation helper now starts in reading mode, because loading a live
    terminal enters Insert. Four existing tests were updated to that contract;
    three previously encoded the old "loads unfocused" behaviour and now assert
    the readiness rule.

No daemon, protocol, storage, or security change.

## 4. Verification executed

Commands were run from the worktree root with CARGO_BUILD_JOBS=2.

| Command | Result |
|---|---|
| cargo check -p codegg-tui --all-targets | pass |
| cargo test -p codegg-tui --lib -- commands::interactive_terminal::tests --test-threads=1 | pass, 41 |
| cargo test -p codegg-tui --test tui_render -- terminal | pass, 26 |
| cargo test -p codegg-tui --test tui_render | pass, 1239 |
| scripts/verify.sh scoped | guards, fmt, and the scoped selection passed; the widened workspace nextest stopped on a pre-existing root-crate failure, 5,359 of 5,360 passing before fail-fast |
| cargo nextest run --workspace --locked --profile ci --no-fail-fast | 17,622 of 17,623 passed (1 leaky, 11 skipped); all codegg-tui targets, including every test added here, passed |

scripts/verify.sh scoped needed a two-line harness fix in this change: it piped
the selection JSON into the formatter, which stops reading once it has parsed
the selection, and pipefail turned the resulting SIGPIPE into exit 141 before
the widened pass could start. The fix replaces both pipes with here-strings.

The one failure is
delegated_transcript::a_delegated_run_records_a_bounded_tool_result_with_truncation_marked
in the root codegg package's agent_loop family. It fails deterministically
(twice, in isolation) on this baseline, tests/agent_loop has no diff in this
change, and the root package does not depend on codegg-tui. It is therefore a
pre-existing, unrelated baseline failure, recorded here rather than fixed
because it belongs to a different subsystem. The leaky test is
search_backend::bootstrap::bootstrap_report_summary_includes_coverage, also in
the root package and untouched here.

Baseline failure was measured, not asserted. The select-path readiness call was
removed from select_panel for one run: selecting_the_terminal_panel_leaves_a_live_shell_ready_for_typing
FAILED (the key was swallowed as navigation). The file was restored byte-for-byte
from a backup and the test passed again. The same run proved the refusal bit:

- with the old assignment (bit set unconditionally) the panel stayed mid-create
  and the retry test failed;
- with start_terminal_create returning whether it began, the bit clears and the
  test passes.

## 5. Invariant review

- No local PTY: create and attach remain daemon commands.
- The WS-attach refusal is unchanged; no transport was added.
- A second select while a create is in flight is still swallowed by the guard.
- Esc still leaves Insert (classify_focused), Shift+Q still leaves the panel,
  and a bare q is still PTY input once focused.
- focus is never called on a handle that cannot take input: the refusal path
  re-attaches or replaces instead.
- A completion that arrives after the keyboard moved elsewhere does not steal it.

## 6. Failure and recovery review

- Create or attach error: the panel bit clears, the typed error is toasted, and
  the controller is not focused.
- Superseded request id: ignored, as before; no stale handle is focused.
- Overlapping select: the in-flight guard remains the single owner.
- User leaves the panel before attach completes: the panel shows the shell and
  does not take focus; the next select or i focuses it.
- Exited or gone handle: left on screen for reading; i starts a replacement.

## 7. Migration and compatibility review

None. The gesture is additive: older clients keep press-i-to-type, and the new
behaviour is that a load is already typing. No schema, protocol, or config change.

## 8. Security review

No new writer, no new authority. Terminal input still travels the existing
attachment-scoped input path, and the panel cannot create a handle the daemon did
not mint.

## 9. Documentation and operations

No docs/tui.md change: the hint still truthfully names i as the way in after Esc,
and the panel's empty placeholder is still the state after a refusal. The comment
budget was banked in docs/comment-budget.toml for the added explanations.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | No live TUI or socket-attached receipt was captured in this session | The load path is proven by deterministic tests, not by a real keystroke reaching a real shell | Select Terminal on a socket-daemon TUI, type one character, and record that it reached the shell (or that a refusal was visible); attach the note to this record |

## 11. Roadmap disposition

Conditionally closed. The capability is implemented and the baseline failure is
measured; the only outstanding evidence is the manual receipt the source plan
names as closure evidence rather than a follow-up.

## 12. Registry updates

- Roadmap milestone table: M001 conditionally closed, closure record linked.
- Registry milestone row: ready -> conditionally closed with this record.
- Subsystem row and execution-order note: M001 implemented, receipts outstanding.
