# Desktop Shell Architecture

## Topology and toolchains

`apps/desktop` is an optional Tauri 2 + React application. Its standalone Rust
workspace pins Rust 1.90, the Tauri 2.12 toolchain baseline. The root Cargo
workspace excludes it and retains Rust 1.89. The app does not add a `codeggd`
binary: its Rust host connects to the existing user-scoped daemon through
`codegg-client`. Development autostart requires `CODEGG_DAEMON_EXECUTABLE` to
point to the workspace `codegg` executable; a packaged resource named `codegg`
is also recognized and can be staged from the root target with
`npm run stage:daemon`.

## Trust boundary

The WebView renders only bundled local assets. The only declared capability is
`main-capability` for the `main` window, with no Tauri plugin permissions and no shell,
filesystem, process, network, updater, or credential plugin. CSP allows local
Tauri assets and IPC; it does not allow CDN scripts or `unsafe-eval`. The
renderer does not access the local daemon transport or accept a raw
`CoreRequest`.

**The editor adds no renderer permission and no CSP relaxation.** The
editor's surface is a bounded viewport over text the **daemon** owns: the
host opens the document through the shared `DocumentController`, the renderer
receives a window of lines and sends **edits**, not content. It declares no
Tauri capability of its own, registers no plugin, and the production CSP and
the empty `main` permission list are unchanged. The renderer's **own** write
authority is nil — `scripts/check_desktop_editor_authority.py` fails on a
renderer-side filesystem read, a renderer-side text field, a retained
whole-document array and a viewport concatenated into one, and
`scripts/check-desktop-boundary.sh` still holds.

## Bridge

The Rust host owns one optional `LocalSocketClient` plus one explicit
connection generation and at most one project-event subscription for that
generation. Its narrow commands are `desktop_connect`,
`desktop_connection_snapshot`, `desktop_project_list`,
`desktop_subscribe_events`, `desktop_unsubscribe_events`, and
`desktop_disconnect`. `desktop_subscribe_events` returns
`{ subscriptionId, connectionGeneration }`; the forwarder task is stored in a
`SubscriptionRegistry` (`apps/desktop/src-tauri/src/lifecycle.rs`) and is
cancelled/joined on replace, unsubscribe, disconnect, or reconnect. Terminal
cleanup clears the stored owner only when the exiting subscription id still
matches, so a stale task can never clear a newer subscription. `Channel` send
failure, event-stream close, and generation mismatch all exit through that
compare-by-id path. Failed reconnects preserve the current connection and
never fabricate a newer generation; successful reconnects cancel the stale
forwarder before installing the new client and bumping the renderer-visible
`connectionGeneration`. Renderer cleanup always invokes Rust unsubscribe, and
the React subscription effect is keyed on `connectionGeneration` rather than
only `connection.state`, so a connected -> connected reconnect re-subscribes.
Project lists request at most 50 active projects. A single bounded
`codegg-client` subscription is filtered to project catalog invalidations and
forwarded through a Tauri IPC `Channel`, retaining daemon event sequence
numbers. Closing the window drops host-owned client state; it does not stop
the daemon or its work.

Bridge DTOs are intentionally small: daemon connection state, project display
summary, and versioned project-catalog invalidation. `npm run bindings:check`
compares the TypeScript surface with its Rust DTO definitions. The M004
route/session surface adds `desktop_project_detail`, `desktop_workspace_select`,
`desktop_session_list/create/open`, `desktop_projection_start/current/stop`,
`desktop_projection_subscribe`, `desktop_prompt_submit`,
`desktop_control_refresh`,
`desktop_permission_respond`, `desktop_question_respond`, and
`desktop_artifact_read`; all stay camelCase-synced across
`src/bridge-types.ts`, `src/bridge.ts`, and `src-tauri/src/bridge.rs`, and
route errors propagate daemon codes (e.g. `model_unselected`) instead of
generic failures.

## Lifecycle linearization and concurrency rules (C002)

One lifecycle transition serialization boundary (`HostState.lifecycle`) defines
connection/subscription linearization. Linearization points, all under the gate:

- connect commit: attempt-serial check + old-subscription take + client install
  + generation bump;
- disconnect / native close / app exit: serial invalidation + generation bump +
  subscription take + client clear;
- subscription install: connection snapshot + owner publication.

Rules:

