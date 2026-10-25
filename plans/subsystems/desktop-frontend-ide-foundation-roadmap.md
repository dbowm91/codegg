# Desktop Frontend and IDE Foundation Roadmap

Status: active

> **Development paused 2026-10-23; resumed 2026-10-24 by the owner's
> direction.** The roadmap's status is `active` throughout — the status
> vocabulary has no `paused` value, and inventing one would change the
> vocabulary every other subsystem is read through. What the pause stopped was
> the *work*, and it was stopped by the owner's decision, not by a dependency.
>
> The pause and its reason are preserved here because they remain the reason
> the hosted lane is red. Between 2026-10-23 and 2026-10-24 the hosted
> visible-window qualification (`.github/workflows/desktop-e2e.yml`) was
> **manual dispatch only** and did not run on push or on pull requests,
> because development on this line had stopped for now. The lane had already
> been red on `main` — its own run `39679968444` at `53d9fc9fd` fails with
> `element ("[data-testid="permission-list"]") still not existing after
> 120000ms` at `apps/desktop/e2e/specs/m004-session.e2e.ts:169:5`, and the
> nine runs behind it are consecutive failures — so the hosted run bought no
> signal while it cost a runner every time the desktop tree changed.
>
> **Resumed 2026-10-24.** The owner directed that this line carry a desktop
> GUI parity effort: a main chat region with a side panel, the same `/`
> commands the TUI exposes, and an editor — keyboard-first, and fully
> operable by mouse and by people used to a conventional IDE. That direction
> is what **Milestone 007** carries, and §7 records its decomposition. The
> red lane is now **work to be done, not a reason to stay paused**: restoring
> an automated gate that compiles and tests the desktop tree is M007-A, and
> the desktop tree is not built by any automated gate today. Nothing about
> the capability, its boundary, or any historical closure record changes.
> `architecture/desktop.md` §"The hosted qualification is paused (2026-10-23)"
> carries the pause note and is reconciled by M007-A.

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
- **M007:** the desktop renders a main chat region and a dockable side panel from the same frontend-neutral snapshot the TUI consumes, and the projection fields it already receives — runs, jobs, subagents, recent turns — are visible rather than discarded.
- **M007:** every desktop action is reachable by keyboard **and** by mouse, over a remappable keymap, with the TUI's conceptual interaction hierarchy carried across rather than its key bytes re-specified.
- **M007:** the desktop exposes the **same** command surface the TUI exposes, reached through the daemon rather than reimplemented in the renderer, so the two frontends cannot drift.
- **M007:** the desktop opens a project document, edits it, undoes, redoes and saves it over the shared document foundation, with LSP diagnostics and completion, and holds no second text buffer.

### Infrastructure

- `crates/codegg-client` reusable transport/client runtime.
- Platform-local endpoint abstraction.
- Cross-platform singleton lock primitive.
- Tauri/TypeScript application boundary with generated bridge DTOs and ordered channels.
- Desktop-specific test harnesses isolated from normal root Rust CI where appropriate.
- **M007-A:** an automated gate that compiles, lints and tests the desktop tree on every change to it, plus one local entry point that runs the same checks. The desktop workspace stays a separate Cargo workspace; the gate is a separate workflow, not workspace membership.

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

### 3.1 Amendment (2026-10-24) — Milestone 007 moves three items in scope

The 2026-10-23 list above was written when this line was deliberately
TUI-first. The owner's 2026-10-24 direction resumes the line with a desktop
GUI parity effort, so three items move from *not delivered by this roadmap*
to *delivered by Milestone 007*, and the surrounding wording changes with
them:

- **file explorer** → in scope for M007-E, as the editor's document-open
  affordance over the existing workspace directory listing;
- **docking system** → in scope for M007-B, as the side-panel frame and its
  placement model;
- **command palette parity** → in scope for M007-D, as the `/` command
  surface the TUI already exposes, reached through the protocol rather than
  reimplemented in the frontend.

The remaining items in that bullet are **unchanged and still deferred**:
**Monaco editor integration** stays out of scope, and the wording is
tightened to say why. The canonical documents name no editor engine, and
adopting a third-party engine would change the renderer's Content Security
Policy, its worker model, and the bundle and signing boundary — which
`architecture/desktop.md` §8 currently pins to local assets only. M007-E
therefore delivers a **first-party** editor surface over the shared document
foundation. Adopting Monaco or any other bundled editor engine is a separate
decision that needs its own ADR, because it materially changes the renderer
boundary this roadmap accepted.

