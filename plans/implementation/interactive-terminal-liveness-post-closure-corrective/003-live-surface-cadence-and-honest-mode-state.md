# Interactive Terminal Liveness Post-Closure Corrective C003 — Live-Surface Cadence Arming and Honest Mode State

Status: closed 2026-10-17 — closure record:
`plans/closure/interactive-terminal-liveness-post-closure-corrective/003-status.md`
Repository baseline: `18c3dca92` (`feat/auto-20261017-f117dc16`)

Source roadmap:

- `plans/subsystems/interactive-terminal-liveness-post-closure-corrective-addendum.md#c003--live-surface-cadence-arming-and-honest-mode-state`

Long-term requirements:

- `plans/000-long-term-specification.md#44-frontends-render-projections`
- `plans/000-long-term-specification.md#45-locality-by-default`

Applicable ADRs: none. This milestone reuses the closed interactive-process
attach/resume/resync contract and the closed panel-key authority exactly as accepted. It
introduces no transport, no protocol variant, and no new identity or grant. If any work
package here decides to introduce a **push** transport for terminal bytes, that is a new
daemon/frontend protocol decision and requires a new ADR before implementation.

Primary class: **capability** (user-visible; requires end-to-end acceptance evidence through
the real event loop).

Corrective pass against (records immutable, not edited):

- `plans/closure/interactive-terminal-liveness-post-closure-corrective/001-status.md` — its
  §2 row "Bounded pull on the existing cadence, no second timer" and its §4 claim that both
  surfaces repaint on chunk rest on the pull being reached from the running loop. At this
  baseline the wiring exists but the arm that carries it is dark whenever nothing else is
  animating the frame, so the composed claim is not delivered.
- `plans/closure/human-execution-corrective/003-status.md` and
  `plans/closure/interactive-process-sessions/003-status.md` — the same liveness implication
  C001 recorded additively; this milestone does not repeat the falsification, it repairs it.

## 1. Objective

A docked terminal panel must be trustworthy while the user is **not** pressing anything, and the
mode the composer advertises must match the surface that actually owns the keyboard. Concretely:

1. With a live mounted terminal surface and no other animation source (no streaming turn, no
   working turn, no toast, no pending resize, no keystroke), process output must still reach the
   rendered surface on its own within the existing cadence interval.
2. While the panel's PTY owns the keyboard, the canonical `[NOR]`/`[INS]` badge above the main
   input must read `[INS]`, and the contextual hint line must describe typing to the shell rather
   than reader navigation.

This milestone delivers (1) and (2) only. Hint-row wording when the panel is *unfocused*, cursor
blink, the fresh-prompt line, and reader-cursor placement on `Esc` are C004.

## 2. Why this milestone is ready

No hard dependency is open. The daemon side is closed and stable under
`interactive-process-sessions` M001–M002 (sequenced ring, bounded `InteractiveProcessResume`,
typed resync). Key classification and VT rendering are closed under `human-execution-corrective`
C003. Panel key authority is closed under `tui-panel-surface-convergence` M001. The event-loop
tick, `spawn_registered_tui_task`, and `AsyncUiRequestState` semantics are in-tree and stable.

The pull service itself is delivered and correct. What is missing is that the loop reaches it,
and that one derived mode answer feeds both the badge and the hint line. Both are frontend-only
conformance against closed interfaces.

C003 was previously scoped as the polish slot. That classification was wrong, and the correction
is recorded in the roadmap §7: K1 is a capability claim and it is not currently delivered in the
idle state, so this milestone is capability class and lands before polish (C004).

## 3. Current implementation evidence

Baseline `18c3dca92`.

**The pull exists and is only reachable from a dark arm.**

- `service_terminal_output_pull` has exactly one call site, inside the timed arm of the
  `select!` at `src/tui/runtime/event_loop.rs:457`.
- `next_wake` is `render_delay.min(resize_delay)` (`src/tui/runtime/event_loop.rs:288-292`).
- `render_interval` is `Duration::ZERO` when `!streaming_active`
  (`src/tui/runtime/event_loop.rs:150-154`), and `render_delay_after(last_render, ZERO)` returns
  `None` because `elapsed < ZERO` is never true — so `render_delay` is `None` even with
  `needs_render` set.
- `animation_interval` is `Some` only when `streaming_active ||
  SessionStatus::Working || toasts ticking` (`src/tui/runtime/event_loop.rs:262-264`).
