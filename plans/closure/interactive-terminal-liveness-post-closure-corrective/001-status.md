# Interactive Terminal Liveness Post-Closure Corrective C001 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/interactive-terminal-liveness-post-closure-corrective/001-live-output-pull-and-terminal-liveness-closure.md`
Source subsystem roadmap: `plans/subsystems/interactive-terminal-liveness-post-closure-corrective-addendum.md#c001--live-output-pull-and-terminal-liveness-closure`
Repository baseline reviewed: `719cfeb5` (`feat/auto-20261017-8835f26d`)
Implementation commits:

- `d4b70c72d` — `feat(tui): pull live PTY output on the existing event-loop tick (C001, WIP)`. The pull service, per-attachment read state, and resize-drain decoupling. Committed mid-milestone with an explicit `Status: NOT closed` body after the acceptance signal still failed.
- `66f67f229` — `fix(tui): make the terminal surface actually round-trip a command (C001)`. The frame-render fix, the two keyboard-authority fixes, input serialization, the stale-test reconciliation, and the documentation updates.

## 1. Executive finding

A user could type a command into the terminal box or the docked panel and see
nothing come back. The reported symptom was one thing; the cause was **four
independent defects stacked behind it**, and every one of them is silent — no
error, no toast, and no keystroke reported as lost.

1. **No read side existed in production.** The TUI constructed exactly two
   output-bearing requests, and `start_terminal_resume` had exactly one caller:
   the `/terminal-resume` slash command. Nothing read the PTY after attach, so
   the daemon held every byte in its ring while both surfaces rendered an empty
   view indefinitely.
2. **The box showed blank space even once bytes arrived.** It mounted the
   emulator's entire 24-row screen as scrollable content and followed the end of
   it. A fresh shell's prompt sits on row 0, so the window was pinned over the
   empty tail — indistinguishable from a terminal that accepted a command and
   produced nothing.
3. **The dock gesture ate ordinary shell letters.** `H`/`J`/`K`/`L` and
   Shift+arrows are the panel-docking gesture. Every other surface gates that
   gesture on "nothing is being typed"; the two terminal surfaces did not. In
   INS, typing a command containing an uppercase `H`, `J`, `K` or `L` docked the
   box into the panel instead of reaching the PTY.
4. **Input raced itself.** Each keystroke spawned its own concurrent daemon
   request, so fast typing could arrive scrambled. Separately,
   `queue_resize` re-queued unconditionally on every render frame while the new
   cadence drained it every tick, so the PTY resized continuously — and a PTY
   resize flushes tty pending input, which drops the half-typed word.

Defects 3 and 4 predate this milestone and were sufficient on their own to make
the surface read as broken. Defect 1 is the one the milestone was chartered to
fix; it was necessary but not sufficient.

## 2. Requirement-to-evidence matrix

| Plan requirement | Evidence | Result |
|---|---|---|
| Work package A: acceptance test fails on baseline | `box_rendered_buffer_shows_live_pty_output` and `panel_rendered_buffer_shows_live_pty_output` demonstrated failing on `719cfeb5` before any production change | pass |
| Bounded pull on the existing cadence, no second timer | `service_terminal_output_pull` called from the existing pump tick; byte-identical `InteractiveProcessResume` with `max_bytes: None` and the view's cursor | pass |
| Per-attachment read state, not the shared slot | `TerminalResumeRequest::{User, Pull}`; the pull never touches `dialog_state.terminal_request` | pass |
| Resize drain independent of typing | `flush_terminal_resize` called from the cadence service; a docked terminal the user never types into is resized | pass |
| Repaint on chunk, both surfaces | both acceptance tests assert marker text in the composed rendered buffer | pass |
| Bounding the resize storm (found during implementation) | `queue_resize` no-ops on an unchanged size | pass |
| Serialized, ordered input (found during implementation) | `begin_input`/`finish_input` single slot; ordering asserted in the rendered projection | pass |
| Dock gesture must not preempt the keyboard (found during implementation) | `claims_panel_directions` hook plus the `DialogType::Terminal` pre-check in `App::on_key` | pass |
| Work package G: pre-existing red test resolved | `the_box_opens_in_insert_while_the_docked_panel_still_needs_i` | pass, with the contract **inverted** from the plan's premise — see §10 finding 2 |
| No existing test weakened | no test was deleted or relaxed; one stale assertion was replaced with a stricter two-surface test | pass |
| I2: no terminal bytes on the bus | no bus event, projection, transcript or observation change; the C003 M003-header structural assertion still passes | pass |
| C002 boundary respected | `src/server/**`, `crates/codegg-client/**`, the WS protocol, `codegg-core`, storage, schema, providers, permission and scheduler are untouched | pass |

