# Desktop Frontend and IDE Foundation Milestone 019 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/desktop-frontend-ide-foundation/019-desktop-terminal-surface.md`

Source subsystem roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md`

Applicable ADRs:

- **None is created or amended by this milestone.** The roadmap row says so
  and the tree bears it out: the daemon already serves every
  interactive-process request, the frontend action `show_terminal` is already
  declared, and the catalog form `terminal_show` is already a
  `FrontendEffect`.
- **ADR-0010** — the desktop frontend and shared client boundary — is
  respected and unchanged.
- **ADR-0052** — cross-frontend command execution ownership — is respected
  and unchanged: the desktop dispatches the daemon's published catalog, it
  does not hold a second one.
- **ADR-0053** — the daemon's published projection surfaces — is respected
  and unchanged; this milestone is outside that record's scope by its own
  §Decision item 5.

Repository baseline reviewed: `4aea3df5e` — `origin/main` with the M007
line through M007-F, M007-D and its corrective pass, and the projection
milestone, merged.

Implementation commit on this milestone's branch:

- `b4edc9c7d` — the desktop host module, the bounded VT renderer, the
  terminal session and panel, the keymap and focus additions, the action
  dispatch, and the recorded bridge baseline.

## 1. Executive finding

The desktop can now attach to a daemon-owned interactive process, read its
output on a bounded cadence, send it bounded input, resize it and detach —
and the `show_terminal` frontend action **performs** instead of declining.
`UNCARRIED_ACTIONS` is empty for the first time since the desktop had a
declared action set: the desktop now carries all **six** of its declared
frontend actions.

The milestone needed **no daemon work and no ADR**, which is what the
roadmap row predicted. The daemon already served all ten
`CoreRequest::InteractiveProcess*` variants, so the work was entirely the
desktop half: a host bridge the tree did not have, a renderer surface the
tree did not have, and the wiring that makes the declared action real.

Two of the milestone's premises were corrected rather than confirmed, and
both corrections are recorded in §10:

1. **The daemon's catalog does not serve the terminal's forms.** The
   milestone's plan assumed the desktop could reach the daemon's
   interactive-process surface through the command catalog. It cannot: the
   effect table has rows for `terminal_attach`, `terminal_resume`,
   `terminal_list` and `terminal_send` and **none** for `terminal_resize`,
   `terminal_detach`, `terminal_terminate` or `terminal_remove`. The
   desktop therefore reaches the daemon through **six direct named host
   commands** rather than through `command.execute`.
2. **`terminal_send`'s catalog row is broken.** It forwards the raw text
   into a field the daemon requires to be valid base64
   (`src/core/daemon_commands.rs:597-601` against
   `src/interactive_process_attach.rs:827-833`). The TUI never uses that row
   — it sends input as a raw request — so the defect is latent today. This
   milestone neither uses it nor inherits it; it is recorded as an
   unresolved finding.

## 2. Requirement-to-evidence matrix

The plan's §13 listed nine acceptance criteria. Each is answered by a
`path:line` anchor or a named test.

| # | Criterion | Evidence |
|---|---|---|
| 1 | Attach, read on the 100 ms cadence, send bounded input, resize, detach — all through the six named bridge commands | `apps/desktop/src-tauri/src/terminal.rs:507-600` (the six routes); `apps/desktop/src/terminal/session.ts` (`useTerminalSession`); the six targets in `apps/desktop/src/bridge.ts:163-178` and in `apps/desktop/src-tauri/src/lib.rs:1279-1284`; tests `terminal::tests::the_attach_route_carries_the_attachment_and_its_first_chunk`, `terminal::tests::the_resume_route_carries_the_chunk`, `terminal::tests::the_input_route_carries_the_accepted_byte_count`, `terminal::tests::the_resize_route_carries_the_applied_size`, `terminal::tests::the_detach_route_carries_the_released_attachment` |
| 2 | `/terminal-show <handle>` focuses the named handle, and `show_terminal` performs | `apps/desktop/src/App.tsx:894-899` (the action case, which reads the action's argument and falls back to the handle the session already shows); `apps/desktop/src/commands/frontendActions.ts:239` (the action's own case); `apps/desktop/src/commands/CommandPalette.tsx` (the argument now reaches the action); the renderer test *'refuses a handle the daemon did not publish, and says which handle'* |
| 3 | `UNCARRIED_ACTIONS` is empty | `apps/desktop/src/commands/frontendActions.ts:87` — `export const UNCARRIED_ACTIONS: Readonly<Record<string, string | undefined>> = {};` — and the renderer test *'the desktop carries every declared frontend action'* |
| 4 | No Tauri permission; the panel layer holds no bridge authority, no filesystem or process API and no durable store | `apps/desktop/src-tauri/capabilities/main.json` is unchanged; `apps/desktop/src/panels/TerminalPanel.tsx:1-4` imports only React, the keymap and the contract types; `scripts/check_desktop_panel_authority.py` passes; `scripts/check-desktop-boundary.sh` passes |
| 5 | The recorded bridge baseline is 50 and agrees with the bridge and the host's table exactly | `scripts/check_desktop_panel_authority.py` — `RECORDED_BRIDGE_TARGETS` holds **50** entries, of which the six are `desktop_terminal_list`, `desktop_terminal_attach`, `desktop_terminal_resume`, `desktop_terminal_input`, `desktop_terminal_resize`, `desktop_terminal_detach`; the bridge measures **50** distinct `invoke` targets; the guard compares the two and passes |
| 6 | No new runtime dependency, and the bounded VT subset's limits are documented | `apps/desktop/package.json` is unchanged; `apps/desktop/src/terminal/vt.ts` (the parser and its documented subset); `architecture/desktop.md` (the surface and its limits) |
| 7 | The cadence does not leak | `apps/desktop/src/terminal/session.ts` — one interval, created when an attachment becomes live and cleared on detach, on a route change and on unmount; the renderer tests *'holds one resume cadence while an attachment is live, and clears it on unmount'*, *'clears the cadence when the terminal is detached'* and *'clears the cadence when the route changes'* |
| 8 | A reconnect re-attaches, or reports `HandleGone` honestly | `apps/desktop/src/terminal/session.ts` — the re-attach on a replaced connection and the typed reason; the renderer tests *'re-attaches to the handle it was showing when the connection is replaced'* and *'reports a handle that is gone rather than showing an empty terminal'*; the host test `terminal::tests::the_attach_route_reports_a_resync_instead_of_an_empty_attach` |
| 9 | Every verification command in the plan's §11 was run and its outcome is recorded | §4 below — including the two that were **not** run, and why |

## 3. What was delivered

### 3.1 The blast radius

Twenty-five files changed and seven were added: **+1982 / −376** over the
milestone's own commit, counting the planning documents.

- **The host** — one new module, `apps/desktop/src-tauri/src/terminal.rs`
  (**877** lines), plus six `#[tauri::command]` wrappers and their
  registration in `apps/desktop/src-tauri/src/lib.rs:1279-1284`.
