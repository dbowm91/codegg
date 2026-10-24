# Desktop Frontend and IDE Foundation Milestone 012 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/desktop-frontend-ide-foundation/012-desktop-chat-region-and-side-panel.md`
  (milestone **M007-B**)

Source subsystem roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md`

Applicable ADR (controlling):

- `plans/adrs/ADR-0010-desktop-frontend-and-shared-client-boundary.md` — accepted.
  M007-B changes **no** ownership contract: it adds a renderer-side layout and a
  panel layer, and moves no state to the frontend.

Repository baseline reviewed: `c5d3f4dc4` — `origin/main` merged with the
desktop-parity work (pull request #185), on which the M008 closure
(`13807cb05`) then landed, and with M007-A's gate milestone (`f11a5e33c`)
already on the branch.

Implementation commits on this milestone's branch:

- `199903d3e` — **work packages A, B, D and E**: the region frame, the panel
  layer, the two projection fields the renderer was dropping, the grid layout,
  and the renderer test sweep.
- `2feaf1b5b` — **the static guard**: `scripts/check_desktop_panel_authority.py`,
  its `--self-test`, and its wiring into `scripts/verify.sh quick` and
  `.github/workflows/ci.yml`.

## 1. Executive finding

M007-B is what turns the desktop from **a control-plane console that happens to
render a session** into **an application with a chat region and a side panel**.

Before it, the desktop was one centred 760-pixel column
(the `main { max-width: 760px; … }` rule that
`apps/desktop/src/style.css:55` carried before this milestone) of stacked
`<section>` cards: the transcript,
the tools, the todos, the subagents and the skills all ran down the same
single column, in the same type scale, with no way to bring two of them side by
side and no way to move one out of the way. The renderer read **21**
`projection.<field>` identifiers and ignored four of them — `runs`,
`recentTurns`, and two more — so the daemon was doing work the desktop threw
away.

After it, the renderer is a **CSS grid frame with named regions** — the chat,
the side panel, the composer and the status row — and a **panel layer** that
can dock a panel on any edge, resize it, and render the two projection fields
the desktop was dropping. The panel layer is a *renderer-local* layer: it holds
no authority, calls no bridge target, and persists nothing.

Two constraints held throughout, and both matter more than the feature:

- **Every `data-testid` that existed before still exists.** The built-app
  e2e specs select on those attributes; a renamed id is a silent break of the
  visible-window trajectory — the one lane that proves the desktop actually
  works. Measured: **191 before, 232 after, none removed or renamed**.
- **The existing renderer test suite passes unmodified.**
  `apps/desktop/src/App.test.tsx` is **byte-identical** to the committed
  version — `git diff --stat -- apps/desktop/src/App.test.tsx` is empty. The
  new tests are in new files. The suite grew from **69** to **131** tests.

## 2. Requirement-to-evidence matrix

| Plan criterion | Evidence | Verdict |
|---|---|---|
| §13.1 — the renderer's layout is split into a **main chat region** and a **dockable side panel**, with the frame's regions named | §3.2 — `apps/desktop/src/regions/` holds `ChatRegion`, `SidePanelFrame`, `ComposerRegion`, `StatusRow`; `apps/desktop/src/style.css` carries `grid-template-areas` for the frame and for the per-edge panel areas | met |
| §13.2 — the renderer renders the projection fields it was dropping, and the daemon's own status tokens | §3.3 — `projection.runs` and `projection.recentTurns` are rendered by `RunHistory.tsx` and `RecentTurns.tsx`; each row shows the daemon's own status token | met |
| §13.3 — a panel can be **docked on any edge** and **resized** | §3.4 — `PanelPlacement` is `'right' \| 'left' \| 'bottom'` with `'right'` the default, mirroring `src/tui/panels/mod.rs:219-229`; `host.ts` owns the per-edge slots and `dock(id, placement)`; `ResizeSeam.tsx` is the pointer seam | met |
| §13.4 — an empty list is **not** shown as "this session has nothing" when no snapshot is installed | §3.3 — `snapshotState.ts` distinguishes the five pre-snapshot states (`connecting`, `subscribing`, `resyncing`, `disconnected`, `unavailable`) from a genuinely empty list | met |
| §13.5 — the **existing `data-testid` values survive** the split unchanged, and the built-app trajectory still passes | §4.1 — 191 → 232, none removed; the local built-app trajectory is **14 of 14** | met |
| §13.6 — the **existing renderer test suite passes unmodified** | §4.1 — `App.test.tsx` byte-identical; the suite grew 69 → 131 in new files | met |
| §13.7 — **no new bridge target, no protocol change, no DTO change, no Rust change** | §5 — `apps/desktop/src/bridge.ts` and `apps/desktop/src/bridge-types.ts` are untouched; `git diff --stat -- apps/desktop/src-tauri/` is empty | met |
| §13.8 — the panel layer holds **no** credential, provider, scheduler, job-admission or writer-permit surface, and introduces **no** new `invoke` target | §3.5 — the guard enforces it, and the desktop's bridge target count stays at the recorded **23** | met |
| §13.9 — `scripts/check-desktop-boundary.sh` passes **unchanged**, and the desktop remains a separate Cargo workspace | §4.1 — "desktop dependency and authority boundaries verified" | met |
| §13.10 — the **renderer's product behaviour** outside the layout is unchanged: the transcript, the turn summary, the tool detail, the git panel and the skills panel keep their own surfaces and their existing content | §3.2, §4.1 — the transcript markup left `App.tsx` (one comment remains) and lives in `ChatRegion`; the baseline's panels are still composed inline and are **not** side-panel registry entries at this milestone | met |
| §13.11 — **no keyboard handling** is introduced | §3.2 — the milestone adds no keydown listener; that is M007-C. The resize seam's missing keyboard operation is recorded in §10 as an M007-C finding | met |
| §13.12 — **no `/` command surface**, no editor, no file tree, no LSP surface, no new Git UI, no graphical terminal | §5 — no such file or capability is in the milestone's diff | met |
| §13.13 — **no durable panel-layout persistence** | §3.4 — placement is renderer-local; the milestone's diff contains no `localStorage`, no `sessionStorage` and no `indexedDB` | met |
| §13.14 — a **static guard** asserts the panel layer's authority boundary, with a `--self-test` that injects the defect, and it runs in **both** `quick` and CI | §3.5 — 18 injected defect classes, each rejected for its expected reason | met |
| §13.15 — the **counted surfaces** this milestone changes are re-measured, not incremented | §3.6 — guard scripts 94 → **95**, quick 71 → **72**, CI 57 → **58**, quick-only **14** unchanged | met |

## 3. Production implementation evidence

### 3.1 WP-A — the two projection fields the renderer still dropped

The baseline's `TaskPanel` already rendered `projection.todos` and
`projection.jobs`, and the baseline's `SubagentPanel` rendered
`projection.subagents` and `projection.agentTree`. Two fields were still
dropped: `projection.runs` (`apps/desktop/src/bridge-types.ts:298`) and
`projection.recentTurns`
(`apps/desktop/src/bridge-types.ts:310`). Both are populated by the presenter
(`apps/desktop/src-tauri/src/present.rs:604`,
`apps/desktop/src-tauri/src/present.rs:700`) and ignored by the
renderer.

They are now rendered, in two new modules:

- `apps/desktop/src/panels/RunHistory.tsx` — the daemon's run records, each row
  showing the daemon's own status token and summary;
- `apps/desktop/src/panels/RecentTurns.tsx` — the recent turns, each row
  showing the turn's status token and summary.

The daemon's status tokens are rendered **as the daemon reports them**, not
re-derived by the renderer. A renderer-side mapping of statuses would be a
second copy of the daemon's vocabulary, and the two would drift.

### 3.2 WP-B — the region split

`apps/desktop/src/regions/` holds four modules:

| Module | What it is |
|---|---|
| `ChatRegion.tsx` | the transcript, the turn summary and the tool detail — the markup that left `App.tsx` |
| `SidePanelFrame.tsx` | the docked panel surface, its per-edge slots, and the panel controls |
| `ComposerRegion.tsx` | the prompt input and its submit |
| `StatusRow.tsx` | the connection and turn status |

`App.tsx` is the composition root and nothing else. It grew from 631 to 868
lines — but it grew by *wiring*, not by markup: the transcript markup is gone
(only one mention of "transcript" remains, in a comment), and the four regions
are composed at the end of the file.

The baseline's `TranscriptPanel`, `TurnSummary`, `ToolDetail`, `GitPanel` and
`SkillsPanel` keep their own surfaces and are composed inline. They are **not**
side-panel registry entries at this milestone: the registry ships **tasks**,
**subagents**, **activity** and **session**, and the plan says so.

### 3.3 The snapshot-state rule

`apps/desktop/src/panels/snapshotState.ts` encodes the rule the daemon's
driver skeleton forces: in the `connecting`, `subscribing`, `resyncing`,
`disconnected` and `unavailable` states the skeleton publishes **empty**
lists. An empty list in one of those states is "no snapshot installed yet",
not "this session has no jobs", and the two must not read the same. Each of
the two new surfaces therefore distinguishes them.

### 3.4 WP-C — the panel registry, the placement model, and the resize seam

`apps/desktop/src/panels/registry.ts` holds the `PanelId` union, the
`PANEL_REGISTRY` map and the `PanelPlacement` type. `PanelPlacement` is
`'right' | 'left' | 'bottom'` with `'right'` the default, mirroring the TUI's
own placement model (`src/tui/panels/mod.rs:219-229`,
`architecture/tui.md:3516-3518`) — the desktop mirrors the TUI's *shape*, not
its catalogue.

`apps/desktop/src/panels/host.ts` owns the per-edge slots and the
`dock(id, placement)` operation. `apps/desktop/src/panels/ResizeSeam.tsx` is
the pointer resize seam, with the `panel-resize-handle` test id and the
`aria-label`s.

**Placement is renderer-local.** The milestone's diff contains no
`localStorage`, no `sessionStorage` and no `indexedDB`: a persisted layout
would be a durable frontend-owned store, and this milestone has no storage or
configuration change for one. The panel-authority guard enforces it (§3.5).

### 3.5 WP-C's authority boundary — the static guard

`scripts/check_desktop_panel_authority.py` proves, against the renderer's
panel and region layers:

1. **No new `invoke` target.** The renderer's bridge surface is
   `apps/desktop/src/bridge.ts`, and the desktop's bridge targets are the
   recorded baseline (**23**, measured). No module under the panel or region
   layers may name a target outside that set, and no panel or region module
   may call `invoke` at all — the panels render props, they do not fetch.
2. **No Tauri capability** — no `@tauri-apps/api`, no `invoke`, no permission
   or capability identifier.
3. **No filesystem, shell or process call** — no `node:fs`, no
   `child_process`, no `exec`, no `spawn`, no `fetch`, no `WebSocket`.
4. **No durable frontend-owned store** — no `localStorage`, no
   `sessionStorage`, no `indexedDB`, no cookie write.

The guard **extends** `scripts/check-desktop-boundary.sh` rather than adding a
second file: the boundary guard's remit is already "the renderer holds no
authority of its own", and a second guard carrying a second copy of the same
rules would be two places to drift. It also gives the boundary guard the
`--self-test` it did not have — the plan requires one, and a guard whose
defects fail silently protects nothing.

Its `--self-test` builds its own fixture tree in a temporary directory and
injects **18 defect classes** across all four rules — a forbidden import, a
forbidden call, a forbidden member expression, an `invoke` outside the
baseline set, a newly invented bridge target, a `localStorage` write, and the
allowed forms that must **not** be flagged — and asserts each is rejected
**for the expected reason**, not merely that some finding appeared. It never
touches the repository's own files.

### 3.6 WP-D — the layout

`apps/desktop/src/style.css` loses the `max-width: 760px` single column for a
grid frame:

- `grid-template-areas` names the frame's regions — the chat, the side panel,
  the composer and the status row;
- a second `grid-template-areas` names the per-edge panel areas, so a panel
  docked on the left, the right or the bottom occupies its own area rather
  than displacing the chat;
- the custom properties `--panel-left-width`, `--panel-right-width` and
  `--panel-bottom-height` carry the panel geometry, and the resize seam
  writes them;
- the transcript and each panel get **independent scroll containers**, so a
  long transcript does not scroll the frame and a long panel does not scroll
  the chat.

### 3.7 The counted surfaces

| Surface | Before | After |
|---|---|---|
| Guard scripts (all) | 94 | **95** |
| Guards in `scripts/verify.sh quick` | 71 | **72** |
| Guards in `.github/workflows/ci.yml` | 57 | **58** |
| Quick-only difference | 14 | **14** — the new guard runs in both |

Every moved row was **re-measured**, not incremented. `architecture/overview.md`
also carried a **stale** count — it claimed 80 guards against a measured 94
before this change — which is corrected with the measurement recorded.

## 4. Verification executed

### 4.1 Local

| Command | Outcome |
|---|---|
| `apps/desktop/scripts/verify-desktop.sh` (the full local gate, on the milestone's head) | **exit 0** — boundary ✓, renderer ✓ (typecheck, **131** tests in 21 files, bindings ✓, build ✓), staging ✓, host ✓, built-app trajectory ✓ — lifecycle **5 passing**, autostart **2 passing**, session **7 passing** = **14 of 14** |
| `npm run typecheck` | **pass** |
| `npm test` | **pass** — 21 files, **131 passed** (was 69) |
| `npm run bindings:check` | **pass** |
| `npm run build` | **pass** |
| `./scripts/check-desktop-boundary.sh` | **pass** — "desktop dependency and authority boundaries verified" |
| `python3 scripts/check_desktop_panel_authority.py` | **pass** |
| `python3 scripts/check_desktop_panel_authority.py --self-test` | **pass** — 18 defect classes, each rejected for its expected reason |
| `python3 scripts/check_desktop_gate_wiring.py` and `--self-test` | **pass** — M007-A's guard is unaffected |
| `git diff --stat -- apps/desktop/src-tauri/` | **empty** — the desktop's Rust workspace has no diff |
| `git diff --stat -- apps/desktop/src/bridge.ts apps/desktop/src/bridge-types.ts` | **empty** |
| `git diff --stat -- apps/desktop/src/App.test.tsx` | **empty** — byte-identical |
| the `data-testid` inventory, measured over the whole renderer | **191 → 232, none removed** |

### 4.2 Hosted

The hosted qualification on the pull-request head that carries M007-A, whose
lane now includes the clippy step and the one local entry point M007-A added:

| Run | Head | Workflow | Conclusion |
|---|---|---|---|
| `40142935043` | `f11a5e33c` | Desktop E2E | **success** (20 m 29 s) — the three phases green, 5 + 2 + 7 = 14 passing |
| `40142935035` | `f11a5e33c` | Shared checkout macOS qualification | **success** |
| `40142935069` | `f11a5e33c` | Windows native probe contract | **success** |

M007-B's own hosted run is recorded on its pull-request head, which is stacked
on M007-A's: the renderer change is exercised by the same Desktop E2E lane,
and the lane's 14 built-app tests select on the `data-testid` values this
milestone had to preserve.

## 5. Invariant review

- **No protocol, storage, migration or product-behaviour change** outside the
  renderer's layout. `crates/codegg-protocol/**`, `crates/codegg-core/**` and
  `src/**` are untouched; `apps/desktop/src-tauri/**` has no diff; no
  migration was added.
- **No new bridge target, no new DTO field.** The desktop's 23 bridge targets
  are the recorded baseline. `projection.runs` and `projection.recentTurns`
  were already on the projection DTO — they were dropped by the renderer, not
  by the host, which is why no host change was needed.
- **The panel layer holds no authority.** No credential, provider, scheduler,
  job-admission or writer-permit surface; no `invoke`; no filesystem, shell or
  process call; no durable store. The guard enforces all four rules.
- **The built-app trajectory is the compatibility contract.** 191 → 232 test
  ids, none removed; 14 of 14 built-app tests pass locally after the DOM
  restructured.
- **The existing renderer test suite is unmodified.** `App.test.tsx` is
  byte-identical; the new tests are in new files.
- **No keyboard handling was introduced.** The resize seam is pointer-only at
  this milestone; that is recorded in §10 as an M007-C finding, not taken
  here.

## 6. Failure and recovery review

- **The renderer's own tests are the first line.** The suite grew from 69 to
  131 tests, and the region and panel modules have their own test files
  (`host.test.ts`, `registry.test.ts`, `ResizeSeam.test.tsx`,
  `RunHistory.test.tsx`, `RecentTurns.test.tsx`).
- **The built-app trajectory is the second.** A structural break in the
  renderer that the unit tests miss is caught by the e2e lane's 14 tests,
  because they select on the same `data-testid` values the milestone had to
  preserve.
- **A panel that cannot be reached is a defect, not a layout.** The panel
  registry's entries must all be reachable from the panel controls; the
  registry test covers the registry's own shape.
- **A stale override cannot half-apply.** Not applicable at this milestone:
  the keymap's persistence and its single-`setItem` commit are M007-C's, and
  panel placement is not persisted at all.

## 7. Migration and compatibility review

None required. The desktop's Rust workspace is unchanged, so the root
workspace's `cargo check` and the desktop's `cargo test` remain independent,
and the root MSRV is still 1.89. The renderer is a pure client of the
projection it already had.

## 8. Security review

- `scripts/check-desktop-boundary.sh` passes **unchanged**, and the panel
  layer's own authority boundary is now a guard rather than a promise.
- The panel layer adds no `invoke` target, so it adds no capability to call
  anything the daemon had not already been asked for.
- The panel layer persists nothing, so it adds no durable frontend-owned
  state to read back.
- The renderer shows the daemon's own status tokens and summaries; it does
  not re-derive or reinterpret them, and it renders no credential, no
  provider secret and no path the daemon did not already publish.

## 9. Documentation and operations

| Document | What changed |
|---|---|
| `architecture/desktop.md` | the deferred-surfaces list and the developer-commands section reflect the new frame and the one local entry point |
| `architecture/overview.md` | the counted surfaces: 95 guards, 58 in CI, 72 in quick, quick-only 14; a stale "80 guards" figure corrected with its measurement |
| `AGENTS.md` | the guard counts, and the new guard's entry in the "Automatic guards" list |
| `scripts/verify.sh` | the panel-authority guard and its self-test |
| `.github/workflows/ci.yml` | the panel-authority guard and its self-test |

## 10. Unresolved findings

| Finding | Severity | Owner |
|---|---|---|
| The **resize seam has no keyboard operation** — it is pointer-only, so a keyboard-only user cannot resize a panel. That is M007-C's work, and the milestone's §14 puts the modality question there. It is recorded here rather than taken here. | medium | M007-C |
| The panel's dock, close and resize controls live in the **catalog rail** rather than in the side-panel frame's header. The rail is the panel the user already has open when they want to reach another panel, so the control is where the user's attention is; but it is a layout decision the plan did not specify. | low | accepted for this milestone; the frame header is the alternative if the rail proves cramped |
| The **tasks surface and the subagents panel both list running subagents**. That mirrors the TUI's `Ctrl+T` overlay, which carries todos, live jobs and active subagents — so it is deliberate, not a duplicate. | low | accepted for this milestone |
| The **desktop Rust host does not build on a 1.89 toolchain**, because the desktop pins 1.90 for Tauri 2.12. The milestone's own verification therefore ran under `rustup run 1.90.0`, which is the pinned toolchain the hosted lane uses. | low | pre-existing; the desktop's pin is intentional |
| `SessionPanel.tsx` and `snapshotState.ts` are **two files beyond the plan's enumerated list**. The session surface and the snapshot-state rule are separable concerns with separable tests, and the plan's list is not exhaustive. | low | accepted for this milestone |

**No medium or high finding keeps M007-B open.** The one medium finding is
M007-C's work, recorded here so it is not lost.

## 11. Roadmap disposition

`plans/subsystems/desktop-frontend-ide-foundation-roadmap.md`:

- the milestone-007 decomposition table's M007-B row moves from *ready for
  handoff* to **closed**;
- the status row records M007-B as closed;
- the "what does not exist" paragraph is corrected: the renderer is no longer
  "one `App()` component of 407 lines rendering stacked `<section>` cards in a
  single 760-pixel column with **zero** keyboard handlers and twelve `onClick`
  bindings". It now has four named regions, a panel layer, and a grid frame.
  The **zero keyboard handlers** claim is unchanged and still true — that is
  M007-C.

## 12. Registry updates

`plans/registry.md`:

- the `## Dependency-ready implementation plans` row for M007-B moves from
  *ready for handoff* to **closed**;
- the `## Active subsystem roadmaps` desktop row records that the desktop has
  a chat region and a dockable side panel;
- the `## Current execution order and dependency gates` section records that
  M007-B is closed, so M007-C's keymap has the region model it binds to.

## 13. Handoff notes

- **The next milestone is M007-C**, and it is now unblocked in the sense that
  matters: the focus model has regions to bind to. The resize seam's missing
  keyboard operation (§10) is the one concrete thing M007-C inherits.
- **M007-D** (the `/` command surface) waits on M007-C's palette chord, and on
  ADR-0052, which was accepted on 2026-10-24.
- **M007-E** (the editor) waits on M007-B's region frame, which now exists.
- The panel layer's `PanelPlacement` and the registry's shape are what
  M007-D's palette and M007-E's editor will dock into; both should extend the
  registry rather than introduce a second placement model.
