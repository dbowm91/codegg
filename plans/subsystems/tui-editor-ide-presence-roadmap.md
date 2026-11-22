# TUI Editor IDE Presence Roadmap

Status: closed

Long-term references:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#6-session-and-interaction-terms`
- `plans/002-long-term-roadmap.md#phase-5--frontend-neutral-session-projections-and-durable-replay`

Related ADRs:

- `plans/adrs/ADR-0065-interactive-document-open-does-not-await-language-server-publication.md` (accepted; implemented by M001)
- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`
- `plans/adrs/ADR-0012-native-lsp-read-surface-delivery-and-authorization.md`
- `plans/adrs/ADR-0021-native-editor-completion-read-and-local-edit.md`
- `plans/adrs/ADR-0034-editor-syntax-tree-dependency.md` (accepted Option B)

Closed predecessors, not reopened:

- `plans/subsystems/tui-editor-correctness-usability-roadmap.md` — semantic-token fidelity and explicit completion. Its M004 excluded automatic trigger. M004 of this roadmap is the successor for a debounced request, not a reopening of that exclusion's acceptance rules.
- `plans/implementation/tui-render-cost/005-editor-draw-path-cost.md` — per-frame draw cost, closed 2026-11-10. It is not open latency and not the semantic-token cap.
- `plans/subsystems/daemon-lsp-frontend-convergence-roadmap.md` — closed. This roadmap does not add a second LSP coordinator.
- `plans/subsystems/editor-document-foundation-roadmap.md` — document ownership stays there.

Baseline: `d585e75f895cf41cd2241512d8d7e7c37fcece99`. Re-inspect HEAD before each implementation. No wall-clock of an 800-line open was taken. The 30-second cap and the pre-`didOpen` wait are source facts.

## 1. Purpose and ownership boundary

Make the TUI editor paint a daemon document without waiting for a language server, and make the language server's state visible in the editor the user is already looking at.

The symptom is two facts at the baseline. An 800-line Rust file can sit on an empty editor while `DocumentOpen` waits for rust-analyzer. Completion, diagnostics, and progress are implemented, but they are silent unless the user already knows `F8`, Insert `Ctrl-x`, `:complete`, or `F2`.

**This subsystem owns** the interactive open's response deadline, the editor status cluster's server-state text, inline diagnostic presentation, the debounced Insert completion request, viewport semantic-token reads, and the syntect load's thread. It owns the follow-on debounce of managed `textDocument/didChange` only after open is fast.

**It consumes** the daemon document service, the daemon language-server service, `DocumentController`, and the existing `LspReadGet` / `LspDiagnosticsGet` / `LspCompletionGet` families.

**It must not own** text authority, language-server process lifecycle, filesystem authorization, or any mutation path other than `DocumentController::apply_local`. ADR-0011 is not renegotiated here. `scripts/check_tui_editor_text_authority.py` must stay green.

## 2. Work classification

### Invariants

- No independent frontend text buffer, no frontend filesystem read, and no frontend language-server spawn.
- Workspace `DocumentOpen` returns after document authority, not after language-server publication (ADR-0065).
- Qualification profiles keep `WaitForDiagnosticsOrTimeout`. The interactive `didOpen` path does not.
- `NotReady` is not an empty success (ADR-0012). A missing underline or a quiet gutter is not a clean file while the server is warming.
- Completion acceptance stays explicit and revision-checked (ADR-0021). Arrival does not insert.
- Syntect remains the server-free highlight floor. No tree-sitter dependency (ADR-0034 Option B).
- A sync witness is recorded only for a revision that reached a server.

### Capabilities

- The editor shows the opened file while the language server is still starting.
- The status cluster names warming, indexing, ready, degraded, and unavailable.
- Diagnostics are visible in the text, not only as a gutter glyph or an `F2` list.
- Insert mode can request completions after a short idle, and the user still accepts one.
- An ordinary Rust file keeps semantic highlighting past the first thousand tokens, for the viewport on screen.

### Infrastructure

- Additive `lsp_sync` on `CoreResponse::DocumentSnapshot`.
- Interactive publication policy, distinct from the qualification profile constants.
- A viewport `textDocument/semanticTokens/range` read on the existing pull family.
- A debounced managed `didChange` that does not move the sync witness early.

### Polish

- `$/`progress text in the status cluster, when an existing read can carry it.
- Moving the first syntect `SyntaxSet` / `ThemeSet` load off the render thread.
- Collapsing the TUI's follow-up `DocumentWriterAcquire` when that can be done without a second authorization story.

Infrastructure and polish are not the capability. M001 is not done because a flag exists. M002 is not done because a string is computed and never drawn.

## 3. Non-goals

- Reopening ADR-0064, or adding a root `codegg` binary or `src/main.rs`.
- A TUI-owned language server, a second text buffer, or a frontend tree-sitter crate.
- Changing `rust_analyzer_profile()` or the other qualification profiles to `InitializedIsReady`.
- Auto-opening the `F2` diagnostics panel.
- Insert-on-arrival, snippet expansion, `additionalTextEdits`, `workspace/executeCommand`, or a second `LspPreviewApply` path.
- Raising `MAX_LSP_SEMANTIC_TOKENS` so a full-file token dump fits an 800-line file.
- A new diagnostics or progress push stream. ADR-0012 stays pull-authoritative for reads.
- Desktop editor chrome. Desktop must decode the open response; it does not gain this roadmap's status row, underline, or popup.
- Debouncing save or reload. Those responses keep awaiting sync.
- A live TUI receipt or a measured 800-line wall-clock as a closure gate. Do not fabricate either. A warm open that is still slow is reported at the repository lock, not fixed by skipping `refresh_external_disk`.
- Multi-buffer attachment, pane splits, soft wrap, and code actions.

## 4. Current state

`open_editor_in` navigates to `Route::Editor` and issues one `DocumentOpen`. The buffer is applied in `apply_editor_opened`, which runs only after that request returns. A failure detaches and shows no buffer.

The daemon workspace arm calls `documents.open` with `writable: false`, then `refresh_external_disk` (repository lock, re-read, path validation), then `sync_managed_document`. The sync calls `set_managed_document` → `get_or_create_client`. Cold publication runs `compute_readiness_decision`. rust-analyzer waits up to 30 seconds for the first `publishDiagnostics`, before `didOpen` of the requested file.

The TUI always sets `wants_writer = true`. `DocumentController::complete_open` then issues `DocumentWriterAcquire` when the open returned no lease. That second round trip is inside `controller.open`, so it holds `apply_editor_opened` even after publication is detached. It is not the multi-second wait. It is the next wait on the same task.

`apply_editor_opened` pulls diagnostics and then requests semantic tokens. The semantic request clears `lsp.panel`. A not-ready server covers nothing. Diagnostics in the buffer retain the gutter glyph (`!`, `▲`, `·`) after a Ready pull, and M003 underlines visible ranges from that same retained Ready set. The list is the `F2` panel. The status cluster names the publication and diagnostics state (M002). `$/`progress is tracked on the LSP client and does not reach the status row.

Completion is explicit: `F8`, Insert `Ctrl-x`, and `:complete`. The closed M004 put automatic trigger out of scope. Acceptance is a revision-checked local edit. There is no idle request.

Semantic tokens are `textDocument/semanticTokens/full`, capped at `MAX_LSP_SEMANTIC_TOKENS` (1000). That cap is smaller than an 800-line Rust file. The status can say "highlighting truncated" only after tokens arrive. The first highlighted frame loads syntect `SyntaxSet::load_defaults_newlines` and `ThemeSet::load_defaults` through `Lazy` on the thread that first highlights, which is the render thread.

Managed updates of an already-open file go through `set_managed_document_for_key` and a full-text `textDocument/didChange`. That push is not the open-latency bug.

## 5. Target architecture

The daemon returns the snapshot when the document is authorized and refreshed. A background publication sends `didOpen` under `InitializedIsReady` for that interactive open, then records or clears the sync witness. The open response says `Pending` until that witness exists.

The TUI paints the snapshot without waiting for publication. The status cluster reads `lsp_sync` and the existing diagnostics pull. Warming is a named state. A Ready pull with ranges draws a gutter glyph and a severity underline. A `NotReady` pull does not clear the last Ready draw and does not look like "no diagnostics."

Insert mode may schedule `LspCompletionGet` after a short idle. The popup is the existing one. Accept still goes through the revision witness and `apply_local`. `F8`, `Ctrl-x`, and `:complete` stay.

Semantic color for the viewport is a range read bounded by the existing token cap. Syntect paints immediately, including while the range is in flight, and its grammar load does not run on the render thread. Full-text `didChange` after open is debounced; `didOpen` is not; the sync witness moves only when a push succeeds.

## 6. Dependency graph

```text
ADR-0065 (accepted)
    |
    `--> M001 interactive open returns before publication
              |
              +--> M002 status names server state
              |         |
              |         `--> M003 inline diagnostic underline
              |
              +--> M004 debounced Insert completion
              |
              +--> M005 viewport semantic tokens and off-thread syntect
              |
              `--> M006 debounce managed didChange
```