**Git UI** and **integrated graphical terminal** also stay deferred; neither
is in the owner's stated direction, and neither has a registered plan.

The remainder of the list — unsaved editor-buffer/LSP synchronization, CRDT
or collaborative source editing, a web or mobile frontend, remote desktop
access over `/core`, TUI deprecation or feature freeze, a new `codeggd`
binary, daemon/TUI executable splitting, signing/notarization and
distribution, Windows promotion, a JavaScript reimplementation of the native
protocol reducer, and broad REST expansion — is **unchanged**. In particular
the CRDT collaborative-source-editor non-goal is canonical
(`plans/000-long-term-specification.md:74`) and is not reopened by this
amendment.

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
[closed]                                 [all M006-A…M006-E closed]
                              |
                              v
              M007 desktop GUI parity line (opened 2026-10-24)
                              |
              +---------------+---------------+---------------+
              |               |               |               |
              v               v               v               v
        M007-A build/     M007-B chat      M007-C         M007-D command
        hosted gate       + side panel     keyboard       surface [ready]
        [ready]          [ready]          [ready]        [ADR-0052 accepted]
              |                               |
              +---------------+---------------+
                              v
                        M007-E editor [ready]
```

M002 is not a hard dependency for a Linux/macOS M003 implementation because the existing Unix transport remains valid there. It is an operational dependency before Windows desktop support can be claimed.

M007's dependencies are stated in full in its decomposition table in §7. Three of them are worth naming here: **M007-A gates the whole line**, because no milestone in it can be closed honestly while the desktop tree is compiled by no automated gate; **M007-B and M007-C are sequenced** because a keymap binds to regions that do not exist until the chat region and side-panel frame land; and **M007-D's only blocker was ADR-0052's acceptance**, which was discharged on 2026-10-24 and was an evidence requirement rather than a dependency on another milestone. **M007-E** depends on M007-B alone and needs no ADR, because it sits under the already-accepted ADR-0011 and ADR-0012.

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

Status: closed. M005-A/B/C remain closed; M005-D's historical closure at `plans/closure/editor-document-foundation/004-status.md` is preserved, with current strict M005 closure at `plans/closure/editor-document-foundation-post-closure-corrective/001-status.md`. ADR-0011 remains accepted. M002's separate Windows evidence condition is independent.

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
4. `plans/implementation/editor-document-foundation/004-client-replica-and-tui-first-qualification.md` — historical M005-D closure: `plans/closure/editor-document-foundation/004-status.md`; current strict authority is post-closure C001 at `plans/closure/editor-document-foundation-post-closure-corrective/001-status.md`.

### Milestone 006 — TUI IDE vertical slice

Status: audit complete; decomposed into M006-A…M006-E. Audit of record:
`plans/subsystems/desktop-frontend-ide-foundation-m006-tui-presentation-audit.md`
(baseline `614e983e`). Strict M005 closure is recorded at
`plans/closure/editor-document-foundation-post-closure-corrective/001-status.md`.

The audit discharged the presentation-audit gate. M006 as originally written is
not a single bounded handoff: it combines an editor widget subsystem, a
layout/route change, a file tree, a new native LSP read protocol surface, an LSP
delivery decision, and an agent dirty-buffer decision. Per
`plans/003-planning-process.md` §5 that exceeds milestone sizing, so M006 is
sequenced below. M006-A is dependency-ready now; M006-B carries an ADR
decision. M006-E was subsequently scoped to saved documents only, which
removed its ADR requirement and deferred the dirty-buffer half.

Expected TUI-first presentation/product direction:

- editor view consuming the shared M005 document controller rather than owning a second buffer;
- workspace/file explorer and search;
- diagnostics/completion/navigation from `egglsp`;
- syntax/semantic highlighting and editor motion/keymap decisions;
- existing interactive-process terminal integration;
- Git/worktree/run/job/agent views from canonical CodeGG state;
- explicit agent edit/apply/review workflows against saved/dirty documents.

#### M006 decomposition

| Sub-milestone | Scope | Dependency | Ready? |
|---|---|---|---|
| M006-A | Editor buffer over the shared `DocumentController`: render, gutter, current line, selection, motion, frontend undo/redo, scroll/wrap, editor route + layout branch, minimal document-open path. No new protocol, no LSP. | M005 strict closed | **closed** — merged as `94f38421` (PR `#92`, implementation `1706b50b`, CI step fix `b993522a`) — `plans/closure/desktop-frontend-ide-foundation/005-status.md` |
| M006-B | Native LSP read surface: protocol + daemon handlers + authorization for diagnostics, hover, definition, references, symbols, semantic tokens; delivery-path decision (poll vs new project-scoped event path). | ADR required | **ADR-0012 accepted**; **implemented and closed** (`4ce0343b`, PR `#99`) — `plans/closure/desktop-frontend-ide-foundation/009-status.md`, plan `plans/implementation/desktop-frontend-ide-foundation/008-native-lsp-read-surface.md`. Transport and authority layer only; TUI rendering is M006-C. Two `CoreRequest` variants plus a dedicated gated subscribe; reads are warm-server-only with payload-free `NotReady`; both read families and the stream are gated at `file.read` |
| M006-C | LSP presentation in the editor: diagnostic markers, semantic/syntax highlighting, navigation actions. | M006-B + post-closure correctives C003 and C004 | **closed** — closure `plans/closure/desktop-frontend-ide-foundation/010-status.md`; implementation `91cbe784`; hosted CI `38256472016` and Desktop E2E `38256472313` green. |
| M006-D | File tree and explorer. | M006-A | **implemented and closed** (`de29e49e`, PR `#94`) — `plans/closure/desktop-frontend-ide-foundation/006-status.md`; no ADR required |
| M006-E | Agent change review against **saved** documents: show a pending LSP candidate and wait for accept or reject before the existing apply runs. | M006-A | **implemented and closed** (`cde7dbfe`, PR `#96`) — `plans/closure/desktop-frontend-ide-foundation/007-status.md`; scoped to saved documents only, so **no ADR**; the apply-into-dirty-buffer half is deferred to a future ADR |

