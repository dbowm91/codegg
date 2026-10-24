# Desktop Frontend and IDE Foundation Milestone M007-E — Desktop Editor Surface over the Shared Document Foundation

Status: ready for handoff

Repository baseline: `c4fa30aeba439894b048768d4829dea0cfa0b84b` (the `origin/main` head)

Source roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#milestone-007--desktop-gui-parity-line`
- M007 decomposition table: `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#m007-decomposition`
- §3.1 amendment (2026-10-24): `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md#31-amendment-2026-10-24--milestone-007-moves-three-items-in-scope`

Planning basis:

- `plans/implementation/desktop-frontend-ide-foundation/005-editor-buffer-vertical-slice.md` — **the
  reference implementation**. M006-A delivered the TUI's editor over the same shared
  `DocumentController`. M007-E is the **second** frontend over that foundation and must reuse
  M006-A's semantics rather than re-derive them.
- The sibling TUI IDE milestones this one mirrors: `.../006-project-scoped-file-tree.md` (M006-D),
  `.../007-agent-change-review.md` (M006-E), `.../008-native-lsp-read-surface.md` (M006-B),
  `.../009-lsp-editor-presentation.md` (M006-C).
- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md` (accepted) —
  controlling for editor document ownership.
- `plans/adrs/ADR-0012-native-lsp-read-surface-delivery-and-authorization.md` (accepted) —
  controlling for the native LSP read surface, which this milestone **consumes** and does not
  re-decide.
- `plans/adrs/ADR-0010-desktop-frontend-and-shared-client-boundary.md` (accepted).

Long-term requirements:

- `plans/000-long-term-specification.md:131` — "An execution node is authoritative for local process
  trees, local workspace and worktree paths, local filesystem state, local Git operations, PTYs,
  **LSP servers**, build caches, test processes, machine resource inventory, and **uncommitted
  editor state**." The canonical anchor for editor ownership.
- `plans/000-long-term-specification.md:476` — "Frontends MUST use project-relative file and
  artifact operations. They MUST NOT assume that a daemon-local path is directly accessible to the
  frontend machine."
- `plans/000-long-term-specification.md:97` — frontends render projections and must not become
  independent owners of durable state.
- `plans/000-long-term-specification.md:602` — authorization is enforced in the daemon.
- `plans/000-long-term-specification.md:74` — the CRDT collaborative-source-editor non-goal is
  canonical and is **not** reopened by this milestone.
- `plans/002-long-term-roadmap.md:17` — typed protocol and storage representations come before
  frontend-only state. `plans/003-planning-process.md:204-210` — authority order.
- `plans/001-terminology-and-domain-model.md:5` — terminology is normative for protocol types,
  schemas, docs, tests and UI labels. **The canonical set contains no `Editor` and no `Document`
  domain term** — neither word is a defined term there. This milestone therefore uses the
  codebase's existing type names (`DocumentController`, `DocumentState`, `document.v1`) and does
  **not** invent canonical vocabulary for the editor surface.

Applicable architecture:

- `architecture/document.md` — the `document.v1` contract and the M005 limitations that still hold.
- `architecture/lsp.md:925` — the native LSP read surface (M006-B, ADR-0012).
- `architecture/desktop.md` — §8 renderer security boundary, §"Deferred surfaces"
  (`architecture/desktop.md:291-292`), and the paused hosted lane
  (`architecture/desktop.md:154-180`).

Predecessor closures (hard dependency, satisfied):

- M001…M006-E are closed. The current strict authorities are the M005 closure set
  (`plans/closure/editor-document-foundation-post-closure-corrective/001-status.md`) and the M006
  records `005-status.md`, `006-status.md`, `007-status.md`, `009-status.md`, `010-status.md`
  under `plans/closure/desktop-frontend-ide-foundation/`.

Primary class: capability

> This milestone delivers an **editor surface**, not an IDE. It adds no protocol variant, no storage
> change, no migration, and no LSP write operation, and it changes nothing in the TUI.

## 1. Objective

Deliver the desktop's first editor over the shared document foundation: a user can open a project
file in the desktop, see it rendered with a line-number gutter, edit it, undo, save it, and see LSP
diagnostics on it — and the desktop holds **no second text buffer** and reads **no file directly**.

One bounded outcome: the desktop becomes the second frontend over the M005 `DocumentController` and
the M006-B LSP read surface, with the same authority the TUI has and none of the TUI's code
changed.

## 2. Why this milestone is ready

**Closed hard dependencies.** M001…M006-E are closed; M005 is **strictly closed**, so the
`DocumentController` semantics, the `document.v1` protocol surface and the daemon `DocumentService`
are immutable inputs here, not work items. ADR-0011 (editor document ownership and frontend
replication), ADR-0012 (native LSP read surface delivery and authorization) and ADR-0010 (desktop
frontend and shared client boundary) are **accepted**. **No new ADR is required** — the roadmap says
so directly (`plans/subsystems/desktop-frontend-ide-foundation-roadmap.md:557-559`: "M007-E does
not need a new ADR. ADR-0011 already decided editor document ownership and frontend replication, and
ADR-0012 already decided the native LSP read surface and its authorization; M007-E is a second
frontend over those two accepted contracts."). This milestone sits under exactly those two.

**Hard dependency on M007-A (the desktop gate).** `apps/desktop/src-tauri` is excluded from the
root Cargo workspace at `Cargo.toml:6`
(`exclude = ["crates/eggwork-test-node", "apps/desktop/src-tauri"]`), so **no automated gate
compiles or tests the desktop tree at all**, and the hosted visible-window lane is **paused and
red** (`architecture/desktop.md:154-180`: `.github/workflows/desktop-e2e.yml` is manual-dispatch
only, and `main`'s most recent run `39679968444` at `53d9fc9fd` fails at
`apps/desktop/e2e/specs/m004-session.e2e.ts:169:5`). The roadmap's rule is binding:
`plans/subsystems/desktop-frontend-ide-foundation-roadmap.md:527-531` and `:618` — "No closure
condition may silently become 'builds on the maintainer machine.'" **M007-A must close before this
milestone can be closed**; its closure evidence must come from a hosted gate, not a local run.

**Hard dependency on M007-B (the region frame).** The editor occupies a region. M007-B delivers the
desktop's chat region and dockable side-panel frame, mirroring the TUI's `PanelRegions { main,
left, right, bottom }` model (`src/tui/layout.rs:37`). The editor renders into that frame and
must not introduce a competing region model. M007-C (the shared remappable keymap) is **not** a
hard dependency: the editor ships with the basics it needs and M007-C may absorb them later.

**Stable interface dependencies — all already exist, none re-derived here:** the `DocumentController`
lifecycle (`crates/codegg-client/src/document.rs:156`); `DocumentTransport` and its
`LocalSocketClient` impl (`:30`, `:35`); `DocumentSnapshot`'s byte/line primitives
(`crates/codegg-document/src/buffer.rs:18`, `:39`, `:45`, `:49`, `:62`, `:109`, `:123`); the
`document.v1` protocol variants (`crates/codegg-protocol/src/core.rs:2317-2400`); the native LSP
read surface (`crates/codegg-client/src/lsp.rs:64`, `:93`, `:107`); and the workspace directory
listing (`crates/codegg-protocol/src/document.rs:163`, `:211`).

**What the desktop already has.** The desktop's Rust host already holds a `LocalSocketClient`:
`apps/desktop/src-tauri/src/lib.rs:53` (`client: Mutex<Option<LocalSocketClient>>`), built at
`apps/desktop/src-tauri/src/lib.rs:672` (`LocalSocketClient::connect(&endpoint, frontend.clone())`).
Because `impl DocumentTransport for crate::LocalSocketClient` already exists
(`crates/codegg-client/src/document.rs:35`), constructing a `DocumentController` over the client the
host already has needs **no new protocol and no new transport**. The editor's transport is *wire
up, don't build*.

## 3. Current implementation evidence

- **The shared editor substrate lives in `crates/codegg-client`**, not in the root package:
  `DocumentController` (`crates/codegg-client/src/document.rs:156`) with `new` (`:237`), `state`
  (`:250`), `attachment_scope` (`:257`), `snapshot` (`:268`), `try_snapshot` (`:279`),
  `try_attachment_info` (`:287`), `open` (`:310`), `create` (`:428`), `delete_file` (`:527`),
  `apply_local` (`:618`), `flush` (`:759`), `save` (`:846`), `reload_from_disk` (`:959`), `resync`
  (`:1030`), `reconnect` (`:1101`), `poll_status` (`:1253`), `close` (`:1335`). `try_snapshot` is
  documented as the synchronous, **non-copying** render-time accessor (`:271-283`), and
  `try_attachment_info` returns **no document text** (`:284-305`; the type at `:97-118` says so).
  `DocumentState` has exactly ten variants (`:48-60`): `Closed`, `Opening`, `Synced`, `DirtyLocal`,
  `Flushing`, `ResyncRequired`, `Conflict`, `ReadOnly`, `Disconnected`, `GoneWithLocalDraft`;
  `DocumentControllerError` has exactly eleven (`:63-86`): `Transport`, `Response`, `Edit`,
  `NotOpen`, `AlreadyOpen`, `LifecycleBusy`, `ReadOnly`, `QueueFull`, `QueueBytes`,
  `ResyncRequired`, `Conflict`.
- `apply_local` is synchronous, updates the rope-backed optimistic replica immediately, and refuses
  when the controller is not idle/saving, when there is no writer lease, or when the pending queue
  is full (`crates/codegg-client/src/document.rs:618-674`).
- `lsp_read`, `lsp_completion` and `lsp_diagnostics` are **inherent methods on `LocalSocketClient`**
  (`crates/codegg-client/src/lsp.rs:64`, `:93`, `:107`), re-exported through the `LspReadClient`
  trait (`crates/codegg-client/src/lib.rs:33-36`), alongside a `DiagnosticsReconciler`
  (`crates/codegg-client/src/lsp.rs:173`).
- The daemon's `document.v1` surface is complete and additive: `DocumentCapabilities`,
  `DocumentOpen`, `DocumentSnapshotGet`, `DocumentStatusGet`, `DocumentWriterAcquire`,
  `DocumentChange`, `DocumentSave`, `DocumentReload`, `DocumentClose`
  (`crates/codegg-protocol/src/core.rs:2317-2400`). Its LSP read surface is `LspReadGet`,
  `LspCompletionGet` and `LspDiagnosticsGet`, with `LspReadStatus::{Ready, NotReady, Ambiguous,
  Unsupported}` (`crates/codegg-protocol/src/lsp.rs:433-443`). The file-tree surface is
  `WorkspaceDirectoryList` (`crates/codegg-protocol/src/document.rs:163`, `:211`), handled at
  `src/core/daemon_documents.rs:1081` (`directory_listing`) and used by the TUI at
  `src/tui/commands/file_tree.rs:209-218`.
- Authorization: `DocumentOpen`/`SnapshotGet`/`StatusGet`/`Close` carry `DirectProject` +
  `FileRead` (`crates/codegg-core/src/authorization/policy.rs:108-114`); `WorkspaceDirectoryList`
  carries `DirectProject` + `FileRead` (`:121-125`); `DocumentWriterAcquire`/`Change`/`Save`/
  `Reload` carry `DirectProject` + `FileModify` (`:127-132`); `LspReadGet` carries `ViaSession` +
  `FileRead` (`:918-922`), `LspCompletionGet` `ViaSession` + `FileRead` (`:927-931`), and
  `LspDiagnosticsGet` `DirectProject` + `FileRead` (`:932-936`).
- **The TUI's editor (M006-A) is non-modal**: `Route::Editor` is a primary view with its own focus
  concept, following the `Route::Workspace` precedent (`src/tui/route.rs:2-22`, `Route::Editor` at
  `:21`). It has a line-number gutter, current-line highlight, horizontal scroll, cursor and
  anchor/selection over UTF-8 byte offsets clamped to byte boundaries, vi-style motion and editing,
  frontend-owned undo/redo (`src/tui/editor.rs:1702` `EditorUndoEntry`, `:5243` `push_undo`, `:5285`
  `undo_entry`, `:5293` `redo_entry`, `:5486` `clear_history`), local edits applied through
  `apply_local`, a checked save/conflict/reload/close mapped to controller states, and a bounded
  explicit open path for one workspace-relative path (`src/tui/commands/editor.rs:91`
  `normalize_relative_path`, `:140` `open_editor`, `:153` `open_editor_in`). It adds **no protocol
  surface and no LSP**. Its undo history is depth- and byte-capped (`MAX_EDITOR_UNDO_DEPTH = 256`,
  `MAX_EDITOR_UNDO_BYTES = 4 * 1024 * 1024` — `src/tui/editor.rs:36`, `:43`, enforced at `:5374`
  and `:5439-5440`).
- The TUI's editor path is guarded statically against a retained whole-document text container and
  any direct filesystem read: `scripts/check_tui_editor_text_authority.py` scans five files
  (`:90-96`), rejects text-named fields (`:137-146`), owned `DocumentSnapshot`/`DocumentBuffer`/
  `Rope` fields and materialized `Vec<String>`/`Vec<Line>` fields (`:122-127`), and filesystem read
  APIs (`:100-109`). It runs in `scripts/verify.sh quick` and in CI.
- **The desktop renderer has no editor and no file tree**:
  `architecture/desktop.md:291-292` — "Editor and terminal, provider credentials, plugin UI, remote
  daemon access, signing, and distribution automation are not implemented." The desktop's Rust host
  has **20** `#[tauri::command]` functions (`apps/desktop/src-tauri/src/lib.rs:654-978`) and uses
  **18** of the daemon's **266** `CoreRequest` variants in production code — **no** `Document*`
  and **no** `Lsp*` variant among them.