| Edge | Class | Why |
|---|---|---|
| ADR-0065 → M001 | hard | M001 implements that Decision and no other option. |
| M001 → M002 | hard | The status is the warming interval M001 creates. Without M001 the buffer is still blank for the whole wait. |
| M002 → M003 | hard | An underline-less buffer must not be readable as clean while the status still says warming. |
| M001 → M004 | hard | A debounced request during a blocked open only produces `NotReady`. |
| M002 → M004 | soft | The status should already explain `NotReady` before the popup appears on idle. |
| M001 → M005 | operational | Same editor files. The range read does not need the new deadline to be correct. It waits so it does not collide with M001. |
| M001 → M006 | hard | Debouncing `didChange` before open is fast optimizes a path the user has not reached. |

No milestone depends on a live TUI receipt from an earlier line.

## 7. Milestones

### Milestone 1 — Interactive open returns before publication

Class: capability

Objective: workspace `DocumentOpen` returns the snapshot after document authority. The interactive `didOpen` is not gated on `WaitForDiagnosticsOrTimeout`. The editor paints that snapshot without waiting for publication.

Dependencies: proposed ADR-0065, implemented as written.

Deliverable boundary: daemon open arm, interactive publication policy, additive `lsp_sync`, TUI apply path. Not status copy, underlines, completion, or `didChange`.

