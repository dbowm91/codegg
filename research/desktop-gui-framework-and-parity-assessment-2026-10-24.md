# CodeGG Desktop GUI — Framework Assessment and TUI-Parity Plan (Research)

Date: 2026-10-24
Scope: **research only** — no product code changed. The worktree was rebased onto
`origin/main`; nothing was written outside this document.
Method: direct reading of the CodeGG source at `a4bbba81e` (`origin/main`), three
delegated read-only sweeps (TUI feature surface, daemon wire-protocol surface,
desktop-app implementation state), and external framework research (Tauri 2.x,
Electron 4x, Wails, Rust GUI toolkits, WebKitGTK/WebView2 engine behaviour).

Every claim about the CodeGG tree is anchored `path:line`. Claims about external
projects are labelled with the source they came from.

> **Outcome.** The premise of the request — "build a desktop GUI for CodeGG" —
> is **already ~30% built, with the right architecture**, and the requested
> "TUI and GUI completely interchangeable with the daemon" goal is
> **architecturally achieved today**, not aspirational. The real work is
> *presenter breadth*, not architecture and not the shell.
>
> Three decisions follow:
>
> 1. **Keep Tauri.** The scarce asset is the 7,817-line Rust host that speaks
>    `codegg-client` directly, plus the guard-enforced trust boundary. An
>    Electron switch buys Chromium rendering consistency but adds a process/ABI
>    seam in exactly the place this repo has built a security model. Linux
>    WebKitGTK is the one genuine risk, and it gets a **measured gate** (§6.4),
>    not a rewrite.
> 2. **Fix the red E2E first** — it is a *stale fixture premise*, not a product
>    bug, and the fix is ~5 lines in the fixture (§5.2). A paused red lane makes
>    every subsequent parity change unverifiable.
> 3. **The parity gap is a presenter gap.** The TUI is 208,754 lines across 200
>    files; the desktop renderer is 1,299 lines of TypeScript. No framework
>    choice changes that ratio. §7 sequences the work against the real
>    checklist in §4.

---

## 0. The one-page answer

| Question | Answer |
|---|---|
| Is the worktree current? | **Yes.** `git rev-list --left-right --count origin/main...HEAD` → `0 0`. It was already at `origin/main` (`a4bbba81e`); no rebase was required. |
| Does a desktop GUI already exist? | **Yes** — `apps/desktop`: Tauri 2.12.1 + React 19 + Vite 8, 7,817 lines of Rust host, 1,299 lines of TS/TSX renderer, 19 narrow Tauri commands. Development was **paused 2026-10-23** (`architecture/desktop.md:154-175`). |
| Is it a mock? | **No.** It is a real vertical slice against the real daemon: connect, project/workspace select, session list/create/open, live projection push, permission and question responses, artifact excerpts, controller lease display. |
| Is "TUI and GUI interchangeable over the daemon" already true? | **The data plane is.** Both consume the same `CoreFrame` protocol and the same `SessionProjectionSnapshot`. `ClientKind::Gui` already exists (`crates/codegg-protocol/src/frames.rs:72-78`). |
| What is *not* interchangeable? | The **capability ceiling**: git, plugin invocation, skills catalogue, tool invocation and web search exist in the daemon but have **zero** `CoreRequest` variants (§3.4). Plus the remote `/tui` transport cannot carry `CoreRequest` at all (`architecture/protocol.md:593-600`). |
| What is the real gap? | **Presenter breadth.** 15 TUI surfaces have no desktop counterpart (§4.2). Todos/`Ctrl+T`, multi-turn transcript, model/provider selection, settings, LSP/editor and terminal are *absent*; subagents, runs and jobs are *stubs* — populated in the DTO and never rendered (§5.1). |
| Best frontend framework? | **Tauri, retained.** With a measured WebKitGTK gate before the renderer scales, and a named fallback that is *not* a rewrite (§6). |
| What must happen first? | **Green the hosted E2E lane.** The failure has a specific, cheap, code-level diagnosis (§5.2). |
| What has been implemented since? | **Phases 0–3.** The lane is green locally (all three phases, against a real daemon), the lane's triggers are restored, the presenter now carries the full projection contract (transcript, tool detail, subagents, background activity, todos) through virtualised React surfaces, and the daemon answers a read-only git + skills catalogue. See §5.2.1 for the measured E2E outcome and §8 for what was executed versus read. |

---

## 1. Starting state: the rebase

```
$ git rev-list --left-right --count origin/main...HEAD
0	0
$ git log --oneline -1
a4bbba81e Merge pull request #183 from dbowm91/fix/tui-composer-helix-parity-post-closure-corrective
```

The branch `feat/auto-20261024-6e9f52e4` was already identical to `origin/main`.
`git fetch origin --prune` reported no change. The working tree was clean, so
there was nothing to rebase and nothing to conflict. All findings below are
against `a4bbba81e`.

---

## 2. What already exists — do not rebuild it

### 2.1 `apps/desktop` — a real Tauri app

| Layer | Size | Location |
|---|---|---|
| Rust host | 7,817 lines across 8 files | `apps/desktop/src-tauri/src/` (`lib.rs` 2,143; `present.rs` 1,036; `route.rs`; `projection.rs`; `control.rs`; `prompt.rs`; `artifact.rs`; `bridge.rs` 287) |
| Renderer | 1,299 lines TS/TSX across 10 files | `apps/desktop/src/` (`App.tsx` 407) |
| Bridge | 88 lines | `apps/desktop/src/bridge.ts` |
| E2E | WebdriverIO + `@wdio/tauri-service` | `apps/desktop/e2e/` |
| Boundary guard | — | `scripts/check-desktop-boundary.sh` |

Stack: Tauri 2.12.1, React 19, Vite 8, TypeScript 7.

**The trust model is the most mature part of the design** and is enforced by
script, not convention (`scripts/check-desktop-boundary.sh`):

- Renderer Tauri permissions are an **empty array**
  (`apps/desktop/src-tauri/capabilities/main.json:6`).
- No `shell`, `filesystem`, `http:default`, `process:`, `updater`, `clipboard`
  or `global-shortcut` capability (guard `:19-22`).
- No generic machine-authority bridge — the guard greps both trees for
  `core_request|read_file|invoke.*shell` (guard `:23-26`); there are **19
  narrow commands**, each mapping to one typed `CoreRequest` variant.
- Canonical workspace roots never cross the bridge: `WorkspaceView` carries only
  `workspace_id` + `display_name` (`bridge.rs:66-73`); `roots` is host-only
  (`route.rs:41-43`, asserted at `route.rs:666`).
