# TUI Editor IDE Presence Milestone 002 — Editor status names server state

Status: closed

Repository baseline: `d585e75f895cf41cd2241512d8d7e7c37fcece99` (re-inspected after M001 closure)

Closure record: `plans/closure/tui-editor-ide-presence/002-status.md`

Source roadmap:

- `plans/subsystems/tui-editor-ide-presence-roadmap.md#milestone-2--editor-status-names-server-state`

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/001-terminology-and-domain-model.md#6-session-and-interaction-terms`

Applicable ADRs:

- `plans/adrs/ADR-0065-interactive-document-open-does-not-await-language-server-publication.md`
- `plans/adrs/ADR-0012-native-lsp-read-surface-delivery-and-authorization.md`
- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`

Primary class: capability

## 1. Objective

The editor's one status cluster names the language-server state of the open document. A warming or degraded server is not a quiet, healthy editor. The `F2` diagnostics panel stays closed unless the user opens it.

## 2. Why this milestone was ready

M001 is closed: `DocumentSnapshot` carries `lsp_sync`, and the editor paints the returned buffer while publication is `Pending`. The existing status cluster is still one bottom row in `crates/codegg-tui/src/components/editor.rs`; this milestone adds one weighted group rather than another row.

## 3. Current implementation evidence

`EditorLspState` retains the snapshot's `DocumentLspSync`. `editor_server_status_word` maps that witness and `DiagnosticsViewStatus` to the short cluster word. `lsp` requires `Synced` plus `Ready`, including an empty Ready result; `Pending` and `NotReady` remain `lsp…`. Legacy omits the word rather than being called warming. `F2` remains opt-in.

The existing diagnostics subscription begins at open. If it reports `NotReady` before a cancellable, one-shot `DocumentSnapshotGet` settles `Pending` to `Synced`, the same subscription path may be refreshed once; no timer, polling, or new push stream is introduced. A failed or superseded snapshot read leaves Pending.

The LSP client tracks `$/progress`, but no bounded progress string is available on an editor-callable read. The closure records progress/indexing prose as skipped; the visible warming word is `lsp…`. `Opening` continues to describe the document request and is replaced by the normal document status when the snapshot is applied.

## 4. Invariants that must not regress

- One bottom chrome row. Do not restore a banner row. The cluster's drop-order (`ClusterWeight`) stays the rule for a narrow terminal. Server state outranks the modified marker and does not outrank a safety caveat the cluster already prefers.
- `F2` is not opened by a pull, a `Pending` sync, or a progress event.
- `NotReady` is not drawn as "no diagnostics" or as ready.
- No frontend language-server spawn, no filesystem read, no second text buffer.
- No new push stream and no new projection event. If the fact cannot be read from `lsp_sync` plus the existing diagnostics pull, ship without progress prose.
- `Esc` remains the mode switch. This milestone adds no key.

## 5. Scope

### In scope

- A server-state word in the existing status cluster.
- Distinct states: pending/warming, indexing, ready, degraded, unavailable, and not configured.
- Using `lsp_sync` from M001 and the existing diagnostics pull to choose the word.
- A regression that a `Pending` open is not drawn as ready.

### Explicitly out of scope

- Inline underlines (M003).
- Auto-opening `F2`, or changing the `F2` list.
- A new `$/`progress push, projection event, or websocket type.
- Desktop status chrome.
- Completion and semantic tokens.

## 6. Required production changes

### Core/domain

None, unless M001's `lsp_sync` is not on the snapshot the TUI already holds. Do not add a daemon status poller. The TUI reads state it already received, plus the diagnostics pull it already issues.

### Storage and migrations

None.

### Protocol and DTOs

No new field. `lsp_sync` is M001's field. Do not add `lsp_sync` to change/save/reload here.

### Runtime and concurrency

A late diagnostics pull must not overwrite a `Pending` or `Degraded` word with "ready" unless the pull is `Ready` and `lsp_sync` is `Synced`. A `NotReady` pull leaves the warming word in place.

### Frontend or operator surface

Draw the word in the status cluster. Suggested words, kept short enough for the one row: `lsp…`, `indexing`, `lsp`, `lsp degraded`, `lsp unavailable`, `no lsp`. The closure may rename them; it may not collapse warming and ready into one word.

Indexing versus warming: warming is `lsp_sync: Pending` or a `NotReady` pull. Indexing is that same interval when an existing read already carries progress text. If no such read exists, use warming and do not invent indexing as a second source of truth. Do not poll the LSP client from the TUI.

Unavailable is a failed or missing server (`lsp_sync: Unavailable`, or the existing not-configured path). Degraded is `lsp_sync: Degraded`. Ready is `Synced` plus a Ready pull, including a Ready pull with an empty list. An empty Ready list is "no diagnostics," which M003 may draw; this milestone's word is still ready, not clean-by-silence. The cluster must not say ready while `lsp_sync` is `Pending`.

`opening…` remains the document-request state and is replaced when the snapshot is applied.

### Security and authorization

No new read capability. Progress text, if shown, comes from a read the session can already make. Do not put document text or server stderr in the status row.

### Documentation and static guards

`docs/tui.md` names the status words. `architecture/tui.md` says the cluster is the server-state surface and `F2` is the list. `scripts/check_tui_editor_text_authority.py` stays green.

## 7. Ordered work packages

### Work package A — State from facts already held

Intent: map `lsp_sync` and the diagnostics pull to one cluster word, with warming distinct from ready.

Required changes: a pure function from those inputs to the word. No I/O in the function.

Acceptance evidence: a table test. `Pending` is not ready. `Synced` plus Ready is ready. `Degraded` and `Unavailable` are not ready. `NotReady` does not become ready.

### Work package B — Draw it in the cluster

Intent: the word is on the one status row, and a narrow terminal drops it by the existing weight rule rather than by wrapping a second row.

Required changes: one cluster element. Server state survives longer than the modified marker. It does not displace a line-too-long or highlighting-truncated caveat.

Acceptance evidence: a render test at the baseline editor width shows the warming word after a `Pending` open, and does not show the `F2` panel.

### Work package C — Progress prose, or a recorded skip

Intent: show `$/`progress only if an existing read already has it.

Required changes: search the daemon read surface. If a bounded progress string is already on a response the editor can issue, show a clipped form of it in the cluster. If it is not, record the skip in the closure. Do not add a projection event.

Acceptance evidence: either a render test with the clipped progress string, or a closure sentence naming the read that does not exist.

## 8. Failure, cancellation, restart, and contention semantics

A failed diagnostics pull does not clear a degraded word and does not invent a ready word. Disconnect clears the server word with the document attachment, same as the rest of the editor state. A generation mismatch drops the late pull, as the existing LSP request scope already requires.

One generation-scoped settlement task is added only for a `Pending` open. Its `DocumentSnapshotGet` is bounded to one attempt per open generation and is canceled on detach or generation bump. It does not poll. The existing diagnostics read/subscription remains the authority for Ready.

## 9. Compatibility and migration

No protocol change. Old daemons without `lsp_sync` are M001's legacy meaning: do not draw them as `Pending`. If this milestone runs only after M001, the TUI is talking to a daemon that sends `lsp_sync`. Still handle a missing field as legacy, not as warming, so a mixed test fixture does not lie.

## 10. Required tests

### Focused unit tests

- The state table in work package A, including `NotReady` and empty-Ready.

### Integration tests

- After a `Pending` open, the rendered status cluster contains the warming word and the `F2` panel is not open.
- A `Degraded` snapshot renders the degraded word.
- `opening…` is gone once the snapshot is applied.

### Restart and recovery tests

None beyond the generation mismatch the existing request scope already tests. Do not add a daemon restart test for a status word.

### Contention and cancellation tests

- A stale diagnostics response for an older generation does not replace the current word.

### Security and negative tests

- The status word contains no document text and no filesystem path beyond the path the cluster already shows.

### Migration and compatibility tests

- A fixture snapshot without `lsp_sync` is not labeled warming solely because the field is absent.

## 11. Required verification commands

```bash
python3 scripts/check_tui_editor_text_authority.py
scripts/verify.sh scoped
```

Run the TUI render family that owns `crates/codegg-tui/src/components/editor.rs`. Select it from `docs/test-ownership.toml`. Do not use a filter that matches zero tests.

## 12. Documentation updates

- `docs/tui.md`: the status words and that `F2` is still the diagnostics list.
- `architecture/tui.md`: the cluster is the server-state surface. No second row.

## 13. Acceptance criteria

- A `Pending` open shows a warming word in the status cluster and does not open `F2`.
- Ready, degraded, unavailable, and not-configured are distinct from warming.
- A `NotReady` pull is not drawn as ready.
- The editor still has one bottom chrome row.
- No new push stream was added. Progress prose is either clipped from an existing read or explicitly skipped.

## 14. Stop conditions

- M001 has not closed, or `lsp_sync` was not added.
- The only way to name the state is a new projection event or a TUI-owned language server.
- Showing the word requires a second chrome row.
- Progress text would require reading server stderr or document text.

## 15. Closure evidence required

- The state-table test and the render test that `F2` stayed closed.
- The progress decision: shown from a named existing read, or skipped.
- The text-authority guard and `scripts/verify.sh scoped`, labeled local.
- No live TUI receipt is required. Do not fabricate one.

## 16. Handoff notes

M003 is blocked on this closure. Do not draw underlines here. An empty Ready list may say ready in the cluster; M003 decides how the text looks. Do not start M003's underline in this change.

Do not edit the protected untracked scripts. Do not reopen ADR-0064.
