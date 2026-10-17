# Interactive Terminal Liveness Post-Closure Corrective C001 — Live Output Pull and Terminal Liveness Closure

Status: implemented
Repository baseline: `719cfeb5` (`feat/auto-20261017-8835f26d`)

Source roadmap:

- `plans/subsystems/interactive-terminal-liveness-post-closure-corrective-addendum.md#c001--live-output-pull-and-terminal-liveness-closure`

Long-term requirements:

- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#44-frontends-render-projections`

Applicable ADRs: none. This milestone reuses the closed interactive-process
attach/resume/resync contract as accepted; a push transport is out of scope and would
require a new ADR.

Primary class: **capability** (user-visible; requires end-to-end acceptance evidence).

Corrective pass against (records immutable, not edited):

- `plans/closure/interactive-process-sessions/003-status.md` (§1 claims a user can
  "watch bounded output … focus the terminal so keystrokes reach the process").
- `plans/closure/human-execution-corrective/003-status.md` (C003 PTY keyboard and VT
  presentation fidelity).

## 1. Objective

Make output produced by an attached interactive process reach the screen. A user who creates
a terminal, types a command, and presses Enter must see the resulting output in the terminal
box **and** in the docked terminal panel, on the socket-daemon frontend, with no manual
recovery action, no second cadence, and no unreached read side.

## 2. Why this milestone is ready

No hard dependency is open. The daemon side is closed and stable under
`interactive-process-sessions` M001–M002: a PTY reader task pumps output into a sequenced
ring, and `InteractiveProcessResume` performs a bounded read with typed resync. Key
classification and VT rendering are closed under `human-execution-corrective` C003. The panel
keymap/chrome ownership is closed under `tui-panel-surface-convergence` M001. The event-loop
tick, `spawn_registered_tui_task`, and `AsyncUiRequestState` semantics are all in-tree and
stable.

The one thing missing is the frontend read side — so this is conformance work against a closed
interface, not new architecture.

## 3. Current implementation evidence

- The TUI constructs exactly two output-bearing requests: `InteractiveProcessAttach`
  (`src/tui/commands/interactive_terminal.rs:409`) and `InteractiveProcessResume` (`:593`).
- `start_terminal_resume` has exactly one caller: the `/terminal-resume` slash command
  (`src/tui/app/mod.rs:8176`). There is no timer, no render-cadence hook, no post-input read.
- `flush_terminal_input` drains the queued input and a pending resize only
  (`src/tui/commands/interactive_terminal.rs:703-704`).
- The daemon cannot push: its only interactive event, `CoreEvent::InteractiveProcessExited`,
  is bytes-free by contract (`crates/codegg-protocol/src/core.rs:4124-4127`).
- Consequence: a freshly spawned shell writes its prompt after the attach request has already
  returned; nothing reads again; the daemon holds the bytes in the ring while both surfaces
  render an empty view indefinitely. No error, no toast, no diagnostic.
- The box routes keys correctly (`src/tui/app/mod.rs:4027` → `handle_terminal_key` →
  `classify_focused` → `send_terminal_input`), and so does the panel
  (`src/tui/panels/terminal.rs:329-337`, `src/tui/app/input.rs:671`).
- The write path end is healthy: `src/core/daemon.rs:4113` →
  `src/interactive_process_attach.rs:792` writes to the PTY master.
- The ring is healthy: `src/interactive_process.rs:1073` pumps PTY output into the sequence
  ring; `src/interactive_process_attach.rs:846` serves bounded resume with typed resync.
- Resize coupling: the panel queues a viewport resize during render
  (`src/tui/panels/terminal.rs:185`); the only drain is `flush_terminal_resize`, reachable only
  via `flush_terminal_input`; a docked terminal the user never types into therefore keeps the
  80x24 size applied at create (`src/tui/commands/interactive_terminal.rs:260`).