- **The renderer** — one new directory, `apps/desktop/src/terminal/`
  (`contract.ts` **141**, `vt.ts` **642**, `session.ts` **386** lines, plus
  three test files), one new panel
  (`apps/desktop/src/panels/TerminalPanel.tsx` plus its test), and edits to
  `bridge.ts`, `bridge-types.ts`'s neighbours, `App.tsx`,
  `commands/frontendActions.ts`, `commands/CommandPalette.tsx`,
  `keymap.ts`, `focus.ts`, `panels/registry.ts` and `style.css`.
- **The guard** — `scripts/check_desktop_panel_authority.py` gains the six
  targets in its recorded set.
- **Nothing underneath moved.** `src/tui/`, `crates/codegg-client/`,
  `crates/codegg-protocol/` and the daemon's storage schema are untouched.
  The daemon's own source is untouched. There is no protocol variant, no
  storage change and no migration.

### 3.2 The host's contract

Seven bounded view types live in the host module and are mirrored by
`apps/desktop/src/terminal/contract.ts`. `apps/desktop/scripts/check-bindings.mjs`
holds the two in one map — a third source pair, beside the bridge's and the
editor's — so the Rust and TypeScript field names cannot drift. The
guard prints `desktop bridge DTO fields match`.

The six operations are `desktop_terminal_list`, `desktop_terminal_attach`,
`desktop_terminal_resume`, `desktop_terminal_input`,
`desktop_terminal_resize` and `desktop_terminal_detach`. None of them is a
generic request passthrough: `scripts/check-desktop-boundary.sh:23-26`
forbids that shape, and the guard passes.

