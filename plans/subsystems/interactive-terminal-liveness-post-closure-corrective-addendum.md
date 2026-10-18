# Interactive Terminal Liveness Post-Closure Corrective Addendum

Status: **complete** (C001 conditionally closed — condition discharged by C003; C002 closed; C003 closed; C004 closed 2026-10-17)

Long-term references:
- `plans/000-long-term-specification.md#45-locality-by-default` — PTYs execute on the node owning the workspace; the coordinator receives **structured events and bounded output** rather than proxying local operation synchronously.
- `plans/000-long-term-specification.md#44-frontends-render-projections` — a frontend renders frontend-neutral snapshots and events and never becomes an independent owner of durable state.
- `plans/003-planning-process.md` §§3 (work classification), 5 (milestone sizing), 6 (agent handoff contract), 7 (corrective passes), 10 (required planning review).

Related ADRs:
- None required. C001 reuses the closed interactive-process contract (`attach`/`resume` with a sequence cursor and typed resync) exactly as accepted under `plans/subsystems/interactive-process-sessions-roadmap.md`. If any milestone here decides to introduce a **push** transport for terminal bytes, that is a new daemon/frontend protocol decision and requires a new ADR before implementation.

Predecessor closures this workstream corrects (records remain immutable):
- `plans/closure/interactive-process-sessions/003-status.md` (M003 — TUI interactive terminal integration).
- `plans/closure/human-execution-corrective/003-status.md` (C003 — PTY keyboard and VT presentation fidelity).
- `plans/subsystems/tui-panel-surface-convergence-corrective-addendum.md` (panel surface; terminal panel added after those closures).

## 1. Purpose and ownership boundary

CodeGG exposes an interactive workspace process in two frontend surfaces — the terminal **box**
(modal dialog) and the docked terminal **panel** — and in both, a user can type arbitrary
commands into a real PTY. This workstream owns the missing half of that contract: **making
process output produced after attachment actually reach the screen**, and keeping the two
surfaces honest about it.

Ownership boundary:

- **In this workstream:** the TUI frontend's read side for interactive-process output; the
  liveness/cadence contract between an attached view and the daemon's bounded ring; resize
  delivery independent of typing; and the user-visible status text on both surfaces.
- **Not in this workstream:** the daemon's PTY service, ring buffer, sequence/resync semantics,
  admission, or authorization (closed under `interactive-process-sessions` M001–M002); the
  key/byte classification and VT rendering primitives (closed under `human-execution-corrective`
  C003); and the daemon/socket vs. server/WS-attach transport matrix, which is C002.

## 2. Work classification

### Invariants

- **I1 — Bounded reads.** Interactive output is read only through the closed
  `InteractiveProcessResume` contract with an explicit `from_seq` cursor and the daemon's
  byte cap. No milestone may add an unbounded read, an "all output" convenience route, or a
  reader that silently shifts history; a cursor that falls behind is a typed resync, not a
  silent drop.
- **I2 — No terminal bytes on the event bus.** Interactive process output stays between the
  daemon, its owning attachment, and the TUI view that holds that attachment. It must never
  enter the global bus, session observation, the transcript, a projection, or an audit record.
  (Preserves the C003 M003-header structural assertion.)
- **I3 — Key routing is not reopened.** Literal `Esc` is forwarded as a byte rather than
  consumed, `Ctrl-]` leaves focus, focus is claimed only in Insert, and typed characters
  reach the PTY unchanged. C001 may not touch this to make liveness work.
- **I4 — Single cadence.** No milestone may add a second timer, thread, or ticker for terminal
  refresh. A terminal pull rides the existing event-loop cadence the working strip and spinner
  already use.
- **I5 — A poll may not supersede a user action.** Request-supersession state is per-operation;
  a background read may not invalidate the in-flight completion of a typed input, resize,
  detach, or terminate.

### Capabilities

- **K1 — C001 (conditionally closed).** Typing a command into the box or the panel produces
  visible output on that surface without any manual recovery action. **Falsified for the idle
  state** at baseline `18c3dca92`: the cadence arm is dark unless something else is animating the
  frame, so output appears only when the user presses another key. C003 owns the repair.