- Supersession hazard: `dialog_state.terminal_request` is a **single-slot**
  `AsyncUiRequestState` shared by all nine terminal operations
  (`src/tui/app/state/dialog.rs:169`; `src/tui/app/state/async_request.rs:48` `begin()`
  increments one counter, `finish(request_id)` rejects mismatches). A poll that calls `begin()`
  on a cadence would supersede an in-flight input/resize/detach/terminate completion and cause
  `finish()` to return `false`, silently dropping it.
- Existing tests cannot catch this:
  `tests/interactive_terminal_tui.rs:83` (`poll_resume_text`) supplies a real 25 ms polling
  loop and `:177` proves the round trip — but it calls `protocol.resume` **directly on the
  protocol, outside `App`**. `tests/tui_panel_docking.rs:1516` asserts only that a key reaches
  the send seam, observing the "terminal input dropped" toast from an app with no
  `CoreClient`. No test observes bytes returning through `App::on_key` to a rendered surface.

## 4. Invariants that must not regress

- **I1 — Bounded reads.** Output is read only through `InteractiveProcessResume` with an
  explicit `from_seq`. Use the daemon's own default (`max_bytes: None`); do not introduce a
  second byte bound, an unbounded read, or a "all output" convenience route. A cursor that
  falls behind must surface the typed resync the daemon already returns — never a silent shift
  of history.
- **I2 — No terminal bytes on the event bus.** Interactive output stays between the daemon,
  its owning attachment, and the view holding that attachment. No bus event, projection,
  transcript entry, session observation, or audit record may carry terminal bytes. The C003
  M003-header structural assertion (no output bytes in headers) must keep passing.
- **I3 — Key routing is not reopened.** Literal `Esc` is forwarded as a byte rather than
  consumed, `Ctrl-]` leaves focus, Insert claims focus, characters reach the PTY unchanged, and
  the panel's `i` gate stays. Do not "fix" liveness by touching classification.
- **I4 — Single cadence.** The pull rides the existing event-loop cadence (the same tick that
  already calls `service_editor_semantic_refresh`). No new timer thread, no new
  `tokio::time::interval`, no second spinner/indicator.
- **I5 — A poll may not supersede a user action.** Read bookkeeping is per attachment. A
  background read must not invalidate the in-flight completion of a typed input, resize,
  detach, or terminate. `dialog_state.terminal_request` must not be used for the pull.
- **I6 — Typed refusals stay typed.** Exited/gone/unknown attachment and client-mismatch
  outcomes remain the daemon's typed errors; the poll must not convert them into a silent retry
  loop, a toast storm, or a generic "terminal error" that hides the class.

## 5. Scope

### In scope

- A per-attachment output pull service on the existing event-loop cadence.
- Per-attachment in-flight/read bookkeeping that does not touch `terminal_request`.
- Resize delivery that no longer depends on the user typing.
- Repainting both surfaces (`refresh_terminal_dialog` for the box, the panel render path) when
  a chunk lands.
- Honest surface status text where the current text implies a live terminal.
- End-to-end regression evidence through `App::on_key` for both surfaces, demonstrated failing
  on the baseline first.

### Explicitly out of scope

- Any daemon-side change (PTY, ring, resync reasons, admission, registry, authorization).
- Any `CoreRequest`/`CoreResponse` variant, protocol, or wire change.
- WS-attach / server transport support — C002 owns it (`src/server/**`,
  `crates/codegg-client`, and the `protocol::tui::TuiMessage` channel).
- Removing or redesigning `/terminal`, `/terminal-list`, `/terminal-attach`,
  `/terminal-resume`. `/terminal-resume` stays as a manual recovery affordance.
- `codegg-core`, storage, schema, model, provider, permission, or scheduler surfaces.
- Persistence, scrollback export, or artifact upload of terminal bytes.
- Box/panel focus-parity ergonomics — C003 (polish).

## 6. Required production changes

### Core/domain