1. one lifecycle transition serialization boundary defines
   connection/subscription linearization;
2. network connect/autostart/request work happens outside it;
3. connect attempt validity is checked inside it immediately before commit;
4. disconnect advances the connect-attempt epoch inside it;
5. subscription ownership is published inside it before arming the task;
6. forwarders never acquire the lifecycle transition boundary while being
   abort/joined under that same boundary;
7. if join happens after releasing the boundary, generation/ownership is
   invalidated before release so the old task cannot publish current events;
8. renderer generation checks are defense in depth, not host-state correctness
   authority.

Concretely: `desktop_connect` splits slow preparation (endpoint resolve,
connect/reuse/autostart, daemon identity, snapshot) outside the gate from the
atomic commit under the gate; `disconnect_host` advances `connect_serial` under
the gate so every earlier in-flight connect is stale. Forwarders spawn UNARMED
(one-shot gate), publish the owner under the gate, and are ARMED only after the
release; an event queued before publication cannot self-terminate the task. The
native main-window close/destroy (`WindowEvent::CloseRequested`/`Destroyed`)
and app exit (`RunEvent::ExitRequested`/`Exit`) run the same host teardown as
explicit `desktop_disconnect` via a bounded synchronous `block_on` (2s budget)
without stopping the daemon; repeated events are idempotent. No correctness
requirement depends on scheduler timing.

## Session/control-plane vertical slice (M004)

Route identity is explicit project+workspace+session
(`apps/desktop/src-tauri/src/route.rs`). Workspaces come only from an
authorized `ProjectGet`; canonical roots are Rust-host only and never
serialized. Every async completion is applied only when its route token
is current, otherwise stale-dropped; a connection generation change
invalidates the projection owner and the renderer clears its route and
requires explicit re-selection.

Projection (`projection.rs`) wraps the canonical
`SessionProjectionDriver` behind a single `ProjectionOwner` per route:
stale completions release the subscription, stop retains the cursor,
and pushes are latest-only atomic replaces. The renderer's push
subscription subscribes only once a projection view exists for the
session (`projectionStart` installs the owner before resolving its
first view), so the effect cannot win the create-vs-subscribe race and
be rejected with "no session projection attached"; subscribe failures
retry bounded with backoff and then surface as route errors, never a
silent swallow that would leave the renderer permanently deaf. Presentation
(`present.rs`) is a pure bounded view: only `Public` + User/Assistant/
Tool content renders (raw tool args/output never serialize), with tail
caps (100 messages / 20 tools / 10 runs-jobs-subagents / 5 recent / 16
handles) and a non-authoritative cursor diagnostic. Two further caps bound
the pending-interaction surfaces: 16 pending permissions and 16 pending
questions per view (`present.rs:55-57`).

Prompt submission is at-most-once through `CoreRequest::SessionPromptSubmit`
(`prompt.rs`): the renderer sends opaque text plus plan-mode only, the
host session-checks and fences on route generation, and the daemon
resolves the durable model/agents/messages before delegating to the
identical `TurnSubmit` body. No durable user message is fabricated on
rejection; the draft stays editable. Active-turn guard gives
at-most-once; blank/overlong text and unselected/legacy models fail
closed with typed codes.

Permission/question/controller interaction (`control.rs`) follows ADR-0007
through daemon controller leases: the renderer sends opaque ids plus
allowed choices/bounded answers only, the host session-checks each id,
`RouteState.responding` coalesces repeats, and denials surface without
upgrading the caller. Artifact reads (`artifact.rs`) use opaque handles
through the same registry validation; unknown handles are typed errors.

## Built-app E2E boundary (C003)

The M003 visible-window trajectory is repeatable executable coverage, not a
one-off transcript: WebdriverIO with `@wdio/tauri-service` and the default
`embedded` provider drives the real Tauri window (real WebView, real `invoke`
commands, real IPC `Channel` traffic) through existing-daemon reuse, visible
reconnect, project-catalog invalidation, renderer reload, native close, TUI
coexistence, explicit-path autostart, and daemon survival
(`apps/desktop/e2e/specs/`, entry point `apps/desktop/e2e/run-e2e.sh`,
hosted in `.github/workflows/desktop-e2e.yml` on Linux/WebKitGTK/xvfb).

### The hosted qualification is un-paused (2026-10-24)