- **K2 — C002.** The interactive terminal is available on every supported frontend transport,
  or an honest typed refusal explains why it is not.
- **K3 — C003.** A mounted, live terminal surface keeps producing output while the user is idle,
  and the canonical mode badge and contextual hint line both report the surface that actually owns
  the keyboard.

### Infrastructure

- The per-attachment in-flight/read bookkeeping C001 introduces is infrastructure for K1. It
  is **not** a deliverable on its own and must not be presented as a capability.

### Polish

- Surface status text ("no output yet", "attach to follow", resize state) must not claim
  liveness the code does not deliver. Polish, and it is inseparable from K1 here because the
  false text is part of the reported defect. The same reasoning moved the canonical `[NOR]`/`[INS]`
  badge into C003 (K3) rather than polish: a badge that reports the wrong keyboard owner is false
  state text, not ergonomics.
- The hint row's visibility when the panel is unfocused, the Insert-mode cursor blink, the prompt
  line after a new or cleared terminal, and reader-cursor placement on `Esc` are C004 polish —
  correctly ordered after C003, since the blink is unimplementable before the loop is armed.

## 3. Non-goals

- No new interactive-process command surface, no new slash command that duplicates an existing
  one, and no `/terminal` redesign.
- No daemon-side change to the PTY, ring, resync reasons, admission, or process registry.
- No persistence of terminal output into session storage, no scrollback export, no artifact
  upload of terminal bytes.
- No claim that the WS-attach/server frontend supports interactive terminals under C001.
- No change to `codegg-core`, storage, schema, or the model/provider surface.
- No removal of the manual `/terminal-resume` escape hatch; it stays as a recovery affordance
  and as a test seam.

## 4. Current state (repository evidence)

At baseline `719cfeb5` on `feat/auto-20261017-8835f26d`:

- **The write path is complete and correct on both surfaces.** The box routes keys at
  `src/tui/app/mod.rs:4027` → `handle_terminal_key` → `classify_focused` →
  `send_terminal_input` (`src/tui/commands/interactive_terminal.rs:1461`, `:689`). The panel
  claims Insert and classifies on the same path
  (`src/tui/panels/terminal.rs:329-337`, `src/tui/app/input.rs:671`). The daemon receives
  `InteractiveProcessInput` (`src/core/daemon.rs:4113`) and writes to the PTY master
  (`src/interactive_process_attach.rs:792`).
- **The read path does not exist in production.** The TUI builds exactly two output-bearing
  requests: `InteractiveProcessAttach` (`commands/interactive_terminal.rs:409`) and
  `InteractiveProcessResume` (`:593`). `start_terminal_resume` has exactly one caller — the
  `/terminal-resume` slash command (`src/tui/app/mod.rs:8176`). There is no timer, no
  render-cadence hook, and no post-input read; `flush_terminal_input` drains input and a
  pending resize only (`:703-704`). The daemon cannot push: its only interactive event is
  `CoreEvent::InteractiveProcessExited`, which is bytes-free by contract
  (`crates/codegg-protocol/src/core.rs:4124-4127`).
- **Consequence.** A freshly created shell writes its prompt after the TUI's attach request
  has already returned, and nothing ever reads again, so both surfaces render a permanently
  empty view while the daemon holds the output in its ring. This reproduces the reported
  symptom (typed commands appear to do nothing) with no error and no toast.
- **The daemon ring and reader task are healthy**, so the capability is recoverable by
  polling alone: `src/interactive_process.rs:1073` pumps PTY output into a sequenced ring, and
  `interactive_process_attach.rs:846` implements bounded resume with typed resync.
- **Resize is coupled to typing.** The panel queues a viewport resize during render
  (`src/tui/panels/terminal.rs:185`) and the only drain is `flush_terminal_resize`, reachable
  only through `flush_terminal_input`. A docked terminal the user never types into is never
  resized and keeps the 80x24 size applied at create (`commands/interactive_terminal.rs:260`),
  so the reader would see wrapped, misaligned output once C001 lands. This must be fixed with
  the pull, not after it.