- The renderer cannot fabricate a permission choice
  (`control.rs:140-143` carries only an opaque id + choice), cannot fabricate a
  cursor (`bridge.rs:253-254`, non-authority), and cannot fabricate an artifact
  handle (`bridge.rs:218-219`).
- CSP must contain neither `unsafe-eval` nor any `https://` origin, and the
  window must load the local `index.html` (guard `:93-96`).
- The desktop is **not a workspace member** — the guard forbids `tauri` /
  `tauri-build` / `codegg-desktop` in root `cargo metadata` (guard `:5-8`). The
  desktop builds on **Rust 1.90** in CI while the root stays on **1.89**
  (guard `:9-12`).

### 2.2 `crates/codegg-client` — the reusable frontend client

The crate that makes "one client, many frontends" real. `architecture/client.md:5-12`
records that it *"contains no daemon authority"*, and `:25-26` states that
*"A headless GUI-kind consumer can use the same API without importing TUI or
daemon modules"*.

| Module | Provides |
|---|---|
| `local.rs` | `LocalSocketClient`, `ClientEvent` — multiplexed `CoreFrame` over the local socket |
| `driver.rs` | `SessionProjectionDriver` — subscribe/resume/ack/resync against the durable projection |
| `compose.rs` | `compose_turn_submit`, `PromptIntent` — the at-most-once prompt state machine |
| `document.rs` | `DocumentController` — the `document.v1` editor-document lifecycle |
| `lsp.rs` | LSP client helpers |
| `paths.rs`, `windows_pipe_security.rs`, `windows_process.rs` | platform plumbing |

### 2.3 The frontend-neutral protocol

`crates/codegg-protocol` is *"only data contracts: request/response/event DTOs …
no runtime services, no I/O, no daemon or scheduler authority"*
(`crates/codegg-protocol/src/lib.rs:1-11`).

| Layer | Type | Count | Site |
|---|---|---|---|
| Core request | `CoreRequest` | **266** | `crates/codegg-protocol/src/core.rs:2315` |
| Core response | `CoreResponse` | **174** | `core.rs:1202` |
| Core event | `CoreEvent` | **92** | `core.rs:4257` |
| Projection event | `ProjectionEvent` | **47** | `crates/codegg-protocol/src/projection/event.rs:122` |
| Remote-TUI message | `TuiMessage` | **40** | `crates/codegg-protocol/src/tui.rs:19` |
| In-process bus event | `AppEvent` | **61** | `crates/codegg-core/src/bus/events.rs:81` |
| Frame | `CoreFrame` | 8 kinds | `crates/codegg-protocol/src/frames.rs:226` |

`CoreFrame` travels over **three transports** carrying the *same* contract:
the local Unix socket / Windows named pipe
(`src/core/transport/daemon_socket.rs`), the **`/core` WebSocket**
(`src/server/http.rs:326`, `src/server/ws.rs:3981`), and child stdio
(`src/core/transport/stdio.rs`).

### 2.4 The governing documents — and the constraint that decides everything

- `plans/000-long-term-specification.md:97` — *"TUIs, ACP clients, web clients,
  and future frontends MUST render frontend-neutral snapshots and events."*
- `plans/000-long-term-specification.md:424` — *"Observation MUST render the same
  logical activity panel from frontend-neutral state. It MUST NOT mirror raw
  terminal screen frames. This permits terminal-size independence, replay,
  redaction, ACP compatibility, web clients, and selective disclosure."*
- `plans/002-long-term-roadmap.md` **Phase 5** — *"Create one canonical session
  view usable by local TUI, remote TUI, observer mode, ACP, web, and future
  clients."* Exit criterion: *"Two different frontend implementations can
  reconstruct the same session state."*
- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md` — the desktop
  line. M001–M006 are closed; the roadmap records *"Development paused
  2026-10-23"*.
- `architecture/desktop.md`, `architecture/client.md`, `architecture/projection.md`,
  `architecture/protocol.md`, `architecture/tui.md` — the authoritative detail.

**The `:424` constraint is load-bearing and kills the cheapest possible path.**
Embedding the real TUI in a terminal emulator (xterm.js and friends) would give
100% feature parity in a weekend. It is **forbidden by the repo's own
specification**, and it is also the wrong architecture: it makes the GUI
terminal-size-dependent, un-redactable, and un-replayable. Parity must be at the
*capability* level, with each surface rendering the shared semantic state in its
own layout.

---

## 3. The interchangeability architecture

### 3.1 What "interchangeable" already means here

The TUI is **not** a client of a TUI-shaped protocol for session state. It is a
consumer of the same durable projection the desktop consumes:

- `src/tui/commands/primary_projection.rs` subscribes via
  `CoreRequest::ProjectionSubscribe`, installs the returned
  `ProjectionSnapshotBundle`, applies `ProjectionEvent`s, and acks cursors.
- `src/tui/app/state/projection_client.rs` uses `ProjectionClientController`
  (`:29`) — a **protocol-crate** type, not a TUI type.
- `apps/desktop/src-tauri/src/projection.rs` uses the same
  `codegg-client` driver.

`architecture/protocol.md` records the one asymmetry that matters: `TuiMessage`
carries **no `CoreRequest` variant at all** (`crates/codegg-protocol/src/tui.rs:19-235`).
The `/tui` WebSocket is a narrow, terminal-shaped surface — `KeyDown`,
`MouseClick`, `Resize`, `Input` — and its `RenderFrame` variant is explicitly
**unsupported** (`architecture/protocol.md:604-605`). That is why the desktop
targets `/core` or the local socket, and why it must never open the socket
itself.

### 3.2 The projection contract

The projection is *"a **derived** frontend contract — never a second session
execution authority. The reducer never performs I/O, network, filesystem, or
provider calls"* (`architecture/projection.md:16-20`). Two compliant reducers
given the same snapshot and ordered event stream MUST reach equivalent values
(`architecture/projection.md:143-151`).

| Concern | Contract | Site |
|---|---|---|
| Snapshot | `SessionProjectionSnapshot` — session, workspace, active turn, recent turns, subagents, runs, jobs, tool programs, agent runs, worktrees, run groups, convergences, diagnostics | `crates/codegg-protocol/src/projection/snapshot.rs:29-81` |
| Subscribe | `ProjectionSubscribe` → `ProjectionSubscribed` **carrying the authoritative snapshot bundle** | `src/core/daemon_projection.rs:136-347` |
| Cursor | `ProjectionCursor { stream_id, event_seq, projection_version }` — **stream-local, never daemon-global** | `crates/codegg-protocol/src/projection/replay.rs:97`; `architecture/projection.md:154-160` |
| Ack | `ProjectionAck { subscription_id, cursor }` — advances retention; the only durable "I have seen this" | `src/core/daemon_projection.rs:584-620` |
| Resume | `ProjectionResume` — **re-checks observe authority on every resume**; unsubscribes on denial | `src/core/daemon_projection.rs:349-582` |
| Resync | 8 reasons: `HistoryExpired, HistoryGap, CursorAhead, StreamMismatch, ScopeMismatch, VersionMismatch, SnapshotUnavailable, SubscriberLagged` | `crates/codegg-protocol/src/projection/replay.rs:185-194` |
| Loss | 3 distinct signals — client `Lagged`, daemon `Gap`, wire `event_stream_gap_message` | `crates/codegg-client/src/local.rs:51-60`; `frames.rs:30-46` |
| Negotiation | `ProjectionCapabilities { min_version, max_version, ... }` — intersection semantics; `Unsupported`, never a silent downgrade | `crates/codegg-protocol/src/projection/caps.rs:35-85` |
| Bounding | 28 named caps — 16 sessions, 256 messages/turn, 32 tools/turn, 4 KiB strings, 8 KiB tool args | `architecture/projection.md:91-120` |
| Visibility | `Public \| ClientLocal \| Internal \| Sensitive` on every payload field | `architecture/projection.md:127-135` |

**Side-by-side use works by construction**: two frontends on the same daemon each
own a subscription with their own cursor. They observe the same events; neither
intercepts the other. The daemon's projection is the single authority, and
neither surface is a mirror of the other.

### 3.3 The two prompt paths

1. **`TurnSubmit`** — the *composed* form. The frontend resolves `model`,
   `agents`, `current_agent_idx` and `messages` itself
   (`crates/codegg-protocol/src/core.rs:2599-2619`). `compose_turn_submit`
   validates fail-closed, with `MAX_PROMPT_TEXT_CHARS = 200_000`
   (`crates/codegg-client/src/compose.rs:24`).
2. **`SessionPromptSubmit`** — the *minimal* form: *"the daemon resolves
   model/agents/messages from durable session selection and daemon-owned agent
   configuration… Narrow clients (desktop) send only user intent; they never
   resolve or supply provider identity"* (`core.rs:2627-2635`;
   `architecture/client.md:66-67`).

The desktop uses path 2. `PromptIntent` is the at-most-once state machine
`Captured → CreatingSession → Submitting → Accepted | Failed`
(`crates/codegg-client/src/compose.rs:161-332`); only `Failed` permits a retry
with the preserved draft.

### 3.4 The four things that still block true interchangeability

These are the *actual* remaining gaps — not the shell, not the renderer.

**G1 — The capability ceiling: daemon features with no wire surface.**
An exhaustive name scan of all 266 `CoreRequest` variants finds **no** git
operation (`git|diff|commit|branch|stage` matches only
`ConnectionRotateSecretStage` and `HumanShellPromotionStage` — false positives).
Likewise there is **no plugin invocation** request (the protocol crate defines
`PluginInvocation`/`PluginResponse`/`PluginManifestDto` in
`crates/codegg-protocol/src/plugin.rs`, but `grep "crate::plugin"` in `core.rs`
returns nothing — the plugin protocol is in-process only), **no skills
catalogue** request, **no tool-invocation** request (the `ToolRegistry` tools are
unreachable from any frontend), and **no web-search** request. Git, plugins,
skills and tools are reachable **only as agent tools** — the model can act, no
frontend can. *This is the single largest difference between the TUI's and the
GUI's ceilings.*

**G2 — The remote `/tui` transport cannot carry core requests.**
`architecture/protocol.md:593-600` names it: a frontend attached over the remote
WebSocket has no channel for `CoreRequest::ReasoningPreferenceSet`. Fix by
adding a `TuiMessage` variant that carries a core request, or by retiring `/tui`
in favour of `/core`.

**G3 — Secret-bearing and local-only requests are unreachable remotely.**
`is_secret_bearing` (`core.rs:4119-4131`) covers `EggpoolConnectionCreate`,
`ProviderConnectionCreate`, `ConnectionRotateSecretStage`, `ConnectionRotateBegin`
and `TeamTokenCreate`; `is_local_only` (`core.rs:4140-4148`) covers
`ScratchUsage`/`ScratchReclaim`. Over a remote transport these are denied
(`architecture/protocol.md:612-619`). *A remote GUI cannot create a provider
connection.* Over the **local** socket all 266 are reachable.

**G4 — No workspace-scope projection.**
`ProjectionStreamKind` has only `Session` and `Project`
(`crates/codegg-protocol/src/projection/replay.rs:24`). A GUI cannot obtain one
view of "everything running in this workspace"; it must subscribe per project
and merge client-side.

**Plus one evidence gap:** the desktop hosted E2E lane is paused and its last
`main` run failed (§5.2), so parity is currently a design claim with a red lane
behind it.

### 3.5 The reference frontends that already prove the pattern

`src/acp.rs` is a **third** frontend adapter — ACP v1 JSON-RPC translated onto
the same native daemon protocol (`architecture/acp.md:1-8`). The daemon, the
TUI, the desktop and ACP all consume one protocol crate. That is the strongest
available evidence that a fourth frontend (a web client, say) is a routine
addition, not an architectural gamble.

---

## 4. The TUI parity surface — the real checklist

### 4.1 Measured surface size

| Measure | TUI | Desktop renderer | Ratio |
|---|---|---|---|
| Lines | **208,754** (`find src/tui -name '*.rs'` + `wc -l`) | **1,299** TS/TSX | 161× |
| Files | **200** | 10 | 20× |
| Slash commands | **172** | 0 | — |
| `InputAction` variants | **62** | 0 | — |
| `TuiMsg` variants | **361** | 0 | — |
| `TuiCommand` variants | **1,279** | 0 | — |
| Dialogs | **53** | 0 | — |
| Side panels | **32** | 0 | — |
| Composer claims | **14** | 0 | — |
| `MsgPart` kinds | **6** | 0 | — |

**This ratio is the headline finding.** No frontend framework changes it. The
parity work is the *translation of a 208k-line terminal presentation layer into
a DOM presentation layer*, and it is bounded by the projection contract, not by
the shell.

### 4.2 The fifteen TUI surfaces with no desktop counterpart

1. **`/files` tree** — infinite-depth browse, peek/slide, `i` filter
   (`src/tui/components/dialogs/files.rs`).
2. **`/help` full help** — 172 commands (`:45`).
3. **`/tasks` + `Ctrl+T` overlay** — todos, live jobs, active subagents
   (the TUI's *only* task-state surface, per the "no status row of its own"
   rule).
4. **`/model` / `/provider` selection** — agents, models, reasoning effort,
   model profile.
5. **`/permissions` / `/approvals`** — rule editing, `AlwaysAllow` grants.
6. **`/git`** — status, diff, branch (TUI-local; see G1).
7. **`/tools`** — tool catalogue and configuration.
8. **`/mcp` / `/plugins`** — server/plugin management.
9. **`/skills`** — skill catalogue.
10. **`/search` / `/web`** — web search (see G1).
11. **`/agents`** — agent roster and configuration.
12. **`/memory`** — durable memory browse.
13. **`/worktree` / `/workspaces`** — worktree and workspace management.
14. **Human shell** — `!cmd` / `!!cmd` / `/shell-include` / `/shell-ask`.
15. **The sessions dialog** — list, switch, rename, archive, delete.

**The hardest-to-mirror group** (not "absent" — present but genuinely hard in a
DOM): the **Helix-style composer** (14 claim types, normal-mode verbs, visual
line selection, `Ctrl+T` as tasks rather than transpose, `Esc` as mode switch);
**mouse** hit-testing and scroll semantics; **Terminal color and glyph**
emulation (CodeGG ships no hardcoded selection colours —
`scripts/check_no_hardcoded_selection_colours.py`); and the **terminal-size
independence** that `:424` explicitly requires.

### 4.3 Divergence between `architecture/tui.md` and the code

`architecture/tui.md` is **accurate** on the architecture but **incomplete** on
the surface inventory: it does not enumerate the 172 slash commands, the 62
`InputAction`s or the 361 `TuiMsg`s. Any parity plan built from the doc alone
will under-scope itself. The source is the truth; the doc is a guide.

Two specifics worth carrying into the plan:

- **The composer's keymap is polymorphic by mode** — `Escape` (mode switch),
  `Ctrl+T` (tasks overlay), `Shift+Q` (panel exit), `Ctrl+C` ×2 (exit arm),
  `Alt+Enter` (newline) — and **frequently disagree with the TUI**. The
  composer *is* the highest-churn surface in the codebase; its corrections are
  recent and repeated.
- **The TUI's own test suite is a much better parity oracle than the TUI's
  code.** `tests/tui_render/` is a **41-member family** — one member per screen
  or surface — and each member asserts a rendered frame. A GUI parity plan can
  reuse this as the **golden fixture set**: render the same semantic state in
  React and assert the same observable properties the TUI family asserts.

---

## 5. The desktop app's real state

### 5.1 Implemented vs. missing

Legend: **impl** = real end-to-end path (renderer → Tauri command → daemon
request → projection → renderer); **partial** = works but narrower; **stub** =
shape exists, value never rendered; **absent** = no code.

| Capability | Status | Evidence |
|---|---|---|
| Daemon connect / identity | **impl** | `bridge.ts:19` → `lib.rs:654-772`; renderer `App.tsx:384-385` |
| Projects list | **impl** (cap 50) | `lib.rs:774-807` (`MAX_PROJECTS = 50` at `lib.rs:35`) |
| Project detail / route select | **impl** | `route.rs:162` (`ProjectGet`); `App.tsx:172-189` |
| Workspace select | **impl** | `route.rs:244-246`; `App.tsx:191-212` |
| Session list | **impl** (cap 50) | `route.rs:281` (`MAX_SESSIONS = 50` at `route.rs:39`) |
| Session create / open | **impl** | `route.rs:328` (`SessionAttach`), `route.rs:379` (`SessionCreate`) |
| Live projection push | **impl** | `projection.rs:235-320` (coalescing at `:41-42`); `App.tsx:125-170` |
| Permissions — respond | **impl** | `control.rs:140` (`PermissionRespond`); `App.tsx:304-323` |
| Artifacts | **impl** | `artifact.rs`; `App.tsx:398,403` |
| Transcript | **partial** — current turn only | `present.rs:230-233` (`active_turn` **or** `recent_turns.front()`); older turns' messages never render |
| Tool activity | **partial** — summary only | `present.rs:151-193`; args/output withheld (`:129`, `:155`); no diff view |
| Questions | **partial** — raw JSON textarea | `App.tsx:402`; no option picker |
| Controller lease | **partial** — display only | `SessionPresentationView.controller` hardcoded `None` at `present.rs:364` |
| **Subagents** | **stub** | `present.rs:336-354` reads `active_turn.agent_tree` but **never** the snapshot's top-level `subagents` — which the protocol calls the liveness authority that *"survive[s] parent-turn replacement"* (`crates/codegg-protocol/src/projection/snapshot.rs:52-55`). Populated in the DTO, **never rendered** |
| **Runs / jobs** | **stub** | `present.rs:310-335` populates them; `App.tsx` never renders them |
| **Todos / `Ctrl+T`** | **absent** | No mention anywhere in the desktop tree |
| **Model / provider selection** | **absent by design** | No bridge command. The renderer "never carries provider authority" (`m004-session.e2e.ts:157-159`) |
| **Settings / config** | **absent** | No command, no panel |
| **LSP / editor / diff** | **absent** | Nothing in the tree, despite `codegg-client`'s `lsp.rs` and `document.rs` existing |
| **Terminal** | **absent** | Nothing |
| **Human shell** | **absent** | Not present |
| **Multi-turn transcript** | **absent** | Only the current turn renders |
| **Reasoning / system messages** | **absent (deliberate)** | `present.rs:79-87` — only `Public` + `User`/`Assistant`/`Tool` roles cross |

### 5.2 Why the hosted E2E fails — a stale fixture premise, not a product bug

`.github/workflows/desktop-e2e.yml:20-27` records it exactly:

> `main`'s own most recent run, `39679968444` at `53d9fc9fd`, fails with
> `element ("[data-testid=\"permission-list\"]")` still not existing after
> 120000ms` at `apps/desktop/e2e/specs/m004-session.e2e.ts:169:5` … and `main`'s
> history behind it is nine consecutive failures with one older success.