The lane was paused on 2026-10-23 because it was red on `main`: `main`'s own
most recent run (`39679968444` at `53d9fc9fd`) fails with
`element ("[data-testid="permission-list"]") still not existing after
120000ms` at `apps/desktop/e2e/specs/m004-session.e2e.ts:169:5` — the same
file, the same line and the same timeout — and the nine runs behind it are
consecutive failures.

The cause was **the spec's premise, not the product**. The spec expected the
mock model's *first* response — a `write` to a path outside the probe
workspace — to raise a human permission. Since `39e1445b6` the daemon answers
a recognised-temp-root `write` with an actionable redirect sent to the
**model**, and the offer is issued once per `(session, tool, canonical
target)` (`src/permission/mod.rs:1365-1411`, `:1393-1409`), so only a
*second* identical attempt reaches the human. The mock answered the second
request with final text, so the human was never asked and the spec waited out
its full timeout for a permission that was never going to arrive.

The fix is in the fixture and the spec, not the product:

- the mock **insists** on the same path on request #2 (a different tool-call
  id, the same target — the shape the product's own
  `temp_root_write_offers_a_redirect_before_asking_the_human` test uses), which
  restores the `permission-list` assertion and pins the one-offer-per-target
  rule;
- the spec answers **every** permission the desktop surfaces, because the
  number of human rounds is platform-dependent: on Linux the phase home is
  under `/tmp` and the redirect fires; on macOS it is under `$TMPDIR` → `/var`,
  which `crates/eggsentry/src/command.rs:161` classifies as a system path, so
  the write escalates as a security finding and the human is asked on the
  *first* attempt (`src/agent/tool_batch.rs:829` consults the redirect offer
  only in the `Ask` arm);
- the harness resolves the phase's fake `HOME` **physically**. The daemon's
  managed-scratch allocator refuses any path with a symlinked component
  (`src/scratch.rs:911-918`) and derives the scratch root from the cache
  directory under `HOME`; on macOS `$TMPDIR` is under the `/var` →
  `private/var` symlink, so an unresolved fake home failed **every** turn with
  "managed scratch path is not a real directory" before any provider was
  called.

`.github/workflows/desktop-e2e.yml` therefore runs on `push` to `main` and on
pull requests again, with the path filters the pause preserved verbatim in the
workflow's own header. The workflow name no longer carries the "(paused)"
marker.

**Where this lane sits in the M007 line.** The owner directed that the desktop
carry a TUI-parity GUI — a main chat region with a side panel, the same `/`
commands the TUI exposes, and an editor — keyboard-first and fully operable by
mouse. That work is the **M007** line, decomposed M007-A…M007-E at
`plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#milestone-007--desktop-gui-parity-line`,
and `plans/adrs/ADR-0052-cross-frontend-command-execution-ownership.md` was
**accepted on 2026-10-24** for the one point in it that changes who may do
what. The restoration described above is M007-A's work; what M007-A still
owes is the desktop Rust host's automated gate — the `clippy -D warnings` step
the host has never had — and a local entry point that runs the desktop's own
checks. A green lane is a restored premise, not a completed gate.

The trajectory, the spec files, the `desktop-e2e` Cargo feature and its
authority boundary, and the `check-desktop-boundary.sh` guard all stand; what
changed is the *hosted qualification*, which now runs again against a lane
whose premise matches the product's behaviour.

**The hosted qualification is green (2026-10-24).** PR #185's `e2e` job
(`40103667676`, job `120900793854`) ran the three phases on Linux/WebKitGTK
605.1.15 against a real daemon: the lifecycle phase **5 passing**, the
autostart phase **2 passing**, the session phase **7 passing** — **14
passing, 0 failing**. The phase homes were under `/tmp`, so this is the
**Linux** trajectory: the recognised-temp-root redirect fired and the human
was asked on the **second** attempt, which is the branch the local macOS run
never exercises. The mock's insist plus the spec's answer-every-permission
loop is therefore proven to converge on the platform that actually produces
the redirect offer.

The macOS branch of the same failure was reproduced locally, before the fix
landed, and it behaved exactly as the harness analysis above predicts: the
lifecycle phase **5 passing**, the autostart phase **2 passing**, and the
session phase **3 passing, 4 failing**, with every one of the four failures an
absent element (`[data-testid="projection-state"]`,
`[data-testid="control-refresh-button"]`, `[data-testid="route-session"]`,
`[data-testid="connection-generation"]`). The turn never reached a provider,
because the phase's fake `HOME` was not resolved physically and therefore sat
under the `/var` → `/private/var` symlink.