M006-A is implemented (`1706b50b`, CI step fix `b993522a`) and **closed**;
it merged as `94f38421` through PR `#92`. It delivers a non-modal
`Route::Editor` primary view over
the shared `DocumentController`: a presentation model, a viewport-bounded
render widget, and vi-style motion, editing, frontend undo/redo, and save. It
adds no protocol surface and no LSP.

Two gates applied. First, it added two read-only accessors
(`try_snapshot`, `try_attachment_info`) to `codegg-client`, which its plan
listed as not-to-edit; the alternative was caching document text in the
frontend, which the plan's own single-buffer invariant forbids. **That
deviation is approved.** Second, hosted evidence was required. Hosted CI
`37219495080` has all sixteen guard and lint steps green, including the new
`check_tui_editor_text_authority.py` as its own named step, and Desktop E2E
`37219495249` is green.

The hosted `nextest` workspace sweep was red on a **pre-existing** causal
tool-advisor 5 ms wall-clock flake. Two attempts failed in two *different*
budget tests, the first attempt's failure passed in the second, and a separate
worktree at baseline `main` reproduces it at 4/8 versus 2/8 on this branch. None
of `tests/causal_*.rs` or `src/tool_advisor/` is in this change set. That flake
was filed as its own corrective and is now **fixed** by the causal frontier
timing corrective C001 (`cdfd6257`; closure
`plans/closure/tool-selection-advisor-causal-frontier-timing-corrective/001-status.md`):
its three single-shot wall-clock asserts measured the CI runner rather than the
advisor, so they now use warm best-of-15 sampling — or, for the live-loop site
that cannot be re-run, a sanity check. Both frozen 5.0 ms budgets and every
assertion are unchanged, so no advisor behavior, disposition, or frozen
threshold moved.

Consequently **M006-D and M006-E are unblocked**, and **M006-B is gated only
on its own ADR**, which was always its real precondition. M006-E later proved
not to carry an ADR at all: scoping it to saved documents leaves the existing
dirty-buffer rejection correct and unchanged, so the milestone adds a review
step in front of the apply that already exists rather than changing what an
apply means.

