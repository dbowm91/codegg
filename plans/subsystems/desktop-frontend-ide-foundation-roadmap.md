# Desktop Frontend and IDE Foundation Roadmap

Status: active

Repository audit baseline: `f388689094866fe4ab8b1bf1dafa680516805ad2`

Long-term references:

- `plans/000-long-term-specification.md#1-product-definition`
- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#13-multi-project-and-multi-session-tui`
- `plans/000-long-term-specification.md#23-acp-boundary`
- `plans/000-long-term-specification.md#24-protocol-and-storage-requirements`
- `plans/002-long-term-roadmap.md#phase-5--frontend-neutral-session-projections-and-durable-replay`
- `plans/002-long-term-roadmap.md#phase-13--acp-adapter`

Accepted architecture:

- `plans/adrs/ADR-0010-desktop-frontend-and-shared-client-boundary.md`
- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`
- `plans/subsystems/editor-document-foundation-roadmap.md` — active shared M005 implementation authority
- `plans/subsystems/session-projections-roadmap.md` — closed frontend-neutral projection/replay dependency
- `plans/subsystems/tui-project-sessions-roadmap.md` — closed project/session frontend dependency
- `plans/subsystems/interactive-process-sessions-roadmap.md` — existing PTY protocol foundation
- `architecture/protocol.md`
- `architecture/server.md`
- `architecture/tui.md`
- `architecture/process-tool-execution-ownership.md`

## 1. Purpose and ownership boundary

This subsystem establishes the smallest durable foundation for a CodeGG desktop GUI while preserving the architecture needed for a later IDE.

It owns:

- the reusable frontend-side native-protocol client boundary;
- configurable client identity/capability negotiation;
- frontend-safe local daemon discovery/connect/start behavior;
- portable local IPC abstractions needed by desktop clients;
- the optional Tauri desktop application shell and narrow Rust-to-TypeScript bridge;
- a first projection-primary desktop control-plane slice proving that a second rich frontend can operate against the same daemon as the TUI;
- architectural seams required before later editor/document/terminal UI work.

It consumes the existing daemon, authorization, project/session, projection/replay, PTY, LSP, jobs, worktree, provider, collaboration, and audit subsystems.

It does not own a second agent runtime, second scheduler, desktop-local persistence authority, browser-facing filesystem API, provider credential store, full IDE editor implementation, or release-signing/package-manager system.

## 2. Work classification

### Invariants

- The singleton daemon remains the only durable execution/control authority.
- TUI, desktop, ACP, and future web clients consume the same native protocol/projection semantics.
- A frontend never becomes authorization authority merely because it is local.
- GUI shutdown or renderer reload never implicitly cancels daemon-owned sessions, jobs, agent runs, or PTYs.
- The desktop WebView has no generic direct filesystem, shell/process, Git, credential, or PTY authority.
- Projection state is reduced through the canonical `codegg-protocol` reducer/controller; no TypeScript fork of projection semantics is authoritative.
- The root CodeGG workspace retains its Rust 1.89 MSRV unless another accepted decision changes it.
- Local IPC is bounded, authenticated/identity-safe, and platform-appropriate.
- The TUI remains supported and first-class throughout the workstream.

### Capabilities

- A GUI-kind client can connect to/reuse the singleton daemon with its own truthful capability declaration.
- The desktop can render bounded project/session activity and submit basic session control-plane intents without `/tui`, localhost REST, or direct machine access.
- The desktop can reconnect/resume projection state and handle pending permission/question state.
- The same daemon can concurrently serve TUI and desktop clients without duplicated durable state.

### Infrastructure

- `crates/codegg-client` reusable transport/client runtime.
- Platform-local endpoint abstraction.
- Cross-platform singleton lock primitive.
- Tauri/TypeScript application boundary with generated bridge DTOs and ordered channels.
- Desktop-specific test harnesses isolated from normal root Rust CI where appropriate.

### Polish

- Development scripts, diagnostics, architecture docs, dependency/toolchain documentation, and bounded smoke testing for the optional desktop app.

## 3. Non-goals

This roadmap does not initially deliver:

- a production-complete IDE;
- Monaco editor integration, file explorer, docking system, command palette parity, Git UI, or integrated graphical terminal;
- unsaved editor-buffer/LSP synchronization;
- CRDT or collaborative source editing;
- a web or mobile frontend;
- remote desktop access over `/core`;
- TUI deprecation or feature freeze;
- a new `codeggd` binary;
- daemon/TUI executable splitting;
- desktop signing/notarization, updater policy, package-manager distribution, or Windows installer support;
- promotion of Windows to a guaranteed CodeGG support tier;
- a JavaScript reimplementation of the native protocol reducer;
- broad REST expansion.

## 4. Current-state evidence

At the audit baseline:

1. `codegg-protocol` is already frontend-neutral and includes `ClientKind::{Tui, Gui, Web, Cli, Automation}`, core request/response/event envelopes, projection DTOs/reducer/controller, plugin UI types, LSP preview DTOs, and the interactive-process attach/resume protocol.

2. `src/core/mod.rs` defines the `CoreClient` request/subscribe facade, while `src/core/transport/socket.rs` owns the production local client. The latter hard-codes:
   - `client_name = "codegg-tui"`;
   - `ClientKind::Tui`;
   - TUI-specific capability values.

3. `src/client/` implements a remote TUI `/tui` client. There is no reusable production `/core` WebSocket client; the real `CoreFrame` WebSocket consumer currently exists mainly in projection transport tests.

4. `ProjectionClientController` is already transport-neutral and explicitly intended for local TUI, remote TUI, and future frontend consumers. TUI-specific caches and route/tab state live above it in `src/tui/app/state/projection_client.rs`.

5. `connect_or_start_daemon` already supports an explicit executable path through `ConnectOrStartOptions.executable`, which is suitable for an optional desktop bundle that ships or locates the existing `codegg` executable.

6. Local daemon transport is Unix-only:
   - `SocketCoreClient` uses `tokio::net::UnixStream`;
   - the server uses `UnixListener`;
   - `DaemonPaths::endpoint_uri` always emits `unix://`;
   - the non-Unix singleton lock path fails closed.