Before that run, the desktop foundation's evidence was the `CI / verify` lane
plus the local `apps/desktop` host and renderer checks — which the root
workspace's gate does not compile, so they were run by hand. The hosted `e2e`
job is now part of the lane's own evidence.

Test instrumentation never widens production authority:

- the embedded WebDriver server (`tauri-plugin-wdio-webdriver` 1.x) is an
  optional dependency enabled only by the `desktop-e2e` Cargo feature and
  registered only under `#[cfg(feature = "desktop-e2e")]`; ordinary
  `cargo build` / `tauri build` dependency trees contain zero WebDriver crates;
- the E2E app binary is built by `e2e/build-e2e-app.sh`, which additionally
  strips `devUrl` through a `TAURI_CONFIG` merge patch (production
  `tauri.conf.json` keeps it for `tauri dev`): a debug binary with `devUrl`
  set points its window at the vite server and embeds an empty asset set, so
  without the strip the window renders `about:blank` when no dev server
  runs. With `devUrl` removed the production bundle from `frontendDist` is
  embedded and the window loads the real built app;
- the `wdio-webdriver:default` permission lives in the checked-in template
  `apps/desktop/e2e/capabilities/e2e.json` and is installed as the gitignored,
  generated `src-tauri/capabilities/e2e.json` only for feature builds (the
  Tauri build script resolves every `capabilities/*.json` at compile time, so
  a static test capability would break production builds — the generated file
  must not exist in a production-clean tree);
- the production `main` capability keeps an empty permission list, the
  production CSP keeps local-only origins, and no shell/filesystem/process/
  HTTP/clipboard/updater/global-shortcut or generic `core_request` bridge
  exists — all mechanically enforced by `scripts/check-desktop-boundary.sh`,
  which additionally fails on unconditional plugin registration, on any
  `tauri-plugin-wdio` (backend execute/mock/log privileges) reference, and on
  `wdio` references in production config/capability;
- the deterministic control seam is a separate test process
  (`src-tauri/src/bin/desktop_e2e_fixture.rs`) speaking the same
  native-protocol/client crates with an isolated `CODEGG_DAEMON_HOME` (temp
  scoped, fail-closed), an isolated workspace, an explicit
  `CODEGG_DAEMON_EXECUTABLE`, bounded timeouts, identity-checked daemon kills,
  and deterministic cleanup — never the operator's real home or projects.
  It serves newline-delimited JSON commands over a Unix socket (not stdio):
  the embedded provider spawns the desktop app once per WebdriverIO
  invocation in the launcher process, so worker-side spec code cannot own the
   fixture's stdio, but any party can dial the socket. `e2e/run-e2e.sh` runs
   one WebdriverIO invocation per phase (lifecycle, then autostart, then the
   M004 `session` phase), each with its own fixture server, isolated home,
   pre-started (or started-then-stopped, for autostart) daemon, and app
   environment inherited from the phase script; the M004 session phase
   (`e2e/specs/m004-session.e2e.ts`) drives route, projection attach,
   controller refresh, daemon-resolved prompt failing closed with
   `model_unselected` (no provider in the fixture daemon; the typed failure
   is the assertion), a deterministic live turn, reconnect convergence,
   reload re-drive without session duplication, and native close with
   daemon/observer survival;

   The live-turn leg arms the session through a test-only loopback mock
   model server (ephemeral `127.0.0.1` port, serves only
   `POST */chat/completions` with a counted two-script SSE program: first
   an out-of-workspace `write` with `finish_reason: tool_calls` to force
   `PermissionPending`, then final text with `finish_reason: stop`; anything
   else 404s; request bodies are never logged). The fixture writes a
   temp-scoped `CODEGG_TUI_CONFIG` naming the mock `base_url` (which also
   disables env-var provider auto-registration) and selects it through the
   ordinary `ProviderConnectionCreate` / `SessionSelectionUpdate` daemon
   APIs; the renderer never carries provider authority. The spec asserts
   assistant text plus `permission-list`, denies once, then asserts
   `completed` turn status and final text — all converging through
   canonical projection. Phase homes redirect `HOME` into the phase home
   so no operator-global config, credential store, or provider environment
   leaks into the isolated daemon;