Two audit findings constrain this sequence. First, the M005 foundation had no
consumer: `TuiDocumentSession` (`src/tui/document_session.rs`) was never
constructed in production and `TuiDocumentPresentation` had no consumer, so
M006-A is the first presentation layer rather than an addition to one. Second,
no LSP read operation exists in the native protocol — the only LSP variant is
the `LspPreviewApply` write — so M006-B is a protocol and daemon slice, not
TUI-local work.

M006 is intentionally TUI-first. Monaco/Tauri graphical IDE work is long-term and should consume the same M005 document foundation later; it is not the next implementation milestone.

### Milestone 007 — Desktop GUI parity line

Status: proposed (2026-10-24). Opened by the owner's direction that the desktop
carry a TUI-parity GUI: a main chat region with a side panel, the same `/`
commands the TUI exposes, and an editor — keyboard-first, and fully operable
by mouse and by people used to a conventional IDE.

Primary class: capability, with M007-A as infrastructure.

**Why this is an interim line and not a canonical amendment.** The canonical
documents are **silent** on a desktop GUI: `desktop` as a product, `GUI`,
`graphical`, `Monaco`, `palette`, `keymap`, `mouse`, `sidebar` and `dock` have
no occurrence in `plans/000-long-term-specification.md`,
`plans/001-terminology-and-domain-model.md` or
`plans/002-long-term-roadmap.md`. The GUI is neither required nor forbidden
there. What the canonical set *does* bind is the shape any frontend must take:
§4.4 requires that "TUIs, ACP clients, web clients, and future frontends
MUST render frontend-neutral snapshots and events" and that they "MUST NOT
become independent owners of provider credentials, project authorization,
global scheduling, or durable session state"
(`plans/000-long-term-specification.md:95-97`); Phase 5's exit criterion is
that "Two different frontend implementations can reconstruct the same session
state" (`plans/002-long-term-roadmap.md:262`); and the cross-phase execution
rule 3 requires adding "typed protocol and storage representations before
frontend-only state" (`plans/002-long-term-roadmap.md:17`). M007 is built to
those constraints. §3.1 records what the amendment moved in scope.

**What already exists, and is not M007's work.** The daemon protocol has
**268** `CoreRequest` variants; the desktop host names **25** of them in its
own production code, across **23** bridge targets — **no** `Document*` and
**no** `Lsp*` variant among them. The TUI already has the whole surface M007 mirrors: **161**
`BuiltinSlashAction` variants (`src/tui/command.rs`), a dispatcher with an arm
for **every one** of them (`src/tui/app/mod.rs:5661-8895`), **61** bindable
`ActionKey`s with a remappable keymap (`src/tui/input.rs:259`), **32**
`SidePanelId` panel kinds (`src/tui/panels/mod.rs`), a dockable
`PanelRegions { main, left, right, bottom }` frame
(`src/tui/layout.rs:37`), **42** dialog files
(`src/tui/components/dialogs/`), and mouse capture with Down/Up/Drag/Scroll
handling (`src/tui/terminal.rs:83`). The shared client crate already carries
the editor substrate: `DocumentController` with
`impl DocumentTransport for LocalSocketClient`
(`crates/codegg-client/src/document.rs:35`) and
`lsp_read`/`lsp_completion`/`lsp_diagnostics`
(`crates/codegg-client/src/lsp.rs:64`, `:93`, `:107`). **M007 builds a
frontend over existing authority; it does not invent a second authority.**

**What did not exist, and is why the line has a gate milestone first.** The
desktop renderer was one `App()` component of 407 lines
(`apps/desktop/src/App.tsx:15`) rendering stacked `<section>` cards in a
single 760-pixel column (`apps/desktop/src/style.css`), with **zero**
keyboard handlers and twelve `onClick` bindings. Its projection DTO already
carried `runs`, `jobs`, `subagents` and `recentTurns`
(`apps/desktop/src/bridge-types.ts:298-310`); the parity work that merged
before this line opened now renders `jobs` and `subagents`, and still drops
`runs` and `recentTurns`. `apps/desktop/src-tauri` is excluded from the root
Cargo workspace (`Cargo.toml:6`) — deliberately, per §9 — so the root's own
gate never compiles or tests the desktop tree, which is why the hosted
visible-window lane is the desktop's only automated gate. That lane was
paused and red (`architecture/desktop.md:154-180`) until M007-A restored it:
the lane is now green, runs on the pull request again, lints the desktop
host at `clippy -D warnings` for the first time, and is protected by a guard
that fails closed when the tree falls out of it. M007-A restores that gate
before any other milestone lands, because "No closure condition may silently
become 'builds on the maintainer machine'" (§10) and nothing else in this
line can be closed honestly without it.