## 3. Production implementation evidence

| Change | Location |
|---|---|
| Cadence pull service, gated on a mounted surface | `src/tui/commands/interactive_terminal.rs` (`service_terminal_output_pull`, `mounted_terminal_surface`) |
| Tick wiring — no new timer | `src/tui/runtime/event_loop.rs` |
| Per-attachment read identity | `src/tui/interactive_terminal.rs` (`TerminalReadState`, `begin_output_read`, `finish_output_read`, `note_read_refusal`) |
| Typed owner separating user actions from background reads | `TerminalResumeRequest::{User, Pull}` |
| Trailing-blank trim so the window sits over the used rows | `refresh_terminal_dialog` |
| Dock gesture gate, box | `App::on_key`, `DialogType::Terminal` pre-check |
| Dock gesture gate, panel | `SidePanel::claims_panel_directions` + `SidePanelHost::handle_key` + `TerminalPanel::claims_panel_directions` |
| Serialized input | `begin_input` / `finish_input` / `input_in_flight` on the view |
| Resize no-op on unchanged size | `InteractiveTerminalController::queue_resize` |
| Reset on lifecycle edges | attach re-arms the read slot; detach/terminate/remove release it |

## 4. Verification executed

All commands below were run **locally in this worktree** on macOS. Nothing was
run on hosted CI, and no Windows, Desktop E2E, or real-LSP evidence was
obtained. Per `AGENTS.md`, CI is a strict *subset* of `verify.sh quick`, so a
green local `quick` is **not** a green CI run and is not presented as one.

```text
cargo test -p codegg --lib tui::commands::interactive_terminal   17 passed; 0 failed
cargo test -p codegg --lib tui::interactive_terminal             48 passed; 0 failed
cargo test -p codegg --lib tui::panels                           40 passed; 0 failed
cargo test -p codegg --test tui_panel_docking                    52 passed; 0 failed
cargo test -p codegg --test interactive_terminal_tui             (not re-run this pass; daemon-side
                                                                   suite, unmodified by this milestone)
cargo fmt --all && cargo fmt --all -- --check                    pass
cargo clippy -p codegg --all-targets --locked -- -D warnings     pass
bash scripts/verify.sh quick                                     "Quick verification passed."
```

**Falsification evidence (Work package A), captured on the untouched baseline
`719cfeb5`:**

```text
box_rendered_buffer_shows_live_pty_output
  -> panicked: the cadence never made "CODEGG-LIVE-PULL-BOX" visible in the rendered buffer.
     view: 8ee498ab… [live] attached focused 28x36 seq=18 sh -c stty raw -echo; exec cat
     projected rows: Some([ "OODEGG-IVE-P-UXB", ])
     focus_manager=0 dialog=None sidebar=true panel=Terminal area=None

pressing_i_on_the_open_box_enters_focused_input
  -> panicked: a freshly opened box starts viewing, never focused   (fails at baseline too)
```

The first is the milestone's premise, proven. The second is the pre-existing
red test analysed in §10.

## 5. Invariant review

| Invariant | Status | Pinned by |
|---|---|---|
| **I1 Bounded reads** | holds | the pull reuses the closed request with `max_bytes: None`; resync handling unchanged |
| **I2 No terminal bytes on the bus** | holds | no bus/projection/transcript change; C003 header assertion passes |
| **I3 Key routing not reopened** | holds — and strengthened | the classification table is untouched; two *pre-emption* paths that were overriding it are now gated |
| **I4 Single cadence** | holds | the pull is one call on the existing tick; no `tokio::time::interval`, no new thread |
| **I5 A poll may not supersede a user action** | holds | typed `TerminalResumeRequest` owner; per-attachment read state |
| **I6 Typed refusals stay typed** | holds | `note_read_refusal` / `read_refusal` re-arm on a user action; no generic error |