- an app binary built with `desktop-e2e` additionally refuses to connect
  unless `CODEGG_DAEMON_HOME` is set under the OS temp directory
  (fail-closed `disconnected`, no daemon touched);
- the renderer exposes only `data-testid` seams plus visible connection
  generation and subscription identity; no privileged test command exists on
  the production bridge. The subscription id is display-only state mirroring
  the installed bridge handle: the trajectory waits for a fresh subscription
  before mutating the catalog because a broadcast emitted while no desktop
  subscriber is installed (reconnect commit takes the old owner; the
  renderer re-subscribes asynchronously) is lost, not queued;
- E2E builds only keep one hidden blank anchor window (`e2e-anchor`,
  `about:blank`, never shown, created under `#[cfg(feature =
  "desktop-e2e")]` in `run()`): closing the last window exits the app, and
  with it the embedded WebDriver server, so the trajectory's native-close
  probe would take its own automation session down before observing the
  daemon-side teardown. The anchor survives the `main` destroy — the Rust
  close/destroy hook still fires for `main` and the daemon still drops exactly
  the desktop client — while the session (pinned to `main` through the
  `wdio:tauriServiceOptions` `windowLabel`, since the embedded server binds
  whichever label it lists first) and its teardown stay clean. The anchor
  loads no app URL, so it hosts no renderer and installs no second client,
  and no spec reads DOM after the close. Every catalog checkpoint converges
  the full rendered set against the daemon list (presence plus row-count
  equality plus row uniqueness), and the fixture's probe seeding reuses the
  same-named live entry across daemon restarts instead of stacking a
  duplicate the renderer would double-list.

## The desktop's input model

The desktop has its **own** renderer-owned, remappable input model. It is not
the TUI's keymap, and the TUI's keymap is not changed by it — the TUI's
`ActionKey` (`src/tui/input.rs:259`, 61 variants) is the **source vocabulary**
the desktop draws from, and `plans/000-long-term-specification.md:564` makes
the conceptual hierarchy normative while the exact keys are configurable. The
desktop borrows concepts; it does not borrow bindings.

**Where it lives.** `apps/desktop/src/keymap.ts` holds the action
declaration table, the concept-parity table and the chord machinery;
`apps/desktop/src/focus.ts` holds the region registry and the focus model;
`apps/desktop/src/panels/KeybindingsPanel.tsx` is the customisation surface.
All three are **renderer-local**: the keymap persists in the renderer's
`localStorage` under `codegg.desktop.keymap.v1`, and the desktop holds **no**
daemon-owned state for it.

**Chord normalisation.** A chord is modifier tokens in a fixed order (`mod`,
`ctrl`, `alt`, `shift`) followed by the key token, lowercased — `mod+shift+p`,
`shift+enter`, `escape`, `enter`, `f1`, `arrowright`. `mod` resolves to
`metaKey` on macOS and `ctrlKey` elsewhere, so one declaration serves both
platforms. Resolution is deterministic and total: an unrecognised key event
normalises to a chord that matches nothing.

**The declaration table.** Each action carries an identity, a label, a
description, its default bindings, its mouse affordance and the TUI concept it
carries. The labels are UI labels, so `plans/001-terminology-and-domain-model.md:5`
applies to them; the milestone **records** the proposed vocabulary in its
closure record and does not amend the canonical document, because that is an
owner-directed step.

**The effective map.** Defaults merged with the user's overrides. An override
for an action **replaces** that action's defaults entirely. A conflict — the
TUI's map cannot have one by construction, the desktop's can — is resolved
deterministically and reported; a remap that would collide is **refused** with
the TUI's shape of message and leaves the keymap byte-identical. Persistence is
one `setItem` per commit, so a reload can never observe a half-applied remap.
A missing, malformed, wrong-version or unparseable document yields the
defaults plus a visible notice; an unknown action id is dropped individually.
Import is atomic.