**What M007-C changed.** The **zero keyboard handlers** claim above was true
of the baseline and is now false: the desktop was a mouse-only application that
happened to be a TUI's distant cousin. M007-C gives it its own
renderer-owned input model — a remappable keymap
(`apps/desktop/src/keymap.ts`), a region-scoped focus model with a visible
indicator (`apps/desktop/src/focus.ts`), and a bindings surface
(`apps/desktop/src/panels/KeybindingsPanel.tsx`) — carrying **19** declared
actions, every one of them reachable by chord **and** by mouse over the same
affordance. The TUI is untouched: `git diff --name-only -- src/tui/` is empty,
and the desktop's catalog is *derived* from the TUI's `ActionKey`, never a
second definition of it. The desktop's shortcuts are **in-window only** — no
OS-global shortcut exists, and none is permitted. Closure:
`plans/closure/desktop-frontend-ide-foundation/013-status.md`.

#### M007 decomposition

| Sub-milestone | Scope | Dependency | Ready? |
|---|---|---|---|
| M007-A | **Desktop build and hosted gate.** An automated gate compiles the desktop Rust workspace and runs the renderer's typecheck and tests on every change to the desktop tree; the red visible-window lane is made green again and its triggers restored; the desktop tree is no longer outside every automated gate. | none — M001…M006-E are closed | **closed** — the lane is green and gated; the host is linted at `clippy -D warnings` for the first time; one local entry point runs the same checks the hosted lane runs; a guard proves the tree is still inside a gate — `plans/closure/desktop-frontend-ide-foundation/011-status.md` |
| M007-B | **Chat region and side-panel frame.** A main chat region and a dockable side panel, mirroring the TUI's `PanelRegions` placement model; the projection fields the desktop already receives (`runs`, `jobs`, `subagents`, `recentTurns`) become a visible tasks panel; the renderer stops being one component. | M007-A (**closed 2026-10-24**) | **closed** — the frame is a named-region CSS grid, the panel layer docks on any edge and resizes, and `runs` and `recentTurns` are rendered — `plans/closure/desktop-frontend-ide-foundation/012-status.md` |
| M007-C | **Keyboard-first input.** A shared, remappable keymap; the TUI's conceptual interaction hierarchy carried over; platform-idiomatic chords alongside it; full mouse operability. | M007-A, M007-B (**both closed 2026-10-24**) | **closed** — the desktop has its own renderer-owned keymap, a region-scoped focus model with a visible indicator, and a bindings surface; all 19 declared actions are reachable by chord **and** by mouse — `plans/closure/desktop-frontend-ide-foundation/013-status.md` |
| M007-D | **`/` command parity.** The TUI's command surface reached through the daemon rather than reimplemented in the renderer. | M007-C (**closed 2026-10-24**), **ADR-0052 accepted 2026-10-24** | **ready for handoff** — the ADR-0052 blocker is discharged and the palette's chord is now bound by M007-C; it waits on M007-A's gate — `plans/implementation/desktop-frontend-ide-foundation/014-desktop-slash-command-parity.md` |
| M007-E | **Editor surface.** A first-party editor over the shared `DocumentController`, with LSP diagnostics, completion and navigation; a document-open affordance over the existing workspace directory listing. | M007-B, accepted ADR-0011 (already accepted) and ADR-0012 (already accepted) | **ready for handoff** — `plans/implementation/desktop-frontend-ide-foundation/015-desktop-editor-surface.md` |