The lane was paused 2026-10-23 (`:14-17`) and is manual-dispatch only (`:52-53`).

**The chain of facts:**

1. The spec expects the mock's first response — a `write` tool call to a path
   outside the probe workspace — to raise a **human** permission, then waits for
   it at `m004-session.e2e.ts:169`.
2. The mock's write target is under the phase `HOME`, which is under `/tmp`
   (`apps/desktop/e2e/run-e2e.sh:85` → `mktemp -d "${TMPDIR:-/tmp}/..."`;
   `apps/desktop/src-tauri/src/bin/desktop_e2e_fixture.rs:259`).
3. `src/tool/util.rs:177-207` (`relative_to_known_temp_root`) classifies that
   path as a **known temp root**.
4. Commit **`39e1445b6` (2026-10-20)** — *"feat(permission): answer the model
   with a temp-root redirect before the human"* — landed in `main`
   (`e52695b9c` / `f71a36fe6` / `568f7b4b2`). `git merge-base --is-ancestor
   39e1445b6 53d9fc9fd` → **yes** (it is in the failing SHA);
   `git merge-base --is-ancestor 39e1445b6 995d5cf8d` → **no** (it post-dates the
   passing M004 closure of 2026-10-03).
5. The commit message states the intent verbatim: *"Observed live: **a write to
   /tmp produced a permission_pending event** and the turn stayed parked for
   eleven minutes before failing."* — i.e. the very event the E2E waits for was
   **deliberately eliminated** for `/tmp` writes.
