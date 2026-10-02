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
compares the TypeScript surface with its Rust DTO definitions.

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

## Built-app E2E boundary (C003)

The M003 visible-window trajectory is repeatable executable coverage, not a
one-off transcript: WebdriverIO with `@wdio/tauri-service` and the default
`embedded` provider drives the real Tauri window (real WebView, real `invoke`
commands, real IPC `Channel` traffic) through existing-daemon reuse, visible
reconnect, project-catalog invalidation, renderer reload, native close, TUI
coexistence, explicit-path autostart, and daemon survival
(`apps/desktop/e2e/specs/`, entry point `apps/desktop/e2e/run-e2e.sh`,
hosted in `.github/workflows/desktop-e2e.yml` on Linux/WebKitGTK/xvfb).

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
  and deterministic cleanup — never the operator's real home or projects;
- an app binary built with `desktop-e2e` additionally refuses to connect
  unless `CODEGG_DAEMON_HOME` is set under the OS temp directory
  (fail-closed `disconnected`, no daemon touched);
- the renderer exposes only `data-testid` seams plus a visible connection
  generation; no privileged test command exists on the production bridge.

## Deferred surfaces

Session creation and projection, editor and terminal, provider credentials,
plugin UI, remote daemon access, signing, and distribution automation are not
implemented. The shell is infrastructure for later user-facing capability.

## Developer commands

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