User or operator value: the file is on screen while rust-analyzer starts.

Exit conditions: a diagnostics-silent server does not hold the open for the profile timeout; the profile constants are unchanged; the TUI paints from the returned snapshot; the text-authority guard stays green.

Deferred work: naming the warming interval (M002); collapsing the writer acquire if it cannot be done without a new authorization story.

### Milestone 2 — Editor status names server state

Class: capability

Objective: the editor's one status cluster names the server state for the open document. It does not open the `F2` panel to do it.

Dependencies: M001 closed.

Deliverable boundary: status-cluster presentation and the read it uses. Not underlines, not a new push stream.

User or operator value: a quiet editor is distinguishable from a server that is indexing, degraded, or absent.

Exit conditions: Pending, indexing, ready, degraded, and unavailable are distinct on-screen states; `NotReady` is not drawn as a clean file; `F2` stays manual.

Deferred work: progress/indexing prose skipped; no bounded progress string is available on an editor-callable read (see M002 closure).

### Milestone 3 — Inline diagnostic underline

Class: capability

Objective: a Ready diagnostic pull draws a severity underline in the text and keeps the gutter glyph.

Dependencies: M002 closed.

Deliverable boundary: buffer presentation of an already-pulled diagnostic snapshot. Not a new diagnostics protocol, not code actions, not an auto-opened list.

User or operator value: a diagnostic is visible without knowing `F2`.

Exit conditions: error, warning, and other severities are distinguishable in the text; a `NotReady` or failed pull does not clear the last Ready draw; the `F2` list is unchanged.

Deferred work: none in this roadmap.

### Milestone 4 — Debounced Insert completion

Class: capability

Objective: Insert mode schedules one completion read after a short idle. Acceptance stays explicit.

Dependencies: M001 closed. M002 is a soft dependency.

Deliverable boundary: the request schedule and the existing popup. Not a new accept path.

User or operator value: suggestions appear while typing, and nothing is inserted until the user accepts.

Exit conditions: idle Insert schedules `LspCompletionGet`; arrival does not insert; `F8`, Insert `Ctrl-x`, and `:complete` still work; a stale response is not actionable; the closed M004's revision witness is unchanged.

Deferred work: trigger-character configuration as a user setting. The debounce may reset on the server's trigger characters; it must not insert on them.