**The scope of these operations is the connection, not the route.** The
daemon derives ownership of an attachment from the transport connection's
client id (`src/core/daemon.rs:4849`), so a route change — another
project, another workspace, another session — does not invalidate an
attachment that is already open. The host's terminal routes therefore
carry **no** `route_generation`: fencing them on the route generation would
refuse valid work after an ordinary route switch. The desktop's editor
commands make the same distinction for the same reason — the operations
that act on an already-open document (`desktop_document_edit`,
`desktop_document_save`, `desktop_document_cursor`,
`desktop_document_viewport`) carry no route generation either, while the
ones that must resolve the project scope (`desktop_document_open`,
`desktop_document_tree`, `desktop_document_preview`) do.

**Three rules the host holds.** A handle is not an attachment: `attach`
names a handle and returns an attachment id, and every later operation
names the attachment, so an id cannot be replayed across owners. A resync
is a typed answer: `InteractiveProcessResyncRequired` carries a reason and
no bytes, and it is reported as a resync, never flattened into a successful
attach with an empty chunk. An unexpected answer is an error: a reply the
daemon did not send becomes a typed failure, never a default view.

**The bytes are bounded.** One read asks for at most
`TERMINAL_CHUNK_BYTES` (**64 KiB**, the protocol's own default, at or below
its hard cap) and one write carries at most `TERMINAL_INPUT_BYTES` (**32
KiB**, the protocol's own cap). The host guarantees the daemon receives
valid base64 whatever form the caller used — the renderer sends base64,
and the host's encoder is checked against the daemon's decoder.

### 3.3 The renderer's half, and its bounded VT renderer

The daemon publishes **raw PTY bytes**
(`InteractiveOutputChunk.data_b64`,
`crates/codegg-protocol/src/interactive_process.rs:255-270`), so the
desktop must interpret escape sequences. The tree has no ANSI or grid
library today, and **this milestone adds none**:
`apps/desktop/package.json` is unchanged.

That was a deliberate decision, not an oversight. The roadmap's stated
reason for refusing a bundled **editor** engine is that adopting one
"would change the renderer's Content Security Policy, its worker model, and
the bundle and signing boundary"
(`plans/subsystems/desktop-frontend-ide-foundation-roadmap.md:154-160`).
The same reasoning applies verbatim to a bundled **terminal** engine, and
M007-E set the precedent by delivering a first-party editor surface over
the shared document foundation.

The bounded subset is exactly: printable bytes written at the cursor; `CR`,
`LF` and `BS`; the `CSI` erase forms (`ED`/`J` and `EL`/`K`, with their
parameter variants); the `CSI` cursor-movement forms (`CUU`/`A`,
`CUD`/`B`, `CUF`/`C`, `CUB`/`D`, `CUP`/`H`); and `CSI` SGR (`m`) for the
eight standard colours plus bold, mapped to a small class set. Everything
else — `OSC`, `CSI r` (the scroll region), `CSI ? … h/l` (the private modes
that select the alternate buffer), and any unknown sequence — is
**consumed and ignored**, and the parser records the class it last saw so
the panel can say that the bounded renderer did not draw it. An unsupported
sequence never leaks into the visible text; the renderer tests prove this
for the operating-system command (in both terminations), a private mode, a
scroll region, an unknown control sequence and an unknown escape sequence.

The scrollback is a bounded ring of at most **2000 lines** and at most
**256 KiB** of retained text, whichever binds first. When the bound is
reached the oldest lines are dropped and the panel says so.

### 3.4 The panel, the session, and the authority boundary

The terminal's state lives **above** the panel, in `useTerminalSession`
(`apps/desktop/src/terminal/session.ts`), exactly as the editor's state
lives in `useEditorSession`. The panel
(`apps/desktop/src/panels/TerminalPanel.tsx`) imports only React, the
keymap and the contract types: it names no bridge target, imports no host
module, and reaches no filesystem or process. `scripts/check_desktop_panel_authority.py`
passes.

Three properties of the session are load-bearing and each has a test:

- **The resume cadence is a single interval**, running only while an
  attachment is live, and cleared on detach, on a route change and on
  unmount. The renderer has no other timer, so this is the only one.
- **Input is queued and flushed in order**, one bounded write per tick, and
  a queue past the daemon's own 32 KiB cap is refused locally and said out
  loud rather than silently truncated.
- **A resync is reported.** A chunk with a gap, or an attach that answers
  with a typed reason, resets the grid and says so; the view is never
  presented as a continuous stream when it is not one.

The keymap gains a `TERMINAL_SCOPE` and three actions — `terminal.focus`,
`terminal.send` and `terminal.backspace` — and the focus model gains a
`terminal` region, declared **last** and inside the side panel, because the
nearest container is the rule: an element inside the terminal belongs to
the terminal rather than to the side panel around it.

### 3.5 The action

`show_terminal` moves from uncarried to carried. The desktop's action
dispatch now carries the action's **argument**, which the palette
previously dropped; the catalog form requires it
(`crates/codegg-core/src/command_catalog.rs:2498-2503`, `argument_hint:
"<handle>"`, `argument_required: true`) and the TUI refuses an unknown
handle (`src/tui/commands/interactive_terminal.rs:1612-1637`). The desktop
does the same rather than focusing an arbitrary terminal.

`UNCARRIED_ACTIONS` is now empty — the desktop carries all six of its
declared frontend actions for the first time.

## 4. Verification executed

Every command is listed with its outcome. **A command that was not run is
not evidence**, and two were not run; both are named.

### 4.1 The desktop's own entry points

| Command | Outcome |
|---|---|
| `apps/desktop/scripts/verify-desktop.sh renderer` | **exit 0** — `npm ci`, typecheck, the test suite, `bindings:check` and the build |
| `apps/desktop/scripts/verify-desktop.sh host` | **exit 0** — `cargo test --lib`, `cargo fmt --check`, `cargo clippy -D warnings` |

### 4.2 The renderer's suite

`cd apps/desktop && npm test` → **34 files passed, 290 tests passed**, 0
failed. M007-E's closure recorded 28 files and 208 tests, so this
milestone adds **6 files and 82 tests**.

### 4.3 The host's suite

`rustup run 1.90.0 cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --locked --lib`
→ **115 passed, 0 failed, 1 ignored**. The ignored test is the pre-existing
live-daemon lifecycle test, which needs `CODEGG_DAEMON_EXECUTABLE` and an
isolated daemon home and is ignored by design.

Eighteen of those tests are the terminal's own, including
`the_process_view_omits_the_child_pid`,
`the_attach_route_reports_a_resync_instead_of_an_empty_attach`,
`an_unexpected_answer_is_a_typed_error_not_a_default_view`,
`the_input_request_encodes_the_callers_bytes`,
`the_base64_decoder_refuses_what_the_daemon_would_refuse` and
`the_read_requests_ask_for_a_bounded_chunk`.

`rustup run 1.90.0 cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check`
→ clean. `rustup run 1.90.0 cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets --locked -- -D warnings`
→ clean.

### 4.4 The guards

| Guard | Outcome |
|---|---|
| `python3 scripts/check_desktop_panel_authority.py` | pass — the recorded set of **50** matches the bridge's **50** |
| `python3 scripts/check_frontend_action_set.py` | pass |
| `python3 scripts/check_command_catalog_ownership.py` | pass |
| `python3 scripts/check_writer_disposition.py` | pass — `60 declared roots, 18 writer tools, 36 writer modules` |
| `python3 scripts/check_adr_integrity.py` | pass |
| `./scripts/check-desktop-boundary.sh` | pass |
| `cd apps/desktop && npm run bindings:check` | pass — `desktop bridge DTO fields match` |
| `cd apps/desktop && npm run typecheck` | pass |

### 4.5 The root workspace

`scripts/verify.sh quick` → **QUICK_EXIT=0**, "Quick verification passed."

### 4.6 Not run, and why

- **The hosted desktop E2E lane** (`.github/workflows/desktop-e2e.yml`) was
  **not run locally**. It needs a hosted runner and a built application
  bundle; its result is the pull request's, not this closure's.
- **`apps/desktop/scripts/verify-desktop.sh` in "full"** was run as its two
  phases (`renderer` and `host`) rather than as one invocation. The two
  phases are the same commands the full mode runs.

## 5. Invariant review

Each of the plan's §4 invariants, and the evidence.

1. **The desktop holds no Tauri permission.** `apps/desktop/src-tauri/capabilities/main.json`
   is unchanged and the host's capability set is unchanged. No capability
   was added.
2. **The renderer holds no machine authority.** No generic request
   passthrough (the boundary guard passes), no bridge call from the panel
   layer, no Tauri import, no filesystem or process API, and no durable
   frontend-owned store. The scrollback is in-memory and bounded; nothing
   is persisted.
3. **The desktop's bridge surface stays a recorded set.** The six new
   targets were added to `RECORDED_BRIDGE_TARGETS` in the same change that
   introduced them, and the guard — which compares the recorded set with
   the bridge and with the host's command table — passes.
4. **The daemon owns the process.** The desktop never spawns, signals or
   inspects a process; it asks the daemon for the daemon's processes. The
   host module contains no process-spawning call.
5. **The bytes are bounded.** A chunk is at most the daemon's
   `MAX_INTERACTIVE_CHUNK_BYTES`, an input write at most the daemon's
   `MAX_INTERACTIVE_INPUT_BYTES`, and the renderer's scrollback at most
   2000 lines and 256 KiB.
6. **A kind is never inferred.** The renderer dispatches the daemon's
   published `action` on the form, never a command spelling.
7. **The desktop's command surface stays the daemon's catalog.** The
   renderer spells no `/command` name of its own; the catalog-ownership
   guard passes.

## 6. Failure and recovery review

- **A reconnect orphans the attachment.** The daemon owns an attachment by
  the transport connection's client id, and the host holds one client. The
  session re-attaches to the handle it was showing when the connection is
  replaced, and reports `HandleGone` if the handle is gone. It never opens
  a second connection for the terminal. Both behaviours have tests.
- **A resync resets the view.** A chunk with `gap: true` means the
  requested cursor predated retention; the returned bytes are the oldest
  retained bytes. The grid is reset and the resync is stated.
- **A lost attachment is not an error state.** `HandleGone` and
  `CursorAhead` are typed answers and are reported in words.
- **Input is bounded.** A queue past the daemon's 32 KiB cap is refused
  locally and the cap is named.
- **Resize is idempotent.** A resize to the size the process already has is
  a no-op on the daemon's side.
- **Detach releases the attachment, not the process.** The desktop never
  terminates a process.
- **The cadence does not leak.** Cleared on detach, on a route change and on
  unmount; a test asserts that only one terminal interval exists.

## 7. Migration and compatibility review

- **No protocol change.** Every variant the milestone uses already existed.
  An older daemon answers an unknown variant with `Error`, and the desktop
  reports that as a typed failure rather than as an empty terminal.
- **No storage change, no migration, no configuration change.**
- **The TUI is untouched.** This milestone adds a second frontend over an
  existing daemon surface; it does not move, reshape or deprecate the
  TUI's terminal, and `git diff --name-only -- src/tui/` is empty.
- **The renderer gains no dependency.** `apps/desktop/package.json` is
  unchanged.
- **The recorded bridge baseline grows** from 44 to 50. That is a deliberate
  edit to a reviewed set, not a measured baseline, and the guard proves
  the two agree.

## 8. Security review

- **No Tauri permission was added** and the capability set is unchanged. The
  desktop's trust boundary is exactly where it was.
- **The terminal's authority is the daemon's.** The interactive-process
  family bypasses `authorize_request` (`src/core/daemon.rs:4569-4597`), so
  the desktop's six commands inherit the daemon's connection-scoped
  ownership — the same authority the TUI's terminal has. A desktop client
  can reach **only** the processes the daemon gave it.
- **A handle cannot be replayed across owners.** Attach returns an
  attachment id, and the daemon resolves it server-side, so a client cannot
  name another client's attachment.
- **The panel holds no authority.** It names no bridge target and imports
  no host module; the panel-authority guard passes.
- **No shell is interpreted in the renderer.** The VT parser renders bytes;
  it does not execute anything. The daemon's process is the only thing that
  runs, and the daemon owns it.
- **Bounded inputs and outputs** at every step, as §5 records.
- **No secret is logged.** The terminal's content is the daemon's process
  output, rendered in the panel and never written to a file by the
  renderer.

## 9. Documentation and operations

- `architecture/desktop.md` — the terminal surface, its bounded VT subset,
  its honest limits, and the six bridge commands.
- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md` — the
  M007-J status cell becomes **closed**, with this record's link.
- `plans/registry.md` — the M007-J row carries the plan and this record.
- The plan itself: `plans/implementation/desktop-frontend-ide-foundation/019-desktop-terminal-surface.md`.
- **No ADR is created or amended**, and no canonical document changes: the
  milestone implements a surface the roadmap already placed inside the
  line, and it changes no terminology and no ownership boundary.

### 9.1 Two corrections the milestone carries, recorded rather than smoothed over

- **The plan's §3.3 described the catalog's terminal coverage as if it were
  the milestone's mechanism.** It is not: the catalog serves only four of
  the ten terminal forms, and one of those four is broken. The plan's §6.2
  already chose the six direct host commands for exactly this reason, and
  the implementation followed the plan. The plan's §3.3 is evidence, not a
  requirement, and it is left as written with this note — the closure
  records the correction, and the plan is not rewritten after the fact.
- **The plan's §6.2 said the host wrappers would take a `route_generation`
  "the way every existing wrapper does".** They do not, and they should
  not: the terminal's operations are scoped by the connection, not the
  route. The implementation's reasoning is recorded in the host module's
  own header and in §3.2 above.

## 10. Unresolved findings

1. **The daemon's `terminal_send` catalog row is broken.** It forwards the
   raw text into `data_b64`, which the daemon requires to be valid base64
   (`src/core/daemon_commands.rs:597-601` against
   `src/interactive_process_attach.rs:827-833`). It is **latent** — the
   TUI sends input as a raw request and never uses the row, and this
   milestone does not either. **Severity: medium** — the row is reachable
   by any client that routes `terminal_send` through `command.execute`, and
   it would fail at the daemon rather than corrupting anything. It is
   outside this milestone's scope, which changes no daemon behaviour, and
   it is a candidate for M007-H's effect-table work.
2. **The catalog serves no form for `terminal_resize`, `terminal_detach`,
   `terminal_terminate` or `terminal_remove`.** The desktop therefore
   cannot reach those through `command.execute` at all — which is why this
   milestone uses direct host commands. The daemon's catalog and the
   desktop's terminal surface are consequently two different surfaces for
   the same service. **Severity: low** — the desktop works, and the
   duplication is bounded to six named commands. Reconciling the catalog
   with the terminal's forms is M007-H's business.
3. **The bounded VT renderer does not support full-screen
   applications.** `vim`, `less` and `htop` select the alternate buffer,
   which the bounded subset consumes and ignores; they will not render
   correctly. This is **by design** and is stated in the panel's own
   notice, not hidden. **Severity: low, accepted.**
4. **The bounded VT renderer does not implement mouse reporting, the
   256-colour palette, or `OSC` title-setting.** **Severity: low,
   accepted**, and stated in `architecture/desktop.md`.
5. **The desktop cannot create a terminal.** `InteractiveProcessCreate` is
   served, but the desktop has no "new terminal" affordance, because
   creating a process needs a working-directory decision, a launcher
   capability and a writer disposition that this milestone does not make.
   **Severity: low** — and an explicit non-goal in the plan's §5.

## 11. Roadmap disposition

**M007-J is closed.** The desktop's terminal surface exists: attach, input
and resize are wired over the daemon's interactive-process service,
`/terminal-show <handle>` focuses a named handle, and the `show_terminal`
frontend action performs rather than declining.

With this milestone closed, **the desktop carries all six of its declared
frontend actions** and `UNCARRIED_ACTIONS` is empty for the first time.

The M007 line's remaining milestones — **M007-G** (the daemon's plugin
catalogue), **M007-H** (the daemon's missing command surfaces) and
**M007-I** (the subsystem moves) — are **not** closed by this record. Their
state is recorded in the roadmap's own rows and in the registry.