6. The new path answers the **model**, not the human:
   `src/permission/mod.rs:1365-1411` (`temp_root_redirect_offer`) fires for tool
   `write` (`:1376`) on a known temp root (`:1381`) outside every authorised root
   (`:1384-1386`); `src/agent/tool_batch.rs:829-855` checks the offer **first**
   on `PermissionResult::Ask` and returns `ToolPermissionOutcome::Denied`
   **without ever raising the human permission**.

**Conclusion (high confidence):** the daemon never raises a human permission for
this call, so `projection.pendingPermissions` stays empty, so
`App.tsx:401` never renders the `<ul data-testid="permission-list">`, and the
WebDriver wait burns its full 120 s. The symptom is a *single* failed
assertion at a *single* line — exactly the observed failure shape.

**The product behaviour is correct and must not be reverted.** The fix belongs
in the fixture. The strongest option is **(b) make the mock model insist**: the
one-offer rule is per `(session, tool, target)`
(`src/permission/mod.rs:1393-1409`), so the *second* identical write attempt
"reaches the human". Changing `desktop_e2e_fixture.rs:227-231` to repeat the
same write on request #2 restores the `permission-list` assertion **and** pins
the one-offer-per-target rule — strictly better coverage than before.

#### 5.2.1 Measured, 2026-10-24 — what the local reproduction actually showed

The baseline was reproduced exactly on macOS (same line, same 120 s timeout),
and three facts that the assessment above did not have came out of it.

**(a) The mock was never reached at all in the first local attempt.** The
daemon's durable `core_event_log` in the phase home recorded the real turn
outcome:

```
turn_started
turn_failed  "general error: managed scratch path is not a real directory:
              <phase home>/fake-home/Library/Caches"
```

The managed-scratch allocator refuses any path containing a **symlinked
component** (`src/scratch.rs:911-918` — the scratch tree must not be
redirectable), and it derives the scratch root from the cache directory under
`HOME`. macOS puts `$TMPDIR` under `/var` → `private/var`, so a fake home
under `$TMPDIR` contains a symlinked component and **every turn fails before
any provider is called**. On Linux (`/tmp`, a real directory) this never
fires — but it means the lane could never have been green on macOS.
Fixed in the harness: `run-e2e.sh` now resolves the fake `HOME` physically.
*Product observation, not fixed:* a user whose `HOME` contains a symlinked
component cannot run a turn at all. The refusal is deliberate; the
portability limit is worth recording.

**(b) The number of human rounds is platform-dependent — and the macOS path
never reaches the redirect offer at all.** `crates/eggsentry/src/command.rs:161`:

```rust
static SYSTEM_PATH_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^/(etc|usr|var|sys|proc|dev)/").unwrap());
```

- **Linux**: the phase home is under `/tmp` → no match → the write is Low risk
  → the permission layer's implicit `Ask` is reached → the redirect offer
  answers the **model** → the human is asked only on the **second** identical
  attempt.
- **macOS**: the phase home is under `$TMPDIR` → `/var/...` → **match** → a
  High-severity `DestructiveFilesystem` finding → the security hint
  **escalates** → `src/agent/tool_batch.rs:829` consults the redirect offer
  only in the `Ask` arm, so the escalate branch answers the **human
  directly** → the human is asked on the **first** attempt.

Measured on macOS: the durable event log shows `permission_pending` for
`call_e2e_write_1`, a denial, then `permission_pending` for
`call_e2e_write_2`. The `/var` in that regex is Linux-shaped; the same
`crates/eggsentry/src/security/sandbox.rs:1195-1201` comment already notes
that a `/var` deny "would block the per-user temporary directory" on macOS.

**(c) The fix therefore has to answer every surfaced permission, not the
first.** The mock insists on the same path (so the Linux lane reaches the
human on its second attempt), and the spec answers **every** pending
permission the desktop renders until the turn converges. The trajectory is
then identical on both platforms, and the assertion becomes the one that
actually matters: *a pending permission is rendered, it can be answered from
the desktop, and the turn converges afterwards.*

**Verified outcome (2026-10-24, macOS, session phase):** `7 passing (3.8s)` —
the lane that failed `6 passing / 1 failing` at `m004-session.e2e.ts:169`
now passes, with the mock logging four chat completions and the durable event
log showing two answered permissions and a `turn_completed`.

### 5.3 Live bugs and dead payload in the current desktop tree

| Issue | Site | Impact |
|---|---|---|
| `pendingPermissions` has **no status filter** — it lists every status, not just `Pending` | `present.rs:282-290`, vs. the correct `Pending`-only count at `present.rs:107-111` | The renderer shows Allow/Deny buttons on **already-answered** permissions. `bridge.rs:196` even says *"Pending (or recently resolved) permission"* — the name and the filter disagree |
| Same for `pendingQuestions` | `present.rs:291-298` | Same |
| `SessionPresentationView.controller` is **always `None`** | `present.rs:363-364` (comment: *"WP E wires SessionControlGet into this field"* — WP E shipped, in `control.rs`/`projection.rs:213-229`, not here) | Dead field; stale comment at `bridge.rs:242-244` |
| `runs`, `jobs`, `subagents`, `recentTurns` populated but **never rendered** | `present.rs:310-360` vs. `App.tsx` | Dead payload — host CPU and bytes on every push, for nothing |
| `bridge.snapshot` and `bridge.disconnect` are **never called** by the renderer | `bridge.ts:20,46` | Dead renderer API |
| `RunSummaryView` / `JobSummaryView` / `SubagentSummaryView` TS types | `bridge-types.ts:95-112` | Types for the dead payload |

These are cheap, high-value fixes and belong in the same pass as the E2E fix.

---

## 6. Framework assessment