- The desktop's bridge is a narrow, reviewed DTO contract: Rust DTOs in
  `apps/desktop/src-tauri/src/bridge.rs`, mirrored field-for-field in
  `apps/desktop/src/bridge-types.ts`, with the mapping asserted by
  `apps/desktop/scripts/check-bindings.mjs`; the renderer reaches the host only through
  `apps/desktop/src/bridge.ts`. The trust boundary is already pinned: the main capability carries
  **no** permissions (`apps/desktop/src-tauri/capabilities/main.json`), the renderer CSP forbids
  remote origins and `unsafe-eval` (`apps/desktop/src-tauri/tauri.conf.json`; asserted at
  `scripts/check-desktop-boundary.sh:95-96`), and the boundary guard already rejects a
  `core_request`/`read_file`/shell-invoke passthrough shape in the desktop tree
  (`scripts/check-desktop-boundary.sh:23`).

Repository evidence wins over this plan. Re-verify every anchor above against the current baseline
before changing code.

## 4. Invariants that must not regress

1. **The daemon owns the document.** Canonical text lives in the daemon's `DocumentService`; the
   controller owns the local replica. The desktop adds a consumer, never a second authority.
2. **The desktop frontend holds no second text buffer.** No owned `DocumentSnapshot`,
   `DocumentBuffer` or `Rope`; no whole-document `Vec<String>`/`Vec<Line>`; no field named `text`,
   `content`, `lines`, `body`, `buffer`, `rope`, `source` or `document` retaining a copy of the file.
   The renderer holds only the bounded viewport slice the host sends.