- **Existing tests cannot catch this.** `tests/interactive_terminal_tui.rs:83` defines
  `poll_resume_text`, a real 25 ms polling loop, and `tui_terminal_full_lifecycle_over_cat`
  (`:177`) proves the round trip — but it calls `protocol.resume` **directly on the protocol,
  outside `App`**. The production poll loop the test supplies does not exist.
  `tests/tui_panel_docking.rs:1516` asserts only that a key reaches the send seam, observing
  the "terminal input dropped" toast from an app with no `CoreClient`. No test observes bytes
  returning through `App::on_key` to a rendered surface.
- **Transport split (C002 scope).** Every terminal command reads `app.core_client`
  (`:78`, `:284`, `:390`, `:577`, `:711`, `:818`), which is `None` for the WS-attach
  frontend, and `src/server/**` has no `InteractiveProcess` routing at all — that transport
  speaks `protocol::tui::TuiMessage`, not `CoreRequest` envelopes.
- **Supersession hazard for C001.** `dialog_state.terminal_request` is a **single-slot**
  `AsyncUiRequestState` shared by all nine terminal operations
  (`src/tui/app/state/dialog.rs:169`, `src/tui/app/state/async_request.rs:48`). A poll that
  calls `begin()` on a cadence would supersede an in-flight input/resize/detach/terminate
  completion, whose `finish(request_id)` would then return `false` and silently drop it
  (I5).
- **A pre-existing defect in the same surface, verified at baseline.** At `719cfeb5` the test
  `tui::commands::interactive_terminal::tests::pressing_i_on_the_open_box_enters_focused_input`
  **fails on the untouched baseline**, asserting `a freshly opened box starts viewing, never
  focused`. The controller's own contract agrees (`src/tui/interactive_terminal.rs:216`:
  "Opening a terminal always starts in `Viewing` so stray keystrokes keep going to the
  composer"), so a freshly opened, attached box comes up `Focused` in violation of it, and the
  first `i` a user types is forwarded to the shell instead of entering INS mode. The panel
  behaves correctly. The test body, `focus()`, and the `apply_attached` →
  `handle_terminal_show` routing are byte-identical across revisions, so this is **not** a
  C001 regression — it predates the milestone and is owned by C001 Work package G so the
  module can end green without a waived red.

## 5. Target architecture

Interactive output stays **pull-based and bounded**, on the contract already accepted in M002:

```text
event-loop cadence (existing tick)
        │
        ▼
service_terminal_output_pull(app)      one shared cadence (I4)
        │  skip unless: a mounted surface holds an attached view,
        │               and that attachment has no read in flight (I5)
        ▼
start_terminal_resume(handle)          reuse the existing command unchanged
        │  per-attachment in-flight marker + request id (NOT terminal_request)
        ▼
CoreRequest::InteractiveProcessResume { attachment_id, from_seq, max_bytes }
        │
        ▼
TuiCommand::TerminalResumeFinished { request_id, handle, chunk, resync, error }
        │
        ▼
apply_resumed → view scrollback → box render and/or panel render
```

Rules this shape encodes:

- One cadence, the existing one (I4). No new thread, no new `tokio::time::interval`.
- Polling only while a surface is actually mounted, so a closed box and an undocked panel
  cost nothing.
- Exactly one in-flight read per attachment, tracked separately from
  `dialog_state.terminal_request` (I5).
- Empty chunks are cheap: the ring answers `Resumed` with an empty chunk for an unchanged
  cursor, so an idle shell costs one bounded request per cadence and no bytes.
- The daemon stays a pull-only provider. A push transport is out of scope and ADR-gated.

## 6. Dependency graph

