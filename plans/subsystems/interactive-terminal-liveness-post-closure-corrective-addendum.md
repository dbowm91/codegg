# Interactive Terminal Liveness Post-Closure Corrective Addendum

Status: closed (C001 closed; C002–C003 deliberately unopened)

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

- **K1 — C001.** Typing a command into the box or the panel produces visible output on that
  surface without any manual recovery action.
- **K2 — C002.** The interactive terminal is available on every supported frontend transport,
  or an honest typed refusal explains why it is not.

### Infrastructure

- The per-attachment in-flight/read bookkeeping C001 introduces is infrastructure for K1. It
  is **not** a deliverable on its own and must not be presented as a capability.

### Polish

- Surface status text ("no output yet", "attach to follow", resize state) must not claim
  liveness the code does not deliver. Polish, and it is inseparable from K1 here because the
  false text is part of the reported defect.

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
| WS-attach / server transport | hard, **C002 only** | absent; blocks C002, does not block C001 |

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

Deliberately **not opened**. It crosses a transport boundary (`src/server/**`, the WS
protocol, `crates/codegg-client`) that C001 must not touch, and it needs an explicit decision
about whether terminal bytes may traverse the WS channel at all — a question with I2
implications. Plan the milestone before scheduling it.

### C003 — Terminal surface polish follow-ups

Class: **polish**.

Objective: any residual ergonomics surfaced by C001 — hint-row wording, focus-ring parity
between box and panel, scrollback affordances.

Deliberately **not opened**, and ordered after C001 and C002 per `plans/003-planning-process.md`
§3 (polish follows functional and correctness closure).

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

## 11. Completion definition

This workstream completes when a user, on the socket-daemon frontend, can create an
interactive terminal, type arbitrary commands, and see the resulting output in the box and in
the docked panel without any manual recovery action (K1); when every terminal surface states
what it is actually doing; when no milestone here has left the read side unreached; and when
K2 is either delivered or recorded as an explicit, typed transport refusal. The closure record
must also additively record that C001 falsifies the liveness implication in the two predecessor
closure records named in §Predecessor closures, **without editing those records**.

## 12. Milestone status table

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| C001 | closed | `plans/implementation/interactive-terminal-liveness-post-closure-corrective/001-live-output-pull-and-terminal-liveness-closure.md` | pending | none |
| C002 | not opened | — | — | needs a transport-scope decision; not planned |
| C003 | not opened | — | — | polish, ordered after C001/C002 |