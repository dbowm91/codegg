# Desktop Frontend and IDE Foundation Milestone 015 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/desktop-frontend-ide-foundation/015-desktop-editor-surface.md`

Source subsystem roadmap:

- `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md`

Applicable ADRs:

- **ADR-0011** — the shared document foundation: one `DocumentController`, one
  writer lease, one base-revision contract, no second text buffer.
- **ADR-0012** — the LSP read surface, and its `file.read` authorization.

Repository baseline reviewed: `d85b167e8` — `origin/main` with M007-A,
M007-B, M007-C and the owner-directed terminology amendment landed.

Implementation commit on this milestone's branch:

- `042ad8cd9` — the desktop host module, the editor surface, the keymap fold,
  the guard and the test suites.

## 1. Executive finding

The desktop can now open a project document, edit it, undo and redo the
edit, save it, and read LSP diagnostics, completion and navigation — over the
**shared** `DocumentController`, with the daemon owning the buffer and the
host holding only a bounded viewport slice.

The load-bearing property is that **there is no second text buffer anywhere**.
The renderer holds no text; the host holds a window; the daemon holds the
document. The editor's chords are declared in M007-C's keymap rather than in
a second table of their own, so the desktop has one dispatch and one conflict
rule, and `scripts/check_desktop_editor_authority.py` fails the build if that
ever stops being true.

## 2. Requirement-to-evidence matrix

| Plan criterion | Evidence | Verdict |
|---|---|---|
| §13.1 — open a project file from the file tree, rendered with a line-number gutter and a current-line indication | §4.2 — `desktop_document_open`, `desktop_document_tree`, `desktop_document_viewport`; the tree is a **navigator** that enumerates entries and never reads file bytes, and selecting a file routes through the same `DocumentOpen` path as a typed path | met |
| §13.2 — edit, undo, redo, save, and report the resulting document state | §4.2 — `desktop_document_edit`, `_undo`, `_redo`, `_save`, `_state`; the undo stack holds only *inverse* transactions and is cleared on every reconciliation transition | met |
| §13.3 — a save conflict has its own distinct presentation, is **never** auto-resolved, and the local draft is retained | §4.3 — `Conflict` is one of the `DocumentState` variants and is rendered distinctly; the draft is retained | met |
| §13.4 — a document that vanished under local edits is surfaced as gone-with-draft and the draft is retained | §4.3 — `GoneWithLocalDraft`, with the draft retained | met |
| §13.5 — the desktop holds **no second text buffer**, guard-enforced | §4.4 — the guard passes on the delivered tree | met |
| §13.6 — the editor performs **no direct filesystem read**, guard-enforced, and the desktop boundary guard still passes | §4.4 — the guard rejects a host-side and a renderer-side filesystem read; `./scripts/check-desktop-boundary.sh` passes | met |
| §13.7 — the guard fails on an injected second buffer and an injected direct file read, and carries a `--self-test` | §4.4 — **11** injected defect classes, each rejected for its expected reason | met |
| §13.8 — LSP diagnostics appear on the edited file, through the existing read surface | §4.5 — `desktop_document_diagnostics` over the ADR-0012 read path | met |
| §13.9 — a not-warm LSP server is reported as "no warm server", not as an error and not as an empty successful read | §4.5 — `NotReady` is presented as a status; `architecture/lsp.md` records the rule | met |
| §13.10 — every `DocumentState` and every `DocumentControllerError` has a distinct, tested presentation | §4.3 | met |
| §13.11 — undo/redo is frontend-local, depth- and byte-capped, applied through `apply_local`, and cleared on every reconciliation transition | §4.2 | met |
| §13.12 — **no protocol, storage or migration change** | §4.1 — `git diff --name-only origin/main...HEAD -- crates/codegg-protocol/ crates/codegg-core/src/session/` is **empty** | met |
| §13.13 — the TUI's editor is unchanged | §4.1 — `git diff --name-only origin/main...HEAD -- src/tui/` is **empty** | met |
| §13.14 — `codegg-client`'s controller semantics are unchanged | §4.1 — `git diff --name-only origin/main...HEAD -- crates/codegg-client/` is **empty** | met |
| §13.15 — the closure evidence comes from the hosted gate M007-A restores | §5 — the hosted Desktop E2E lane is the gate; the local entry point's `e2e` phase was run on this head as well (§4.6) | met |

## 3. What was delivered

### 3.1 The blast radius

The change is confined to the desktop tree, two guards and the gate wiring:

| Area | Changed |
|---|---|
| `apps/desktop/src-tauri/src/document.rs` | **new** — the host module |
| `apps/desktop/src-tauri/src/lib.rs` | **purely additive** — `mod document;`, one `use` group, one `HostState` field, one `Default` initializer, and 15 entries appended to `generate_handler!` |
| `apps/desktop/src-tauri/tests/document_editor.rs` | **new** — 14 host tests |
| `apps/desktop/src/editor/` | **new** — `EditorSurface.tsx`, `actions.ts`, `contract.ts`, `session.ts`, `fixtures.ts` and their tests |
| `apps/desktop/src/panels/EditorPanel.tsx` | **new** |
| `apps/desktop/src/keymap.ts` | the `surface` affordance class, `EDITOR_SCOPE`, and **22** editor action declarations |
| `apps/desktop/src/keyboard.test.tsx` | the `surface` class's affordance rules |
| `scripts/check_desktop_editor_authority.py` | **new** — the guard |
| `scripts/check_desktop_panel_authority.py` | the bridge baseline, re-measured for the editor's targets |
| `scripts/verify.sh`, `.github/workflows/ci.yml`, `AGENTS.md` | the guard is wired into the gate, and the counts move 95 → 96 / 72 → 73 / 58 → 59 |

**Nothing else moved.** `src/tui/`, `crates/codegg-client/`,
`crates/codegg-protocol/` and the daemon's storage schema are untouched.

### 3.2 The host's contract

Fifteen `#[tauri::command]`s, all registered contiguously at the end of
`generate_handler!`:

`desktop_document_open`, `_viewport`, `_cursor`, `_edit`, `_undo`, `_redo`,
`_save`, `_reload`, `_resync`, `_close`, `_state`, `_tree`, `_diagnostics`,
`_completion`, `_navigate`.

**Every mutating path goes through the daemon.** The host builds a
`TextTransaction` and calls the shared `DocumentController` — `DocumentOpen`,
`DocumentChange`, `DocumentSave`, `DocumentStatusGet` — so the daemon grants
and enforces the writer lease, the base-revision check and the workspace
containment. The host module contains **no** `std::fs` and **no**
`tokio::fs` call, and the guard rejects one.

The undo stack holds only *inverse* transactions, is depth- and byte-capped,
and is cleared on every reconciliation transition — so an undo after a resync
is **refused** rather than applied against a moved base.

### 3.3 The renderer's half, and its presentations

`DocumentState` has its ten variants and `DocumentControllerError` its
eleven, and every one of them has a **distinct** presentation. The two that
carry a user-visible consequence are separate on purpose:

- **`Conflict`** — a save conflict is surfaced as a conflict and is **never**
  auto-resolved. The local draft is retained.
- **`GoneWithLocalDraft`** — a document that vanished while it had local edits
  is surfaced as gone-with-draft, and the draft is retained.

Neither is folded into a generic "error" state, because the user's next move
differs: one is "resolve the conflict", the other is "recover the draft".

### 3.4 The guard

`scripts/check_desktop_editor_authority.py` — **11** injected defect classes,
each rejected for its expected reason:

| Class | What it rejects |
|---|---|
| `host-buffer-construction` | the host constructing a whole-document buffer |
| `host-whole-document-array` | the host retaining a whole-document array |
| `host-filesystem-read` | a host-side `std::fs` / `tokio::fs` read |
| `host-text-field` | the host holding a native text field |
| `renderer-text-field` | the renderer holding a native text field |
| `renderer-whole-document-array` | the renderer retaining a whole-document array |
| `renderer-viewport-concatenation` | the viewport concatenated into a retained array |
| `renderer-filesystem-read` | a renderer-side filesystem read |
| `contract-drift` | a Rust `*View` struct and its TypeScript interface diverging field for field |
| `guard-not-wired` | the guard not being wired into the gate |
| `guard-would-be-vacuous` | the guard being present but able to pass vacuously |

The last two matter more than they look: a guard that is not wired protects
nothing, and a guard whose rule matches nothing passes on a tree that has
already broken. Both are injected and both must be rejected.

### 3.5 The LSP half

`desktop_document_diagnostics`, `_completion` and `_navigate` read through
the ADR-0012 read surface, under the same `file.read` authorization, with the
daemon resolving the server and the warm-client key still deciding which
server answers.

**`NotReady` is presented as "no warm server"** — a status, not an error, and
not an empty successful read. An empty diagnostics list would read as "the
file is clean", and the file may simply not have been analysed. `Ambiguous`
and `Unsupported` are presented distinctly too.

### 3.6 The keymap fold

`apps/desktop/src/editor/keys.ts` is **deleted**. The editor's chords live in
M007-C's `apps/desktop/src/keymap.ts` as **22** action declarations under a
new `EDITOR_SCOPE` (`regionScope('editor')`), so an editor chord never
shadows a global one, and every editor key event is resolved with the
keymap's own machinery — `normalizeChord` then
`resolveAction(effectiveMap, [EDITOR_SCOPE], chord)` — honouring each
declaration's `captured` flag before `preventDefault()`. The effective map
comes from the composition root, so there is one keymap load and one map.

