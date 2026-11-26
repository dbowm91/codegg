# TUI Editor IDE Presence Milestone 006 — Debounce managed didChange

Status: closed

Closure record:

- `plans/closure/tui-editor-ide-presence/006-status.md`

Repository baseline: `d585e75f895cf41cd2241512d8d7e7c37fcece99` (re-inspect at M001's closure SHA before editing)

Source roadmap:

- `plans/subsystems/tui-editor-ide-presence-roadmap.md#milestone-6--debounce-managed-didchange`

Long-term requirements:

- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/001-terminology-and-domain-model.md#13-protocol-terms`

Applicable ADRs:

- `plans/adrs/ADR-0065-interactive-document-open-does-not-await-language-server-publication.md`
- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`
- `plans/adrs/ADR-0012-native-lsp-read-surface-delivery-and-authorization.md`

Primary class: infrastructure

This milestone is not the open-latency fix. M001 is. Do not present a debounced `didChange` as a faster file open.

## 1. Objective

A burst of local edits to an already-open managed document produces one full-text `textDocument/didChange` of the latest text. `didOpen` is not debounced. The sync witness advances only when that push succeeds.

## 2. Why this milestone is ready

It is not ready. Hard dependency: M001 closed. The open path must already return before publication, so this debounce is not papering over the diagnostics wait. The change path still awaits sync at the baseline, which ADR-0065 left in place on purpose. This milestone is the one that may loosen the change path's notification schedule. It does not loosen save or reload.

## 3. Current implementation evidence

`set_managed_document_for_key` sends a managed change when the file is already open, and an open when it is not. The client sends `textDocument/didChange` as a full-text notification. `sync_managed_document` records `record_lsp_synced` only when that push succeeds, and `clear_lsp_synced` otherwise. That witness rule is the one this milestone must keep.

The change response (`DocumentChanged`) still carries `lsp_degraded` and, at the baseline, awaits sync. ADR-0065 did not add `lsp_sync` to that response. This milestone must not add it unless the change response stops awaiting sync. Prefer to keep the change response awaited for document authority, and debounce only the language-server notification that follows a successful local edit. If those two are the same await today, splitting them is the work. Do not return `DocumentChanged` before the document revision is committed.

Save and reload also await sync. They stay that way.

## 4. Invariants that must not regress

- `didOpen` is immediate on the interactive publication path M001 established. This milestone does not delay it.
- `record_lsp_synced` runs only after the push of that revision succeeds. An unpushed or superseded revision is not synced.
- A failed or cancelled push clears the witness for the revision it did not deliver. It does not clear a newer revision that a later push already recorded.
- Save and reload still await their own sync.
- `DocumentChanged` is not returned before the document service has committed the revision.
- No frontend language server and no second text buffer. The debounce lives in the daemon language-server sync, not in the TUI.
- Full text remains the wire shape unless a server rejects it. Partial ranges are a stop, not a silent improvement.
- The repository lock is not held across the debounce interval.

## 5. Scope

### In scope

- Debounce the managed `didChange` that follows committed local edits.
- One push of the latest text per burst.
- Witness updates only on successful push.

### Explicitly out of scope

- Debouncing `didOpen`, save, or reload.
- Changing `DocumentOpen`'s deadline (M001).
- Partial `didChange` content changes.
- Adding `lsp_sync` to `DocumentChanged` unless the implementation must split the response from the notification and the closure explains why the boolean alone would lie. Default is: do not add the field.
- A user-facing setting for the interval. Name the interval in the closure. A starting point is 50–150 ms, shorter than the completion debounce, because this push is what diagnostics and completion read. Do not share a timer with M004.

## 6. Required production changes

### Core/domain

After a committed local edit, schedule one managed `didChange` for that document. Further committed edits before the timer fires replace the pending text and revision. When the timer fires, push the latest text once. On success, record the witness for the revision that was pushed, not for a revision that arrived during the push. If a newer revision arrived during the push, the push's success does not record the newer revision; the newer one stays pending.

`didOpen` and the first sync of a document that has never been opened on the server are not put on this timer.

### Storage and migrations

None. The witness stays the existing in-memory record. Do not persist a "pending sync" row.

### Protocol and DTOs

No change if `DocumentChanged` still means the document revision committed. The language-server notification is not the document response.

### Runtime and concurrency

The debounce is per document, on the daemon language-server service, not on the TUI task channel. Two clients editing the same document share one pending push of the canonical text, which is the document service's text, not a client's uncommitted draft.

Cancellation of a client does not cancel a push of a revision the document service has committed, except when the document is closed. Close sends `didClose` and clears the pending push and the witness, as eviction does today.

Do not hold the workspace repository lock while the timer sleeps.

### Frontend or operator surface

None. The TUI already sends document changes. It does not learn about this debounce. Do not add a "syncing" word here; M002 owns server state. If M002 is closed, a pending `didChange` may leave `lsp_sync` at the last synced revision. That is correct. Do not report the unpushed revision as synced.

### Security and authorization

The push uses the text the document service already accepted. It does not re-read the file from disk and it does not accept a client-supplied path. A document the service has closed is not pushed.

### Documentation and static guards

`architecture/lsp.md` says managed `didChange` is debounced, `didOpen` is not, and the witness trails the push. `architecture/document.md` says `DocumentChanged` is still the committed revision, not the language-server round trip.

## 7. Ordered work packages

### Work package A — Pending push

Intent: committed edits coalesce to one pending revision per document.

Required changes: replace-in-place pending text. `didOpen` does not enter the pending map.

Acceptance evidence: three committed edits inside the interval produce one `didChange` whose text is the third revision.

### Work package B — Witness follows the push

Intent: success records the pushed revision only. Failure clears that revision's witness and does not mark a newer pending revision synced.

Required changes: the record/clear calls move to the push outcome. A superseded success does not record the newer revision.

Acceptance evidence: a push that succeeds for revision N, while N+1 is already pending, leaves N synced and N+1 not synced. A failed push of N leaves N not synced.

### Work package C — Close and save stay immediate

Intent: close flushes or drops the pending push without a witness for an undelivered revision. Save still awaits its own sync and is not debounced.

Required changes: close clears pending. Save's path does not use the debounce timer.

Acceptance evidence: close during a pending interval does not record a witness. A save during a pending interval still awaits sync and does not wait only for the debounce timer as a substitute.

## 8. Failure, cancellation, restart, and contention semantics

Push failure clears the witness for the revision that failed and leaves the latest committed text pending for one retry, or clears it and surfaces the existing degraded state. Pick one and test it. Do not retry in a loop. A single retry is allowed. An unbounded retry is not.

Daemon restart drops the pending map. The document registry replay is the existing open replay, not a replay of debounced changes that were never pushed. After restart the server does not have those edits until a new `didOpen` or a new push. Do not record a witness across restart for a push that did not happen.

Two daemons are out of scope. One daemon, one pending push per document.

The TUI cancelling a change request after the document service committed does not un-commit. The pending push still sends the committed text.

## 9. Compatibility and migration

No protocol field. No config. Servers still receive full-text `didChange`, less often. A server that required a change per keystroke is not a server this codebase targets. If a test double counted one notification per edit, update the test to the burst rule rather than weakening the debounce.

Save and reload clients see the same awaited sync as before.

## 10. Required tests

### Focused unit tests

- Three edits, one notification, latest text.
- `didOpen` is not delayed by the timer.
- Success records only the pushed revision.

### Integration tests

- Failed push does not leave the failed revision synced.
- Close during the interval does not record a witness.
- Save does not use the debounce timer.

### Restart and recovery tests

- A pending push that dies with the process is not synced after restart.

### Contention and cancellation tests

- A newer revision arriving during an in-flight push is not marked synced by that push's success.

### Security and negative tests

- The push text is the document service's text. A client cannot name a different path on the debounced notification.

### Migration and compatibility tests

None.

## 11. Required verification commands

```bash
python3 scripts/check_tui_editor_text_authority.py
scripts/verify.sh scoped
```

Run the egglsp or daemon document tests that own `set_managed_document` and the sync witness. Select them from `docs/test-ownership.toml`. The TUI guard runs because the milestone must show the TUI did not grow a sync timer of its own.

## 12. Documentation updates

- `architecture/lsp.md`: debounce, witness, `didOpen` excluded.
- `architecture/document.md`: `DocumentChanged` is still the committed revision.
- Do not describe this milestone as the open-latency fix in any user-facing doc.

## 13. Acceptance criteria

- A burst of committed edits produces one `didChange` of the latest text.
- `didOpen` is not on that timer.
- The sync witness matches the last successful push, not the latest committed revision, until that push succeeds.
- Save and reload still await sync.
- Close does not record a witness for a push that did not complete.
- No TUI timer performs this sync.

## 14. Stop conditions

- M001 has not closed.
- The only way to debounce is to return `DocumentChanged` before the revision commits.
- Preserving the witness requires a storage migration or a new protocol field, and the boolean on the change response cannot stay honest. Report that. Do not add `lsp_sync` to save or reload in passing.
- Full-text `didChange` is rejected by the server. Partial ranges are a new plan, not an improvisation here.
- The repository lock would be held across the timer. Stop and leave the lock where `refresh_external_disk` has it.

## 15. Closure evidence required

- The burst test, the witness test, and the save-not-debounced test.
- The interval that shipped, named.
- A sentence that this is not the open-latency fix and that M001's deadline was not changed.
- The text-authority guard and scoped verify, labeled local.
- No 800-line open timing claimed.

## 16. Handoff notes

This is the last milestone of the roadmap. Closing it does not unblock a registered successor. The closure's registry section says so, including if nothing was unblocked.

Do not edit the protected untracked scripts. Do not reopen ADR-0064, ADR-0034, or the closed completion milestone.