### 6.1 The candidates and what the evidence says

| Candidate | Size | Host | Renderer engines | Measured notes |
|---|---|---|---|---|
| **Tauri 2.12.x** | 3–15 MB | **Rust** | **3 engines**: WKWebView (macOS), WebView2 (Windows), WebKitGTK (Linux) | 8.0 MiB raw binary, 204 crates, 36 s clean build, 211 MiB max RSS, 14.0% avg CPU in an empirical cross-framework study of the same app |
| **Electron 43** | 50–150 MB | Node | **Chromium everywhere** | Consistent rendering; the choice OpenCode made when moving *from* Tauri on exactly these grounds |
| **Wails 3** | ~10 MB | Go | 3 engines | Same Linux engine exposure, no Rust host |
| **Rust GUI** (iced / egui / slint / gpui / dioxus / xilem) | 5–15 MB | **Rust** | own renderer | Difficulty ranking from the same study (lower = easier): egui 14 ≈ slint 14 < iced 16 = gpui 16 < **tauri 17** < dioxus 18 < xilem 21. Iced: 22 s build, 9.9 MiB, 140 crates, 74 LoC Rust, 95 MiB RSS, **lowest CPU (3.5%)**. Slint has AccessKit |

### 6.2 The three criteria that decide it for CodeGG

**C1 — Does the host stay Rust, and can it keep the trust boundary intact?**
This is the strongest constraint. The desktop host *is* the daemon client: it
speaks `codegg-client` (`LocalSocketClient`, `SessionProjectionDriver`,
`DocumentController`) directly, in the same process, with the same crate the TUI
uses. Under Electron the Rust host survives only as a **NAPI module** (painful
tokio/N-API bridging) or a **sidecar process** (an extra process, an IPC hop, a
second trust boundary, and a second thing to qualify). The repo's entire
boundary story — 19 narrow commands, empty Tauri permissions, host-only
canonical roots, generation fencing on both connection and route — is enforced
by `scripts/check-desktop-boundary.sh` against a **Tauri-shaped** model.

**C2 — Can it render a dense, continuously-updating, scrolling transcript at
60 fps on every supported platform?**
This is the strongest argument *against* Tauri, and it is real: on WebKitGTK
2.44+ with DMABUF disabled (the NVIDIA case), the webview falls back to
non-composited rendering and CPU-paints the dirty region every frame — measured
at **40–90 ms frames at 2560×1440**. That is the exact surface CodeGG's GUI is
made of. WebKitGTK also lags on CSS: `zoom` vs `transform: scale()` differs,
`filter: blur()` can silently fail, and general CSS fragmentation produces
different layout than Chromium/WebKit. A dense, custom, IDE-class layout is
the **worst case** for webview divergence — not a marketing page.

**C3 — Is the widget ecosystem sufficient for diffs, code, markdown and a
terminal pane?**
The web ecosystem wins outright: Monaco/CodeMirror for diffs, Shiki for syntax
highlighting, `xterm.js` for a terminal pane, `react-virtualized` for the
transcript. Rust GUI toolkits (iced, egui, slint, gpui) each ship their own
widget set, which would mean rebuilding diff viewing, code highlighting, markdown
rendering and terminal emulation *in the framework* — a large, separate project.

### 6.3 Recommendation: **keep Tauri**, with Linux as the measured risk

Tauri wins C1 and C3 decisively and loses only on C2, and only on Linux.

- **C1 is dispositive.** The Rust host is the hard-won asset: it holds daemon
  authority, owns projection subscriptions, fences generations, and is guarded
  by script. The renderer is only 1,299 lines and is *portable* — the shell is
  where the sunk cost is **not**. Switching shells before the presenter even
  exists is premature by construction.
- **C3 is answered by the web renderer**, which is what Tauri already uses.
- **C2's Linux leg is real but bounded and mitigable.** macOS (WKWebView) and
  Windows (WebView2 = Chromium) are both good; Linux is the one weak leg, and
  the mitigations are known and cheap: a **virtualised transcript** (never one
  DOM node per message), a **WebKitGTK-safe CSS baseline** (no `zoom`, no
  `filter: blur()`, no `backdrop-filter`, no container queries), and an
  explicit **Linux renderer policy** for the DMABUF case.
- **The desktop app already works.** It is a real slice, not a mock. A rewrite
  discards working, guarded, tested code.

**Corollary:** React 19 is the right renderer framework and should not change.
For a dense streaming surface the correct answer is **virtualisation**
(`@tanstack/react-virtual`), not a framework swap — a React re-render per
streaming delta is exactly what virtualisation solves. Do not re-litigate the
renderer framework; do budget for virtualisation from day one.

### 6.4 The gate — measure Linux before scaling the renderer

Because C2 is the one open risk, make it a **measurement**, not an opinion.

**Gate G1 (before the renderer scales past the current slice):** build the
densest real surface — a scrolling transcript with tool cards, a permission
dialog and a composer — and measure on Linux/WebKitGTK (Ubuntu 24.04,
WebKitGTK 4.1, the same image the CI lane uses) and on macOS:

- scroll frame time (p50/p95) with 1,000 messages rendered, virtualised;
- first paint and re-layout cost on a streaming delta;
- a **WebKitGTK-safe CSS baseline** applied and verified.

**Decision rule.** If Linux frame time is interactive (p95 ≲ 33 ms) with
virtualisation → **stay Tauri**, full stop. If it is not, and Linux is a
first-class audience → the fallback is **Electron with the Rust host as a
sidecar** — explicitly **not a rewrite**, because the host is
transport-agnostic behind its command layer: `present.rs`, `route.rs`,
`projection.rs`, `control.rs`, `prompt.rs` and `artifact.rs` are untouched by
the shell, and only the 19-command IPC plumbing plus the E2E harness change.

Run G1 **once**, early. It is cheap, and it is the only question that could
change the shell recommendation.

### 6.5 The path this repo's own specification forbids

For completeness: the cheapest way to 100% TUI parity is to embed the real TUI
in a terminal emulator. **`plans/000-long-term-specification.md:424` forbids it**
("It MUST NOT mirror raw terminal screen frames"), and it would forfeit
terminal-size independence, replay, redaction and selective disclosure — all
four are named there as the reasons the rule exists. Do not revisit this.

---

## 7. The plan

### Phase 0 — Make the lane trustworthy (prerequisite; ~1 day)

Nothing in Phases 1+ is verifiable until this is green.

1. **Confirm the E2E diagnosis** with one manual-dispatch hosted run, checking
   `apps/desktop/e2e/logs/<phase>-fixture.log` for
   `mock-model: chat completions request #N` (§5.2).
