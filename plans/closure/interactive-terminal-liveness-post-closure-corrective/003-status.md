# Interactive Terminal Liveness Post-Closure Corrective C003 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/interactive-terminal-liveness-post-closure-corrective/003-live-surface-cadence-and-honest-mode-state.md`
Source subsystem roadmap: `plans/subsystems/interactive-terminal-liveness-post-closure-corrective-addendum.md#c003--live-surface-cadence-arming-and-honest-mode-state`
Repository baseline reviewed: `18c3dca92` (`feat/auto-20261017-f117dc16`)
Implementation commits: none — the working tree at closure time contains the change; it is **not committed** at the time of writing. See §9 finding 3.

## 1. Executive finding

Both findings in the premise were real, and both are now closed.

**The lag was an unreachable code path, not a slow one.** `service_terminal_output_pull`
has exactly one call site: the timed arm of the loop's `select!`. That arm parks on
`futures_util::future::pending()` whenever `next_wake` is `None`, and `None` does not mean
"sleep for a while" — it means the arm never runs again until some *other* event wakes the
loop. `next_wake` is built from `runtime::event_loop::animation_interval`, whose three
contributors (streaming, working, a live toast) are **all transient**. A turn finishes, its
toast expires, and the frame goes completely idle. A mounted terminal was therefore read only
while a toast happened to be alive, a keystroke arrived, or a resize was debouncing. The bytes
were already sitting in the daemon's ring the whole time; nothing was fetching them, so a
shell's output became visible when the user pressed the *next* key. This is why the panel read
as broken rather than as slow, and why `/terminal-resume` appeared to be the missing step when
it is not.

**The badge was reporting the wrong subject.** `[NOR]`/`[INS]` above the main input is a
statement about *where the keyboard is*. It was reading `App::ui_state.input_mode`, which is
the *composer's own* mode — and that field is pinned to `Normal` by `focus_panel` for as long
as a panel is focused, and is never touched at all by the terminal box. So a terminal holding a
live PTY in Insert displayed `[NOR]` for the entire time the user was typing into it. The same
false field also gated the terminal panel's own Insert hint branch, which made that branch
**unreachable dead code**. One field, read twice, both wrong.

## 2. Requirement-to-evidence matrix

| Plan requirement | Evidence | Result |
|---|---|---|
| Work package A: the idle path is dark on baseline, and proven **on the real arming decision** | `animation_interval` was extracted as a pure `&App` function **before** any behaviour change (behaviour-preserving), then the two cadence tests were written against it. On baseline both failed with `the event loop is not armed for a mounted live terminal surface … animation_interval returned None, so the loop's timed arm selects futures_util::future::pending()` | pass |
| Headline: output appears with no further keystroke, toast, resize or turn event | `a_live_terminal_surface_repaints_with_no_further_user_input` (box) and `a_docked_terminal_keeps_output_after_the_keyboard_leaves_the_panel` (panel). Both drive the real key router, then go strictly idle; the harness supplies only the *tick*, and every pass re-asserts the production arming decision first | pass |
| The armed path is bounded by **mount**, not focus or adoption | `a_hidden_terminal_surface_does_not_arm_the_loop` closes the box on an attached, live terminal and asserts the loop parks exactly as it did before any terminal existed | pass |
| I4 — no second timer, no new clock | The contributor is a fourth branch of the existing `animation_interval`, on the existing pump tick. No `tokio::time::interval`, no thread, no new constant beyond the interval itself | pass |
| I2 — no terminal bytes on the bus | No bus event, projection, transcript or session-observation change. The cadence changes *when* an existing bounded read is issued, not what it carries | pass |
| I3 — key routing not reopened | No `classify_key`, `TerminalKey`, `TerminalFocus` or `handle_key` change. The only key-path edit extracts an existing inline expression in `on_key` into a named accessor with identical semantics | pass |
| Badge and hint line can never disagree | Both read `App::composer_input_mode`. The terminal panel's previously unreachable Insert branch is now live, asserted by `a_docked_terminal_in_insert_is_reported_as_insert` (`INS typing to the shell`, and **not** `hjkl move`) | pass |
| Render may not move the keyboard | `reporting_the_mode_never_moves_the_keyboard` reads the effective mode three times, reads the hint text three times, renders the real frame, and asserts focus, terminal focus, and the composer's private mode are all unchanged | pass |
| No second definition of "holds the keyboard" | `App::terminal_box_has_keyboard` is the extracted form of the gate `on_key` already used inline; the panel half is the existing `PanelHost::claims_insert_keys` | pass |
| No existing test weakened or deleted | None removed or relaxed. One call site outside the changed crate needed a `let mut` because `composer_hint_text` now reads through a `&mut` seam (`tests/tui_chat_surfaces.rs:466`) | pass (with the recorded reason) |