None. `crates/codegg-core` is untouched; `scripts/check-core-boundary.sh` must stay green.

### Storage and migrations

None.

### Protocol and DTOs

None. `InteractiveProcessResume`, `InteractiveProcessResumed`,
`InteractiveProcessResyncRequired`, and `CoreEvent::InteractiveProcessExited` are reused
verbatim.

### Runtime and concurrency

Required, in `src/tui/`:

1. **A per-attachment pull service** (naming: the repo's precedent is
   `service_editor_semantic_refresh`, called from the existing tick in
   `src/tui/runtime/event_loop.rs`). It should skip work unless a mounted surface holds an
   view with a live attachment and no read for that attachment is in flight. Mounted means the
   box dialog is open with a terminal shown, or the terminal panel is docked and visible.
2. **Per-attachment read state.** A small amount of state on the terminal view/controller —
   an in-flight marker plus the request id — rather than `dialog_state.terminal_request`. It
   must survive a detach/attach cycle cleanly and must drop the marker when the attachment or
   handle disappears.
3. **Reuse `start_terminal_resume` for the wire call**, or a sibling that takes an explicit
   request id. If it is refactored, `apply_terminal_resume_finished` keeps the existing
   `finish`/`fail` shape and gains the ability to accept a caller-supplied id so a poll and a
   manual `/terminal-resume` cannot collide on one slot.
4. **Resize drain decoupled from input.** `flush_terminal_resize` must be reachable from the
   same per-attachment service as the pull, so a docked terminal that the user never types into
   is still resized. Preserve the current ordering guarantee (no byte is dropped when input and
   resize are queued in the same frame).
5. **Surface repaint on chunk.** A landed chunk must reach the rendered buffer on both
   surfaces; the panel and the box must not each need a separate fix.
6. **Stale-completion discipline.** Follow `src/tui/async_cmd.rs`: a completion whose request
   is not current, or whose handle/attachment no longer exists, is discarded without applying.

### Frontend or operator surface

- Box: `refresh_terminal_dialog` is called when a chunk lands while the box is mounted.
- Panel: the panel render path reflects newly applied scrollback without requiring focus.
- Status text must state what the surface is actually doing. Where a view has no attachment,
  say so plainly instead of implying a live session; keep the existing recovery hints
  (`/terminal-attach`, `/terminal-resume`) discoverable.
- Panel focus remains opt-in through `i`; the panel must not silently steal the keyboard.

### Security and authorization

No change. C001 must not add identity, grant, capability, or scope; must not let the pull read
an attachment the creating client does not own (the daemon already enforces this, and the typed
refusal must stay typed); must not log terminal bytes; must not widen what the `authority`
check in `src/interactive_process_attach.rs` admits.

### Documentation and static guards

- `architecture/tui.md` §"Interactive Terminals (M003)": state the delivered liveness contract,
  the single cadence (I4), the per-attachment read state (I5), and that the read side is
  pull-only and bounded (I1).
- `docs/tui.md`: same contract in user-facing terms, and an explicit note that interactive
  terminals are socket-daemon-frontend only until C002.
- `.opencode/skills/tui/SKILL.md`: keep the hazard list current (single cadence, per-attachment
  read state, no terminal bytes on the bus).
- No new guard is strictly required. If the cadence or read-state rule can be pinned cheaply by
  an existing guard, prefer that over a new script; a new guard must prove it fails on an
  injected violation before being cited as evidence.

### False-claim ledger

The defect survived a long tail of input-side fixes because its closure evidence tested the
halves separately. The closure record must state additively, without editing those records:

- M003 closure §1's implication that a user can watch output while focused.
- C003's "VT presentation fidelity" title implying a rendered screen the user can drive.
- `architecture/tui.md` §"Interactive Terminals (M003)" and `docs/tui.md`, which describe the
  box/panel surfaces without stating that no read side existed.

## 7. Ordered work packages