## 6. Failure and recovery review

Exited/gone/unknown-attachment outcomes stay the daemon's typed errors and stop
the cadence for that attachment until a user action re-arms it, rather than
retry-looping on the tick. A read whose attachment or handle is gone is
discarded rather than applied. The input slot is released by the *arrival* of
its completion, not by a successful one and not only when the completion is
current — otherwise a stale or failed input would leave every later keystroke
waiting behind a gate that never opens.

## 7. Migration and compatibility review

No database, wire, config, tool-schema or API change. No migration. No
`CoreRequest`/`CoreResponse` variant. `/terminal-resume` remains valid as a
recovery affordance and a test seam.

## 8. Security review

No change to identity, authorization, capability, or scope. A read is still
resolved by the daemon against the attachment's creating client; the typed
refusal is preserved rather than retried. No terminal byte is logged. The
`authority` check in `src/interactive_process_attach.rs` is untouched.

## 9. Documentation and operations

Updated: `architecture/tui.md` (the liveness contract and its three load-bearing
consequences), `docs/tui.md` (the user-facing gesture rule and the fact that
liveness needs no manual step), `.opencode/skills/tui/SKILL.md` (the two new
hazards).

The two predecessor closure records were **not** edited. Their correction is
recorded additively here and in `plans/registry.md`.

## 10. Unresolved findings

**1. The terminal box has no viewport plumbing (low).** `queue_viewport_resize`
is called from the *panel* render path only, so a box whose terminal is not
also docked keeps the create-time 80x24 and wraps to columns the dialog cannot
show. Pre-existing, not a regression, and not fixed here — sizing the box's PTY
to the dialog rect is a geometry change with its own evidence, and C003
(polish) is the natural owner. Recorded rather than silently absorbed.

**2. The plan's Work package G premise was wrong, and the plan's own stop
condition fired (resolved).** The plan assumed the box opening `Focused` was a
defect to be restored to `Viewing`. It is not: `01db38bd5` introduced the
behaviour deliberately, with the rationale "requiring a second `i` before the
first character reached the PTY is what made the box feel broken" — the exact
symptom this milestone exists to fix. The **test** and the **`TerminalFocus` doc
comment** were the stale artefacts, not the behaviour. Per the plan, the agent
stopped and reported rather than changing a recorded contract. Resolution: the
test was replaced with `the_box_opens_in_insert_while_the_docked_panel_still_needs_i`,
which pins both halves of the real contract, and the doc comment now states the
box/panel asymmetry explicitly. **This also corrects a premise recorded in the
roadmap and the plan before implementation.**

**3. The worker's mid-milestone commit claimed no tests it did not add
(informational).** `d4b70c72d` shipped the pull with its acceptance signal
failing. It was committed honestly — the body says `Status: NOT closed` and names
the failure — but a reviewer scanning commit subjects alone would have read it
as the completed feature.

**4. CI evidence is outstanding (medium, operational).** Everything above is
local. Hosted CI has not run this change.

**5. WS-attach/server transport remains without terminals (unchanged, owned by
C002).** Recorded, not fixed here.

## 11. Roadmap disposition

C001 is closed. C002 (transport) and C003 (polish) remain deliberately unopened.
Finding 1 is the concrete input C003 should start from.

## 12. Registry updates

`plans/registry.md` updated for this workstream: C001 moved to closed in both
the active-roadmap and dependency-ready tables, the execution-order note updated,
and the two predecessor rows retain their additive correction notes. The
additive correction on the `Interactive process sessions` row is extended with
the additional falsifications (blank-tail window, dock-gesture pre-emption,
input racing), since the earlier note named only the missing read side.

**Unblock audit: nothing newly unblocked.** C002 and C003 were already registered
as unopened and their dependencies are unchanged. No other registered plan lists
this workstream as a dependency.