`apps/desktop/src/editor/actions.ts` is the editor's half of the split: it
maps an **action id** to what the editor does, and is keyed by action id,
never by chord.

`mod+s`, which M007-C had reserved for exactly this milestone, is now bound.

### 3.7 One shared-model change, recorded

The keymap's `AffordanceClass` gained `surface` — "a custom editing surface:
an element that owns a caret and is focused by a click, but holds **no text of
its own**". The editor's viewport cannot be an `editing` affordance, because a
native text field would keep a second copy of the text. This is the one place
where the milestone changed a shared model, and it is recorded here rather
than left to be discovered.

## 4. Verification executed

### 4.1 The diff itself

```text
git diff --name-only origin/main...HEAD -- src/tui/                          → (empty)
git diff --name-only origin/main...HEAD -- crates/codegg-client/            → (empty)
git diff --name-only origin/main...HEAD -- crates/codegg-protocol/          → (empty)
git diff --name-only origin/main...HEAD -- crates/codegg-core/src/session/  → (empty)
```

### 4.2 The editor's behaviour

| Command | Outcome |
|---|---|
| `npm test` (renderer) | **pass** — **28 files, 208 passed** (was 25 files, 176) |
| `npm run typecheck` | **pass** |
| `npm run bindings:check` | **pass** |
| `rustup run 1.90.0 cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --locked` | **pass** — 90 lib + **14** `document_editor` + 5 `frontend_parity` |
| `rustup run 1.90.0 cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets --locked -- -D warnings` | **pass** — no warnings |

### 4.3 The guards

| Command | Outcome |
|---|---|
| `python3 scripts/check_desktop_editor_authority.py` | **pass** — "the host serves a bounded viewport window and the renderer holds no second copy of the text" |
| `python3 scripts/check_desktop_editor_authority.py --self-test` | **pass** — **11** injected defect classes rejected |
| `python3 scripts/check_desktop_panel_authority.py` | **pass** |
| `python3 scripts/check_desktop_panel_authority.py --self-test` | **pass** |
| `./scripts/check-desktop-boundary.sh` | **pass** — "desktop dependency and authority boundaries verified" |
| `scripts/verify.sh quick` | **pass** — "Quick verification passed." |

### 4.4 The compatibility contract

| Measurement | Result |
|---|---|
| `data-testid` inventory, measured over the shipped renderer sources | **262 → 311** (249 literal ids + 13 template families → 298 + 13), **none removed or renamed** |
| `src/tui/` | untouched |
| `crates/codegg-client/` | untouched |

The `data-testid` count was measured by reading the blobs of every shipped
renderer source at both revisions and extracting the literal attributes and
the template families separately. The method reproduces M007-C's recorded
262 exactly at its own head, which is why it is the method used here.

### 4.5 The built-app trajectory

`apps/desktop/scripts/verify-desktop.sh` in full — the phases are `boundary`,
`renderer`, `stage-daemon`, `host` and `e2e` — exited **0**, and the
built-app trajectory gave **5 + 2 + 7 = 14 specs, 0 failures**, against a
daemon binary built from this worktree.

### 4.6 The hosted gate

M007-A's hosted lane is the desktop's gate, and it is the lane that runs the
built-app trajectory on the pull request. Its run is recorded on this
milestone's own pull-request head. The local entry point's `e2e` phase was
run as well, so the local and hosted evidence agree on the trajectory's shape.

## 5. Invariant review

- **No second text buffer.** The renderer holds no text; the host holds a
  bounded viewport; the daemon holds the document. The guard enforces it, and
  the guard itself is guarded against being vacuous.
- **No direct filesystem read by the editor.** Every read goes through the
  daemon's document path, under the daemon's containment and authorization.
- **The writer lease, the base revision and the containment are the daemon's.**
  The host never grants itself authority over a file.
- **An undo cannot be applied against a moved base.** The undo stack is
  cleared on every reconciliation transition.
- **The TUI is untouched**, and `codegg-client`'s controller semantics are
  untouched. The desktop is a *second consumer* of the shared foundation, not
  a second implementation of it.
- **The LSP read surface's authority is unchanged.** The desktop reads through
  it; it does not extend it.
- **The renderer gains no permission and no CSP relaxation.** The production
  `main` capability keeps its empty permission list.

## 6. Failure and recovery review

- **A save conflict is never auto-resolved.** The user's draft stays theirs.
- **A vanished document keeps its draft.** Gone-with-draft is a distinct
  state, not a failure.
- **A read-only document refuses the edit** rather than silently dropping it.
- **A not-warm LSP server is a status.** The desktop does not present an
  unanalysed file as a clean one.
- **An undo after a resync is refused**, not applied.

## 7. Migration and compatibility review

