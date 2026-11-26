# TUI Editor IDE Presence M002 — Closure Status

Status: closed

Source implementation plan: 

- `plans/implementation/tui-editor-ide-presence/002-editor-status-names-server-state.md`

Source subsystem roadmap: `plans/subsystems/tui-editor-ide-presence-roadmap.md#milestone-2--editor-status-names-server-state`

Dependency: M001 is closed (`plans/closure/tui-editor-ide-presence/001-status.md`).

## 1. Finding

M002 is complete. The existing editor status cluster now names the daemon's
publication and diagnostics state without adding a row or opening `F2`.
`lsp` is shown only for `DocumentLspSync::Synced` plus
`DiagnosticsViewStatus::Ready`; a Ready empty diagnostics set still means
ready. Pending publication or a NotReady diagnostics result shows `lsp…`.
Degraded, unavailable, and not-configured states remain distinct, and a legacy
sync witness is not mislabeled as warming.

A single cancellable `DocumentSnapshotGet` may settle Pending once per editor
open generation. It retains only the sync witness, not the returned text, and
uses the attachment epoch to reject a stale response. Failure or supersession
leaves Pending unchanged. If the existing diagnostics subscription completed
NotReady before this settlement, it may be refreshed once; this is finite and
uses the existing read path, not a poll or a new stream.

## 2. Progress decision

Skipped. The reviewed editor-callable read surface has no bounded `$/progress`
text to display. The row uses `lsp…` for warming; it does not invent an
`indexing` distinction or add a progress event, websocket type, or server
stderr read.

## 3. Implementation evidence

- `crates/codegg-tui/src/editor_lsp.rs`: pure status-word mapping and state-table coverage.
- `crates/codegg-tui/src/components/editor.rs`: one weighted server-state group; caveats retain higher priority.
- `crates/codegg-client/src/document.rs`: one snapshot settlement operation, attachment-epoch fence, and text-retention regression test.
- `crates/codegg-tui/src/app/state/editor.rs` and `crates/codegg-tui/src/commands/editor.rs`: at-most-once tracking, cancellation, stale-generation/document fencing, and one-shot diagnostics refresh after Synced when needed.
- `crates/codegg-tui/tests/tui_render/tui_editor_cost.inc`: Pending and Degraded render without opening the diagnostics panel.

## 4. Verification executed

- `cargo fmt -p codegg-client -p codegg-tui` — passed.
- Focused client tests `pending_lsp_sync_settles_with_one_snapshot_read_without_replacing_text` and `failed_lsp_sync_settlement_preserves_pending` — each passed (1 test).
- Focused TUI tests `settlement_is_one_shot_and_generation_bump_cancels_it`, `stale_lsp_sync_settlement_cannot_replace_the_current_pending_state`, and `editor_server_state_word_requires_synced_ready_and_distinguishes_absence` — each passed (1 test).
- `cargo test --locked -q -p codegg-tui --test tui_render pending_and_degraded_server_state_render_without_opening_f2` — passed; 1 selected, 1,239 filtered.
- `python3 scripts/check_tui_editor_text_authority.py` — passed.
- `cargo fmt --all -- --check` — not clean because it reports formatting diffs in the out-of-scope M001 files `crates/egglsp/src/service.rs` and `src/core/daemon_documents.rs`; those files were left unchanged.
- `scripts/verify.sh scoped` — not run per the task constraint.

No live TUI receipt is claimed.

## 5. Corrective after the line's sweep

The first full `scripts/verify.sh scoped` run after this closure found three
existing editor tests failing — `a_conflict_draft_is_visible_in_the_banner`,
`a_gone_document_says_the_draft_is_kept`, and
`a_disconnected_transport_says_the_draft_is_kept`. The new server-state group
had displaced the status text at the tests' 60-column width, so a conflict or
a lost document was dropped in favour of an informational word. That is the
lie this milestone exists to prevent, so it was fixed in production rather
than in the tests: `EditorStatus::is_caveat` classifies the safety states and
`EditorWidget::status_caveat` gives them the caveat weight, ahead of the
server word. The three tests pass again, unchanged in what they assert.

## 6. Handoff

M002's hard dependency is closed; M003 may proceed. M004, M005, and M006
remain outside this closure.
