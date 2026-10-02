# Desktop Frontend and IDE Foundation Milestone 003 — Tauri Desktop Shell and Secure Bridge

Status: closing

Repository baseline: `86898d1f2bcd9f179c8354ebb4c11a04eed73ac6`

Source roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#milestone-003--tauri-desktop-shell-and-secure-bridge`

Long-term requirements:

- `plans/000-long-term-specification.md#1-product-definition`
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`

Applicable ADRs:

- `plans/adrs/ADR-0010-desktop-frontend-and-shared-client-boundary.md`

Primary class: infrastructure

Hard dependency:

- M001 shared frontend client runtime must close first.

Operational dependency:

- M002 is required before Windows desktop qualification, but Linux/macOS M003 work may proceed on the existing Unix local transport after M001.

## 1. Objective

Introduce the optional CodeGG desktop application shell as a separately tooled Tauri 2 + TypeScript/React application, connect its Rust host to the singleton daemon exclusively through `codegg-client`, and establish a narrow typed Rust-to-WebView bridge with bounded ordered streaming, restrictive CSP/capabilities, and no generic WebView machine authority.

The milestone is intentionally a shell/foundation milestone. It should render connection/daemon/project summary state and prove the security/toolchain boundary; it must not attempt full session UX, an editor, terminal, or IDE parity.

## 2. Why this milestone is ready

M001 is closed and `codegg-client` owns the reusable frontend connection boundary.
The desktop host can consume that crate without importing the root TUI or
constructing `CoreDaemon`.

M002 is conditionally closed. Its Unix transport is qualified, so no hard
dependency exists for Linux/macOS M003. Windows desktop qualification remains
gated on M002's live Windows transport and lifecycle evidence.

## 3. Current external/toolchain evidence

Research snapshot on 2026-10-01:

- Tauri 2.12 (2026-09-26) raised its MSRV to Rust 1.90 and adopted a `stable - 3` policy:
  https://tauri.app/blog/tauri-2.12/
- Tauri capabilities constrain which windows/webviews receive framework/plugin permissions:
  https://v2.tauri.app/security/capabilities/
- Tauri recommends IPC channels for ordered/high-throughput streaming rather than relying on general event delivery:
  https://v2.tauri.app/develop/calling-rust/
- bundled external binaries are supported through the sidecar/`externalBin` mechanism:
  https://v2.tauri.app/develop/sidecar/
- WebDriverIO's Tauri service supports desktop testing on Windows, Linux, and macOS and can also run renderer-only browser-mode tests:
  https://v2.tauri.app/develop/tests/webdriver/
- Tauri shell/process functionality is permission-scoped and potentially dangerous; the desktop foundation does not need to expose it to renderer JavaScript:
  https://v2.tauri.app/plugin/shell/

Repository constraints:

- the root workspace declares Rust 1.89;
- default CodeGG builds currently have no Node/Tauri/webview dependency;
- the existing distribution roadmap keeps one `codegg` executable for CLI/TUI/daemon use and does not own desktop packaging;
- `ConnectOrStartOptions.executable` already provides the required seam to launch an explicit existing `codegg` binary.

## 4. Invariants that must not regress

- Root `cargo build/test` remains independent of Node, Tauri, WebKitGTK, WebView2 tooling, or desktop package installation.
- Root Rust MSRV remains 1.89.
- The Tauri Rust host never constructs a second in-process CodeGG core.
- The WebView does not connect directly to the local daemon socket/pipe.
- The WebView does not receive a generic `CoreRequest` escape hatch.
- No renderer-visible generic filesystem, shell/process, Git, credential, or arbitrary network plugin permission is enabled for CodeGG operations.
- Daemon authorization remains authoritative for every operation.
- The desktop uses bundled/local application assets; remote-origin IPC is disabled.
- High-rate ordered daemon/projection streams cross a bounded channel path with explicit cancellation.
- Closing/reloading the renderer does not terminate daemon-owned work.
- Desktop bridge DTOs do not become a second durable domain model.

## 5. Scope

### In scope

- `apps/desktop/` application root.
- TypeScript + React + Vite renderer.
- `apps/desktop/src-tauri/` Tauri Rust host.
- Separate desktop Rust toolchain/lockfile/workspace boundary.
- Root workspace exclusion/guard so Tauri's MSRV cannot alter the root contract.
- Minimal Tauri configuration, one main window, restrictive capabilities, and CSP.
- Rust-owned `codegg-client` connection state.
- Rust-only daemon executable resolution/autostart using M001's explicit executable seam.
- Narrow typed bridge DTOs.
- Generated TypeScript definitions for those bridge DTOs where practical.
- Ordered Tauri channel carrying a bounded desktop event envelope.
- Minimal renderer states: starting, connecting, connected, incompatible, disconnected/error.
- Daemon identity/version/status display.
- Bounded project catalog/summary display sufficient to prove request/response.
- Component/unit tests and a built-app smoke path.
- `architecture/desktop.md`.

### Explicitly out of scope

- session creation/prompt/permission workflow (M004);
- Monaco;
- xterm.js;
- direct repository file read/write from JavaScript;
- LSP editor-buffer integration;
- Git mutations from the desktop;
- provider credential entry;
- plugin UI parity;
- remote `/core` connection;
- web/mobile builds;
- updater/signing/notarization;
- public desktop release automation;
- broad TUI parity;
- changing the `codegg` CLI binary topology.

## 6. Required production changes

### Repository layout and toolchains

Create an explicit optional application boundary such as:

```text
apps/desktop/
  package.json
  package-lock.json | pnpm-lock.yaml
  tsconfig.json
  vite.config.*
  src/
  tests/
  src-tauri/
    Cargo.toml
    Cargo.lock
    rust-toolchain.toml
    tauri.conf.json
    capabilities/
    src/