| Dependency | Kind | State |
|---|---|---|
| `interactive-process-sessions` M001–M003 | hard (interface) | closed; its attach/resume/resync contract is stable and is reused as-is |
| `human-execution-corrective` C003 | hard (interface) | closed; key classification and VT rendering are reused unchanged |
| `tui-panel-surface-convergence` M001 | interface | closed; the panel's keymap/chrome ownership is reused unchanged |
| Existing event-loop tick + `spawn_registered_tui_task` + `AsyncUiRequestState` semantics | interface | stable, in-tree |
| WS-attach / server transport | hard, **C002 only** | partially present: `/core` already routes `InteractiveProcess*`; `/tui` (the WS-attach TUI frontend) does not and will not in C002. C002 delivers the typed refusal instead |

No dependency of C001 is open. C001 is dependency-ready.

## 7. Milestones

### C001 — Live output pull and terminal liveness closure

Class: **capability** (K1), carrying the I1–I5 invariants and the resize drain.

Objective: make a typed command in the box or the panel produce visible output, with no
manual recovery action, on the socket-daemon frontend.

Deliverable boundary: TUI frontend only — a per-attachment pull service on the existing
cadence, per-attachment in-flight bookkeeping, an input-independent resize drain, honest
surface status text, and end-to-end regression evidence through `App::on_key`.

Exit conditions: every acceptance criterion in
`plans/implementation/interactive-terminal-liveness-post-closure-corrective/001-live-output-pull-and-terminal-liveness-closure.md`
§13, with closure at
`plans/closure/interactive-terminal-liveness-post-closure-corrective/001-status.md`.

**Closed.**

### C002 — Interactive terminal on the remaining frontend transports

Class: **capability** (K2).

Objective: either route interactive-process requests/responses over the WS-attach frontend
and the server, or fail with a typed, honest refusal that names the transport.

**Opened 2026-10-17. Option B selected. Closed 2026-10-17.**

Research into the transport decision (baseline `8b6191140`) found that C002's original premise
was incomplete, and the finding changed the milestone's shape:

- The server exposes **three** WS endpoints, not one. `/core` speaks
  `CoreFrame::Request(RequestEnvelope<CoreRequest>)`
  (`crates/codegg-protocol/src/frames.rs:181`) and `handle_core_frame` has **no variant
  allowlist** — its only gates are a five-variant `is_secret_bearing()` check that does not cover
  interactive processes, and projection-specific checks matching only `Projection*` variants. It
  calls `daemon.handle_request_for_client(envelope, client_id)` with the authenticated connection
  id (`src/server/ws.rs:4429`). So the interactive family is **already routable over `/core`**;
  there is no missing route to add.
- The addendum's original claim — that "the server frontend has no `InteractiveProcess` routing at
  all" — is therefore true of `/tui` only, and remains the accurate description of the WS-attach
  TUI frontend: `src/client/attach.rs` opens `/tui`, speaks `protocol::tui::TuiMessage`, and never
  constructs a `CoreClient`, which is why every terminal command's `core_client` guard fires.
- C002 therefore splits in two, and only one half belongs here.

**Selected: Option B — the typed refusal.** The nine terminal refusal sites already exist but all
emit "Daemon connection unavailable; …", which is **false** on the WS-attach frontend: the daemon
is healthy and serving every other surface. A refusal that names a healthy component sends the
operator to debug the wrong thing — the same defect class C001 closed, in the diagnostic instead
of the render. C002 makes the refusal name the transport and the real cause, and records the
boundary precisely.

**Not selected, and why.** Option A (routing terminal bytes) requires answering whether a remote
principal may obtain a host shell. That is an authorization-contract change requiring a new ADR,
and giving the TUI-attach frontend a `CoreClient` would affect every `core_client`-based command,
not just terminal — far beyond a bounded milestone. ADR-0010 already names `/core` the
remote-client protocol, so Option A is architecturally plausible; it is simply not this
milestone's scope.

**Explicitly out of this milestone.** `/core`'s interactive-process authorization posture is a
separate registered workstream: `plans/subsystems/remote-interactive-process-authorization-corrective-roadmap.md`.
C002's closure must not claim that remote principals cannot create interactive processes.