### Work package A — Falsify the premise first

Intent: prove the defect and the acceptance signal before changing production code.

Required changes: write the end-to-end test (§10) and demonstrate it **failing on baseline
`719cfeb5`** for the right reason — output never appears after a typed command, on both
surfaces. Do not proceed on a test that cannot be made to fail.

Acceptance evidence: the failing run captured with its assertion message, for box and panel.

### Work package B — Per-attachment read state

Intent: establish the bookkeeping the pull needs without touching `terminal_request`.

Required changes: add the per-attachment in-flight marker and request id; keep
`dialog_state.terminal_request` exclusively for user-initiated operations; prove a poll in
flight cannot supersede a typed input completion (I5).

Acceptance evidence: a test that starts a read, then drives an input completion, and shows both
apply; plus a test that a superseded read completion is discarded.

### Work package C — The pull service on the existing cadence

Intent: read output while a surface is mounted, at the existing cadence only.

Required changes: call the pull from the existing event-loop tick; gate on mounted surface +
attached view + no in-flight read; use `max_bytes: None`; keep resync handling exactly as the
daemon returns it.

Acceptance evidence: the end-to-end test passes for box and panel; a cadence test shows the
pull rides the existing tick and adds no second timer.

### Work package D — Resize drain independent of typing

Intent: stop the docked panel's PTY from staying at its create-time size.

Required changes: drain the queued viewport resize from the same service, not only from the
input flush; preserve input+resize ordering within a frame.

Acceptance evidence: a test that docks a terminal, never types, and shows a resize request was
sent; plus a test that input and resize queued together both arrive.

### Work package E — Repaint and honest status text

Intent: a landed chunk is visible on both surfaces, and no surface text overstates liveness.

Required changes: repaint on chunk for box and panel; correct status text for the
no-attachment case.

Acceptance evidence: rendered-buffer assertions on both surfaces; a test asserting the
no-attachment text does not imply a live session.

### Work package G — Pre-existing box-focus defect (found during baseline falsification)

Intent: close a defect that predates this milestone but lives in the same surface and the
same test module, so C001 can leave `tui::commands::interactive_terminal` green without a
"pre-existing red" caveat.

Verified at baseline `719cfeb5`, not a regression: `cargo test -p codegg --lib
tui::commands::interactive_terminal::tests::pressing_i_on_the_open_box_enters_focused_input`
fails on the **untouched baseline** with the assertion `a freshly opened box starts viewing,
never focused`. The test body is byte-identical to the current revision, and `focus()` and the
`apply_attached` → `handle_terminal_show` routing are unchanged between revisions — the
baseline simply never satisfied its own documented contract.

**RESOLVED — this package's premise was wrong, and the stop condition fired.**
The box reaching `TerminalFocus::Focused` is deliberate (`01db38bd5`), not a defect: the
comment there states that requiring a second `i` "is what made the box feel broken", which is
this milestone's own symptom. The stale artefacts were the assertion and the `TerminalFocus`
doc comment, so both were corrected and the behaviour left alone. See closure §10 finding 2.

Acceptance evidence: that test passes, with the box starting in `Viewing` and `i` entering INS
mode; a note in the closure record recording it as a pre-existing defect distinct from C001's
liveness work.

### Work package F — Qualify and close

Intent: prove nothing regressed.

Required changes: run §11 in full; record in the closure record the falsification ledger from
the §6 false-claim ledger, additively, without editing the two predecessor closure records.

Acceptance evidence: §11 command results plus the closure record at
`plans/closure/interactive-terminal-liveness-post-closure-corrective/001-status.md`.

## 8. Failure, cancellation, restart, and contention semantics

- **Daemon unreachable:** the pull must fail closed and quietly (no toast storm). Surface the
  existing reconnecting state; do not retry-loop on the event-loop tick.
- **Process exits mid-poll:** the daemon returns its typed exited state; the view shows exited
  and stops polling that attachment. Input remains refused per the existing contract.
