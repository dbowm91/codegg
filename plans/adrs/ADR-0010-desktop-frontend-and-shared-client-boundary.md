# ADR-0010: Desktop Frontend and Shared Client Boundary

Status: accepted

Date: 2026-10-01

Decision owners: project maintainers

Related specification sections:

- `plans/000-long-term-specification.md#1-product-definition`
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`
- `plans/000-long-term-specification.md#23-acp-boundary`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/002-long-term-roadmap.md#phase-5--frontend-neutral-session-projections-and-durable-replay`
- `plans/002-long-term-roadmap.md#phase-13--acp-adapter`

Affected subsystem roadmaps:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md`
- `plans/subsystems/session-projections-roadmap.md` (closed dependency; not reopened)
- `plans/subsystems/tui-project-sessions-roadmap.md` (closed dependency; not reopened)
- `plans/subsystems/distribution-installation-roadmap.md` (closed CLI distribution boundary; not reopened)

## Context

CodeGG is already daemon-centered. The singleton daemon owns durable sessions, project/workspace identity, provider connections, authorization, jobs, agent execution, PTYs, collaboration, audit, persistence, and canonical frontend-neutral projection/replay state. The local TUI is a client over `CoreClient`; remote non-TUI clients can use the native `CoreFrame` protocol at `/core`; `codegg-protocol` already defines `ClientKind::Gui` and `ClientKind::Web`.

That architecture is a good foundation for a desktop client and, later, an IDE. The present implementation nevertheless has frontend coupling that would make a GUI either duplicate the TUI or bypass the intended boundary:

- `SocketCoreClient::send_client_hello` hard-codes `codegg-tui`, `ClientKind::Tui`, and TUI capability values.
- `CoreClient`, the production local socket client, daemon path discovery, and connect-or-start orchestration live in the root `codegg` package rather than a reusable frontend client crate.
- `src/client/` is a remote-TUI `/tui` adapter, not a generic `/core` client.
- the canonical `ProjectionClientController` is already transport-neutral, but the surrounding TUI state contains presentation-specific caches and asynchronous state that must not be copied into a GUI.
- local IPC is Unix-domain-socket based and singleton locking explicitly fails closed on non-Unix targets.
- CodeGG's root workspace MSRV is Rust 1.89. Tauri 2.12, released 2026-09-26, requires Rust 1.90 and follows a `stable - 3` MSRV policy.

The desktop work is also the first concrete step toward a CodeGG-native IDE. That future IDE will need editor buffers, diagnostics, completion, diffs, terminals, worktrees, jobs, agents, and collaboration, but those surfaces must remain clients of daemon-owned authority rather than creating a second filesystem/LSP/execution authority inside a WebView.

The existing one-`codegg`-executable CLI/daemon distribution contract remains valuable. A desktop application is an optional frontend bundle, not a split of the daemon into `codeggd` plus another mandatory role binary.

Research snapshot used for this decision (2026-10-01):

- Tauri 2.12 / MSRV policy: https://tauri.app/blog/tauri-2.12/
- Tauri capabilities: https://v2.tauri.app/security/capabilities/
- Tauri Rust/JS IPC and channels: https://v2.tauri.app/develop/calling-rust/
- Tauri sidecars: https://v2.tauri.app/develop/sidecar/
- Tauri WebDriver testing: https://v2.tauri.app/develop/tests/webdriver/
- Rust 1.89 file locking: https://doc.rust-lang.org/std/fs/struct.File.html
- Tokio Windows named pipes: https://docs.rs/tokio/latest/tokio/net/windows/named_pipe/
- Windows named-pipe security: https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights
- Monaco Editor: https://microsoft.github.io/monaco-editor/
- monaco-languageclient: https://github.com/TypeFox/monaco-languageclient
- xterm.js: https://github.com/xtermjs/xterm.js

## Decision drivers

- Preserve the singleton daemon as the sole durable execution and authorization authority.
- Reuse the native protocol and canonical projection reducer rather than inventing GUI-specific state semantics.
- Keep the TUI first-class and avoid a broad TUI rewrite as a prerequisite for desktop work.
- Make frontend connection/reconnect/projection machinery independently reusable and testable.
- Keep local desktop traffic off a localhost HTTP server when a native local IPC path is available.
- Preserve the root CodeGG Rust 1.89 MSRV until a separate project-wide decision changes it.
- Avoid granting WebView JavaScript direct filesystem, shell, Git, credential, PTY, or daemon-spawn authority.
- Make the first desktop slice small enough to qualify before committing to a full IDE surface.
- Leave a clean path to a browser frontend and remote `/core` client later without coupling desktop rendering to local IPC.
- Avoid claiming native Windows desktop support before local IPC/singleton semantics are actually qualified there.

## Considered options

### Option A — Render the existing TUI in a desktop terminal

This minimizes frontend work but preserves terminal layout/input constraints, does not create an IDE-quality editor surface, and makes graphical composition depend on TUI presentation state. It is rejected as the product architecture.

### Option B — Build a native Rust GUI with GPUI, iced, egui, or Slint

This can keep more code in Rust, but it makes CodeGG own substantially more editor/widget/rendering infrastructure and provides a weaker path to Monaco-class editing, browser reuse, and mature terminal/editor accessibility tooling. It remains viable for specialized future views but is not selected for the first desktop frontend.

### Option C — Tauri 2 shell with a web frontend and direct daemon protocol

A small Rust host connects to the CodeGG daemon through a reusable Rust client layer. Bundled TypeScript UI code receives only narrow typed bridge operations and bounded streams. The WebView does not receive general filesystem/process authority. This is selected.

### Option D — Browser UI over the existing Axum server, wrapped in a WebView

This maximizes browser parity but makes ordinary local desktop startup depend on the network server, HTTP/WebSocket authentication, local ports, CORS, and a second exposure surface even though CodeGG already has local IPC. It remains appropriate for a later true web frontend but is not the normal local desktop path.

## Decision

### 1. Desktop is a peer frontend

The desktop GUI is a peer of the TUI, ACP, automation, and future web clients. It is not a new core runtime and does not own durable session/project/job/provider state.

The daemon remains authoritative for:

- project/workspace/session identity;
- authorization and control ownership;
- agent/tool/job execution;
- files and Git mutations;
- PTY lifecycle;
- LSP runtime ownership;
- provider credentials;
- persistence, collaboration, presence, and audit;
- canonical projection/replay streams.

Closing or reloading the desktop frontend must not implicitly terminate daemon-owned work.

### 2. A reusable Rust frontend client boundary is required

A new `crates/codegg-client` will own reusable frontend-side native protocol machinery. It will depend downward on `codegg-protocol` and low-level transport/runtime crates, not upward on the root `codegg` application or daemon execution implementation.

The reusable boundary includes:

- configurable client identity and capability advertisement;
- request correlation and bounded event subscription;
- local endpoint connection and reconnect primitives;
- frontend-safe daemon discovery/connect-or-start orchestration;
- transport/protocol error classification;
- integration with the existing transport-neutral `ProjectionClientController`.

The root package may retain compatibility adapters while migration is bounded. TUI presentation state remains in `src/tui`.

### 3. Local and remote transport remain distinct adapters

Normal local desktop use connects to the singleton daemon over platform-local IPC. It must not start an Axum server solely to talk to itself.

Remote desktop/web use may later connect through authenticated `/core` WebSocket transport using the same `CoreFrame` semantics. `/tui` remains a TUI compatibility/presentation protocol and is not the desktop contract.

### 4. Tauri 2 is the desktop host

The initial desktop host is Tauri 2 with a TypeScript web frontend. React/Vite is the initial renderer/tooling choice, but React is not a daemon/protocol contract and may be replaced without changing CodeGG authority.

The desktop Tauri Rust workspace is isolated from the root Cargo workspace/toolchain contract so the current Tauri MSRV does not force CodeGG's core MSRV above 1.89. The desktop may depend on path crates such as `codegg-client` and `codegg-protocol`, but its lockfile/toolchain is independently owned.

The optional desktop bundle may carry the existing `codegg` executable as a sidecar or resolve an installed compatible `codegg` binary. It must not introduce a separate `codeggd` implementation. Daemon startup remains the existing connect-or-start semantics with an explicit executable path.

### 5. The WebView receives a narrow bridge, not machine authority

The Tauri host exposes narrow typed commands and ordered channels for CodeGG frontend operations. It must not expose a generic unrestricted `CoreRequest` passthrough to arbitrary web content.

No JavaScript-visible Tauri filesystem, shell/process, credential, or arbitrary HTTP permission is enabled merely to implement CodeGG operations. Those actions continue through the daemon's native protocol and authorization gates.

Bundled/local assets are the default origin. The initial desktop capability set does not grant remote-origin IPC. Content Security Policy must disallow remote script execution unless a later reviewed requirement explicitly adds it.

High-rate ordered streams such as agent deltas, projection events, and PTY output use bounded channel/stream paths rather than thousands of global best-effort UI events.

### 6. Desktop bridge DTOs are independently typed

The desktop bridge uses narrow serializable DTOs and generated TypeScript definitions where practical. Type generation must not force TypeScript concerns into every `codegg-protocol` type.

A stable generator such as `ts-rs` may be used at the bridge boundary. Pre-release binding generators are not required for the foundation.

### 7. Windows requires a real local-IPC qualification milestone

The portable local endpoint abstraction must support Unix-domain sockets on Unix and a secure local primitive on Windows. The planned Windows primitive is a Tokio named pipe.

A default Windows named-pipe security descriptor is not sufficient: Microsoft documents that it grants read access to Everyone and anonymous users. The CodeGG pipe must be local-only and carry an explicit restrictive DACL/identity boundary appropriate to the LocalOwner trust model. Any Windows-specific unsafe required to construct that security descriptor must be narrowly isolated, reviewed, and separately guarded.

Rust 1.89 `File::try_lock` is the preferred replacement for CodeGG's hand-written Unix-only singleton flock because the standard library maps it to `flock` on Unix and `LockFileEx` on Windows.

This work improves portability but does not by itself promote Windows to a guaranteed CodeGG support tier or add a Windows installer.

### 8. IDE editor state is a later daemon/client contract

Monaco and xterm.js are suitable future presentation components, and the existing `egglsp` and interactive-process protocols provide substantial backend capability. They are intentionally outside the desktop foundation.

Before a CodeGG IDE treats unsaved text as authoritative input to agents or LSP, a later milestone must define a versioned document/buffer/overlay contract covering open/change/save/close, disk revision/hash, dirty state, external modification/conflict, and LSP synchronization.

The WebView must not become the sole owner of editor truth by silently reading/writing repository files outside daemon policy.

## Consequences

### Positive

- Desktop work reuses the same daemon, authorization, replay, PTY, jobs, and collaboration model as the TUI.
- The TUI and desktop exercise one production local client implementation.
- A future web client can add a `/core` transport without rewriting frontend state semantics.
- Tauri's higher MSRV does not force an unrelated core MSRV change.
- GUI code cannot bypass CodeGG security simply because it runs locally.
- The architecture supports a later IDE without making editor widgets part of the daemon.

### Negative

- A new `codegg-client` crate and optional desktop application must be maintained.
- The desktop build has a second Rust toolchain/lockfile boundary plus Node/TypeScript tooling.
- Windows local IPC requires explicit security work rather than a trivial `cfg(windows)` compile fix.
- Some TUI-specific client state must be classified and extracted or intentionally left presentation-local.

### Neutral or deferred

- Full desktop packaging, signing/notarization, updater policy, and release cadence remain separate distribution work.
- Remote `/core` desktop access is deferred until the local desktop path is proven.
- Monaco, xterm.js, document buffers, LSP synchronization, docking, file explorer, Git UI, and full IDE parity are deferred.
- The TUI remains the reference frontend under the current long-term specification.
- Mobile is not part of this decision.

## Compatibility and migration

The native `CoreFrame`, projection, session, authorization, and storage contracts remain authoritative. The client extraction must preserve existing protocol versions and TUI behavior.

`ClientKind::Gui` and `ClientKind::Web` already exist; the foundation should use those existing identities rather than add GUI-specific transport messages unless concrete missing semantics are discovered.

The root `codegg` binary remains the daemon/CLI/TUI artifact. An optional desktop bundle is an additional frontend distribution and does not require existing CLI users to install Node, Tauri, or a second daemon.

A desktop release must negotiate protocol/capability compatibility and produce an actionable mismatch instead of assuming the bundled/installed daemon is compatible.

## Security and reliability implications

- Local IPC identity must remain transport-derived; WebView payloads cannot supply trusted principal/client identities.
- Tauri capabilities are defense in depth, not a substitute for daemon authorization.
- Remote WebView origins do not receive IPC permission in the foundation.
- Secrets and provider credentials do not enter frontend projections or bridge logs.
- Ordered streams are bounded and support reconnect/resync rather than unbounded buffering.
- Desktop reconnect must renegotiate capabilities and discard stale subscription ownership through the existing projection controller semantics.
- Desktop process exit must release only client-owned subscriptions/attachments; daemon jobs, sessions, and PTYs follow their existing explicit lifecycle contracts.
- Windows named-pipe default ACLs are rejected as insufficient; the Windows transport cannot close until restrictive ACL/runtime evidence exists.

## Verification

Conforming implementations must prove:

- a GUI-kind client can handshake without TUI hard-coded identity/capabilities;
- the TUI continues to pass through the shared production client path with no protocol regression;
- daemon connect/start and request/event behavior are reusable without importing TUI modules;
- projection state continues to use the canonical reducer/controller;
- no desktop JavaScript permission directly grants filesystem, process, Git, PTY, or credential authority;
- desktop assets use a restrictive CSP and no remote-origin IPC;
- desktop/root Rust toolchain constraints are independently testable;
- Unix local IPC remains compatible;
- Windows local IPC, when claimed, uses explicit current-user/local security and live runtime evidence;
- desktop closure does not require an editor, terminal widget, web frontend, or TUI deprecation.

## Supersession

None.