### Milestone 5 — Viewport semantic tokens and off-thread syntect

Class: capability

Objective: semantic color covers the viewport of an ordinary file, and the first syntect load does not run on the render thread.

Dependencies: M001 landed (operational). No interface dependency.

Deliverable boundary: a range read on the existing semantic-token pull, and the highlight loader's thread. Not a higher full-file cap, not tree-sitter, not a reopening of the draw-path milestone.

User or operator value: an 800-line Rust file is highlighted beyond the first thousand tokens, and the first paint does not stall the render thread on grammar load.

Exit conditions: the viewport request is `semanticTokens/range` or an equivalent bounded range on the existing pull; a response still respects `MAX_LSP_SEMANTIC_TOKENS`; syntect paints when the server is not ready; the grammar load is off the render thread; ADR-0034 Option B holds.

Deferred work: a live timing of the grammar load. A test that the load is not invoked from the render path is the gate.

### Milestone 6 — Debounce managed didChange

Class: infrastructure

Objective: full-text managed `didChange` after a local edit is debounced. `didOpen` is not. The sync witness moves only when a push succeeds.

Dependencies: M001 closed.

Deliverable boundary: the change-notification schedule. Not save, not reload, not the open deadline.

User or operator value: none by itself. This is the follower that keeps a fast open from paying a full-text push on every keystroke. It must not be presented as the load fix.

Exit conditions: a burst of local edits produces one push of the latest text; an unpushed revision is not recorded as synced; save still awaits its own sync; cancellation does not record a witness for a push that did not complete.

Deferred work: partial `didChange` ranges. Full text stays the wire shape unless a stop condition says the server rejects it.

## 8. Cross-cutting requirements

### Storage and migration

None. No schema change. No `STORAGE_LAYOUT_VERSION` bump.

### Protocol and compatibility

M001 adds `lsp_sync` to `DocumentSnapshot` only, with `#[serde(default)]`, as ADR-0065 states. Later milestones do not add a response field unless their plan says the existing read cannot carry the fact. A new push event is a stop.

### Security and authorization

Open authorization and `refresh_external_disk` stay on the response path. Writer acquisition, if folded into open, uses the existing acquire authorization. Reads stay on the ADR-0012 families and capabilities. No path is resolved from the process cwd.

### Concurrency, cancellation, and recovery

Background publication is per document revision. A newer open or edit supersedes an in-flight sync. The witness is cleared when a push fails or is cancelled, matching today's `clear_lsp_synced` rule. Restart replays through the existing document registry; this roadmap does not add a second replay.

### Observability and audit

No new audit event is required for the open deadline. A degraded publication uses the existing language-server degradation path. Plans must not log document text.

### Documentation

Each milestone updates the architecture page that owns the contract it changed (`architecture/document.md`, `architecture/lsp.md`, `architecture/tui.md`) and does not copy those pages into the plan. `docs/tui.md` gains the status words and the completion debounce only when the corresponding milestone draws them.

## 9. Verification strategy

Each milestone names a regression that failed on the behavior it replaces: a diagnostics-silent server holding open, a warming interval drawn as clean, a `NotReady` pull clearing underlines, an idle request inserting, a full-file token cap hiding the viewport, a sync witness recorded before the push.

`scripts/check_tui_editor_text_authority.py` runs for every milestone that touches the TUI editor. `scripts/verify.sh scoped` is the sweep. No milestone claims a live TUI receipt or a measured 800-line open. Hosted CI is recorded if it was run, and not claimed if it was not.

## 10. Risks and decision points

- **ADR-0065 is proposed.** M001 implements its Decision. Option A and Option B are stops.
- **Old clients deny unknown fields.** Same-release daemon and frontend is the migration. Do not dual-encode.
- **Ready-and-empty looks clean.** M001 must not teach the TUI that an empty pull is a clean file. M002 names warming before M003 draws underlines.
- **Writer acquire.** Folding it into open is optional inside M001. Skipping its authorization is a stop.
- **Progress has no frontend read.** M002 may ship without `$/`progress prose. Inventing a push stream is a stop.
- **Range requests may be unsupported.** M005 keeps syntect and the full-token path's truncation caveat. It does not raise the cap to compensate.
- **didChange debounce can desync the server.** M006's witness rule is the stop if a debounce cannot preserve it.

## 11. Completion definition