3. **The frontend never reads a file directly.** Every byte arrives through the controller, which is
   daemon-authorized and path-contained.
4. **Undo/redo is frontend-owned and depth-capped** — a bounded stack of inverse `TextTransaction`s
   applied through `apply_local`, never a controller rewind, never a second buffer. ADR-0011 assigns
   undo/redo to the frontend.
5. **Ordinary agent/model file tools remain disk-authoritative.** The desktop editor is not a
   participant in agent change review.
6. **`egglsp` mirrors canonical unsaved text but does not own editor truth.** The editor's truth is
   the controller's replica.
7. **One writer per document.** A second writer is refused by the daemon's lease; the desktop
   presents the refusal rather than working around it.
8. **LSP reads are warm-server-only and `file.read`-authorized.** A read never starts a server, and
   `NotReady` is reported as *no warm server* — never an error, never an empty successful read.
9. **The renderer holds no daemon authority.** Every host command is a bounded domain operation;
   there is no generic protocol passthrough.
10. **The desktop boundary guard keeps its meaning** — no `core_request` passthrough, no shell, no
    remote-origin CSP relaxation, no new renderer permission.
11. **The TUI is unchanged.** No file under `src/tui/`, no change to `crates/codegg-client`'s
    controller semantics, no change to the daemon `DocumentService`.

## 5. Scope

### In scope

- Constructing a `DocumentController` in the desktop's Rust host over the `LocalSocketClient` it
  already holds, exposing the document lifecycle to the renderer as **bounded domain operations**
  through the existing narrow-bridge pattern.
- A **first-party** editor surface in the renderer: text viewport, line-number gutter, current-line
  indication, cursor and selection over UTF-8 byte offsets clamped to byte boundaries, scrolling,
  and frontend-owned undo/redo over `apply_local`.
- Save, conflict, reload, resync and close mapped to the controller's states, with **every**
  `DocumentState` and **every** `DocumentControllerError` given a distinct, tested presentation. A
  conflict or a gone-with-draft buffer is surfaced and **never** auto-resolved.
- LSP diagnostics markers, completion and navigation, consumed through `lsp_diagnostics` /
  `lsp_completion` / `lsp_read` under ADR-0012's warm-server-only read policy and its `file.read`
  authorization. A `NotReady` result is presented as "no warm server".
- A document-open affordance: a project-scoped file tree over the existing `WorkspaceDirectoryList`
  variant, with the same containment and authorization the TUI's M006-D uses.
- Renderer and host tests, plus a guard proving the editor holds **no second text buffer** and
  performs **no direct filesystem read** — the desktop analogue of
  `scripts/check_tui_editor_text_authority.py`.

### Explicitly out of scope (non-goals and stop conditions)