None required. No protocol variant, storage layout, migration or DTO field
changed; no existing `data-testid` was removed or renamed; the `document.v1`
variants, the LSP read variants and the daemon's storage layout are
byte-identical to the baseline.

## 8. Security review

- **The renderer holds no text and no authority.** Its write path is a
  transaction request to the daemon, and its read path is a bounded window.
- **The editor adds no Tauri capability and no plugin**, and the production
  CSP is unchanged.
- **The host reads no file directly.** The guard fails on `std::fs` and
  `tokio::fs` in the host module.
- **Containment is the daemon's.** A workspace-relative path from the tree
  gets exactly the same containment, `file.read` authorization and symlink
  policy as a typed one, because it is literally the same code path.
- **LSP reads are authorized** under ADR-0012's `file.read`.

## 9. Documentation and operations

| Document | What changed |
|---|---|
| `architecture/document.md` | a new **"The desktop host as the second frontend consumer (M007-E)"** section — the host module, the shared foundation, the M005 limitations unchanged, the guard and its eleven defect classes, and the `surface` affordance class |
| `architecture/lsp.md` | a new **"The desktop as a second consumer of the read surface (M007-E)"** section — the read surface's authority is unchanged, and `NotReady` is presented as "no warm server" |
| `architecture/desktop.md` | the **"Deferred surfaces"** list now says the editor is implemented and the terminal and the remaining graphical surfaces are deferred; the **"Trust boundary"** section records that the editor adds no renderer permission and no CSP relaxation |
| `architecture/overview.md` | the desktop editor as a consumer of the shared document foundation, and the guard counts |
| `plans/subsystems/desktop-frontend-ide-foundation-roadmap.md` | the M007-E row and the line's status row |
| `plans/registry.md` | this milestone's row and disposition |

## 10. Unresolved findings

| Finding | Severity | Owner |
|---|---|---|
| **The editor's DTOs are outside the shared `bindings:check` map.** `apps/desktop/scripts/check-bindings.mjs` reads `src-tauri/src/bridge.rs` and `src/bridge-types.ts` only, and the editor's own view structs are not in either file. The editor's **own** `contract-drift` rule compares every `*View` struct the host serves with its TypeScript interface field for field, so the editor's contract is guarded — but it is guarded by the *editor's* guard, not by the shared map. | medium | **M007-D's integration pass** — it owns `bridge.rs`, `bridge-types.ts` and `check-bindings.mjs`, and it must add the editor's twelve DTO field lists to the shared map so the two guards do not drift. |
| **The desktop host's `route_scope` duplicates `route_selection`.** `route_selection` is private to `route.rs`, so the host carries its own scope check. | low | **M007-D's integration pass** — it owns `route.rs`, and the collapse belongs there, not in a second implementation of the same rule. |
| **The panel authority guard's bridge baseline is measured against the editor's targets alone.** The guard's recorded baseline now counts the editor's bridge targets; M007-D adds its own. | low | **M007-D's integration pass** — the baseline must be re-measured once both sets of targets exist. |
| **A third-party editor engine stays out of scope.** The editor is a first-party surface over the shared `DocumentController`. | low | accepted; adopting an engine changes the renderer's CSP, worker and bundle boundary and needs its own ADR. |
| **The graphical terminal remains deferred.** | low | accepted; it is outside this milestone's plan. |

**No medium or high finding keeps M007-E open**, and the one medium finding
is **M007-D's integration pass**, which is recorded here so it is not lost.
M007-E's own acceptance criteria are met on this tree.

## 11. Roadmap disposition

`plans/subsystems/desktop-frontend-ide-foundation-roadmap.md`:

- the milestone-007 decomposition table's M007-E row moves from *ready for
  handoff* to **closed**, with the closure record;
- §13's status row records M007-E as closed.

**No other roadmap row changes.** The M007-A, M007-B, M007-C and M007-D rows
are not this milestone's to touch.

## 12. Registry updates

`plans/registry.md`:

- the `## Dependency-ready implementation plans` row for M007-E moves from
  *ready for handoff* to **closed**, and points at this closure record;
- the `## Active subsystem roadmaps` desktop row records the editor surface
  and the shared foundation's second consumer;
- a dated entry in `## Source verification`.

## 13. Handoff notes

- **M007-D** owns three integration points this milestone could not take:
  the editor's twelve DTO field lists for the shared `bindings:check` map,
  the collapse of the host's `route_scope` into `route.rs`, and the
  re-measurement of the panel authority guard's bridge baseline.
- **M007-D** must not disturb `apps/desktop/src-tauri/src/lib.rs`'s existing
  entries. This milestone appended its 15 commands contiguously at the end of
  `generate_handler!`, immediately after `desktop_disconnect`; M007-D's own
  commands were appended the same way. The list is therefore additive from
  both sides, and nothing needed reordering.