7. The workspace is pinned to Rust 1.89. Current Tauri 2.12 requires Rust 1.90, so placing the desktop Tauri crate directly under the root workspace would unnecessarily raise or fragment the root MSRV contract.

8. Rust 1.89 itself stabilized `std::fs::File::{lock,try_lock,...}`, with `try_lock` mapping to `flock` on Unix and `LockFileEx` on Windows. Tokio exposes asynchronous Windows named pipes. Microsoft documents that a named pipe created with a default security descriptor grants read access beyond the creator, so a CodeGG Windows endpoint requires an explicit restrictive security descriptor rather than a default pipe.

9. The existing PTY contract already supports create/list/attach/detach/input/resize/terminate/remove/resume through the daemon. The TUI is explicitly a projection of that service. This is sufficient backend foundation for a later xterm-style desktop terminal without introducing Tauri process authority.

10. `egglsp` already owns language-server lifecycle and rich semantic operations, but CodeGG does not yet have a durable/versioned editor document-buffer contract for unsaved text. That boundary is deferred until the desktop control-plane foundation is proven.

## 5. Target architecture

```text
                         CodeGG daemon
                  authority / storage / jobs
                  agents / PTYs / LSP / Git
                           |
                     CoreFrame protocol
                           |
                    codegg-client
              request + events + reconnect
              local IPC + projection driver
                    /                \
                   /                  \
               TUI adapter        Desktop Rust host
                                      |
                               narrow typed bridge
                               ordered Tauri channels
                                      |
                              React/TypeScript UI
```

Remote/web transport can later attach below the same `codegg-client` service boundary:

```text
codegg-client transport
    |-- local IPC: Unix socket / Windows named pipe
    `-- future remote: authenticated /core WebSocket
```

The desktop renderer is presentation-only. It receives already-authorized, bounded domain/projection data and submits typed user intent.

## 6. Dependency graph

```text
closed session projections / daemon singleton / project-session foundations
                              |
                              v
M001 shared frontend client runtime                         [closed]
             |                                \
             | hard                            \ hard
             v                                  v
M002 portable local daemon IPC                    M003 Tauri shell + secure bridge
[closed]                            [conditionally closed]
             |                                      |
             | operational for Windows              | hard
             +-------------------+------------------+
                                 v
M004 desktop session/control-plane vertical slice           [strict closed; post-closure C001 closed]
                                 |
                                 v