## 3. Production implementation evidence

Four production files changed:

- `src/tui/runtime/event_loop.rs` — `animation_interval` extracted as `pub(crate) fn
  animation_interval(app: &app::App) -> Option<Duration>` (behaviour-preserving refactor done
  first, specifically so the arming decision became testable), plus the terminal contributor.
- `src/tui/commands/interactive_terminal.rs` — `TERMINAL_PULL_RENDER_INTERVAL` (100ms) and
  `terminal_pull_armed(app)`, which mirrors only the pull's two *free* early returns (a mounted
  surface, and a client to read with). The nine new tests and the
  `pull_until_rendered_armed` helper live in the same file.
- `src/tui/app/mod.rs` — `terminal_box_has_keyboard`, `terminal_box_owns_keyboard` (extracted
  from `on_key`), and `composer_input_mode`; `composer_hint_text` now passes the derived mode
  to `panel_host.composer_hints`.
- `src/tui/app/render.rs` — the badge reads the derived mode.

The production diff is small; the test module accounts for the largest share of the line count
in `interactive_terminal.rs`.

### 3.1 The two rules that keep `terminal_pull_armed` honest

- **Mount, never read state.** It must not consult `begin_output_read`,
  `output_read_in_flight`, a recorded refusal, or `TerminalLinkState`. Gating the arm on any of
  those latches the loop off: the arm goes dark, so nothing wakes it to issue the read that
  would clear the condition, and the surface stays dead until a keystroke arrives. Those checks
  belong to the pull, where refusing one pass is harmless.
- **Mounted means on screen.** `mounted_terminal_surface` is box first, then panel. A
  background view that is attached, alive, and on no surface is not read, because nothing it
  produces would reach a screen.

## 4. Verification

All commands were run locally in the worktree. **CI has not run this change**, and per
`AGENTS.md` CI is a strict subset of `quick`, so a green local `quick` is not a CI result.

| Check | Result |
|---|---|
| `bash scripts/verify.sh quick` | **passed** (including agent schema, core/client/desktop boundaries, sandbox, execution-ownership, TUI authority, http-route-disposition, audit-coverage, scheduler-bypass, config-merge-coverage, provider wire/catalog/resilience, OpenAI endpoint, Eggwork routing, no-hardcoded-models, and `cargo check --workspace --all-targets --locked`) |
| `cargo fmt --all -- --check` | clean (after `cargo fmt --all`) |
| `cargo test -p codegg --lib tui::` | **1949 passed, 0 failed** |
| `cargo test -p codegg --lib tui::commands::interactive_terminal` | **32 passed, 0 failed** (23 pre-existing, 9 new) |
| `cargo test -p codegg --test interactive_terminal_tui` | 6 passed |
| `cargo test -p codegg --test tui_panel_docking` | 52 passed |
| `cargo test -p codegg --test tui_chat_surfaces` | 29 passed |
| `cargo test -p codegg --test tui_render` | 111 passed |
| `cargo clippy -p codegg --all-targets -- -D warnings` | **clean** |

