# TUI Buffer Chrome, Exit Protocol, and Files Panel Navigation M003 — Closure Status

Status: closed

Source implementation plan: none. This roadmap's milestones were implemented
from the roadmap's own deliverable boundaries and exit conditions, so the
roadmap **is** the plan and the closure record below is the only evidence
artefact it has.

Source subsystem roadmap:

- `plans/subsystems/tui-buffer-chrome-and-panel-navigation-roadmap.md` (M003)

Repository baseline reviewed: `a588a264` (branch
`feat/auto-20261020-513a8ce6`)

Applicable ADRs (unchanged by this milestone): none.

## 1. What landed

**The working strip is gone.** The main buffer reserved a status row between
the transcript and the composer — `idle` when nothing ran, the live phase when
something did, `awaiting N task[s]` when the agent was blocked on delegated
work, and one clickable row per advertised subagent below it. It spent a row
of vertical space to say `idle` most of the time, and it made the buffer's task
state visible whether the user wanted it or not.

Removed: `working_strip_rows`, `render_working_strip`,
`working_elapsed_fallback`, `ClickTarget::Working`, `UiState::working_area`,
`SubagentState::hits` / `hit_at`, and `App::awaiting_task_counts`. The strip's
height is no longer subtracted from the transcript, so **nothing is reserved**
between the transcript and the composer: the transcript cannot reflow when the
turn state changes, because there is no row that appears or vanishes.

**The task state moved to where it already was.** The `Ctrl+T` tasks overlay
already unified todos, in-flight subagents and live jobs, and it is on demand.
The strip's task rows went there.

**The row above the composer is now situational.** It used to list the
composer's bare verb keys — `a b d e i j k o p u w x` and the rest — without
saying what any of them did. A list of fourteen letters is not a map: the user
learns the vocabulary and still cannot act on it, and the row that was meant to
help the buffer became the noisiest thing in it. The row now carries:

- `space help` — the chord that opens the help where every verb *is* described;
- the **situational** facts (`App::situational_hints`): a latched turn failure
  (`✗ {error}`) and an armed exit confirmation.

The latched failure is the one thing that used to depend on the strip's row 0:
it is latched *because* the toast that said the same thing expired, so it
needed a durable surface, and the row above the composer is now that surface.

**Two things moved with the strip.** The stop-agent confirmation's expiry was
called from inside `render_working_strip`; with the strip gone it is called
from the render pump instead, so a lapsed confirmation still clears itself.
And `ui_state.subagents` was synced by the strip's render path; the sync now
runs in the pump, so the `/subagent` addressee lookup and the tasks overlay see
the same vector whether or not the sidebar is drawn.

The sync kept the strip's **position**, and the position is load-bearing: it
runs *after* the subagent view is painted, exactly where the strip's own sync
sat. A tab whose subject has left the projection has to render its detached
state — "this run is no longer in the session's projection" — for that frame,
and a sync above the viewport would re-attach the tab before the body is
drawn and leave the view claiming a run is live.

## 2. Exit conditions

| Exit condition | Result | Evidence |
|---|---|---|
| the main buffer reserves no status row | met | `working_strip_rows` and `render_working_strip` are gone |
| the transcript gains the row | met | `prompt_height` no longer adds the strip |
| the latched turn failure is still durable | met | `App::situational_hints` carries `✗ {error}` on the row above the input |
| the row names the `space help` chord and no verb list | met | `App::composer_hint_row` |
| the `Ctrl+T` overlay still lists todos, active subagents, and live jobs | met | `src/tui/app/state/tasks.rs` (unchanged) |

## 3. What was verified

On the reconciled head (`45ffe2acb`, merged with `origin/main` at `2c1953347`):

- **Hosted CI run `39669710537` — the `verify` job passed** (21m4s): `cargo fmt
  --check --all`, `cargo clippy --workspace --all-targets --locked -- -D
  warnings`, and the workspace test suite. The Linux jobs
  (`btrfs-snapshot`, `native-probe-contract`, `protected-mount`,
  `restricted-userns-fails-closed`) passed too.
- `scripts/verify.sh quick` — **"Quick verification passed."**, all twelve TUI
  guards included.
- `cargo fmt --all -- --check` — clean.
- `cargo nextest run -E 'binary(tui_main_buffer) + binary(tui_composer_keymap_consistency)'
  — green, including the rewritten `tests/tui_main_buffer.rs` and
  `tests/tui_composer_keymap_consistency.rs`.
- `cargo nextest run --workspace --locked --profile ci --no-fail-fast` —
  **15,769 tests run, 15,763 passed, 5 failed, 1 timed out, 8 skipped**, every
  test this milestone touched green. The six non-passing are not this
  milestone's, and each is in a file this branch does not touch: the two
  `core::daemon_refresh` projection tests fail with `InvalidRootPath("/tmp")`
  on the untouched base commit too, the 300 s
  `agent::asset_refresh` coalescing wait becomes a timeout under load, the
  `managed_process` OfflineRequired refusal is macOS-only, and the two
  `scratch_cache_root_contract` tests are main's new ones resolving the
  scratch root under the tempdir's `HOME`.
- The twelve TUI guards (`check_tui_editor_*`,
  `check_tui_composer_key_authority`, `check_tui_transport_authority`,
  `check_daemon_listing_authority`) — green.

On this macOS host the merged tree reports eight dead-code errors under
`cargo clippy --workspace --all-targets --locked -- -D warnings`, all in
main's `#[cfg(target_os = "linux")]` code (`src/lsp/m004.rs`,
`src/managed_process.rs`, `src/security/sandbox.rs`,
`src/interactive_process.rs`): it compiles on macOS but nothing calls it.
Linux CI compiles the same files with their call sites present and is green.
Those files are byte-identical between this head and `origin/main`, so the
warnings are main's, inherited through the reconcile.

## 4. Recorded deviations

- **The clickable subagent rows are gone with the strip.** Clicking one used
  to open that subagent's buffer. The overlay carries the same rows and its
  own keyboard path (`↵ select` opens the row under the cursor), and the
  subagent tab bar still opens a buffer, but there is no longer a *mouse* path
  from a subagent row to its buffer. The request moved the rows into a
  collapsible surface; it did not ask for a mouse affordance there, and
  building one is recorded as an open item rather than smuggled in.
- `App::awaiting_task_counts` was deleted rather than kept: the overlay counts
  the same work from the same projection with the same predicates, so the
  method had no production caller left. Its doc reference in
  `src/tui/components/messages.rs` now points at the overlay.