- With `next_wake == None`, the arm at `src/tui/runtime/event_loop.rs:423-428` selects
  `futures_util::future::pending()` and parks indefinitely.

Consequence: a mounted, live terminal surface is read only while a toast is alive, a keystroke
arrives, or a resize is debouncing. When the user types a command and then waits for output, the
PTY bytes sit in the daemon's ring and the panel shows nothing until the next keystroke. Observed
by the user as "exceptionally laggy and slow, to the point that I thought it was not actually
working". A freshly spawned shell's `bash-3.2$` prompt is subject to the same defect, which is
the "new terminal is blank" half of the reported symptom.

Secondary cost, for visibility rather than correctness: every pull is a
`spawn_registered_tui_task` doing a full daemon round-trip
(`src/tui/commands/interactive_terminal.rs:891-906`). While only toasts drive it the cadence is
`TOAST_RENDER_INTERVAL` = 250ms, so a live surface is at best 4Hz.

**Why C001's evidence did not catch this.**

The C001 acceptance tests call the pull **directly** rather than through the loop —
`service_terminal_output_pull(&mut app)` at
`src/tui/commands/interactive_terminal.rs:2975`. They prove the pull function issues a bounded
read, that the completion is applied, and that both surfaces repaint. They do not prove the
loop arms it, because the test harness supplies the polling loop the production loop withholds.
`tests/interactive_terminal_tui.rs:83` (`poll_resume_text`) likewise calls `protocol.resume`
outside `App`. No test in the tree drives the real `select!` with a live surface and an idle
frame budget.

**The canonical badge cannot report the terminal's INS state.**

- The badge reads `self.ui_state.input_mode` directly (`src/tui/app/render.rs:1929-1940`).
- `focus_panel` **forces** `input_mode = Normal` whenever a panel takes the keyboard
  (`src/tui/commands/panels.rs:692-702`), and `blur_panel` restores the parked mode
  (`:710-714`, `:737-744`).
- `focus_terminal_panel` and `blur_terminal_panel` mutate only the controller
  (`interactive_terminals.focus` / `.escape`) and never touch `ui_state.input_mode`.
- The only writer of `InputMode::Insert` is the composer's `i` at
  `src/tui/app/mod.rs:3987-3991`, which a focused terminal panel pre-empts.

So with the terminal panel selected the badge is pinned to `[NOR]` in both modes, and the row
reads `[NOR] [build] [TERM]` while the user is typing into a shell.

**A dead branch follows from the same cause.** `TerminalPanel::composer_hints`
(`src/tui/panels/terminal.rs:397-414`) branches on `hints.input_mode == InputMode::Insert` at
`:409`, but `input_mode` is always `Normal` while the panel holds the keyboard, so the INS
wording is unreachable. The hint line therefore describes reader navigation (`i type · hjkl
move …`) precisely while the user is typing into the shell.

**The precedent for the fix already exists on the adjacent route.** `Route::Editor` derives its
badge from `editor_state.buffer_focused(self.ui_state.input_mode)` rather than raw `input_mode`
(`src/tui/app/render.rs:767-788`), and the host already exposes the panel's claim as
`PanelHost::claims_insert_keys` (`src/tui/panels/mod.rs:1840-1842`), implemented by the terminal
panel at `src/tui/panels/terminal.rs:391-395`. Both halves of the derived answer are in-tree.

## 4. Invariants that must not regress

- **I1 — Bounded reads.** Unchanged. Output is read only through `InteractiveProcessResume` with
  an explicit `from_seq` and the daemon's own default (`max_bytes: None`). No unbounded read, no
  "all output" convenience route, no silent shift of history; a cursor that falls behind stays a
  typed resync.
- **I2 — No terminal bytes on the event bus.** Unchanged. No bus event, projection, transcript
  entry, session observation, or audit record may carry terminal bytes. The C003 M003-header
  structural assertion (no output bytes in headers) must keep passing. Arming the loop must not
  become a route for bytes onto the bus.
- **I3 — Key routing is not reopened.** Literal `Esc` is forwarded as a byte rather than
  consumed, `Ctrl-]` leaves focus, focus is claimed only in Insert, and typed characters reach
  the PTY unchanged. This milestone changes what the frame *reports* about mode; it must not
  change what a key *means*.