**The focus model.** Five regions in focus order — the catalog rail, the chat,
the side panel, the composer and the status row — bound to the frame's own
containers. `mod+1`…`mod+5` focus a region directly; `Tab` and `Shift+Tab`
move within the focused region and **wrap**, with a roving `tabindex` so `Tab`
never escapes it. The focused region carries `data-focus-region` and an
outline; the focused control carries the `focus-ring` class. An overlay traps
the focus, and `Esc` pops exactly one overlay layer and restores the element
that had the focus before that overlay opened; `Esc` on the base view does
nothing. A focused region that unmounts hands the focus to its declared
fallback, never to `document.body`.

**One entry point.** A single `keydown` listener, installed once at the
renderer's root, so there is exactly one owner per chord: the capture field
owns its key, `Tab` is the region's traversal, then the overlay scope, then the
region scope, then the global map. Only a **captured** chord calls
`preventDefault()`, so ordinary typing and native editing are untouched.

**Affordances and the dual-reachability rule.** Every action has a mouse
affordance, and the affordance is the action's *only* pointer target — a
keyboard action and its mouse affordance are the same operation, not two. A
chord is bound per **scope** (global, region, overlay), so `enter` is the
composer submit and the catalog's row activation without either being a mode.

**In-window only.** The desktop's shortcuts are **in-window only**. There is
**no OS-global shortcut**, no global hot-key registration and no background
activation: the desktop is a window, and the shortcuts belong to it.

## Deferred surfaces

Terminal, provider credentials, plugin UI, remote daemon access,
signing, and distribution automation are not implemented. The live-turn
built-app trajectory (assistant text, tool activity, permission round-trip
against a deterministic offline provider) **is** implemented — see the
live-turn leg in the M004 `session` phase above, which asserts the typed
`model_unselected` fail-closed path before arming a loopback mock model
server and then a real assistant turn with a denied write.