**M007-D is the one milestone that changes an ownership contract**, which is
why it is gated on an ADR and the other four are not. The TUI's command
catalog is TUI-local: `BuiltinSlashAction` appears nowhere outside `src/tui/`,
and no `CoreRequest` variant executes a command. Making the desktop's `/`
surface *the same commands* therefore moves the command surface from the
frontend to the daemon, which is a new protocol and a new ownership boundary
— an ADR-scale decision under `plans/003-planning-process.md`'s threshold
rules, and the one the canonical documents leave open
(`plans/000-long-term-specification.md:341` says only that "The TUI **and
protocol** SHOULD expose commands or views", and `:368-379` fixes the
discoverability requirement while explicitly allowing the final names to
differ). M007-D therefore waited on ADR-0052, whose acceptance on 2026-10-24 discharged that wait; M007-D's remaining dependencies are M007-A and M007-C, and the other milestones do not wait on the ADR.

**M007-E does not need a new ADR.** ADR-0011 already decided editor document
ownership and frontend replication, and ADR-0012 already decided the native
LSP read surface and its authorization; M007-E is a second frontend over
those two accepted contracts.

**Explicitly not in this line.** A Git UI; an integrated graphical terminal;
a third-party or bundled editor engine (see §3.1); collaborative editing; a
web or mobile frontend; a remote-desktop or screen-sharing path; and any
change to the TUI's own behaviour. M007 mirrors the TUI; it does not
modify it.

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

**M007 has its own completion boundary, and it is a different one.** The M001–M004 boundary above is about the desktop *connecting* to the daemon as a second frontend at all. M007's boundary is about that frontend being **usable and honest at TUI parity**: an automated gate compiles, lints and tests the desktop tree (M007-A); the desktop renders a main chat region and a dockable side panel from the same frontend-neutral snapshot the TUI consumes, with the projection fields it already receives actually visible (M007-B); every action is reachable by keyboard **and** by mouse over a remappable keymap (M007-C); the `/` command surface is the TUI's, reached through the daemon and audited as a `Run`, with one catalog rather than two (M007-D); and the desktop opens, edits and saves a project document over the shared document foundation, with LSP diagnostics and no second text buffer (M007-E). M007 does **not** claim IDE completion either — a Git UI, an integrated graphical terminal, collaborative editing and any third-party editor engine remain outside it (§3.1). Its own closure evidence is the five milestone closure records, and per `plans/003-planning-process.md:249` no part of it may be marked complete solely because its infrastructure milestone landed.

## 13. Milestone status