```

Use exactly one committed JavaScript package lock.

Make `src-tauri` an independently owned Cargo workspace/package or explicitly exclude it from the root workspace. The root workspace must not resolve Tauri dependencies during ordinary CodeGG commands.

Pin a desktop Rust toolchain compatible with the selected Tauri 2 minor. Do not edit `[workspace.package].rust-version = "1.89"` merely for desktop.

### Tauri host composition

The Rust host owns:

- one reusable `codegg-client` instance;
- connection lifecycle/cancellation state;
- immutable frontend descriptor using `ClientKind::Gui`;
- bounded event/channel fan-out;
- narrow bridge handlers;
- daemon executable resolution for local connect-or-start.

The renderer owns only presentation state, drafts, selection/focus/layout, and other ephemeral UI state.

### Daemon executable strategy

M003 must not add `codeggd`.

Development may point the connector at a workspace-built `codegg` executable.

For bundled-app proof, stage the existing target-specific `codegg` executable using a reproducible helper compatible with Tauri's external-binary/resource conventions. The launch itself remains Rust-host-owned; renderer JavaScript does not receive shell-plugin spawn permission.

If a self-contained sidecar proof would materially inflate M003, it may be a closure subtask after the installed-binary path works, but the plan must document exactly which mode the smoke test used. Do not claim self-contained desktop packaging from an installed-binary-only test.

### Bridge surface

Do not export:

```text
invoke("core_request", arbitrary_json)
invoke("shell", ...)
invoke("read_file", arbitrary_path)
```

Instead expose narrow operations for this milestone, for example:

- `desktop_connect`;
- `desktop_connection_snapshot`;
- `desktop_project_list`;
- `desktop_subscribe_events(channel)`;
- `desktop_disconnect` / renderer subscription teardown.

Exact names may differ.

Every request is bounded and maps to an existing daemon operation through `codegg-client`.

### Bridge DTO typing

Define desktop bridge DTOs in Rust near the host boundary.

Generate TypeScript types using a stable generator such as `ts-rs` or an equivalent non-pre-release tool. Do not add TypeScript derives across all of `codegg-protocol` simply to make the GUI convenient.

Add a drift check that regenerates bridge types and fails when committed output differs.

Bridge DTOs should wrap or narrow protocol DTOs rather than copying durable semantics.

### Ordered event channel

Use `tauri::ipc::Channel<T>` or the current Tauri equivalent for ordered streamed events.

The Rust host must:

- bound upstream buffering;
- define an explicit desktop event envelope/version if needed;
- cancel the channel forwarder when the renderer/window subscription ends;
- classify lag/disconnect rather than silently growing memory;
- never bypass projection/replay sequencing by inventing a renderer-side sequence source.

M003 may stream coarse connection/project invalidation events only; full projection activity is M004.

### Tauri capabilities

Use an explicit main-window capability file.

The foundation should avoid installing/enabling renderer-facing:

- filesystem plugin;
- shell/process execute/spawn;
- arbitrary HTTP client;
- clipboard write beyond an explicitly justified UI need;
- global shortcut;
- updater;
- credential/keychain plugins.

If a plugin is required for window ergonomics, enable only the exact commands used.

Custom commands are narrow by construction and must validate input even though daemon authorization is authoritative downstream.

### CSP and origin policy

- load renderer assets from the bundled application origin;
- no CDN scripts;
- no `unsafe-eval` unless a documented framework/tool constraint proves it unavoidable and a follow-up removes it;
- no remote URL gets Tauri IPC access;
- disable navigation/window creation to arbitrary privileged origins or validate it narrowly.

Record the final CSP/capability inventory in `architecture/desktop.md`.

### Renderer architecture

Use React/TypeScript as presentation.

Keep daemon-derived state in a small explicit store/controller layer rather than distributing request side effects across arbitrary components. React components should render immutable/bounded snapshots and dispatch typed intents.

Do not implement a TypeScript copy of `ProjectionClientController`.

### Storage and migrations

No SQLite migration.

Renderer-local settings should be minimal and non-authoritative. Do not introduce a second project/session database.

### Protocol and DTOs

No CoreFrame wire change expected.

The desktop descriptor advertises `ClientKind::Gui` and only truthful capabilities. M003 must not advertise full plugin UI/session surfaces before they exist.

### Runtime and concurrency

- one Rust host connection owner;
- one cancellation root for renderer/window subscription work;
- bounded stream fan-out;
- no task-per-token unbounded spawning;
- stale renderer generation cannot apply responses from a previous connection generation;
- app/window close releases client resources but leaves daemon-owned work alone.

### Documentation

Create `architecture/desktop.md` covering:

- process topology;
- toolchain split;
- trust boundary;
- Tauri capabilities/CSP;
- bridge commands/channels;
- daemon launch modes;
- unsupported/deferred surfaces.

Update `architecture/overview.md` to describe the optional desktop frontend without representing it as a mandatory daemon binary split.

## 7. Ordered work packages

### Work package A — Isolated desktop build skeleton

Intent:

Establish toolchain boundaries before connecting to CodeGG.

Required changes:

- app directory;
- React/TS/Vite;
- standalone/excluded Tauri Rust workspace;
- pinned toolchain and lockfiles;
- root guard proving ordinary Cargo metadata/build excludes Tauri.

Acceptance evidence:

- renderer dev/build succeeds;
- Tauri shell builds on one supported development host;
- root `cargo metadata` contains no Tauri dependency;
- root MSRV remains 1.89.

### Work package B — Security-first Tauri configuration

Intent:

Make least privilege the default rather than a cleanup item.

Required changes:

- one explicit main capability;
- restrictive CSP;
- local assets only;
- no broad plugins;
- navigation/origin controls;
- security inventory test/guard.

Acceptance evidence:

- config test rejects forbidden capability identifiers/origins;
- no renderer shell/fs/process permission;
- no remote IPC origin.

### Work package C — Rust host client composition

Intent:

Connect the desktop host through the M001 boundary.

Required changes:

- GUI descriptor;
- connect-or-start;
- connection snapshot;
- daemon identity/version;
- cancellation/reconnect generation state.

Acceptance evidence:

- existing daemon connection;
- autostart via explicit `codegg` path;
- incompatibility/error state;
- app close leaves daemon live.

### Work package D — Typed bridge and generated bindings

Intent:

Keep JavaScript decoupled from raw CoreFrame authority.

Required changes:

- narrow commands;
- bridge DTOs;
- type generation/drift check;
- ordered channel type.

Acceptance evidence:

- generated TS types compile;
- malformed/oversized bridge inputs are rejected;
- no generic CoreRequest invoke exists.

### Work package E — Minimal renderer vertical proof

Intent:

Prove end-to-end request + stream + render without starting M004.

Required changes:

- connection status UI;
- daemon summary;
- bounded project list/summary;
- reconnect/error state;
- renderer subscription teardown.

Acceptance evidence:

- UI can connect to the same daemon used by CLI/TUI;
- project list comes from daemon, not filesystem scanning;
- reload/close has no daemon side effect.

### Work package F — Test harness and docs

Intent:

Make the optional app maintainable without burdening root CI.

Required changes:

- renderer unit/component tests;
- mocked bridge tests;
- Tauri mock-runtime tests where applicable;
- one built-app smoke procedure;
- desktop architecture docs;
- proportionate CI placement or manual lane decision.

Acceptance evidence:

- exact commands documented;
- root quick verification remains unchanged unless a justified desktop-specific non-blocking job is added.

## 8. Failure, cancellation, restart, and contention semantics

- Initial connect failure renders an actionable disconnected/error state; it does not construct an in-process core.
- Protocol mismatch renders incompatibility and disables control operations.
- Renderer reload increments connection/subscription generation and drops stale callbacks.
- Rust host reconnect uses `codegg-client`; it does not reuse stale client/subscription ids.
- Channel consumer disappearance cancels the forwarder.
- GUI close releases only GUI-owned connection/subscriptions.
- If the GUI autostarted the daemon, the daemon remains detached/user-scoped exactly like TUI autostart.
- Concurrent TUI/desktop clients receive distinct daemon client ids.

## 9. Compatibility and migration

No existing CLI/TUI install requires the desktop app or Node.

No database migration.

No CoreFrame version bump expected.

The optional desktop build may require newer Rust than the root workspace; that requirement is documented under `apps/desktop`, not propagated to root package metadata.

An installed `codegg` executable and an app-bundled sidecar must both pass native protocol compatibility checks.

## 10. Required tests

### Renderer tests

- connection state reducer/store;
- project list bounded rendering;
- error/incompatible state;
- stale generation drop;
- bridge mock success/failure.

### Rust host tests

- GUI descriptor/capabilities;
- bridge input bounds;
- connect/reuse/autostart;
- event channel cancellation;
- renderer generation invalidation;
- app shutdown leaves daemon alive.

### Security and negative tests

- no forbidden Tauri permissions;
- no remote IPC origin;
- CSP snapshot/assertion;
- no generic core_request/shell/fs bridge command;
- renderer-supplied principal/client id ignored/not present.

### Integration/smoke

- launch built app;
- connect to test/real local daemon;
- render daemon id/status and project summary;
- run concurrently with TUI/headless TUI client;
- close app and verify daemon remains responsive.

### Migration and compatibility

- root Cargo metadata/build unaffected;
- root Rust 1.89 check unaffected;
- desktop toolchain builds independently;
- protocol mismatch produces explicit UI state.

## 11. Required verification commands

Exact package manager may differ; record only commands actually used.

```bash
cargo metadata --no-deps
cargo test -p codegg-client
scripts/verify.sh quick