- **Monaco or any third-party/bundled editor engine.** Roadmap §3.1
  (`plans/subsystems/desktop-frontend-ide-foundation-roadmap.md:149-166`) keeps Monaco out of scope
  and requires a **first-party** editor, because adopting an engine would change the renderer's
  Content Security Policy, its worker model, and the bundle and signing boundary — which
  `architecture/desktop.md` §8 pins to local assets only. Adopting one is a separate decision that
  needs its own ADR.
- Any **new** `CoreRequest` variant, protocol change, storage change or migration. The transport,
  the document surface and the LSP read surface already exist. **If something appears to need new
  protocol, stop and report.**
- **Agent change review** (M006-E's desktop equivalent) and the **apply-into-dirty-buffer** half
  M006-E deliberately deferred. The dirty-buffer question is still open.
- **LSP write operations** — rename, code action, format-on-save. ADR-0012's read surface is
  read-only by construction; extending it is a separate protocol decision.
- Soft wrap, code actions, semantic-token streaming beyond the accepted surface, and editor
  performance work beyond what the viewport needs.
- The editor's keymap beyond the basics — Helix motion parity is the TUI's own workstream
  (`plans/subsystems/tui-editor-correctness-usability-roadmap.md`), not this milestone's.
- Any change to the TUI's editor, the TUI's file tree, or `codegg-client`'s controller semantics.
  If the seam looks wrong, **report it** — M005/M006 are strictly closed.
- Any renderer capability that widens the renderer trust boundary (no shell, filesystem, process,
  HTTP, clipboard or updater plugins).
- **CRDT / collaborative editing** — the canonical non-goal (`plans/000-long-term-specification.md:74`).

## 6. Required production changes

### Frontend (the renderer)

The renderer is a **view**: it holds the bounded viewport slice the host sends, the scroll offset,
the focus state and the key routing — no document authority, no whole-document text.

- A new editor module under `apps/desktop/src/` (e.g. `apps/desktop/src/editor/`) with the viewport
  component, the gutter, and the editor state hook.
- The renderer must **not** accumulate the file. The host sends only the visible line range; the
  renderer renders those lines and nothing else. Its editor state carries no whole-document string
  and no whole-document line array, and it must not concatenate the viewport into one document
  string (the desktop guard checks this).
- The file tree is a second panel bound to the current route token's workspace
  (`RouteTokenView` in `apps/desktop/src/bridge-types.ts`). It lists workspace-relative entries and
  opens one of them.
- The editor is a **non-modal primary region**, mirroring `Route::Editor` in the TUI
  (`src/tui/route.rs:21`): the chat composer stays usable while the editor is open unless the user
  explicitly focuses the editor buffer.
- New bridge DTOs mirrored in `apps/desktop/src/bridge-types.ts` and registered in
  `apps/desktop/scripts/check-bindings.mjs`.

### Runtime (the Tauri host)

- The host's client cell becomes shared: `HostState.client: Mutex<Option<LocalSocketClient>>`
  (`apps/desktop/src-tauri/src/lib.rs:53`) must become `Arc`-shareable so the controller and the
  existing commands use **one** client. `DocumentController::new` takes
  `Arc<dyn DocumentTransport>` (`crates/codegg-client/src/document.rs:237`) and `LocalSocketClient`
  already implements `DocumentTransport` (`:35`), so this is wiring, not a transport.
- A new host module (e.g. `apps/desktop/src-tauri/src/document.rs`) holding the desktop's document
  session: the `Arc<DocumentController>`, the bounded undo stack, the cursor/selection and viewport
  window, and the presentation mapping. It is the desktop analogue of `src/tui/document_session.rs`.
- New `#[tauri::command]` functions exposing **bounded** operations: open, viewport, edit, undo,
  redo, save, reload, resync, close, state, directory listing, LSP diagnostics, LSP completion and
  LSP read. Every one returns a bounded DTO. There is **no** generic protocol passthrough —
  `scripts/check-desktop-boundary.sh:23` already rejects that shape and the new commands must not
  trip it.
- The viewport projection slices the controller's own snapshot through
  `DocumentSnapshot::line_range` (`crates/codegg-document/src/buffer.rs:62`) for the visible line
  window only. It must never materialize the whole document.
- The LSP calls use the inherent `LocalSocketClient` methods (`crates/codegg-client/src/lsp.rs:64`,
  `:93`, `:107`). `NotReady` maps to an explicit *no warm server* status, distinct from an empty
  successful result and from an error.
- On daemon reconnect the host calls `DocumentController::reconnect`
  (`crates/codegg-client/src/document.rs:1101`) with the new transport, so the attachment reopens
  at the same scope.
- The desktop's own Cargo manifest gains one path dependency —
  `codegg-document = { path = "../../../crates/codegg-document" }` — because the host needs
  `TextTransaction`/`TextEdit` (`crates/codegg-document/src/edit.rs:29`, `:47`) and
  `DocumentSnapshot` (`crates/codegg-document/src/buffer.rs:18`) to build transactions and slice the
  viewport. `codegg-client` depends on `codegg-document` (`crates/codegg-client/Cargo.toml:13`) but
  does not re-export it (`crates/codegg-client/src/lib.rs:19-42`). **This is a manifest addition to
  the desktop's own workspace only**; the root workspace is untouched.

### Protocol

**NONE.** No new `CoreRequest` variant, no new DTO in `codegg-protocol`, no capability change.

The anchors that make this possible: the document lifecycle (`DocumentOpen`, `DocumentSnapshotGet`,
`DocumentStatusGet`, `DocumentWriterAcquire`, `DocumentChange`, `DocumentSave`, `DocumentReload`,
`DocumentClose` — `crates/codegg-protocol/src/core.rs:2317-2400`) is complete and additive; the
file tree (`WorkspaceDirectoryList` — `crates/codegg-protocol/src/document.rs:163`, `:211`) exists
and is authorized (`crates/codegg-core/src/authorization/policy.rs:121-125`); and the LSP reads
(`LspReadGet`, `LspCompletionGet`, `LspDiagnosticsGet`) exist under ADR-0012. If an implementation
step appears to require a new variant or DTO, that is a **stop condition** (§14), not a design
problem to solve here.

### Storage

**NONE.** No schema change, no migration, no `STORAGE_LAYOUT_VERSION` movement. The document's
durable state is the file on disk plus the daemon's `document.v1` service; the desktop contributes no
durable state of its own.

### Security

- The renderer gains **no** Tauri permission: `apps/desktop/src-tauri/capabilities/main.json` keeps
  `"permissions": []`.
- The renderer CSP is unchanged: `apps/desktop/src-tauri/tauri.conf.json` keeps `script-src 'self'`
  and no remote origin; `scripts/check-desktop-boundary.sh:95-96` asserts it.
- Every new host command is a bounded domain operation; the daemon remains the authorization point
  (`plans/000-long-term-specification.md:602`). The host adds **input validation only** — path
  containment for the open path, offset bounds for edits, and a workspace-relative prefix
  validation mirroring `WorkspaceDirectoryListRequestDto::validate_prefix`
  (`crates/codegg-protocol/src/document.rs:174-201`).
- The editor performs no filesystem read; the desktop boundary guard and the new desktop guard both
  hold.
- Document text must not reach logs: the new host commands must not log buffer contents, edit
  payloads, or LSP payloads containing source text.

### Documentation

See §12.

## 7. Ordered work packages

### Work package A — Host-side document session and the bounded bridge operations

**Intent.** Give the desktop host a `DocumentController` over the client it already has, and expose
the document lifecycle as bounded operations.

**Required changes.** Share the `LocalSocketClient` through `Arc`; construct the controller with
`DocumentController::new(client as Arc<dyn DocumentTransport>)`; add the host document module with
the session state; add the bounded `#[tauri::command]` set; add the matching bridge DTOs to
`apps/desktop/src-tauri/src/bridge.rs` and `apps/desktop/src/bridge-types.ts`; register the DTO
field lists in `apps/desktop/scripts/check-bindings.mjs`.

**Acceptance evidence.** The host compiles; a unit test drives open → edit → save → close against
a scripted transport; `npm run bindings:check` passes; the boundary guard still passes.

### Work package B — The editor viewport, gutter, cursor and selection

**Intent.** Render the file with a gutter, a current-line indication and a byte-offset cursor.

**Required changes.** The host returns a bounded viewport payload (first visible line number, the
visible line strings, the byte length, the line count, the revision, the cursor and the selection).
The renderer draws the gutter and the lines. Cursor and selection are **byte offsets**, clamped to
UTF-8 byte boundaries by the host using `DocumentSnapshot::is_byte_boundary`
(`crates/codegg-document/src/buffer.rs:45`). Scrolling is horizontal and vertical over the visible
window.

**Acceptance evidence.** A renderer test renders a multi-line buffer and asserts the gutter numbers,
the highlighted current line, and that the payload carries only the visible window; a host test
asserts that a cursor in the middle of a multi-byte character is clamped rather than splitting it.

### Work package C — Editing and frontend-owned undo/redo

**Intent.** Type, edit, undo and redo.

**Required changes.** The renderer sends an intent; the host computes the `TextTransaction`
(`crates/codegg-document/src/edit.rs:47`) and applies it through
`DocumentController::apply_local` (`crates/codegg-client/src/document.rs:618`). The host keeps a
bounded undo stack of inverse transactions, mirroring the TUI's caps (`MAX_EDITOR_UNDO_DEPTH = 256`,
`MAX_EDITOR_UNDO_BYTES = 4 MiB`, `src/tui/editor.rs:36`, `:43`). Undo applies the inverse
transaction through `apply_local` — it never rewinds the controller. The stack is cleared on every
reconciliation transition (save, reload, resync, reconnect, close), mirroring
`src/tui/editor.rs:5486`.