**The editor is implemented; the terminal and the remaining graphical
surfaces are deferred.** The
M007 line delivered, in order: the desktop build and hosted gate (M007-A), a
main chat region with a dockable side panel (M007-B), keyboard-first input
with mouse parity (M007-C), the shared `/` command surface (M007-D,
unblocked by ADR-0052's acceptance on 2026-10-24), and a first-party editor
over the shared `DocumentController` and the native LSP read surface
(M007-E). The **editor surface** is implemented: the desktop opens a project
document, edits it, undoes and redoes the edit, saves it, and reads LSP
diagnostics, completion and navigation — over the **shared** document
foundation, with the daemon owning the buffer and the host holding only a
bounded viewport slice.

The **graphical terminal**, a Git UI and the other graphical surfaces remain
deferred. A third-party editor engine remains deferred too, and would need
its own ADR, because adopting one changes the renderer's CSP, worker and
bundle boundary.

The renderer has gained a panel set over a projected session — activity, git,
skills, subagents, tasks, tool detail, transcript and turn summary — and now
renders `projection.messages`, `projection.transcript`, `projection.subagents`,
`projection.jobs`, `projection.todos` and `projection.pendingPermissions`. It
is still a single-`App()` component in one 760-pixel column with **no keyboard
handling at all**, and it is still a control-plane console rather than a chat
application. **M007-B** is what splits that column into a main chat region and
a dockable side panel; **M007-C** is what gives it a keymap; **M007-D** is
what gives it the `/` surface; **M007-E** is what gives it the editor.

**A third-party editor engine remains deferred.** Monaco and equivalents are
out of scope for M007: adopting one changes the renderer's Content Security
Policy, its worker model, and the bundle and signing boundary that §8
currently pins to local assets only, so it needs its own ADR.

## Developer commands

**One entry point runs the desktop's gate.** From the repository root,
`apps/desktop/scripts/verify-desktop.sh` runs, in order: the desktop boundary
guard, the renderer checks (`npm ci`, `npm run typecheck`, `npm test`,
`npm run bindings:check`, `npm run build`), the daemon staging, the host
checks (`cargo test --lib`, `cargo fmt --check`, `cargo clippy --all-targets
--locked -- -D warnings`, all on the pinned 1.90.0 toolchain), and — only when
a display is available — the built-app trajectory. It prints which steps it
skipped and why. The hosted lane calls the same script, phase by phase, so
the two cannot drift; the script fails closed if the workflow's own lint line
and its copy disagree. Each phase is individually invocable
(`verify-desktop.sh boundary`, `renderer`, `stage-daemon`, `host`, `e2e`).

From `apps/desktop`, run `npm ci`, `npm run typecheck`, `npm test`,
`npm run bindings:check`, and `npm run build`. With Tauri system libraries
installed, run `npm run tauri dev`. Set `CODEGG_DAEMON_EXECUTABLE` to a built
`codegg` path before connecting/autostarting. For packaging, stage the root
binary first with `npm run stage:daemon`. For built-app qualification, run
`CODEGG_DAEMON_EXECUTABLE=<path-to-codegg> ./e2e/run-e2e.sh` (builds the
`desktop-e2e` feature app plus the fixture helper, runs the WebdriverIO
trajectory, then restores the production-clean tree). Root verification does not invoke
Node, WebKitGTK, or the desktop Cargo workspace; `scripts/check-desktop-boundary.sh`
guards that separation and the renderer authority inventory.

## Source verification

Verified 2026-10-23 against `apps/desktop/`: `src-tauri/Cargo.toml`,
`src-tauri/tauri.conf.json`, `src-tauri/src/{lib,bridge,lifecycle,route,projection,present,prompt,control,artifact}.rs`,
`src-tauri/src/bin/desktop_e2e_fixture.rs`, `src-tauri/capabilities/main.json`,
`src/{bridge-types.ts,bridge.ts}`, `package.json`, `scripts/{stage-daemon.sh,check-bindings.mjs}`,
`e2e/{run-e2e.sh,build-e2e-app.sh,capabilities/e2e.json,specs/}`,
the root `Cargo.toml`, `scripts/check-desktop-boundary.sh`, and
`.github/workflows/desktop-e2e.yml`.
- **Verified the pause claim still holds**: `.github/workflows/desktop-e2e.yml`
  is named `Desktop E2E (paused)` and its only trigger is
  `on: workflow_dispatch:` — no `push`, no `pull_request`. The preserved
  path filters are in the workflow header, and the recorded failure
  (`element ("[data-testid="permission-list"]") still not existing after
  120000ms` at `apps/desktop/e2e/specs/m004-session.e2e.ts:169`) still
  matches the file — line 169 is the `waitForExist` on `permission-list`
  with the `timeout: 120_000` option.
- **Verified the toolchain pins**: `rust-version = "1.90"` at
  `src-tauri/Cargo.toml:6`, `tauri = { version = "2.12" }` at
  `src-tauri/Cargo.toml:20`, and the root `rust-version = "1.89"` at
  `Cargo.toml:12`. `apps/desktop/src-tauri` is in the root workspace's
  `exclude` list (`Cargo.toml:6`) and declares its own `[workspace]`
  (`src-tauri/Cargo.toml:8-10`).
- **Verified the host module set**: `apps/desktop/src-tauri/src/` holds
  exactly `artifact`, `bridge`, `control`, `lib`, `lifecycle`, `main`,
  `present`, `projection`, `prompt`, `route`, plus
  `src/bin/desktop_e2e_fixture.rs`. The doc's module list matches.
- **Verified the bridge command inventory**: all 19 documented commands
  exist in `src-tauri/src/lib.rs` — `desktop_connect` (`:655`),
  `desktop_connection_snapshot` (`:760`), `desktop_project_list` (`:775`),
  `desktop_subscribe_events` (`:810`), `desktop_unsubscribe_events`
  (`:820`), `desktop_disconnect` (`:831`), `desktop_project_detail`
  (`:840`), `desktop_workspace_select` (`:848`), `desktop_session_list`
  (`:859`), `desktop_session_open` (`:867`), `desktop_session_create`
  (`:876`), `desktop_prompt_submit` (`:888`), `desktop_artifact_read`
  (`:903`), `desktop_control_refresh` (`:919`),
  `desktop_permission_respond` (`:927`), `desktop_question_respond`
  (`:939`), `desktop_projection_start` (`:954`),
  `desktop_projection_stop` (`:963`), `desktop_projection_current`
  (`:972`), and `desktop_projection_subscribe` (`:979`).
  `MAX_PROJECTS = 50` at `src-tauri/src/lib.rs:35`.
- **Added the two missing presentation caps**: `MAX_PENDING_PERMISSIONS = 16`
  and `MAX_PENDING_QUESTIONS = 16`
  (`src-tauri/src/present.rs:55-57`) were absent from the doc's tail-cap
  list, which named only the other seven.
- **Verified the trust-boundary claims**: `src-tauri/capabilities/main.json`
  declares `main-capability` for the `main` window with an empty
  `permissions` array; the CSP in `src-tauri/tauri.conf.json:10` is
  `default-src 'self' asset: http://tauri.localhost; connect-src ipc:
  http://ipc.localhost; ...; script-src 'self'` — local assets and IPC
  only, no CDN script source, no `unsafe-eval`; and the packaged daemon
  resource is `"resources": { "binaries/codegg": "codegg" }`
  (`tauri.conf.json:12`), staged by `scripts/stage-daemon.sh` (which reads
  `CODEGG_DAEMON_EXECUTABLE` or falls back to
  `$repo_root/target/debug/codegg`).
- **Verified the E2E instrumentation claims**: the `e2e-anchor` hidden
  blank window is created under the `desktop-e2e` feature
  (`src-tauri/src/lib.rs:1061-1071`, `about:blank`, `visible(false)`); the
  `desktop-e2e` build refuses to connect unless `CODEGG_DAEMON_HOME` is
  set and temp-scoped (`src-tauri/src/lib.rs:584-602`); the fixture
  transport is a Unix-domain socket rather than stdio
  (`src-tauri/src/bin/desktop_e2e_fixture.rs:10,62`); and
  `scripts/check-desktop-boundary.sh` rejects any `core_request`,
  `read_file`, or `invoke.*shell` in the renderer/host sources
  (`:23`), requires the `desktop-e2e` feature to enable exactly
  `tauri-plugin-wdio-webdriver` (`:31`), requires that dependency to stay
  optional (`:35`), rejects the backend `tauri-plugin-wdio` crate
  (`:39`), and asserts the production capability's `permissions` is empty
  (`:92`).
- **Verified the E2E layout**: `apps/desktop/e2e/specs/` holds exactly
  `m003-autostart.e2e.ts`, `m003-lifecycle.e2e.ts`, and
  `m004-session.e2e.ts`; the live-turn leg is at
  `m004-session.e2e.ts:156`; `e2e/run-e2e.sh` and `e2e/build-e2e-app.sh`
  exist; the template capability is `e2e/capabilities/e2e.json`.
- **Verified the npm surface**: `package.json` defines `dev`, `build`,
  `typecheck`, `test`, `tauri`, `bindings:check`, `stage:daemon`, and
  `test:e2e`. `scripts/check-bindings.mjs` compares `src/bridge-types.ts`
  against `src-tauri/src/bridge.rs` field by field.
- Verified accurate, no change: the bridge DTO names and the
  camelCase sync across `src/bridge-types.ts`, `src/bridge.ts`, and
  `src-tauri/src/bridge.rs`; the C002 linearization rules and the
  `SubscriptionRegistry` compare-by-id cleanup (`lifecycle.rs:40-52`); the
  M004 route/projection/prompt/control/artifact contracts
  (`route.rs:61` `RouteState.responding`, `projection.rs:56`
  `ProjectionOwner`, `prompt.rs:138` `CoreRequest::SessionPromptSubmit`,
  `present.rs:80-85` the `Public` + User/Assistant/Tool filter); and the
  "Deferred surfaces" list.

Verified 2026-10-06 against `apps/desktop/`: the standalone Rust 1.90 /
Tauri 2.12 pins (`src-tauri/Cargo.toml:6, 20`) against root `rust-version
= "1.89"` (`Cargo.toml:12`); the host module set
(`src-tauri/src/{lib,main,bridge,lifecycle,route,projection,present,prompt,control,artifact}.rs`)
and the E2E fixture binary (`src-tauri/src/bin/desktop_e2e_fixture.rs`);
the registered bridge command inventory including the previously
undocumented `desktop_projection_subscribe` (`src-tauri/src/lib.rs:979, 1030`);
`MAX_PROJECTS = 50` (`lib.rs:35`); the presentation tail caps
(`present.rs:41-54`: 100 messages / 20 tools / 10 runs / 10 jobs /
10 subagents / 5 recent turns / 16 artifact handles);
`scripts/check-desktop-boundary.sh`; and the E2E specs
(`e2e/specs/{m003-lifecycle,m003-autostart,m004-session}.e2e.ts`) with the
live-turn leg present at `m004-session.e2e.ts:156`. The "Deferred surfaces"
section previously contradicted this doc's own live-turn description; it
now records the live-turn trajectory as implemented.