- **Detach/terminate while a read is in flight:** the completion is discarded (handle gone);
  the in-flight marker is cleared so the attachment does not wedge as permanently "polling".
- **Reattach:** a reattach creates a new attachment; the pull follows the new attachment's
  cursor, and the older attachment's late completion cannot be applied to it.
- **Rapid typing:** input and resize coalesce per frame; the pull never reorders or drops input
  bytes.
- **Cursor gap:** typed `InteractiveProcessResyncRequired` is rendered as a resync, not as
  silent history loss.
- **Multiple views on one attachment:** at most one read is in flight per attachment; a second
  view shares the landed chunk rather than issuing a duplicate read.

## 9. Compatibility and migration

No database, wire, config, tool-schema, or API change. No migration. The existing manual
`/terminal-resume` remains valid. No legacy path is removed, so no removal criteria apply.

## 10. Required tests

### Focused unit tests

- Per-attachment read state: begin, land, discard-if-superseded, clear-on-handle-gone.
- The pull service skips when: no view, no attachment, read in flight, surface not mounted.
- The pull uses the daemon default byte cap and the view's current cursor.
- Resize drain is reachable without an input flush.

### Integration tests

- **The acceptance signal (must fail on baseline):** for **each** of the box and the panel —
  create an attached interactive process, drive `App::on_key` to type a command that produces
  identifiable output, drive the cadence, and assert the output appears in the rendered buffer.
  Assert the marker text, not "something changed".
- **CORRECTED after baseline verification:** the panel focus gate still requires `i`
  before keys route. The box **also** requires `i` — that is the documented contract
  ("Opening a terminal always starts in `Viewing` so stray keystrokes keep going to the
  composer", `src/tui/interactive_terminal.rs:216`) — but the **product behaviour does not
  honour it**: a freshly opened, attached box comes up `Focused`, so the first `i` a user
  types is forwarded to the shell instead of entering INS mode. This is a pre-existing
  defect, not a C001 regression; see Work package G.
- Key routing is unchanged: literal `Esc` forwarded as a byte, `Ctrl-]` exits focus,
  Enter sends a newline, Backspace sends `0x7f`.

### Restart and recovery tests

- Reattach after disconnect: output continues from the retained scrollback.
- Daemon restart: the view shows the existing gone/reconnecting state and does not spin.

### Contention and cancellation tests

- A poll in flight does not supersede a typed input completion, and vice versa.
- A superseded read completion is discarded without applying.
- Two views on one attachment produce one read, not two.

### Security and negative tests

- A read for an attachment the client does not own returns the typed refusal and renders no
  bytes.
- Terminal output never appears in a bus event, projection, transcript, or session observation
  (keep the existing C003 M003-header structural assertion).

### Migration and compatibility tests

None.

## 11. Required verification commands

```bash
# focused first
cargo test -p codegg --test interactive_terminal_tui
cargo test -p codegg --test tui_panel_docking
cargo test -p codegg --lib tui::interactive_terminal
cargo test -p codegg --lib tui::commands::interactive_terminal

# guards touched by TUI ownership and cadence
python3 scripts/check_tui_transport_authority.py
python3 scripts/check_execution_ownership.py

# formatting and linting
cargo fmt --all -- --check
cargo clippy -p codegg --all-targets --locked -- -D warnings

# broader suite
bash scripts/verify.sh quick
```

Report locally run commands truthfully. CI is a strict subset of `quick` (see `AGENTS.md`), so a
green local `quick` is not a green CI run and must not be described as one.

## 12. Documentation updates

- `architecture/tui.md` §"Interactive Terminals (M003)": the delivered liveness contract; the
  single cadence (I4); per-attachment read state (I5); bounded pull-only reads (I1); and the
  observation boundary (I2).
- `docs/tui.md`: the same in user-facing terms, plus an explicit note that interactive terminals
  are socket-daemon-frontend only until C002.
- `.opencode/skills/tui/SKILL.md`: refresh the hazard list with the cadence, read-state, and
  no-bus rules.
- Do **not** edit `plans/closure/interactive-process-sessions/003-status.md` or
  `plans/closure/human-execution-corrective/003-status.md`. Record the correction additively in
  this milestone's closure record and in `plans/registry.md`.

## 13. Acceptance criteria

- A user types a command into the **box** and its output appears on that box, with no manual
  recovery action.
- A user focuses the **panel** with `i`, types a command, and its output appears in the panel
  column, with no manual recovery action.
- Nothing a user can observe depends on `/terminal-resume` for liveness.
- A docked terminal that the user never types into is resized to its panel geometry.
- Every read is bounded and cursor-tracked; a gap renders a typed resync.
- No terminal byte appears on the bus, in a projection, in the transcript, or in session
  observation.
- Key routing behaviour is byte-for-byte what it was on the baseline (I3).
- A freshly opened box starts in `Viewing`, and `i` enters INS mode in the box exactly as
  it does in the panel — the pre-existing defect from Work package G is closed, not waived.
- Exactly one cadence drives the refresh; no new timer or second indicator exists.
- A read in flight never drops a typed input, resize, detach, or terminate completion.
- The two predecessor closure records are unchanged, and this milestone's closure record states
  additively which of their claims were falsified and why the original verification missed them.

## 14. Stop conditions

Stop and report rather than improvise when:

- the acceptance test in Work package A cannot be made to fail on the baseline;
- the fix appears to require a new `CoreRequest`/`CoreResponse` variant or any protocol change;
- anyone proposes a push transport for terminal bytes (new ADR required);
- the pull cannot be built on the existing cadence without a second timer;
- the pull requires per-attachment state in `dialog_state.terminal_request` (I5);
- preserving bounded reads would mean an unbounded read or a second byte cap (I1);
- any requirement appears to need terminal bytes on the bus or in a projection (I2);
- work expands into C002's transport boundary (`src/server/**`, `crates/codegg-client`, WS
  protocol) or into `codegg-core`, storage, or the model/provider surface.