**Acceptance evidence.** A host test asserts that undo restores the prior bytes through
`apply_local`, that the stack is depth- and byte-capped, that a save clears it, and that an undo
after a resync is refused rather than applied against a moved base.

### Work package D — Save, conflict, reload, resync and close presentation

**Intent.** Map every controller state and error to a distinct, tested presentation.

**Required changes.** A presentation table over all ten `DocumentState` variants
(`crates/codegg-client/src/document.rs:48-60`) and all eleven `DocumentControllerError` variants
(`:63-86`), each with its own message and affordance. A conflict (`DocumentState::Conflict`,
`DocumentControllerError::Conflict`) and a gone-with-draft (`DocumentState::GoneWithLocalDraft`) are
surfaced and **never** auto-resolved; the draft is retained.

**Acceptance evidence.** A table-driven test asserts a **distinct** presentation for every state and
every error — no two states share a message — and a test asserts that a conflict leaves the local
draft intact and offers only explicit user choices.

### Work package E — The document-open affordance (project-scoped file tree)

**Intent.** Let the user pick a file to open.

**Required changes.** The host calls `CoreRequest::WorkspaceDirectoryList`
(`crates/codegg-protocol/src/document.rs:163`, `:211`) for the current route token's workspace, with
a workspace-relative prefix validated by `validate_prefix`
(`crates/codegg-protocol/src/document.rs:174-201`). The daemon's handler resolves the canonical root
itself and enforces containment (`src/core/daemon_documents.rs:1081`). The renderer renders the tree
and opens a selected entry through the same bounded open command the editor uses, so the open path
has exactly one implementation.

**Acceptance evidence.** A host test asserts the request carries a workspace-relative prefix and
never an absolute path; a test asserts that a `Busy` or `Unavailable` listing is presented as such
and never as an empty tree; a test asserts that an entry outside the workspace is refused.

### Work package F — LSP diagnostics, completion and navigation

**Intent.** Show the language server's view of the edited file.

**Required changes.** The host calls `lsp_diagnostics` (`crates/codegg-client/src/lsp.rs:107`),
`lsp_completion` (`:93`) and `lsp_read` (`:64`). Diagnostics are pulled on open, after a successful
save, and on an explicit refresh, exactly as M006-C does in the TUI; no push stream is promised.
`NotReady` is presented as "no warm server" — distinct from an error and distinct from an empty
successful result (`LspReadStatus` at `crates/codegg-protocol/src/lsp.rs:433-443`). Navigation
results are workspace-relative and are validated before the editor opens them; an out-of-workspace
result is surfaced, not opened.