`cargo clippy --workspace --all-targets` (the full workspace, including every crate's targets)
was **not** run for this change, and `verify.sh full` was not run. Only the root package was
clippy-checked; the change touches no crate outside `codegg`, but the workspace-wide form
remains outstanding. See §9 finding 2.

### 4.1 Falsification and non-vacuity

Falsification-first was followed for the cadence. `animation_interval` was extracted with no
behaviour change, the tests were written against it, and they **failed on the unfixed
behaviour** with the message quoted in §2. Non-vacuity was then demonstrated in the required
direction — the fix added, tests pass, the fix reverted, tests fail again — for the arming
contributor and, separately, for the badge read. Reverting the badge alone leaves the hint line
and the badge disagreeing, which is not a production state but did confirm the two consumers are
genuinely wired to the accessor.

## 5. What was deliberately not done

- **No second timer, no new clock** (I4). The terminal rides the existing pump tick.
- **No push transport and no bus-carried bytes** (I2). Those remain ADR-gated stop conditions.
- **No change to what any key means** (I3).
- **C004's four ergonomics items were not touched**: the unfocused hint row, the Insert cursor
  blink, the post-clear prompt line, and `Esc` reader-cursor placement. The first is a layout
  change to `composer_hint_text`, which this milestone deliberately kept narrow so a one-row
  reflow could not be confused with a mode-reporting regression; the rest are polish that C003
  was reclassified away from.

## 6. Why C001's evidence missed this

Worth stating plainly, because the failure mode is reusable. C001's acceptance tests call
`service_terminal_output_pull(&mut app)` directly, and
`tests/interactive_terminal_tui.rs`'s `poll_resume_text` calls `protocol.resume` directly.
Both prove the *function*. Neither proves the *wiring*. The harness supplied the polling loop
that the production loop withheld, so the tests passed on a code path the running loop never
took.

The generalisable rule, now recorded in `.opencode/skills/tui/SKILL.md`: when you add a service
to a `select!` arm, assert the **arming decision**, not just the function body. Extracting the
arming into a pure function is what made that observable at all.

## 7. Scope note on the two readings

`terminal_box_owns_keyboard` (does the box consume keys *as input for the process*) and
`terminal_box_has_keyboard` (is the box the keyboard destination at all) are deliberately
separate, because they answer different questions and the reporting one needs the wider form.
After `Esc` the box still holds the keyboard for navigation, and the composer's private mode
still says `Insert` — `the_badge_returns_to_normal_after_esc_even_though_the_composer_forgot`
pins exactly that stale field as the trap the accessor has to see through.

## 8. Registry updates

- Roadmap row status → `active (C001 conditionally closed — condition discharged by C003;
  C002 closed; C003 closed; C004 ready)`.
- C003 row → **closed**, with the closure record linked and the falsification recorded.
- **C001's conditional status is discharged.** Its record is **not** edited; the correction
  remains additive, per registry rule 6.
- C004 moved from *Blocked work* to *Dependency-ready implementation plans* as **ready** — its
  blocker was technical (a parked loop cannot animate a blink; the hint row shared
  `composer_hint_text` with this milestone), and both reasons are now gone.
- Roadmap §12 status table and the C004 section updated to match.

## 9. Unresolved findings

**1. Two unexplained test-run failures were observed and could not be reproduced (medium,
affects confidence in the suite, not in the change).** Across roughly 25 runs of the terminal
module, two runs reported failures (one run: 2 failed; one run: 8 failed) and **in both cases the
failing test names and messages were filtered out of the captured output before being read**, so
the cause is unknown. Both runs coincided with other cargo work running concurrently against the
same target directory; the 8-failure run coincided with `verify.sh quick`. It did not reproduce
in 19 subsequent isolated runs, in 3 runs deliberately contended against a concurrent
`cargo check --workspace --all-targets`, or under `--test-threads=1`. The most plausible
mechanism is resource contention for real PTYs — these tests spawn an actual
`sh -c 'stty raw -echo; exec cat'`, and several background test runs were interrupted during
this session — but that is a hypothesis, not a finding. **The honest statement is that two red
runs were produced and never diagnosed.** Two mitigations were added rather than leaving it
open: both cadence tests now assert, immediately after typing, that the surface still holds the
keyboard, so a fixture problem cannot resurface later as a misattributed cadence or mode
failure; and the negative test pins the unmounted case so a widened visibility rule is caught.

**2. Workspace-wide clippy and `verify.sh full` were not run (low, operational).**
`cargo clippy -p codegg --all-targets -- -D warnings` is clean, and no crate outside the root
package is touched, so the residual gap is narrow — but `cargo clippy --workspace --all-targets`
and `verify.sh full` (which includes clippy plus the workspace test sweep under the `ci` nextest
profile) are outstanding.

**3. The change is not committed (operational).** Four production files, four documentation
surfaces, and the planning documents are modified in the working tree. No commit was made,
because none was requested.

**4. The loop's `select!` and sleep are still not exercised by a test (low, accepted).** What is
tested is the arming decision — the fact that was actually wrong — plus the pull and the
rendered result. Driving the real `select!` needs a real terminal and was out of reach; the
harness supplies the tick and asserts the production decision on every pass, which is a weaker
but honest claim, stated as such.

**5. A docked terminal now repaints at ~10Hz whenever it is visible (low, accepted).** Each tick
is one bounded daemon round-trip and each completion sets `needs_render`. That is ordinary
terminal-emulator behaviour and is the direct cost of liveness, but it is a real change in idle
CPU for a session with a docked terminal and was not benchmarked.

## 10. Roadmap disposition

K1 (live surface) and K3 (honest mode state) are delivered. C001's condition is discharged.
C004 is unblocked and ready. The remote-interactive-process authorization workstream (R001)
remains `proposed` and blocked behind its own ADR; nothing here resolves it.