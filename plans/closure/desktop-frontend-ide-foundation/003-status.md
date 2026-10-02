# Desktop Frontend and IDE Foundation M003 — Closure Record

Status: conditionally closed

Implementation plan: `plans/implementation/desktop-frontend-ide-foundation/003-tauri-desktop-shell-and-bridge.md`

Implementation commit: `fc3aaac8` (`feat: add optional Tauri desktop shell`)

Closure transition commit: `6a1cddb8` (`plans: move desktop shell milestone to closing`)

## Outcome

The repository now contains an optional Tauri 2 / React desktop shell under
`apps/desktop`, with independent npm and Cargo lockfiles and a Rust 1.90
toolchain. The root Cargo workspace excludes the Tauri package and retains its
Rust 1.89 MSRV. The Rust host composes `codegg-client` as a `ClientKind::Gui`
client, connects to or starts the existing `codegg` executable using an
explicit executable path, and exposes a bounded command/DTO surface to the
renderer. No `codeggd`, second core, SQLite migration, or generic CoreRequest
bridge was added.

The renderer presents connection/daemon state and at most 50 project summaries.
Project catalog invalidations pass through a bounded client subscription into
an ordered Tauri `Channel` and retain daemon `event_seq`. Desktop commands cover
connect, snapshot, project listing, event subscription, and disconnect. The
bridge only maps existing daemon operations.

Linux host compilation and a debug Debian bundle succeeded. The package
contains the staged existing `codegg` executable at
`/usr/lib/CodeGG/codegg`; staging is reproducible with `npm run stage:daemon`.
This is build/package evidence, not runtime qualification of the rendered
WebView's daemon flow.

## Toolchain and dependency boundaries

- Root workspace: Rust MSRV 1.89; `apps/desktop/src-tauri` is excluded.
- Desktop Cargo workspace: Rust 1.90.0; `tauri` 2.12.1; lockfile-resolved
  `tauri-build` 2.7.1; direct dependencies include `codegg-client` and
  `codegg-protocol` only for CodeGG runtime access.
- Desktop JavaScript: Node 22.23.3 in this verification environment;
  `@tauri-apps/api` and CLI 2.12.1, React 19.3.0, Vite 8.3.2,
  TypeScript 7.0.2, Vitest 5.0.3. One `package-lock.json` is committed.
- `scripts/check-desktop-boundary.sh` asserts root metadata/MSRV isolation,
  local asset loading, a main-window-only capability, no renderer plugin
  permissions, and no generic machine-authority bridge.

## Security and bridge inventory

The only capability is `main-capability`, scoped to window `main`, with an
empty `permissions` list. No shell, filesystem, process, network, updater,
credential, clipboard, or other Tauri plugin is enabled. The CSP is:

```text
default-src 'self' asset: http://tauri.localhost; connect-src ipc: http://ipc.localhost; img-src 'self' asset: http://tauri.localhost data:; style-src 'self' 'unsafe-inline'; script-src 'self'
```

The main window loads local `index.html`; remote navigation, CDN scripts, and
`unsafe-eval` are not configured. JavaScript uses Tauri `invoke` and `Channel`
only. There is no raw `CoreRequest`, path, shell, process, Git, or arbitrary
network command. The host event forwarder consumes a bounded
`LocalSocketClient::subscribe()` receiver and exits when the Tauri channel send
fails. Renderer connection/project results are generation-checked, and
project output is capped at 50 entries.

## Verification evidence

- `scripts/verify.sh quick` — passed, including the desktop boundary guard and
  root `cargo check --workspace --all-targets --locked`. An existing
  `AtomicU64::fetch_update` deprecation warning remains unrelated.
- `cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check` —
  passed.
- `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml` — 3 passed,
  covering the truthful GUI capability declaration, bridge serialization, and
  bounded project result helper.
- From `apps/desktop`: `npm run typecheck`, `npm test` (2 passed),
  `npm run bindings:check`, and `npm run build` — passed.
- `npm run tauri build -- --debug --no-bundle` — passed on Linux.
- `npm run tauri build -- --debug --bundles deb` — passed; the resulting
  283.93 MiB debug `.deb` contains the staged `codegg` resource.
- `scripts/check-desktop-boundary.sh` and `git diff --check` — passed.
- Running the built host in this shell with the explicit daemon executable
  returned exit status 0, but no display server is available, so this did not
  exercise a visible window or renderer bridge call.

## Conditional finding and support truth

This environment has no `DISPLAY`, `WAYLAND_DISPLAY`, or Xvfb. Therefore the
plan's end-to-end acceptance evidence has not been collected for a visible
desktop window connecting/reusing/autostarting a real daemon, rendering its
project catalog, coexisting with a live TUI client, or surviving renderer
reload/close while the daemon remains responsive. The app and bundle compile,
but this correctness evidence is needed before M003 can be strictly closed.

Windows qualification is also not claimed. M002's live Windows transport,
security, lifecycle, and graceful-stop evidence remains outstanding. No
platform support tier has changed.

## Dependency audit and disposition

- M004 remains `blocked` on strict M003 closure. Conditional M003 closure does
  not satisfy its hard dependency, and no M004 behavior was implemented.
- M005 document/buffer work and M006 IDE-shell work remain deferred.
- No other plan in this desktop foundation line was unblocked by this record.
- No unrelated ready roadmap was started; the requested next milestone is
  blocked by the named M003 evidence finding.

To resolve the finding, run the documented app on a Linux/macOS host with a
display server, exercise existing-daemon reuse and explicit-path autostart,
verify daemon/project rendering while a TUI client is connected, then close
and reload the renderer and verify the daemon stays live. Record the exact
platform and commands in a follow-up closure correction before promoting M003
to strict `closed` and registering M004 as `ready`.