foundation closure / architecture review
                                 |
                    +------------+-------------+
                    |                          |
                    v                          v
M005 document/buffer contract            M006 TUI IDE vertical slice
[closed]                                 [eligible for fresh planning + presentation audit]
```

M002 is not a hard dependency for a Linux/macOS M003 implementation because the existing Unix transport remains valid there. It is an operational dependency before Windows desktop support can be claimed.

## 7. Milestones

### Milestone 001 — Shared frontend client runtime

Status: closed.

Primary class: infrastructure.

Plan:

- `plans/implementation/desktop-frontend-ide-foundation/001-shared-frontend-client-runtime.md`

Outcome:

- introduce `crates/codegg-client`;
- remove TUI identity/capability hard-coding from the reusable local client;
- move frontend-safe local daemon discovery/connect/start semantics behind a reusable API;
- keep daemon authority and root in-process execution out of the client crate;
- preserve TUI behavior through a compatibility adapter and real TUI production use of the extracted client;
- establish dependency/ownership guards preventing `codegg-client` from growing into a second core.

### Milestone 002 — Portable local daemon transport and singleton lifecycle

Status: conditionally closed. Windows live qualification remains outstanding.

Primary class: infrastructure / invariant.

Plan:

- `plans/implementation/desktop-frontend-ide-foundation/002-portable-local-daemon-transport.md`

Outcome:

- introduce a platform-local endpoint abstraction;
- retain Unix-domain sockets on Unix;
- add securely ACL-constrained Windows named-pipe transport;
- replace hand-written Unix-only singleton flock with the Rust 1.89 standard file-lock API where semantics match;
- preserve CoreFrame framing/handshake/replay behavior across both transports;
- require live Windows evidence before closing the Windows portion;
- do not change Windows distribution/support-tier claims.

### Milestone 003 — Tauri desktop shell and secure bridge

Status: closed. C001 and C002 production fixes are landed and conditionally closed per their records. Hosted root-CI reconciliation is green (`CI / verify` runs `37073506756` and `37092317678`, PTY test passing, test untouched) and the repeatable built-app visible-window/WebDriver qualification is green (`Desktop E2E` run `37092317709`: lifecycle 5/5, autostart 2/2) — both owned by closed C003 (`plans/closure/desktop-frontend-ide-foundation-corrective/003-status.md`). Strict M003 closure is complete. M004 is unblocked to ready. M002 remains an operational dependency for Windows qualification.

Primary class: infrastructure.

Plan:

- `plans/implementation/desktop-frontend-ide-foundation/003-tauri-desktop-shell-and-bridge.md`
- corrective control: `plans/subsystems/desktop-frontend-ide-foundation-m003-lifecycle-corrective-addendum.md`

Outcome:

- add an optional `apps/desktop` Tauri 2 application with an independently owned Rust toolchain/lockfile;
- establish React/TypeScript/Vite renderer tooling;
- connect from the Rust host through `codegg-client`;
- expose only narrow typed bridge commands and ordered channels;
- enforce local bundled assets, restrictive CSP, and minimal Tauri capabilities;
- prove daemon status/project summary rendering without creating editor, terminal, or broad machine-access surfaces.

### Milestone 004 — Desktop session/control-plane vertical slice

Status: closed. Historical closure is preserved at `plans/closure/desktop-frontend-ide-foundation/004-status.md`; the strict post-closure corrective is closed at `plans/closure/desktop-frontend-ide-foundation-post-closure-corrective/001-status.md`. Corrected head `738142d8` passed hosted root CI run `37134452335` and Desktop E2E run `37134452312`.

Primary class: capability.

Plan:

- `plans/implementation/desktop-frontend-ide-foundation/004-desktop-session-control-plane-vertical-slice.md`

Outcome:

- project selection via `ProjectList`/`ProjectGet`, project-scoped workspace selection, and bounded session list/attach/create;
- `codegg-client` loss-aware projection driver built around the production `HeadlessProjectionConsumer`;
- canonical projection subscribe/replay/resume/ack/resync with typed transport-loss recovery;
- prompt submission and streamed visible agent activity;
- permission/question presentation and response through daemon authority;
- reconnect/restart handling;
- concurrent TUI + desktop use of one daemon;
- renderer reload/close without daemon-owned work loss.

M004 is the closure boundary for the desktop foundation. It intentionally does not require an editor.

### Milestone 005 — Shared editor document/buffer and LSP synchronization foundation

Status: conditionally closed. M005-A/B/C remain closed; M005-D has historical closure at `plans/closure/editor-document-foundation/004-status.md`, but current strict M005 closure is controlled by ready post-closure C001 at `plans/subsystems/editor-document-foundation-post-closure-corrective-addendum.md`. ADR-0011 remains accepted. M002's separate Windows evidence condition does not block this root/TUI-first corrective.

Ownership direction:

- daemon-owned ephemeral canonical open-document text/revision;
- pure frontend-neutral `codegg-document` text/transaction crate;
- optimistic `codegg-client` document replica/controller;
- UTF-8 byte-range transactions with explicit stale-revision/resync;
- checked disk-base hash/save/conflict behavior under existing workspace mutation authority;
- `egglsp` mirrors canonical unsaved text but does not own editor truth;
- ordinary agent/model file tools remain disk-authoritative;
- one writer/document in M005; no CRDT/OT requirement;
- cursor/selection/viewport/modal editor state remains frontend-owned;
- TUI is the first qualification frontend;
- graphical IDE editor integration is long-term and is not a closure dependency.

Registered implementation line:

1. `plans/implementation/editor-document-foundation/001-text-core-and-transaction-contract.md` — closed.
2. `plans/implementation/editor-document-foundation/002-daemon-document-service-and-protocol.md` — closed.
3. `plans/implementation/editor-document-foundation/003-lsp-sync-checked-save-and-external-conflict.md` — closed.
4. `plans/implementation/editor-document-foundation/004-client-replica-and-tui-first-qualification.md` — historical M005-D closure: `plans/closure/editor-document-foundation/004-status.md`; current strict authority is post-closure C001 at `plans/implementation/editor-document-foundation-post-closure-corrective/001-document-controller-linearization-scheduler-and-recovery.md`.

### Milestone 006 — TUI IDE vertical slice

Status: blocked on post-closure M005-D C001. Do not begin the TUI IDE presentation audit or register an M006 implementation handoff until `plans/subsystems/editor-document-foundation-post-closure-corrective-addendum.md` closes. No M006 implementation plan exists.

Expected TUI-first presentation/product direction:

- editor view consuming the shared M005 document controller rather than owning a second buffer;
- workspace/file explorer and search;
- diagnostics/completion/navigation from `egglsp`;
- syntax/semantic highlighting and editor motion/keymap decisions;
- existing interactive-process terminal integration;
- Git/worktree/run/job/agent views from canonical CodeGG state;
- explicit agent edit/apply/review workflows against saved/dirty documents.

M006 is intentionally TUI-first. Monaco/Tauri graphical IDE work is long-term and should consume the same M005 document foundation later; it is not the next implementation milestone.

## 8. Cross-cutting security and reliability

### Authorization

Every desktop mutation crosses the same daemon operation gate as the TUI/other clients. Tauri bridge validation is additional input validation only.

### Renderer compromise containment

The initial renderer receives no generic filesystem/shell/process plugin permission. Remote-origin IPC is disabled. Renderer requests are mapped to bounded domain operations in Rust.

### Stream bounds

High-frequency channels have explicit capacities, lag/resync behavior, and cancellation ownership. Projection events never bypass the canonical controller to avoid ordering forks.

### Daemon lifecycle

Desktop autostart may launch the existing `codegg daemon start` path using an explicit executable. The daemon must outlive the frontend that happened to launch it.

### Platform security

Unix socket/path permissions retain current user-scoped constraints. Windows named pipes require explicit restrictive ACLs and local-only behavior; default named-pipe security is not acceptable.

### Version skew

The desktop negotiates native protocol and feature capabilities. A GUI bundle and daemon with incompatible protocol generations fail with an actionable diagnostic; no silent downgrade to `/tui` or broad REST occurs.

## 9. Toolchain and distribution boundary

The root Cargo workspace remains Rust 1.89.

`apps/desktop/src-tauri` is a separate Cargo workspace/package boundary using the Rust version required by the selected Tauri minor. It consumes CodeGG path crates as libraries but cannot change their declared MSRV merely through workspace inheritance.

Node/TypeScript dependencies and lockfile are confined to `apps/desktop`.

Ordinary CLI/TUI builds and tests must not require Node, npm/pnpm, WebKitGTK development packages, Tauri, or desktop build tooling.

The existing Linux/macOS CLI installer/release contract remains closed and unchanged. Desktop bundling/signing/update policy is a separate future distribution decision.

## 10. Verification strategy

Foundation verification is layered:

1. Rust-only `codegg-client` tests and ownership guards.
2. Local-IPC transport fixtures, including peer death, concurrent request correlation, reconnect, and singleton races.
3. Desktop renderer unit/component tests with mocked narrow bridge calls.
4. Tauri command/bridge tests using a mock runtime where possible.
5. WebDriverIO/Tauri smoke coverage for the built desktop once M004 establishes meaningful behavior.
6. One two-client integration scenario with a TUI-compatible client and desktop client attached to the same daemon.
7. Windows local-IPC runtime evidence before Windows is described as supported for the desktop path.

No closure condition may silently become “builds on the maintainer machine.”

## 11. Documentation obligations

As milestones land, update at minimum:

- `architecture/overview.md`
- `architecture/client.md`
- `architecture/protocol.md`
- `architecture/core.md`
- `architecture/tui.md` where ownership descriptions move
- a new `architecture/desktop.md` once M003 creates a real desktop surface
- `architecture/process-tool-execution-ownership.md` if desktop launch/PTY ownership documentation needs classification
- `README.md` only when the desktop is actually runnable enough to document
- release/distribution docs only when distribution behavior changes

## 12. Completion definition

The desktop foundation workstream reaches its first closure boundary when M001-M004 close with evidence that:

- a separately built desktop frontend connects to the same singleton daemon as the TUI;
- it consumes canonical project/session/projection state;
- it can drive a bounded session interaction including permission/question handling;
- reconnect/resync and renderer shutdown preserve daemon-owned work;
- no WebView-general machine authority was introduced;
- root Rust 1.89/default CLI builds remain unaffected;
- Windows claims, if any, match actual portable-IPC evidence.

That closure makes the later document-buffer/IDE work dependency-ready. It does not itself claim IDE completion.

## 13. Milestone status

| Milestone | Status | Implementation plan | Hard/operational blocker |
|---|---|---|---|
| M001 shared frontend client runtime | closed | `plans/implementation/desktop-frontend-ide-foundation/001-shared-frontend-client-runtime.md` | `plans/closure/desktop-frontend-ide-foundation/001-status.md` |
| M002 portable local daemon transport | conditionally closed | `plans/implementation/desktop-frontend-ide-foundation/002-portable-local-daemon-transport.md` | `plans/closure/desktop-frontend-ide-foundation/002-status.md`; live Windows transport/lifecycle evidence and graceful stop remain required for strict closure |
| M003 Tauri desktop shell + secure bridge | closed | `plans/implementation/desktop-frontend-ide-foundation/003-tauri-desktop-shell-and-bridge.md` | Strict closure: `plans/closure/desktop-frontend-ide-foundation-corrective/003-status.md`; hosted root CI + built-app visible-window E2E green. Historical C001/C002 conditional records remain immutable. Windows qualification remains separately gated by M002 |
| M004 desktop session/control-plane slice | closed | `plans/implementation/desktop-frontend-ide-foundation/004-desktop-session-control-plane-vertical-slice.md` | Historical closure plus strict corrective: `plans/closure/desktop-frontend-ide-foundation/004-status.md`; `plans/closure/desktop-frontend-ide-foundation-post-closure-corrective/001-status.md` |
| M005 shared document/buffer foundation | conditionally closed; post-closure C001 ready | `plans/subsystems/editor-document-foundation-roadmap.md` + four original plans + `plans/implementation/editor-document-foundation-post-closure-corrective/001-document-controller-linearization-scheduler-and-recovery.md` | M005-A/B/C closed; M005-D historical closure preserved. C001 must harden the client controller before strict M005 closure is restored. ADR-0011 accepted; TUI-first, GUI editor deferred |
| M006 TUI IDE vertical slice | blocked | not yet written | Post-closure M005-D C001 strict closure, then fresh TUI IDE presentation audit |