## 15. Closure evidence required

- The Work package A falsification run on baseline `719cfeb5`, with the assertion message, for
  both surfaces.
- End-to-end proof through `App::on_key` → daemon → rendered buffer for box and panel.
- Resize-without-typing proof.
- Supersession proof (I5) in both directions.
- The baseline-verified falsification for Work package G: the same assertion failing on
  `719cfeb5`, and passing after the fix without a test edit.
- Per-attachment single-read proof with two views.
- The I1–I6 invariant review, each with the test or guard that pins it.
- Full §11 command results, labelled local vs CI truthfully.
- The §6 false-claim ledger, recorded additively.
- An unblock audit of `plans/registry.md` (explicitly "nothing newly unblocked" is a valid
  result, but it must be stated).
- Unresolved findings with severity, including any low findings.

## 16. Handoff notes

- **Start at Work package A.** This defect shipped behind a green suite; the first task is
  proving the acceptance signal can fail, not writing the pump.
- `tests/interactive_terminal_tui.rs:83` (`poll_resume_text`) already contains a correct
  polling loop. Read it as the reference shape — but do **not** satisfy this milestone by
  calling the protocol directly from a test again; the loop must exist in production and the
  test must exercise it through `App`.
- The pump must reuse `start_terminal_resume`'s wire request verbatim; do not fork the request
  construction.
- `dialog_state.terminal_request` is shared by nine operations — check it before you touch any
  request-id handling.
- Rendering budgets: the terminal view renders bounded scrollback; do not grow it to
  accommodate a poll.
- Panel geometry is the resize source of truth; the create-time 80x24 is a placeholder, not a
  default to preserve.
- If a milestone here later decides on a push transport, stop: that is a new ADR, not a
  follow-on task.