- **I4 — Single cadence.** No second timer, thread, `tokio::time::interval`, or ticker for
  terminal refresh. Arming must reuse the existing event-loop tick. Per-attachment read state and
  one-in-flight-read-per-attitlement (`begin_output_read` / `finish_output_read`) are unchanged;
  a permanently armed cadence must not make the read race itself.
- **I5 — A poll may not supersede a user action.** Unchanged. Per-attachment request ids,
  `TerminalResumeRequest::{User, Pull}` ownership, and the typed refusals that stop the cadence
  for one attachment must all keep their current behaviour.
- **Render may not move the keyboard.** The badge and the hint line are derived **reads**. No
  work package here may write `ui_state.input_mode`, `panel_host` focus, or the controller's
  `TerminalFocus` from the render path.

## 5. Scope

### In scope

- Arming the event loop whenever a terminal surface is mounted and live, on the existing tick.
- One derived "effective input mode" answer, owned above the render path, consumed by both the
  composer badge and the contextual hint line.
- Regression evidence that drives the **real** event loop in the idle state.
- Additive correction of C001's liveness claim in the registry and roadmap. The C001 closure
  record itself is **not** edited.
- Documentation of the delivered cadence contract.

### Explicitly out of scope

- **Any transport change.** No push, no new `CoreRequest`/`CoreResponse` variant, no bus event for
  terminal bytes, no change to `/core`, `/tui`, or the server.
- Changing `mounted_terminal_surface`'s visibility rule. A collapsed, swapped, or off-edge panel
  must keep costing nothing.
- Changing pull semantics, byte bounds, resync handling, or `note_read_refusal` behaviour.
- Changing `ui_state.input_mode` itself, the park/restore mechanism, or `claims_insert_keys`
  semantics. This milestone reports the effective mode; it does not introduce a second owner of it.
- Hint-row wording and visibility when the panel is unfocused (C004).
- Cursor blink, fresh-prompt line, reader-cursor placement on `Esc` (C004).
- Box-surface viewport plumbing, already recorded as low-severity C004 work.

## 6. Required production changes

### Core/domain

None. No domain type changes.

### Storage and migrations

None. No migration, no schema change.

### Protocol and DTOs

None. `CoreRequest`/`CoreResponse` are unchanged.

### Runtime and concurrency

**Arming.** Make a mounted-and-live terminal surface a contributor to the event loop's existing
tick so the pull is reached without another source of animation. The natural seam is the same
`animation_interval` computation that already gates the working strip, because it is the value
that decides whether the loop is ever awake; the contributor must be cheap enough to evaluate
every iteration and must not itself issue work. `mounted_terminal_surface`
(`src/tui/commands/interactive_terminal.rs:851-858`) already answers the question and already
encodes the visibility rule — reuse it rather than re-deriving liveness at the loop.

Two properties the implementation owes:

- **The arm is bounded by mount, not by focus.** Losing the keyboard to the LLM buffer must not
  stop output; `visible_terminal_handle` is keyed on visibility, which is the correct rule and
  must be preserved.
- **An armed tick with nothing to say must stay cheap.** An empty chunk is already the daemon's
  cheap answer for an unchanged cursor, so the cost is one bounded round-trip per cadence per
  mounted surface, and the existing one-in-flight-read-per-attachment gate means a slow daemon
  cannot accumulate requests.

**Derived mode.** Introduce one accessor owned above the render path that answers the mode the
user is actually in, resolving in this order: the focused panel's insert claim (via the existing
`PanelHost::claims_insert_keys` seam) wins when it holds; otherwise the current
`ui_state.input_mode`. Both the composer badge and `composer_hint_text` must read that single
answer, so the badge and the hint line can no longer disagree. This mirrors the `Route::Editor`
precedent rather than inventing a third pattern.

### Frontend or operator surface

- The composer badge's `[NOR]`/`[INS]` label and its style must follow the derived answer.
- `PanelHints` must carry the derived answer, so `TerminalPanel::composer_hints`'s INS branch at
  `src/tui/panels/terminal.rs:409` becomes reachable and describes typing to the shell in INS.
- No change to the badge's row membership, the `[TERM]` destination badge, the agent badge, or
  the chat-context badge.

### Security and authorization

Unchanged. No new identity, grant, scope, or transport. The typed refusals that stop the cadence
for one attachment must keep working, and a permanently armed tick must not turn a refusal into
a retry loop.