2. **Fix the fixture, not the product**: make the mock model repeat the same
   denied write on request #2 (`desktop_e2e_fixture.rs:227-231`), which restores
   the `permission-list` assertion and pins the one-offer-per-target rule.
3. **Restore the lane triggers** per `.github/workflows/desktop-e2e.yml:32-50`
   (the exact recipe is preserved in the file).
4. **Fold in the §5.3 cleanups**: add the `Pending`-only filter to
   `present.rs:282-298` (a live UX bug), populate or delete
   `SessionPresentationView.controller` (`present.rs:364`), and drop the dead
   payload — or, better, *render* it in Phase 1.

### Phase 1 — Close the data layer (the largest gap; no new UI yet)

Parity is bounded by what `present.rs` reads from the projection.

1. **Subagents** — read `snapshot.subagents` (the liveness authority,
   `crates/codegg-protocol/src/projection/snapshot.rs:52-55`) in addition to
   `active_turn.agent_tree` (`present.rs:336-354`).
2. **Background activity** — surface `agent_runs`, `tool_programs`, `worktrees`,
   `run_groups`, `convergences` (`snapshot.rs:66-79`), all currently ignored.
3. **Multi-turn transcript** — `present.rs:230-233` renders one turn; the TUI
   renders history. The projection already carries `recent_turns`; extend the
   tail caps deliberately.
4. **Todos / `Ctrl+T`** — the TUI's *only* task-state surface. Requires a
   projection read plus a new panel.
5. **Tool detail** — the TUI shows args/output; the desktop withholds them
   (`present.rs:129,155`). Decide the redaction policy deliberately, then
   implement diff viewing.
6. **Reasoning** — `present.rs:79-87` drops it by policy. That is a *decision to
   revisit*, not an oversight.

### Phase 2 — The presenter (the bulk of the work)

Build the React counterparts of the §4.2 surfaces, in TUI-priority order, with
**virtualisation and the WebKitGTK CSS baseline from day one** (Gate G1).

**Reuse the TUI's own test family as the oracle:** `tests/tui_render/` is a
41-member family asserting a rendered frame per surface. For each desktop
surface, assert the same observable properties against the same semantic state.
That converts a fuzzy "feels like the TUI" into a mechanical parity check, and
it is the highest-leverage reuse available in this repo.

### Phase 3 — Raise the capability ceiling (G1–G4)

These are the *protocol* work that makes "all underlying features" true:

| Gap | Work | Site |
|---|---|---|
| Git | Add read-only `GitStatus`/`GitDiff` requests; `codegg-git` already has the typed ops and risk model | `crates/codegg-git`, `src/git_mutations.rs` |
| Plugins | Add list/inspect/invoke requests; the DTOs already exist | `crates/codegg-protocol/src/plugin.rs` |
| Skills | Add a catalogue read | `crates/codegg-protocol/src/runtime_assets.rs` |
| Tool invocation | Decide whether a frontend may invoke a tool directly, or only the model | `CoreRequest` |
| `/tui` core channel | Add a `TuiMessage` variant carrying a `CoreRequest`, or retire `/tui` for `/core` | `crates/codegg-protocol/src/tui.rs` |
| Remote secrets | Decide the policy for secret-bearing requests over a remote transport | `core.rs:4119-4148` |
| Workspace projection | Add a workspace scope to `ProjectionStreamKind` | `crates/codegg-protocol/src/projection/replay.rs:24` |

### Phase 4 — Optional: the web client

The cheapest possible third surface, and the one that most directly serves
"developers use both side by side". The daemon **already** exposes `/core` over
WebSocket with the full `CoreFrame` contract, and `ClientKind::Web` already
exists (`frames.rs:72-78`). A browser client speaking `CoreFrame` needs no
native code at all. It is the natural proof that the protocol is genuinely
frontend-neutral.

### Phase 5 — Side-by-side verification

Two frontends on one daemon, each with its own subscription and cursor,
observing the same events without intercepting each other. Add an integration
test that drives **one** session through the **TUI** and the **desktop** in
parallel and asserts both reconstruct equivalent state — this is Phase 5's
stated exit criterion (*"Two different frontend implementations can reconstruct
the same session state"*) and it is finally testable.

**The constraint that shapes it:** the desktop crate is **excluded from the root
workspace** (`Cargo.toml:6`), and `scripts/check-desktop-boundary.sh:5-8` fails
if `codegg-desktop` appears in the root's `cargo metadata`. A root-package
integration test therefore **cannot import the desktop crate** — the two
frontends cannot be driven from one test binary.

The parity proof is therefore split across two crates against **one shared
golden**:

1. **The canonical input is the daemon's snapshot.** Both frontends derive
   their view from `SessionProjectionSnapshot`
   (`crates/codegg-protocol/src/projection/snapshot.rs`). The TUI applies
   `ProjectionEvent`s locally through
   `src/tui/app/state/projection_client.rs:344` (`apply_envelope`) and can
   seed from a snapshot (`:133` `from_snapshot`); the desktop calls
   `apps/desktop/src-tauri/src/present.rs:488` (`present_snapshot`). Neither
   front-end reaches into the other's state, and neither invents state.
2. **One golden, two assertions.** A single JSON fixture holds the canonical
   snapshot; a second holds the expected canonical state (message roles and
   texts, pending permissions, turn status, subagents, background activity).
   The root package's new family asserts the **TUI**'s reconstruction against
   it; the desktop crate's integration test asserts the **desktop**'s
   reconstruction against the *same* file. Same input, same expected output,
   two independent implementations — which is the exit criterion, mechanically.
3. **The live leg** is the desktop E2E lane itself: the fixture
   (`apps/desktop/src-tauri/src/bin/desktop_e2e_fixture.rs`) is a **TUI-kind**
   observer client and the desktop app is a **GUI-kind** client on the *same*
   daemon, each with its own subscription and cursor, neither intercepting the
   other. The session phase now asserts the GUI renders the turn the TUI-kind
   observer saw, end to end, against a real daemon.

#### 7.1 What the proof found (2026-10-24) — and one premise that was wrong

The proof is built (`tests/frontend_parity/` — 6 root tests — and
`apps/desktop/src-tauri/tests/frontend_parity.rs` — 5 desktop tests — against
one golden at `tests/frontend_parity/golden/session_projection_parity.json`),
and it found something the assessment above had assumed.

**The TUI is not a visibility boundary, and the desktop is.** This
assessment's premise — that both frontends apply the same visibility policy to
the same projection — is **only half true**:

- the **TUI** fills `messages_state` from the **raw event stream**, not from the
  projection snapshot. Its projection client holds a non-public message
  verbatim, and its rendered transcript stays empty. The sanitisation for a
  headless client happens **daemon-side**
  (`crates/codegg-protocol/src/projection/consumer.rs:766-773`).
- the **desktop** filters **again on the client**
  (`apps/desktop/src-tauri/src/present.rs:134-157`), so a `reasoning` /
  `internal` message and a `client_local` tool never reach its renderer.

The difference is pinned mechanically, not in prose: the golden carries the
leak bait, and
`tui_transcript_is_not_reconstructed_from_the_projection_snapshot`
(`tests/frontend_parity/tui_reconstruction.inc:327`) asserts both halves of
it. **What this means for interchangeability:** the daemon is the boundary
for a TUI-kind client, and the desktop is a second, stricter boundary on top.
Two frontends with *different* visibility policies is a divergence in the
parity contract, not in the implementation — and it is the one thing Phase 5
found that the plan did not know.

Two further asymmetries are reported rather than hidden: the desktop's
**tail caps** (messages 100, tools 20, subagents 10, jobs 10, recent turns 5
— `present.rs:50-99`) and the TUI sidebar's **head caps** (subagents 16
`src/tui/components/sidebar.rs:197`, jobs 12 `:198`) mean that above ten
subagents the two frontends show *different items*. The golden deliberately
stays below that threshold; the divergence is recorded here, not papered over.