Plan: `plans/implementation/interactive-terminal-liveness-post-closure-corrective/002-honest-transport-refusal-and-capability-boundary.md`.
Closure: `plans/closure/interactive-terminal-liveness-post-closure-corrective/002-status.md`.

**Delivered.** One shared classifier (`TerminalRefusal` — `TransportUnsupported` vs
`NoDaemonConnection`) now owns the wording, and all nine entry points route through
it via `terminal_core_client`, so the two causes can no longer share one sentence.
The connection wording is preserved byte-for-byte where it is true. K2's exit
condition is met on its second half, so this is closure rather than a deferral.

Implementation also found a defect the plan did not anticipate, in the same silent
class: `flush_terminal_input` classified *after* `take_pending_input` and
`begin_input`, so a refused flush dropped the queued keystrokes **and** left
`input_in_flight` permanently set — one visible toast, then silence for the life of
the attachment. It was unreachable on the attach frontend, which is why it
survived. Fixed, with the drained resize restored to its queue on refusal.

The frontend boundary is now stated in `architecture/tui.md`, `docs/tui.md`, and the
TUI skill. As required, none of them claims that remote principals are prevented from
creating interactive processes; `/core` still routes that family and R001 owns it.

### C003 — Live-surface cadence arming and honest mode state

Class: **capability** (K1), corrective against C001's strict closure claim.