### Documentation and static guards

- `architecture/tui.md` — the terminal liveness contract must state that the cadence is armed by
  a mounted live surface, not by incidental frame activity, and must state the derived-mode rule.
- `docs/tui.md` — the user-facing description must not depend on the user typing to see output.
- `.opencode/skills/tui/SKILL.md` — record the hazard: a cadence service reachable only from a
  `select!` arm that a test drives directly is not evidence that the loop drives it.
- No new guard script is required by this milestone. If the implementation introduces any new
  ticker, that is an I4 violation and the milestone must stop rather than add a guard to legalize
  it.

## 7. Ordered work packages

### Work package A — Prove the idle path is dark on this baseline

Intent: falsify before changing anything, per the addendum §9 discipline. C001 closed on evidence
that supplied the poll loop from the test harness; the whole point of this package is to
establish that the production loop does not.

Required changes: none to production code. Add the failing test described below.

Acceptance evidence:

- A test that stands up the real event loop (`run_event_loop` or the seam it exposes) with a
  live mounted terminal surface, drives one command through the real key path, then **stops
  sending input** and lets the loop run with `streaming_active == false`,
  `session_status == Idle`, and no toast and no resize pending.
- The test asserts the command's output appears in the rendered buffer **without any further
  keystroke, toast, resize, or turn event**.
- On this baseline the assertion fails, and the failure message names the reason: the wake arm
  was never armed. Capture the exact failure output for the closure record.
- Non-vacuity: the same test must be shown to **pass** once a deliberate arm is added, and the
  arm must then be reverted and the test re-run to confirm it fails again. A test that cannot be
  made to fail, or that passes for a reason other than arming, invalidates this milestone's
  premise — stop and report.

### Work package B — Arm the cadence on a mounted live surface

Intent: make the pull reachable while the user is idle, on the existing tick only.

Required changes:

- Add the mounted-live-surface contributor to the loop's tick arming, reusing
  `mounted_terminal_surface` so the visibility rule stays single-sourced.
- Keep `service_terminal_output_pull` byte-identical in what it requests; this package changes
  *when the loop is awake*, not what the pull asks for.
- Confirm the one-in-flight-read gate still bounds request rate, and that an attachment with a
  typed refusal stays parked until a user action re-arms it.

Acceptance evidence:

- The Work package A test passes.
- A test asserting the arm is **not** added when no terminal surface is mounted, and not added
  when the panel is collapsed or moved off-edge (visibility rule preserved).
- A test asserting that losing the keyboard to the LLM buffer does not stop output.
- The 157 C001 tests and the 6 C002 tests across the terminal/panel suites pass **unmodified**.
  If any must change, record the replacement and the reason.

### Work package C — One derived mode answer for the badge and the hint line

Intent: the `[NOR]`/`[INS]` badge and the contextual hint line must both report the surface that
actually owns the keyboard.

Required changes:

- Add the derived accessor above the render path, resolving the focused panel's insert claim
  first and falling back to `ui_state.input_mode`.
- Point the composer badge's label and style at it (`src/tui/app/render.rs:1929-1940`).
- Point `composer_hint_text`'s `PanelHints` at it (`src/tui/app/mod.rs:12322-12331`), which makes
  the terminal panel's existing INS branch reachable.
- Do **not** write `input_mode`, panel focus, or controller focus from the render path. A test
  must assert the render pass leaves focus and mode byte-identical.

Acceptance evidence:

- A test driving the real key path — select the terminal panel, press `i` — asserts the rendered
  composer row contains `[INS]` while the PTY owns the keyboard, and `[NOR]` after `Esc`.
- A test asserting the hint line reads the INS wording in INS and the reader wording in NOR.
- A test asserting every other route's badge is unchanged: composer Insert/Normal, editor
  Insert/Normal, chat, session. This milestone must not move a badge it does not own.
- A test asserting the derived answer is a pure read: two consecutive reads with no intervening
  event return the same value, and a render does not change focus state.

### Work package D — Reconciliation and documentation

Intent: leave the planning and architecture record truthful about what C001 did and did not
deliver.

Required changes:

- Registry: C001 moves to **conditionally closed** with the condition named and owned by C003,
  per `plans/registry.md` maintenance rule 6 ("use `conditionally closed` when a post-closure
  correctness finding invalidates a strict claim"). The C001 closure record is **not** edited.
- Roadmap: the C003 section is rewritten to capability class with the falsification recorded;
  a new C004 section owns the polish work that C003 previously held.
- `architecture/tui.md`, `docs/tui.md`, `.opencode/skills/tui/SKILL.md` updated together per
  AGENTS.md.

Acceptance evidence: the three documents describe the same contract, and the registry's C001,
C003, and C004 rows agree with their linked documents' own status lines.

## 8. Failure, cancellation, restart, and contention semantics

- **Attached surface, daemon unreachable.** `core_client` is `None` (the WS-attach frontend).
  `service_terminal_output_pull` returns `false` after the input flush. Arming must not turn this
  into a busy loop, and the typed refusal text from C002 must be unchanged.
- **Attachment exited / gone / unknown.** The daemon's typed error must stop the cadence **for
  that attachment only**, as it does today. It must not retry-loop on the tick, and it must not
  stop other attachments.
- **Resync.** A chunk that reports the cursor fell behind applies the existing typed resync path
  and its notice. Arming must not add a second resync trigger.
- **Superseded read.** A read whose attachment or handle changed under it is discarded, not
  applied. `finish_output_read` returning `false` must keep that behaviour under a permanently
  armed cadence.
- **Panel unmounted mid-cadence.** The arm evaluates every iteration; a closed box or an undocked
  panel must stop being read within one iteration.
- **User action during a cadence read.** I5 holds. A user-initiated resume, input, resize,
  detach, or terminate is never invalidated by a background pull.
- **Render panic.** The existing progressive panic recovery is untouched. A render that panics
  while a terminal surface is armed must not leave the tick permanently disabled.

## 9. Compatibility and migration

No compatibility surface changes. No wire format, no stored data, no configuration key, no
migration, no negotiation. The request the pull issues is byte-identical to today's; only the
frequency at which the frontend asks changes, and only while a surface is mounted.

Rolling back is a revert of the arming and the derived accessor. Because neither changes what a
key means, a revert degrades liveness and mode reporting only — never correctness of input
delivery.

## 10. Required tests

### Focused unit tests

- The tick-arming contributor: `None` with no mounted surface, `Some` with a mounted live one,
  and `None` for a collapsed/off-edge panel.
- `render_delay_after` / `animation_interval` interaction: assert the loop is armed in the idle
  case that currently parks, so the regression is pinned at the unit level too.
- The derived mode resolver: every combination of (panel focused?, panel claims insert?,
  `input_mode`) that can occur, including a focused non-terminal panel that does not claim insert.

### Integration tests

- **The headline test (Work package A):** real event loop, live mounted surface, type a command,
  then go idle; output must appear with no further input. This is the acceptance signal and the
  one that fails on this baseline.
- Both surfaces (box and panel), because C001's own contract is two surfaces.
- Full key path → daemon → rendered buffer for `[INS]` and `[NOR]`, plus the hint-line wording in
  each mode.
- Badge non-regression across composer, editor, chat, and session routes.

### Restart and recovery tests

- A refused pull (exited attachment) parks that attachment's cadence until a user action
  re-arms it, and a later re-attach resumes cleanly.

### Contention and cancellation tests

- A cadence read in flight while a user resume is issued: the user completion is not superseded,
  and the poll's answer is discarded if superseded.
- Sustained output with a slow daemon: request rate stays bounded by the one-in-flight gate.

### Security and negative tests

- No terminal byte reaches the bus, a projection, the transcript, or an audit record. The
  existing C003 M003-header structural assertion must keep passing unmodified.
- Arming does not introduce polling against an attachment the caller may not read. The daemon's
  existing per-client attachment binding is the authority; this milestone must not widen it.

### Migration and compatibility tests

None. Nothing migrates.

## 11. Required verification commands

```bash
# the headline acceptance test, plus the terminal/panel suites
cargo test -p codegg --lib tui::commands::interactive_terminal
cargo test -p codegg --lib tui::panels
cargo test --test tui_panel_docking
cargo test --test interactive_terminal_tui
cargo test --test tui_render

# static guards
bash scripts/verify.sh quick

# formatting and linting
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

Label honestly in the closure record which of these ran locally and which did not. A green
`verify.sh quick` is a **subset** of `quick`'s full guarantee only when CI runs the omitted
client/desktop/provider guards; do not claim hosted CI unless a run exists.

## 12. Documentation updates

- `architecture/tui.md` — the interactive-terminal liveness section must state that the cadence is
  armed by a mounted live surface rather than by incidental frame activity, and must state the
  derived-mode rule for the composer badge and hint line.
- `docs/tui.md` — the user-facing gesture/liveness text must not imply the user must type for
  output to appear.
- `.opencode/skills/tui/SKILL.md` — add the two hazards this milestone exists for: a cadence
  service reachable only from a `select!` arm a test drives directly is not evidence the loop
  drives it; and a mode badge reading a raw global is not the mode the user is in.
- `plans/registry.md` and the roadmap, per Work package D.

Per AGENTS.md, a module contract change updates the skill and its `architecture/` doc together.

## 13. Acceptance criteria

1. With a live mounted terminal surface and every other animation source absent, output produced
   by the process appears on that surface within the existing cadence interval, with no
   keystroke, toast, resize, or turn event required.
2. While the panel's PTY owns the keyboard, the canonical badge above the main input reads
   `[INS]` and the hint line describes typing to the shell; after `Esc`, both read the reader
   state. The badge and the hint line cannot disagree, because both read one derived answer.
3. The same test fails on this baseline for the stated reason and passes after the arm.
4. No second timer, thread, or ticker exists; the pull issues the same bounded request as C001.
5. Request rate on a mounted idle surface is bounded by the one-in-flight-read gate.
6. Losing the keyboard to the LLM buffer does not stop output; unmounting or hiding the panel
   does resume costing nothing.
7. Every other composer/editor badge is byte-identical to baseline.
8. The render path writes no focus or mode state.
9. Terminal bytes reach neither the bus, a projection, the transcript, nor an audit record.
10. `verify.sh quick` passes; the existing terminal/panel/render suites pass unmodified unless a
    change is recorded with its reason.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- Work package A's test cannot be made to fail on this baseline for the stated reason — the
  premise is wrong and the milestone should not be closed around it.
- Arming the loop appears to require a second timer, thread, or interval. That violates I4;
  report it rather than smuggling in a cadence.
- The fix appears to require terminal bytes on the bus, a new `CoreRequest` variant, or a push
  transport. That is a new daemon/frontend protocol decision: stop and raise an ADR.
- The derived mode answer turns out to require changing what a key *means* (rather than what the
  frame reports about it). That would reopen I3 and belongs in a separate milestone.
- Fixing the badge would change focus ownership, i.e. render would have to write mode or focus.
- Work would expand into the WS-attach/server transport (C002's boundary) or into
  `remote-interactive-process-authorization-corrective` R001.

## 15. Closure evidence required

- The Work package A baseline failure output, verbatim, and the same test passing after the arm.
- The non-vacuity demonstration: arm added → pass → reverted → fail again.
- The requirement-to-evidence matrix mapping each §13 criterion to its test and result.
- The production-change inventory with locations, distinguishing the arming change from the
  derived-mode change.
- Invariant review for I1–I5 with explicit evidence that each still holds.
- Exact verification commands run, with local vs CI labelled truthfully, and any command not run
  named as such.
- An **additive** note that this milestone falsified a strict claim in
  `plans/closure/interactive-terminal-liveness-post-closure-corrective/001-status.md`, recorded
  without editing that record.
- The unblock audit: whether C004 became `ready`, and what (if anything) else this closure
  unblocked.

## 16. Handoff notes

- Start at Work package A. The whole milestone's premise is that the loop does not arm the pull
  in the idle state; prove that before writing production code.
- The C001 tests at `src/tui/commands/interactive_terminal.rs` drive
  `service_terminal_output_pull` directly. Do not "fix" them by calling it more often — the
  defect is in the loop, not in the pull.
- `mounted_terminal_surface` is the single source of truth for "a terminal is on someone's
  screen". Do not re-derive liveness from `link == Live` at the loop; that is the mistake the
  doc comment on `visible_terminal_handle` explicitly warns against ("treating 'adopted' as
  'mounted' is how a background terminal ends up polling forever for nobody").
- `render.rs:767-788` is the pattern to copy for the derived mode. Prefer extending it over
  inventing a parallel mechanism.
- Broad test execution runs under nextest profile `ci`; env mutation in tests is process-global.
- If a hosted CI run is claimed, the receipt must name the exact head.