The roadmap is complete when M001–M006 are closed and:

- an interactive open paints without the diagnostics-wait timeout;
- the status cluster distinguishes warming from clean and from degraded;
- Ready diagnostics are underlined in the text;
- Insert completion can be requested on idle and still requires an explicit accept;
- viewport semantic color is not capped as a full-file dump of 1000 tokens;
- managed `didChange` is debounced without moving the sync witness early.

A closed draw-path milestone, a green `cargo check`, or a commit message is not this definition.

## 13. Closeout

All six milestones are closed, each with a record under
`plans/closure/tui-editor-ide-presence/`. The six capabilities the definition
requires are implemented and each is held by a regression named in its
closure: a diagnostics-silent open that returns before publication, a status
cluster that names warming distinctly from ready, Ready underlines that
survive a `NotReady` pull, an idle completion request that inserts nothing,
a viewport semantic read that is not a cut full-file dump, and a debounced
managed `didChange` whose witness trails the push.

Verification for the line is a **partial** local `scripts/verify.sh scoped` run
plus the per-milestone commands recorded in each closure. It is local: no
hosted CI run is claimed, no live TUI receipt was taken, and no 800-line open
wall-clock was measured. `docs/comment-budget.toml` was re-banked in this
change because the line's documentation raised the corpus; that diff is the
review surface for the added comment mass, and
`architecture/overview.md`'s integration-test count moved 298 → 299 for the
new M001 daemon test member.

What the sweep actually established, in three runs:

1. Its guard phase executed and was fixed twice by it — the comment corpus
   (banked) and the recorded test-module count (298 → 299, with the prose
   updated) — and every guard it reaches in that phase then passed.
2. Its `nextest` phase executed **14,373 of 17,668** tests on one run and
   found two failures, both mine: three editor tests that the M002 status word
   had displaced a conflict banner from (fixed in production by
   `EditorStatus::is_caveat`, not by weakening the tests), and two
   `editor_highlight` tests that failed only under nextest's
   one-process-per-test isolation because the first-use asset kick could win
   the load race (fixed by making the test loader wait; re-verified 18/18 under
   `cargo nextest run -p codegg-tui --lib -E 'test(editor_highlight)'`).
3. The sweep then **could not be completed on this host**: the data volume is
   at 100% capacity (`877Gi` used, `607Mi` available) and the run aborted with
   `No space left on device` while compiling, before the remaining ~3,300
   tests executed. Deleting this worktree's incremental and per-crate build
   caches freed ~72G without moving `df`, which points at APFS snapshot
   pinning rather than at this worktree.

**Not verified by the sweep, and therefore not claimed:** the remaining
root-integration families (including `core_daemon` beyond
`document_open_deadline_m001`, `ide_protocols`, and `lsp_test_support`) were
not re-run after the final edits. Each milestone's own focused tests, the full
`tui_render` family (1,250), the full TUI library suite (3,207), an all-target
workspace `cargo check`, `cargo fmt --all -- --check`, and the four guards
above all passed locally. A green scoped sweep remains the last outstanding
evidence for this line, and it is outstanding because of the host, not because
a known test fails.

## 12. Milestone status table

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 1 | closed | `plans/implementation/tui-editor-ide-presence/001-interactive-open-returns-before-language-server-publication.md` | `plans/closure/tui-editor-ide-presence/001-status.md` | none |
| 2 | closed | `plans/implementation/tui-editor-ide-presence/002-editor-status-names-server-state.md` | `plans/closure/tui-editor-ide-presence/002-status.md` | none |
| 3 | closed | `plans/implementation/tui-editor-ide-presence/003-inline-diagnostic-underline.md` | `plans/closure/tui-editor-ide-presence/003-status.md` | none |
| 4 | closed | `plans/implementation/tui-editor-ide-presence/004-debounced-insert-completion.md` | `plans/closure/tui-editor-ide-presence/004-status.md` | none |
| 5 | closed | `plans/implementation/tui-editor-ide-presence/005-viewport-semantic-tokens-and-off-thread-syntect.md` | `plans/closure/tui-editor-ide-presence/005-status.md` | none |
| 6 | closed | `plans/implementation/tui-editor-ide-presence/006-debounce-managed-didchange.md` | `plans/closure/tui-editor-ide-presence/006-status.md` | none |