---

## 8. Risks, and what I did not verify

**Verified by reading (source-level):** every CodeGG claim above is anchored to
`path:line` and was read directly. The three delegated sweeps were read-only;
no file was modified.

**Verified by execution (2026-10-24, the implementation pass):**

1. `scripts/verify.sh quick` — **passes** on the tree that carries Phase 1
   (data layer), Phase 3 (the daemon-side git + skills slice), the Phase 0
   E2E fixes, and the architecture-doc corrections. That is rustfmt, the two
   generated-asset checks, the 63 guard scripts, and a workspace
   `cargo check --workspace --all-targets --locked`.
2. The desktop **E2E suite**, all three phases, against a real daemon —
   **14 passing, 0 failing** (lifecycle 5, autostart 2, session 7), after the
   fixture insist, the answer-every-permission spec loop, and the harness's
   physical fake `HOME` (§5.2.1). The baseline for the same suite was
   `6 passing / 1 failing` with a 120 s timeout.
3. The desktop's own gates: `npm run bindings:check`, `npm run typecheck`,
   `npm test` (**68 passed / 10 files**), and the host crate's
   `cargo test --lib` (**90 passed, 0 failed, 1 ignored**).
4. The Phase 3 slice's own suites: `cargo test -p codegg-core` (933 passed),
   `cargo test -p codegg-protocol` (312 passed), `cargo test --lib
   daemon_reads` (8 passed), and the authorization/audit/writer-disposition/
   execution-ownership/core-boundary guards.
5. The Phase 5 proof: `cargo test --test frontend_parity` (**6 passed**) and
   the desktop crate's `cargo test --test frontend_parity` (**5 passed**),
   against one shared golden — and the visibility-policy divergence §7.1
   records.

**Still not verified:**

1. The **hosted** run. The lane was un-paused and the local reproduction is
   green, but only a hosted run proves the Linux trajectory — where the
   temp-root redirect *does* fire and the human is asked on the **second**
   attempt — converges. The local run exercised the macOS trajectory, where
   `/var` is a system path and the human is asked on the **first** attempt
   (§5.2.1b). The fixture and the spec handle both.
2. **Six workspace tests fail on macOS, and they failed before this work
   started.** The broad sweep is `16,144 tests: 16,137 passed, 7 failed, 9
   skipped`; six of the seven failures are reproducible on the **pristine**
   tree (`git stash` → `a4bbba81e` → the same six fail → restore), and the
   seventh (`command_git::git_read_consistency::a_repeat_read_of_an_unchanged_
   repository_is_served_from_the_capture_cache`) passes on a re-run. All six
   are the **same macOS fact** this document keeps running into: `/tmp` and
   `/var` are symlinks (`/tmp` → `private/tmp`, `/var` → `private/var`), and
   the product deliberately refuses a path with a symlinked component —
   `register workspace: InvalidRootPath("/tmp")` in
   `src/core/daemon_refresh.rs:1114`, the scratch-cache-root contract in
   `tests/scheduler_jobs/scratch_cache_root_contract.inc`, and the sandbox
   gate, which says so outright: *"protected mount view is available only on
   Linux"*. None of them is in a file this work touched.
3. The `/tui` ↔ `/core` capability asymmetry (G2) is asserted from
   `architecture/protocol.md:593-600`; I confirmed the structural cause
   (`TuiMessage` has no `CoreRequest` variant) but did not exercise the
   endpoint.
4. The desktop E2E pause is a repo-state claim read from
   `architecture/desktop.md:154-175` and `.github/workflows/desktop-e2e.yml`; I
   did not query GitHub Actions.
5. Framework measurements in §6.1 come from published comparisons and vendor
   documentation, not from a CodeGG-specific benchmark. **That is precisely why
   Gate G1 exists** — the CodeGG-specific measurement has not been taken yet.
6. `architecture/protocol.md:131-274` cites `CoreEvent` at `core.rs:4162` while
   the declaration is at `core.rs:4257`; `architecture/client.md:51` cites
   `driver.rs:483` (actual `:498`) and `compose.rs:123`/`:110` for
   `PromptIntent`/`PromptIntentState` (actual `:174`/`:161`). The *counts* in
   those documents matched my independent extraction exactly, so this is doc
   line drift, not a counting error.

**The three risks that matter most:**

| Risk | Why it matters | Mitigation |
|---|---|---|
| The parity estimate is under-scoped | The TUI is 208k lines; §4.2's fifteen surfaces are a *count*, not a *cost* | Use §4.1's measured counts and `tests/tui_render/`'s 41 members to size each surface before committing to it |
| WebKitGTK cannot carry a dense streaming transcript | It is the product's main surface, and Linux is a supported platform | Gate G1, early; virtualisation and a CSS baseline from day one; the Electron-with-sidecar fallback is *not* a rewrite |
| The red lane stays red | Every parity change becomes unverifiable | Phase 0, before anything else |

---

## 9. The one-sentence version

**CodeGG already has the GUI's architecture, the shared protocol, and a working
vertical slice — keep Tauri, green the paused E2E lane (a stale fixture premise,
not a product bug), and spend the effort where the real gap is: a
208,754-line terminal presentation layer that has to be re-expressed as a DOM
presentation layer over a protocol that is already frontend-neutral.**