**Acceptance evidence.** A host test asserts `NotReady` maps to the no-warm-server presentation and
that a `Ready` empty diagnostics set maps to a different presentation; a test asserts that a
navigation result outside the workspace is refused.

### Work package G — The desktop editor text-authority guard

**Intent.** Make "no second text buffer, no direct filesystem read" mechanically checkable.

**Required changes.** A new `scripts/check_desktop_editor_text_authority.py` with two halves.

- **Host (Rust)** — the same rules as `scripts/check_tui_editor_text_authority.py`, applied to the
  desktop's editor files: no field named `text`/`content`/`lines`/`body`/`buffer`/`rope`/`source`/
  `document`; no owned `DocumentSnapshot`/`DocumentBuffer`/`Rope` field (a borrowed `&DocumentSnapshot`
  is advisory only, as in the TUI guard at
  `scripts/check_tui_editor_text_authority.py:221-232`); no `Vec<String>`/`Vec<Line>` field; no
  filesystem read API (`:100-109`); no `DocumentBuffer::`/`Rope::` construction (`:112-115`). A
  bounded `TextTransaction` undo entry is **explicitly permitted**, with the TUI guard's own
  reasoning (`:52-55`).
- **Renderer (TypeScript)** — the editor module may not hold a whole-document container: no field
  or state named `text`/`content`/`lines`/`body`/`buffer`/`rope`/`source`/`document` typed as a
  whole-document string or line array; no concatenation of the viewport into one document string; no
  filesystem or network read of a local path in the renderer.

The guard must assert the declarations it reasons about still exist (as the TUI guard does at
`scripts/check_tui_editor_text_authority.py:297-314`), so it cannot pass by the file being emptied.
It must carry a `--self-test` that injects (a) a `text: String` field in the host and (b) a
whole-document line array in the renderer, and proves the guard rejects both: a guard that cries
wolf gets deleted rather than fixed (`scripts/check_tui_editor_text_authority.py:24-26`), and a
guard that cannot fail proves nothing.

**Acceptance evidence.** The guard passes on the delivered tree; `--self-test` fails on both
injected violations; the guard is wired into `scripts/verify.sh quick` **and** CI. It is a
source-only Python guard, so it runs in CI even while the desktop workspace itself is excluded from
the root build.

### Work package H — Tests

**Intent.** Close the milestone with evidence.