**Reclassified 2026-10-17.** C003 was previously scoped as the polish slot ("any residual
ergonomics surfaced by C001"). That classification was wrong, and the research that produced this
milestone is the reason. At baseline `18c3dca92` K1 is **not** delivered in the state a docked
terminal spends most of its life, and the residue is not ergonomics — it is a false capability
claim plus a false mode claim, both of which `plans/003-planning-process.md` §3 treats as
correctness rather than polish.

**Finding 1 — the pull is reachable only from a dark arm.**
`service_terminal_output_pull` has exactly one call site, inside the timed arm of the `select!`
at `src/tui/runtime/event_loop.rs:457`. That arm runs only when `next_wake` is `Some`
(`:288-292`), which requires an animation source. When `!streaming_active`,
`render_interval` is `Duration::ZERO` (`:150-154`) and `render_delay_after(last_render, ZERO)`
returns `None`, because `elapsed < ZERO` is never true; `animation_interval` is `Some` only while
a turn is streaming, a turn is working, or a toast is alive (`:262-264`). With `next_wake == None`
the arm parks on `futures_util::future::pending()` (`:423-428`) indefinitely. So a mounted, live
terminal surface is read only while a toast is alive, a keystroke arrives, or a resize is
debouncing — and a command's output appears only when the user presses the next key. Reported as
"exceptionally laggy and slow, to the point that I thought it was not actually working". A fresh
shell's `bash-3.2$` prompt is subject to the same defect, which is the "new terminal is blank"
half of the report.

**Why C001's evidence did not catch it.** The C001 acceptance tests call the pull **directly** —
`service_terminal_output_pull(&mut app)` at
`src/tui/commands/interactive_terminal.rs:2975` — so they prove the pull issues a bounded read,
that the completion applies, and that both surfaces repaint. They do not prove the loop arms it,
because the test harness supplies the polling loop the production loop withholds.
`tests/interactive_terminal_tui.rs:83` likewise calls `protocol.resume` outside `App`. No test in
the tree drives the real `select!` with a live surface and an idle frame budget. C001's closure
record §2 row "Bounded pull on the existing cadence, no second timer" and §4 "Both surfaces
repaint on chunk" are therefore not established for the idle state. Per `plans/registry.md`
maintenance rule 6, C001 moves to **conditionally closed**; its closure record is **not** edited.

**Finding 2 — the canonical badge cannot report the terminal's INS state.** The badge reads
`ui_state.input_mode` directly (`src/tui/app/render.rs:1929-1940`); `focus_panel` forces
`input_mode = Normal` whenever a panel takes the keyboard
(`src/tui/commands/panels.rs:692-702`); `focus_terminal_panel`/`blur_terminal_panel` mutate only
the controller and never touch `ui_state.input_mode`; and the only writer of `InputMode::Insert`
is the composer's `i` (`src/tui/app/mod.rs:3987-3991`), which a focused terminal panel
pre-empts. The badge is pinned to `[NOR]` in both modes and the row reads
`[NOR] [build] [TERM]` while the user types into a shell. The same cause makes the INS branch of
`TerminalPanel::composer_hints` (`src/tui/panels/terminal.rs:409`) **unreachable**, so the hint
line describes reader navigation precisely while the user is typing. This is false state text,
which §2 already classifies as inseparable from K1 ("must not claim liveness the code does not
deliver").

Objective: make the cadence reachable while the user is idle, and make one derived mode answer
feed both the badge and the hint line.

Deliverable boundary: TUI frontend only — an arming change in the event loop that reuses the
existing tick, one derived "effective input mode" accessor consumed by both surfaces, and
end-to-end evidence driven through the **real** event loop. No transport, no protocol variant, no
storage, no bus event, and no change to what a key means.

Exit conditions: every acceptance criterion in
`plans/implementation/interactive-terminal-liveness-post-closure-corrective/003-live-surface-cadence-and-honest-mode-state.md`
§13, with the Work package A baseline failure recorded before any production change.

Plan: `plans/implementation/interactive-terminal-liveness-post-closure-corrective/003-live-surface-cadence-and-honest-mode-state.md`.

### C004 — Terminal surface ergonomics

Class: **polish**.

Objective: the affordances the earlier milestones left — the contextual hint row when the panel is
unfocused, an Insert-mode cursor blink, a usable prompt line for a new or cleared terminal, and
reader-cursor placement when leaving Insert. The polish work C003 previously held.

**Unblocked 2026-10-17** by C003's closure, which delivered the armed cadence this milestone's
blink depends on; the loop now wakes for a mounted surface, so a time-varying render has a frame
to land on. The dependency was technical rather than conventional: a blink is a time-varying
render, and the loop only repaints while something arms it, so before C003 a blinking cursor
would have advanced only while a toast or keystroke happened to drive the frame — a worse symptom
than no blink. The hint-row change is also a layout change to the same
`composer_hint_text` and reserved-row arithmetic C003's derived-mode work touches; doing both in
one milestone makes a one-row reflow indistinguishable from a mode-reporting regression.

Two decisions are recorded here rather than left implicit:

- **The unfocused hint row.** `TerminalPanel::composer_hints` returns `Some` even when unfocused
  (`src/tui/panels/terminal.rs:397-414`), where every other panel returns `None`, so the TERM line
  persists above a composer the terminal no longer owns. This reverses the seam's own documented
  rationale (`src/tui/panels/mod.rs:1519-1527`: "a docked panel names itself and the gesture that
  enters it"). C004's default is to return `None` when unfocused and narrow the seam's contract
  rather than delete it; the seam documentation must be updated in the same change.
- **Clear.** `clear_scrollback` wipes the emulator locally and tells nothing
  (`src/tui/interactive_terminal.rs:1482-1504`), so the shell never re-prints `PS1` and the panel
  stays blank. C004's default is to drive the shell's own clear through the existing typed input
  path, because synthesizing a prompt fabricates a line the process never emitted — the same
  reasoning that already forbids manufacturing a resync.

`escape` (`src/tui/interactive_terminal.rs:1440-1451`) never moves `nav`, so leaving Insert leaves
the reader cursor wherever `sync_viewport` clamped it — the bottom of live output. C004 owns the
placement rule and must record which rule it chose for a multi-line output block.

**Closed 2026-10-17** at `plans/closure/interactive-terminal-liveness-post-closure-corrective/004-status.md`,
all four items delivered, each falsified on baseline first. The two decisions recorded above were
taken as written (the unfocused row returns `None`; the shell's own clear is driven rather than a
prompt synthesized), and the placement rule resolved as **the last row that has content, or row 0
when nothing has** — superseding the plan's default wording of "the line above the last one",
which trades one wrong place for another on the common case of a prompt sitting on row 0 of an
otherwise blank screen.

Plan: `plans/implementation/interactive-terminal-liveness-post-closure-corrective/004-terminal-surface-ergonomics.md`.

## 8. Cross-cutting requirements

- **Storage:** none. No migration, no schema change.
- **Protocol:** none for C001. `CoreRequest`/`CoreResponse` are unchanged; no new variant.
- **Security/authorization:** unchanged. The daemon's attachment resolution already binds a
  read to the creating client; C001 introduces no new identity, grant, or scope. C001 must not
  weaken the typed refusals (exited/gone/unknown attachment) that make the poll safe to retry.
- **Concurrency:** one in-flight read per attachment; per-view or per-attachment request ids;
  no shared mutable cursor across views; a completion whose attachment or handle is gone is
  discarded, not applied.
- **Observability:** no new bus event for terminal bytes (I2). A terminal that cannot be read
  must be visible as surface state, not as telemetry.
- **Docs:** `architecture/tui.md` §"Interactive Terminals (M003)", `docs/tui.md`, and
  `.opencode/skills/tui/SKILL.md` must describe the delivered liveness contract and must stop
  describing a live terminal on transports that have none.

## 9. Verification strategy

- **Falsification first.** Each C001 regression test must be demonstrated **failing on
  baseline `719cfeb5`** for the right reason before the fix lands. A test that cannot be made
  to fail means the premise is wrong; stop and report rather than close around it (the rule the
  agent-recovery C001 handoff states, and the reason this defect shipped).
- **Composition, not halves.** The acceptance signal is `App::on_key` → daemon → rendered
  buffer, for both surfaces. Protocol-level and controller-level tests are necessary but are
  not the acceptance signal; `poll_resume_text` must not remain the only place a poll loop
  exists.
- **No weakening.** The existing panel/render/interactive-terminal suites must pass unmodified
  unless a test asserted the defect itself, in which case the replacement and the reason are
  recorded.
- **Guards.** `scripts/verify.sh quick` plus the TUI-authority guard; any new cadence must be
  visible to the one-cadence rule rather than smuggled in.

## 10. Risks and decision points

| Risk / decision | Disposition |
|---|---|
| A poll that shares `dialog_state.terminal_request` silently drops typed input completions | C001 must use per-attachment bookkeeping; pinned by test (I5) |
| Panel resize never drains when the user does not type, so liveness ships wrapped output | Owned by C001, not deferred |
| Cadence cost on an idle shell | Bounded by design: one empty-chunk request per cadence only while a surface is mounted; the ring answers `Resumed` for an unchanged cursor |
| An agent decides the pull "should" be a push | Stop condition. Push is a new daemon/frontend protocol decision and requires a new ADR |
| C001 pulls a `max_bytes` default that exceeds the daemon cap | Use the daemon's default (`max_bytes: None`); do not introduce a second bound |
| WS-attach parity gets bundled into C001 | Prohibited. C002 owns the transport boundary; C001 stops at the socket-daemon frontend and says so |
| Option A (routing terminal bytes) gets bundled into C002 | Prohibited. It is ADR-gated and belongs to a new milestone; C002 delivers the typed refusal (Option B) only |
| C002's closure is read as "remote principals cannot get a PTY" | Prohibited. C002 closes the **frontend** half only; `/core` already routes `InteractiveProcess*`, tracked by `plans/subsystems/remote-interactive-process-authorization-corrective-roadmap.md` |
| The pull service is reachable from a test but not from the loop | **This is why C001's strict claim failed.** A cadence service called directly by a test proves the function, not the wiring; the harness supplied the loop the production loop withholds. C003 Work package A must drive the real `select!` and fail on baseline first |
| Arming the cadence grows into a second timer, thread, or interval | Prohibited by I4. Reuse the existing tick. Stop and report rather than add a cadence |
| Arming the loop turns every mounted terminal into an unbounded poller | The arm reuses `mounted_terminal_surface`, whose visibility rule is what makes an unmounted surface cost nothing, and the one-in-flight-read gate bounds request rate. Re-deriving liveness from `link == Live` at the loop is the mistake its doc comment warns against |
| The mode badge is "fixed" by writing `ui_state.input_mode` from render | Prohibited. The badge must read a derived answer; render may not move the keyboard. `Route::Editor` (`src/tui/app/render.rs:767-788`) is the precedent |
| The derived mode fix reopens what a key means | Prohibited by I3. Reporting mode is in scope; changing key classification is not. Stop and report |
| C003's reclassification is read as reopening closed polish scope | Not a reopening. C003 was never opened; its *classification* was wrong. Polish moves to C004 and stays correctly ordered after correctness (`plans/003-planning-process.md` §3) |
| C004's blink is implemented before C003 closes | Prohibited and technically unsound: with the loop parked, the cursor would advance only while a toast or keystroke drove the frame |
| C004 fabricates a prompt line on clear | Prohibited without an explicit recorded decision. The default is to drive the shell's own clear; a synthesized prompt is a product decision, not an implementation detail |
| C004's hint-row change silently alters other panels | Prohibited. The shared seam (`src/tui/panels/mod.rs:1519-1527`) is documented; overturning it requires updating that documentation in the same change |

## 11. Completion definition

This workstream completes when a user, on the socket-daemon frontend, can create an
interactive terminal, type arbitrary commands, and see the resulting output in the box and in
the docked panel without any manual recovery action **and without typing another key to
provoke it** (K1, as repaired by C003); when that same surface keeps producing output while the
user is idle, and when the canonical mode badge and contextual hint line agree about which
surface owns the keyboard (K3); when every terminal surface states what it is actually doing;
when no milestone here has left the read side unreached; and when K2 is either delivered or
recorded as an explicit, typed transport refusal. Every acceptance signal must be driven through
the **real** event loop — §9's "the harness must not supply the loop the production loop
withholds" is now a standing requirement of this workstream, not only of C003.

Closure also requires the C004 polish boundary to be either delivered or explicitly deferred
with a recorded reason, so that no part of the reported user-facing surface is left silently
unowned. The closure record must additively record that C001 falsifies the liveness implication in
the two predecessor closure records named in §Predecessor closures **and in C001's own strict
claim**, without editing those records.

## 12. Milestone status table

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| C001 | **conditionally closed** — condition **discharged** by C003's closure 2026-10-17 (strict liveness claim was falsified 2026-10-17) | `plans/implementation/interactive-terminal-liveness-post-closure-corrective/001-live-output-pull-and-terminal-liveness-closure.md` | `plans/closure/interactive-terminal-liveness-post-closure-corrective/001-status.md` | none; the record is **not** edited — the correction is additive, per registry rule 6, and C003's closure discharges the condition rather than reopening it |
| C002 | closed (Option B — honest typed refusal) | `plans/implementation/interactive-terminal-liveness-post-closure-corrective/002-honest-transport-refusal-and-capability-boundary.md` | `plans/closure/interactive-terminal-liveness-post-closure-corrective/002-status.md` | none; no ADR required — C002 delivered no transport |
| C003 | **closed 2026-10-17** (reclassified from polish to capability 2026-10-17) | `plans/implementation/interactive-terminal-liveness-post-closure-corrective/003-live-surface-cadence-and-honest-mode-state.md` | `plans/closure/interactive-terminal-liveness-post-closure-corrective/003-status.md` | none; conformance against the closed event loop and panel-key interfaces. No ADR required; no second timer (I4); no terminal bytes on the bus (I2) |
| C004 | **closed 2026-10-17** | `plans/implementation/interactive-terminal-liveness-post-closure-corrective/004-terminal-surface-ergonomics.md` | `plans/closure/interactive-terminal-liveness-post-closure-corrective/004-status.md` | none; both former blockers were discharged by C003's closure — the loop is now armed for a mounted surface, and the derived-mode work has landed. No ADR required. The only item left open is the box's viewport plumbing, deferred with a recorded reason in the closure record |