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

The Rust host owns one optional `LocalSocketClient`. Its narrow commands are
`desktop_connect`, `desktop_connection_snapshot`, `desktop_project_list`,
`desktop_subscribe_events`, and `desktop_disconnect`. Project lists request at
most 50 active projects. A single bounded `codegg-client` subscription is
filtered to project catalog invalidations and forwarded through a Tauri IPC
`Channel`, retaining daemon event sequence numbers. Closing the window drops
host-owned client state; it does not stop the daemon or its work.

Bridge DTOs are intentionally small: daemon connection state, project display
summary, and versioned project-catalog invalidation. `npm run bindings:check`
compares the TypeScript surface with its Rust DTO definitions.

## Deferred surfaces

Session creation and projection, editor and terminal, provider credentials,
plugin UI, remote daemon access, signing, and distribution automation are not
implemented. The shell is infrastructure for later user-facing capability.

## Developer commands

From `apps/desktop`, run `npm ci`, `npm run typecheck`, `npm test`,
`npm run bindings:check`, and `npm run build`. With Tauri system libraries
installed, run `npm run tauri dev`. Set `CODEGG_DAEMON_EXECUTABLE` to a built
`codegg` path before connecting/autostarting. For packaging, stage the root
binary first with `npm run stage:daemon`. Root verification does not invoke
Node, WebKitGTK, or the desktop Cargo workspace; `scripts/check-desktop-boundary.sh`
guards that separation and the renderer authority inventory.