**Required changes.** The test surface listed in §10, at the narrowest target that covers each
change: host unit tests (Rust, in the desktop's own workspace), renderer component tests (Vitest),
and the guard's `--self-test`.

**Acceptance evidence.** The full command list in §11 runs green on the hosted gate M007-A restores.

## 8. Failure, cancellation, restart, contention semantics

| Situation | Required behavior |
|---|---|
| **Stale document revision** | The daemon rejects the change; the host surfaces the daemon's error and does not silently rebase. A `ResyncRequired` state prompts an explicit resync. |
| **A concurrent writer** | `DocumentWriterAcquire` is refused by the daemon; the desktop presents `DocumentState::ReadOnly` / `DocumentControllerError::ReadOnly` and offers no write path. |
| **A save conflict** | `DocumentState::Conflict` / `DocumentControllerError::Conflict`. The local draft is **retained**, the conflict is surfaced, and **nothing is auto-resolved**. `save` refuses while conflicted (`crates/codegg-client/src/document.rs:846-860`). |
| **A daemon restart mid-edit** | The controller's transport fails; the host surfaces the transport error, keeps the local draft, and offers reconnect. On reconnect the host calls `DocumentController::reconnect` (`crates/codegg-client/src/document.rs:1101`), which reopens the attachment at the same scope and reports whether the daemon's revision moved. |
| **An LSP server not warm** | `NotReady` → "no warm server". **Not** an error, **not** an empty successful read. A read never starts a server. |
| **An LSP server restarting** | The read answers `NotReady` (or `Unsupported` for an operation the server does not advertise); the editor keeps its text and shows the status, and never falls back to a local parse. |
| **A resync** | `DocumentController::resync` (`crates/codegg-client/src/document.rs:1030`) is the only reconciliation path that may discard the local edit set; the undo stack is cleared with it, and the user is told the local edits were discarded. |
| **A reconnect** | The transport is replaced and the attachment reopens at the same scope; the undo stack is cleared; a document whose file vanished opens as `GoneWithLocalDraft` with the draft retained. |
| **A renderer reload with unsaved local edits** | The renderer holds no durable state, so a reload loses the *view*, not the document: the host still owns the replica and the draft and re-sends the viewport on the next open. The host must not treat a renderer reload as a document close. |

In every case a conflict, a gone-with-draft and a `NotReady` LSP result are **states the user is
shown**, never conditions the editor resolves on its own.

## 9. Compatibility and migration

- **No migration.** No schema change, no `STORAGE_LAYOUT_VERSION` movement, no data rewrite.
- **No protocol change.** The desktop uses the existing `document.v1` and LSP read variants. The
  desktop's new commands are additive host operations, not new wire variants.
- **The render path is unchanged.** `apps/desktop/src/App.tsx` keeps its existing behavior for
  everything that is not the editor; the editor and the file tree are additive regions. The existing
  projection render path, the 20 existing `#[tauri::command]` functions and the existing bridge DTOs
  keep their shapes.
- **`codegg-client`'s controller semantics are untouched.** No file under `crates/codegg-client` is
  edited. `DocumentState`, `DocumentControllerError`, `apply_local`'s queue and reconciliation
  behavior, and `DocumentTransport` are inputs, not outputs.
- **The TUI is untouched.** No file under `src/tui/` is edited.
- **One manifest addition.** `apps/desktop/src-tauri/Cargo.toml` gains a `codegg-document` path
  dependency. The desktop has its own lockfile (`apps/desktop/src-tauri/Cargo.lock`), so the root
  workspace's dependency graph and the root `Cargo.lock` are unchanged.

## 10. Required tests

### Focused unit tests (host)

- Every `DocumentState` variant maps to a **distinct** presentation (a table-driven test over all
  ten); every `DocumentControllerError` variant maps to a **distinct** presentation (all eleven).
- A conflict leaves the local draft intact and is never auto-resolved; a gone-with-draft retains the
  draft and surfaces it.
- Undo/redo is frontend-local: it restores the prior bytes **through `apply_local`**, it is depth-
  and byte-capped, and it is **cleared on every reconciliation transition** (save, reload, resync,
  reconnect, close).
- A cursor offset in the middle of a multi-byte character is clamped to a byte boundary rather than
  splitting it.
- The viewport payload contains only the visible window, and its size is bounded.
- A `NotReady` LSP read maps to the no-warm-server presentation, and a `Ready` empty diagnostics set
  maps to a **different** presentation.
- A directory-listing request carries a workspace-relative prefix and never an absolute path; a
  `Busy`/`Unavailable` listing is presented as such, not as an empty tree.

### Integration tests

- Open → edit → undo → redo → save → close against a scripted transport (the desktop's own
  equivalent of the TUI's `FakeDaemon`/`DeadTransport`/`ScriptedTransport` fixtures at
  `src/tui/commands/editor.rs:4860`, `:4997`, `:5007`).
- The bridge round-trip: a host command's DTO matches the renderer-side TypeScript contract
  (covered by `npm run bindings:check`).

### Restart and recovery tests

- A daemon restart mid-edit surfaces the transport error, retains the draft, and a reconnect reopens
  the attachment at the same scope.
- A resync discards the local edit set **only** when explicitly requested, and clears the undo stack
  with it. A reconnect clears the undo stack.

### Contention and cancellation tests

- A second writer is refused and the document becomes read-only.
- A full pending queue (`QueueFull`) and an over-budget pending byte count (`QueueBytes`) are
  surfaced distinctly. `LifecycleBusy` is surfaced distinctly and does not corrupt the replica.

### Security and negative tests

- A path outside the workspace is refused at the open boundary.
- A directory-listing prefix with `..`, an absolute path, or a backslash is refused.
- No renderer permission is added; the capability file stays empty.
- **No buffer text reaches the logs**: a test captures host logging across open/edit/save and asserts
  no document text appears.

### Migration and compatibility tests

**None required** — there is no migration and no protocol change (§9). The compatibility evidence is
the unchanged `codegg-client` and TUI suites plus the unchanged root workspace build.

### Guard tests

- The desktop text-authority guard **fails** on an injected second buffer and on an injected direct
  file read, and passes on the delivered tree.

## 11. Required verification commands

```bash
# Desktop renderer and bridge contract
cd apps/desktop && npm run typecheck
cd apps/desktop && npm test
cd apps/desktop && npm run bindings:check

# Desktop Rust host (its own Cargo workspace, lockfile and toolchain)
cargo build --manifest-path apps/desktop/src-tauri/Cargo.toml
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --locked

# Shared crates this milestone consumes — no source change, so these must stay green
cargo test -p codegg-document --locked
cargo test -p codegg-client --locked

# Root workspace hygiene
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
scripts/verify.sh quick
```

**`scripts/verify.sh quick` does NOT compile the desktop.** `apps/desktop/src-tauri` is excluded
from the root workspace at `Cargo.toml:6`, so `cargo check` inside `verify.sh` never touches it.
That is exactly the gap M007-A closes, and it is why the desktop commands above are listed
separately and why the hosted gate — not a local run — is the closure evidence (§2, §14).

Never use `--all-features` for a workspace sweep: it drags in the real-server tests. The broad sweep,
when run, uses the nextest `ci` profile with a private cache root:

```bash
XDG_CACHE_HOME="$(mktemp -d)" cargo nextest run --workspace --locked --profile ci
```

## 12. Documentation updates

- `architecture/document.md` — record the **second frontend consumer**: the desktop host now
  constructs a `DocumentController` over the shared client, and the M005 limitations still hold
  unchanged. Name the desktop host module and the guard that protects it.
- `architecture/lsp.md` — record that the desktop is a second consumer of the ADR-0012 read surface
  and that it presents `NotReady` as "no warm server". Do not restate or re-decide the read surface's
  authority.
- `architecture/desktop.md` — **the "Deferred surfaces" list must change**
  (`architecture/desktop.md:291-292`): the editor is no longer deferred. Reword that paragraph to
  say the editor surface is implemented and move the editor out of the deferred list, leaving the
  terminal, provider credentials, plugin UI, remote daemon access, signing and distribution
  automation deferred. Update §8's renderer security boundary note to record that the editor adds no
  renderer permission and no CSP relaxation.
- `architecture/overview.md` — record the desktop editor as a consumer of the shared document
  foundation, and update the verified counts if the desktop's command or DTO counts move.
- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md` — **M007-E only**: mark the
  milestone closed with its implementation SHA, and update §7's decomposition row and §13's status
  table. Change nothing else in the roadmap.
- `plans/registry.md` — register the milestone and its closure record.

**Do NOT edit** `plans/000-long-term-specification.md`, `plans/001-terminology-and-domain-model.md`,
`plans/002-long-term-roadmap.md`, ADR-0011, or ADR-0012. They are accepted authority and are not this
milestone's to change.

## 13. Acceptance criteria

1. A user can open a project file in the desktop from the file tree, and see it rendered with a
   line-number gutter and a current-line indication.
2. A user can edit the open file, undo the edit, redo it, and save the file, and the desktop reports
   the resulting document state.
3. A save conflict is surfaced with its own distinct presentation and is **never** auto-resolved; the
   local draft is retained.
4. A document that vanished while it had local edits is surfaced as gone-with-draft and the draft is
   retained.
5. The desktop holds **no second text buffer**, guard-enforced: the new desktop text-authority guard
   passes on the delivered tree.
6. The editor performs **no direct filesystem read**, guard-enforced by the same guard, and the
   desktop boundary guard still passes.
7. The guard is demonstrated to **fail** on an injected second buffer and on an injected direct file
   read before being reverted, and it carries a `--self-test` that proves this.
8. LSP diagnostics appear on the edited file, pulled through the existing read surface.
9. A not-warm LSP server is reported as "no warm server" — not as an error, and not as an empty
   successful read.
10. Every `DocumentState` and every `DocumentControllerError` has a distinct, tested presentation.
11. Undo/redo is frontend-local, depth- and byte-capped, applied through `apply_local`, and cleared on
    every reconciliation transition.
12. **No protocol, storage or migration change landed**: the `document.v1` variants, the LSP read
    variants and the daemon's storage layout are byte-identical to the baseline.
13. The TUI's editor is unchanged: no file under `src/tui/` is modified, and the TUI editor test
    suites are green.
14. `codegg-client`'s controller semantics are unchanged: no file under `crates/codegg-client` is
    modified, and `cargo test -p codegg-client --locked` is green.
15. The closure evidence comes from the hosted gate M007-A restores, not from a local run.

## 14. Stop conditions

Stop and report to the owner rather than proceeding when any of these appears:

1. **The editor appears to need a new `document.v1` request or DTO.** The surface is complete and
   additive (`crates/codegg-protocol/src/core.rs:2317-2400`); a new variant is a protocol decision,
   not this milestone's.
2. **The editor appears to need filesystem access outside the controller.** That would break
   invariant 3 and defeat the guard.
3. **Undo/redo cannot be made frontend-local without weakening the controller's queue or
   reconciliation behavior.** `apply_local`'s queue and lifecycle rules
   (`crates/codegg-client/src/document.rs:618-674`) are closed; the undo stack must not need to
   change them.
4. **The editor appears to need LSP write operations** (rename, code action, format-on-save). ADR-0012
   makes the surface read-only by construction; extending it is a separate protocol decision.
5. **A third-party or bundled editor engine appears necessary.** Roadmap §3.1 requires a first-party
   editor and routes engine adoption to its own ADR
   (`plans/subsystems/desktop-frontend-ide-foundation-roadmap.md:149-166`).
6. **The work appears to pull in agent change review or apply-into-dirty-buffer.** Both are out of
   scope; the dirty-buffer question is still open (M006-E's closure defers it).
7. **The layout cannot host the editor without changing M007-B's region model.** That is M007-B's
   decision, not this milestone's.
8. **`codegg-client`'s controller semantics would have to change.** M005/M006 are strictly closed;
   report the seam instead.
9. **The daemon's `WorkspaceDirectoryList` authorization or containment turns out to differ from
   M006-D's TUI use.** The tree must use the same authority; a divergence is a finding, not a
   license to work around it.

## 15. Closure evidence required

1. **A requirement-to-evidence matrix**, mapping every acceptance criterion in §13 to the specific
   test, guard run or hosted-lane record that demonstrates it.
2. **The full command list from §11 with actual outcomes** — each command, its exit status, and
   where it ran (hosted lane vs local). A local-only result is recorded as a local result.
3. **Proof that no protocol, storage or migration change landed**: a diff against
   `c4fa30aeba439894b048768d4829dea0cfa0b84b` showing no change under `crates/codegg-protocol/`, no
   change under `crates/codegg-core/src/session/schema.rs`, and no change to
   `STORAGE_LAYOUT_VERSION`.
4. **Proof the no-second-buffer guard was demonstrated to fail on a violating construct before being
   reverted**: the injected-violation output, plus the `--self-test` run.
5. **Restart/recovery results** for resync, reconnect and daemon restart, with the observed behavior
   for each row of §8.
6. **Security evidence**: containment rejection (a path outside the workspace refused), read-only
   enforcement (a second writer refused), the empty renderer capability file, the unchanged CSP, the
   passing desktop boundary guard, and confirmation that **no buffer text reached the logs**.
7. **An explicit statement of what was deferred**: soft wrap, code actions, LSP write operations,
   agent change review, apply-into-dirty-buffer, third-party editor engines, and the editor keymap
   beyond the basics.
8. **The diff against the baseline**, summarized by directory, showing that `src/tui/` and
   `crates/codegg-client/` are untouched.

## 16. Handoff notes

**Authority order.** ADR-0011 and ADR-0012 outrank this plan, and this plan re-decides neither. The
M005 and M006 closure records are immutable. If this plan and an accepted ADR disagree, the ADR wins
— stop and report.

**What not to edit.** `src/tui/document_session.rs`'s ownership shape (`TuiDocumentSession` /
`TuiDocumentPresentation`) — the desktop has its own session and the TUI's is untouched; the daemon
`DocumentService` (`src/core/daemon_documents.rs`); `crates/codegg-document` — a dependency, not a
work item; `crates/codegg-client`'s controller semantics (`DocumentController`, `DocumentState`,
`DocumentControllerError`, `DocumentTransport`, `apply_local`'s queue and reconciliation behavior);
any closure record (`plans/closure/**`); and any ADR.

**Environment notes.**

- The root workspace is Rust **1.89** (`Cargo.toml:12`, `rust-version = "1.89"`; `AGENTS.md:5`).
- The desktop is a **separate Cargo workspace** (`apps/desktop/src-tauri/Cargo.toml:8-9`:
  `[workspace]` / `members = ["."]`) pinned to **Rust 1.90**:
  `apps/desktop/src-tauri/Cargo.toml:6` (`rust-version = "1.90"`) and
  `apps/desktop/src-tauri/rust-toolchain.toml` (`channel = "1.90.0"`). Use the desktop's own
  toolchain for the desktop's commands; do not lower the root MSRV or the desktop's.
- The desktop's dependency tree is confined to `apps/desktop` (`apps/desktop/package-lock.json`,
  `apps/desktop/src-tauri/Cargo.lock`).
- Broad sweeps use the nextest `ci` profile with a private cache root
  (`XDG_CACHE_HOME="$(mktemp -d)" cargo nextest run --workspace --locked --profile ci`).
- Storage tests use `isolated_pool()` (migrations run inside; never add an extra `migrate()`).
- Never use `--all-features` for a workspace sweep.
- Preserve unrelated user changes: this milestone edits only the desktop tree, its new guard, the
  guard wiring in `scripts/verify.sh` and CI, and the documentation listed in §12.

**A commit message saying "closed" is not closure evidence.** Closure is the evidence list in §15,
recorded in the milestone's closure record.