| Milestone | Status | Implementation plan | Hard/operational blocker |
|---|---|---|---|
| M001 shared frontend client runtime | closed | `plans/implementation/desktop-frontend-ide-foundation/001-shared-frontend-client-runtime.md` | `plans/closure/desktop-frontend-ide-foundation/001-status.md` |
| M002 portable local daemon transport | conditionally closed | `plans/implementation/desktop-frontend-ide-foundation/002-portable-local-daemon-transport.md` | `plans/closure/desktop-frontend-ide-foundation/002-status.md`; live Windows transport/lifecycle evidence and graceful stop remain required for strict closure |
| M003 Tauri desktop shell + secure bridge | closed | `plans/implementation/desktop-frontend-ide-foundation/003-tauri-desktop-shell-and-bridge.md` | Strict closure: `plans/closure/desktop-frontend-ide-foundation-corrective/003-status.md`; hosted root CI + built-app visible-window E2E green. Historical C001/C002 conditional records remain immutable. Windows qualification remains separately gated by M002 |
| M004 desktop session/control-plane slice | closed | `plans/implementation/desktop-frontend-ide-foundation/004-desktop-session-control-plane-vertical-slice.md` | Historical closure plus strict corrective: `plans/closure/desktop-frontend-ide-foundation/004-status.md`; `plans/closure/desktop-frontend-ide-foundation-post-closure-corrective/001-status.md` |
| M005 shared document/buffer foundation | closed | `plans/subsystems/editor-document-foundation-roadmap.md` + four original plans + `plans/closure/editor-document-foundation-post-closure-corrective/001-status.md` | M005-A/B/C and current strict M005-D closure are recorded; historical M005-D closure remains immutable. ADR-0011 accepted; TUI-first, GUI editor deferred |
| M006 TUI IDE vertical slice | audit complete; decomposed M006-A…M006-E; all M006-A…M006-E implemented and closed | M006-A: `plans/implementation/desktop-frontend-ide-foundation/005-editor-buffer-vertical-slice.md`; M006-D: `plans/implementation/desktop-frontend-ide-foundation/006-project-scoped-file-tree.md`; M006-E: `plans/implementation/desktop-frontend-ide-foundation/007-agent-change-review.md`; M006-B: `plans/implementation/desktop-frontend-ide-foundation/008-native-lsp-read-surface.md`; M006-C: `plans/implementation/desktop-frontend-ide-foundation/009-lsp-editor-presentation.md` | M006-A: `plans/closure/desktop-frontend-ide-foundation/005-status.md` (merged `94f38421`). M006-A is closed and its two additive read-only `codegg-client` accessors are approved. **M006-D is closed** at `plans/closure/desktop-frontend-ide-foundation/006-status.md` (`de29e49e`, PR `#94`). **M006-E is closed** at `plans/closure/desktop-frontend-ide-foundation/007-status.md` (`cde7dbfe`, PR `#96`), scoped to saved documents only; the dirty-buffer question remains deferred. **M006-B is closed** at `plans/closure/desktop-frontend-ide-foundation/009-status.md` (`4ce0343b`, PR `#99`) under accepted ADR-0012. **M006-C is closed** at `plans/closure/desktop-frontend-ide-foundation/010-status.md` (`91cbe784`; historical PR #111 attribution requires reconciliation); exact-SHA hosted CI and built-app Desktop E2E passed. C003 and C004 are closed at their corrective records; location identity remains optional and validated. |
| M007 desktop GUI parity line | proposed (opened 2026-10-24 by the owner's direction); decomposed M007-A…M007-E; **M007-A, M007-B and M007-C closed 2026-10-24; M007-D and M007-E ready for handoff; ADR-0052 accepted 2026-10-24** | M007-A: `plans/implementation/desktop-frontend-ide-foundation/011-desktop-build-and-hosted-gate.md`; M007-B: `plans/implementation/desktop-frontend-ide-foundation/012-desktop-chat-region-and-side-panel.md`; M007-C: `plans/implementation/desktop-frontend-ide-foundation/013-desktop-keyboard-first-input.md`; M007-D: `plans/implementation/desktop-frontend-ide-foundation/014-desktop-slash-command-parity.md`; M007-E: `plans/implementation/desktop-frontend-ide-foundation/015-desktop-editor-surface.md` | No hard dependency is outstanding for M007-A, M007-B, M007-C or M007-E — M001…M006-E are closed, ADR-0010, ADR-0011 and ADR-0012 are accepted, and the shared `DocumentController` and native LSP read surface are stable interface dependencies. **M007-D's only blocker is ADR-0052's acceptance** (`plans/adrs/ADR-0052-cross-frontend-command-execution-ownership.md`), which is the one place in this line that changes an ownership contract. M007-A must close first: no milestone in this line can be closed honestly while the desktop tree is compiled by no automated gate. |


## 14. TUI editor correctness/usability successor (2026-10-13)

M006-A…E historical closures remain intact. A source-level audit of `main` at `6b611b5` found post-closure risks: the TUI drops entire valid >100-entry semantic-token responses although the daemon supports 1000 entries; full logical-line reads and width calculations can scale render work with multi-megabyte lines; file-tree traversal runs synchronous I/O in async tasks and lacks an independent examined-entry budget. Subsequent modal-editor/agent scope and `/files` changes landed on main outside the original M006 decomposition.

The separately registered `plans/subsystems/tui-editor-correctness-usability-roadmap.md` defines C001 (semantic fidelity) and C002 (viewport/tree work), both ready, plus M003 (native LSP completion contract) and M004 (transactional popup), gated. Proposed `plans/adrs/ADR-0021-native-editor-completion-read-and-local-edit.md` uses 0021 because ADR-0019 and ADR-0020 are already allocated on the active Windows sandbox branch. M003 is blocked until ADR-0021 is accepted; M004 is blocked until M003 and C001 close. Do not bypass accepted document/LSP authority or treat proposed ADR as approval.

Historical M006-C closure cites implementation SHA `91cbe784` and PR #111; the currently open PR #111 is unrelated shell-promotion work. The exact SHA and hosted CI records are controlling evidence until the PR attribution is independently reconciled; historical closure records remain unchanged.