cd apps/desktop
npm ci
npm run typecheck
npm test
npm run build
npm run tauri build   # or the repository's bounded desktop build helper
```

If WebDriverIO is introduced in M003, run only a bounded shell smoke here; full session E2E belongs to M004.

## 12. Documentation updates

- new `architecture/desktop.md`
- `architecture/overview.md`
- `architecture/client.md`
- developer build prerequisites under `apps/desktop`
- root README only if the desktop is intentionally exposed to users at this stage

## 13. Acceptance criteria

M003 is complete when:

1. an optional Tauri desktop app exists outside the root Cargo dependency graph.
2. root CodeGG MSRV remains 1.89 and default builds need no desktop tooling.
3. the Rust host connects through `codegg-client` with `ClientKind::Gui`.
4. the renderer receives only narrow typed CodeGG operations.
5. ordered streaming uses a bounded channel path.
6. Tauri capability/CSP configuration grants no generic renderer filesystem/shell/process authority and no remote-origin IPC.
7. daemon/project summary data renders end-to-end.
8. renderer reload/app close does not terminate daemon-owned state.
9. TUI and desktop can coexist against one daemon.
10. architecture/build/security documentation matches the implementation.

## 14. Stop conditions

Stop and report if:

- M001 is not closed;
- the desktop must import root TUI modules or construct `CoreDaemon`;
- implementation requires raising root CodeGG MSRV;
- a generic `CoreRequest` JSON bridge is proposed to avoid designing narrow DTOs;
- WebView JavaScript needs direct filesystem/shell access for M003;
- remote-origin IPC or CDN scripts are introduced;
- packaging the existing `codegg` binary is confused with creating a new daemon binary;
- the milestone expands into Monaco, PTY UI, full session UX, or provider credential management.

## 15. Closure evidence required

The closure record must include:

- implementation commits;
- desktop/root workspace and toolchain graphs;
- exact Tauri version/MSRV used;
- Tauri capabilities and CSP inventory;
- bridge command/channel inventory;
- proof no generic machine-authority plugin is renderer-accessible;
- root Cargo dependency/MSRV non-regression;
- daemon connect/reuse/autostart smoke evidence;
- TUI + desktop coexistence evidence;
- renderer close/reload lifecycle evidence;
- exact renderer/Rust/Tauri test commands;
- unresolved findings and support-platform truth.

## 16. Handoff notes

Keep this shell deliberately plain. Visual polish is not a prerequisite for proving the boundary.

Do not use a shell plugin from JavaScript to start `codegg`; use the Rust host and M001's explicit executable connector.

M004 owns meaningful session interaction. A successful M003 should make M004 mostly a domain/view-model exercise rather than another transport/security redesign.
