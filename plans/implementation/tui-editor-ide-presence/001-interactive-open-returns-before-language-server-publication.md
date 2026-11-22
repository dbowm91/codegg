# TUI Editor IDE Presence Milestone 001 — Interactive open returns before publication

Status: closed

Closure record:

- `plans/closure/tui-editor-ide-presence/001-status.md`

Repository baseline: `d585e75f895cf41cd2241512d8d7e7c37fcece99`

Source roadmap:

- `plans/subsystems/tui-editor-ide-presence-roadmap.md#milestone-1--interactive-open-returns-before-publication`

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#13-protocol-terms`

Applicable ADRs:

- `plans/adrs/ADR-0065-interactive-document-open-does-not-await-language-server-publication.md` (proposed; this plan implements its Decision and no other option)
- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`
- `plans/adrs/ADR-0012-native-lsp-read-surface-delivery-and-authorization.md`

Primary class: capability

## 1. Objective

Workspace `DocumentOpen` returns the document snapshot after authorization and disk refresh. It does not wait for language-server publication. The interactive `didOpen` of that file is not gated on `WaitForDiagnosticsOrTimeout`. The TUI paints the returned snapshot.

## 2. Why this milestone is ready

ADR-0011 and ADR-0012 are accepted. The document open, disk refresh, and language-server service already exist. ADR-0065 records the deadline this milestone implements. No closed milestone is reopened. Desktop presentation, status copy, underlines, completion, and `didChange` debounce are later milestones, not missing dependencies.

Starting this milestone records acceptance of ADR-0065's Decision for this work. If that Decision is rejected, stop. Do not implement Option A or Option B from the ADR.

## 3. Current implementation evidence

At the baseline, `CoreDaemon::handle_document_request`'s workspace `DocumentOpen` arm calls `documents.open(..., writable: false)`, then `refresh_external_disk`, then `sync_managed_document`, and only then returns `CoreResponse::DocumentSnapshot { snapshot, writer_lease, lsp_degraded }`.

`sync_managed_document` calls `LspService::set_managed_document`, which calls `get_or_create_client`. Cold publication runs `compute_readiness_decision`. `rust_analyzer_profile()` is `WaitForDiagnosticsOrTimeout { timeout: 30s }`. `wait_for_first_diagnostics` polls for the first `publishDiagnostics`. That wait is before `didOpen` of the file just requested. The pyright profile waits 15 seconds. Other tier-1 profiles also wait for diagnostics.

`refresh_external_disk` takes the workspace repository lock unless the caller already holds it, re-validates the path, and re-reads the file. That stays on the response path. It is document authority, not the language-server wait.

The TUI `open_editor_in` navigates to `Route::Editor` and waits in `controller.open` for `EditorOpened`. `apply_editor_opened` is what paints. `wants_writer` is always true. The daemon open does not take a writer, so `DocumentController::complete_open` issues `DocumentWriterAcquire` before `open` returns. That second round trip is inside the same task as the publication wait.

The global-agent branch of `DocumentOpen` returns through `open_global_agent` before the workspace arm. Its filename rule is a different authority.

`DocumentChanged`, `DocumentSaved`, and `DocumentReloaded` also carry `lsp_degraded` and currently await sync. They are not this milestone.

No wall-clock of an 800-line open was measured. Do not cite one.

## 4. Invariants that must not regress

- The daemon owns document text and language-server processes. The TUI does not gain a buffer, a filesystem read, or a server. `scripts/check_tui_editor_text_authority.py` stays green.
- Authorization and path validation still happen before a snapshot is returned. `refresh_external_disk` still runs on the open response path.
- `rust_analyzer_profile()` and the other qualification profiles keep `WaitForDiagnosticsOrTimeout`. Do not edit those constants.
- A sync witness is recorded only when a push succeeds. A cancelled or failed publication clears it.
- `NotReady` is not turned into an empty diagnostic success.
- Change, save, and reload still await their own sync.
- The global-agent filename rule is unchanged.
- No new process launcher and no new writer-disposition row. The existing language-server spawn is the spawn.

## 5. Scope

### In scope

- Detach `sync_managed_document` from the workspace `DocumentOpen` response.
- Interactive publication uses `InitializedIsReady` or otherwise does not wait on `WaitForDiagnosticsOrTimeout` before `didOpen`.
- Additive `lsp_sync` on `CoreResponse::DocumentSnapshot` only, as ADR-0065 specifies.
- The TUI paints when that response arrives, without waiting for publication.
- The follow-up writer acquire must not re-introduce a wait on publication. Folding it into the open is allowed only with the acquire's existing authorization.

### Explicitly out of scope

- Status-cluster copy, diagnostic underlines, completion, semantic-token range, syntect threading, and `didChange` debounce.
- Editing qualification profile constants.
- Desktop chrome.
- Making save, reload, or change responses asynchronous.
- Skipping `refresh_external_disk`.
- A live timing receipt. If a warm open is still slow, report the repository-lock wait and stop. Do not skip the refresh to make the number smaller.

## 6. Required production changes

### Core/domain

The workspace `DocumentOpen` arm returns after `refresh_external_disk`. Publication is scheduled and does not hold the response. The scheduled work still calls the existing managed-document sync, still records or clears the sync witness, and still sends `didOpen` for the requested file. The policy on that interactive publication is not the qualification profile's diagnostics wait.

Inspect `open_global_agent`. If it awaits publication, apply the same deadline. Do not weaken its filename rule. If it does not await publication, leave it.

### Storage and migrations

None.

### Protocol and DTOs

Add `lsp_sync` to `CoreResponse::DocumentSnapshot` with `#[serde(default)]`. Variants: `Pending`, `Synced`, `Degraded`, `Unavailable`. `lsp_degraded` is true only for `Degraded` or `Unavailable`.

A missing `lsp_sync` means the legacy meaning: the sender awaited sync, and `lsp_degraded: false` meant that wait completed without error. Do not default a missing field to `Pending`. New responses always serialize the field.

Do not add the field to `DocumentChanged`, `DocumentSaved`, or `DocumentReloaded`.

### Runtime and concurrency

The background sync is tied to the document revision it read. A newer revision supersedes it. Failure and cancellation clear the witness. The open response's `lsp_sync` is `Pending` when publication has not finished. A later read of server state is M002; this milestone must not pretend `Pending` is synced.

The daemon task that runs publication must not use the process cwd as a workspace root.

### Frontend or operator surface

`apply_editor_opened` runs from the `DocumentOpen` response, as it does today, and that response no longer waits for publication. Do not add a second paint path or a TUI-side read of the file.

The writer acquire stays authorized. If it remains a second request, it may still run inside `complete_open`, because it is document authority and it is not the language-server wait. Do not hold it on publication. If folding it into `DocumentOpen` needs a new request field, that field is `#[serde(default)]` and the daemon runs the same authorization `DocumentWriterAcquire` runs. If that cannot be done without a second authorization story, leave the second request and say so in the closure.

### Security and authorization

No new capability. Open still refuses before any snapshot. The background sync does not open a path the document service refused.

### Documentation and static guards

Update `architecture/document.md` and `architecture/lsp.md` so open-returned is document authority, not server-has-the-file. Do not edit `scripts/check_tui_editor_text_authority.py` to allow a new read. Do not add a writer-disposition row.

## 7. Ordered work packages

### Work package A — Response deadline

Intent: the workspace open arm returns the refreshed snapshot without awaiting sync.

Required changes: split the await. Schedule sync. Set `lsp_sync: Pending` on the response. Keep `refresh_external_disk` on the response path.

Acceptance evidence: a test double that never publishes diagnostics returns a snapshot without waiting out the 30-second profile timeout.

### Work package B — Interactive didOpen policy

Intent: the publication that gates `didOpen` for this open does not use the qualification diagnostics wait.

Required changes: pass `InitializedIsReady`, or an equivalent non-waiting policy, on that path only. Leave `rust_analyzer_profile()` and the other profile constants alone.

Acceptance evidence: the interactive path does not call `wait_for_first_diagnostics`. The existing profile test that expects `WaitForDiagnosticsOrTimeout { 30s }` still passes.

### Work package C — Decode and paint

Intent: a new client decodes old and new payloads, and the TUI paints from the open response.

Required changes: `#[serde(default)]` with the legacy meaning for a missing field, not `Pending`. `apply_editor_opened` still paints the snapshot. It must not treat `lsp_sync: Pending` as a failed open, and it must not treat it as synced.

Acceptance evidence: an old payload without `lsp_sync` and `lsp_degraded: false` decodes as legacy success. A new `Pending` payload decodes, paints, and is not a sync witness.

### Work package D — Writer acquire does not wait on publication

Intent: the second acquire cannot put the language-server wait back on the paint path.

Required changes: confirm the acquire runs against the returned snapshot, not against publication. Fold it only under the rule in §6.

Acceptance evidence: the open-response test returns before publication even when the TUI asked for a writer. A refused acquire is still a refused acquire.

## 8. Failure, cancellation, restart, and contention semantics

Publication failure sets the later state to `Degraded` or `Unavailable` and clears the witness. It does not roll back the snapshot the user is reading. The open response has already returned `Pending`.

Cancelling the client request after the snapshot is committed does not un-open the document. It must not record a witness for a push that did not finish.

A second open of the same document keeps today's `AlreadyOpen` / switch behavior. Two publications for two revisions: the older one must not record a witness over the newer revision.

Daemon restart uses the existing document registry replay. This milestone does not add a replay path. An in-flight `Pending` that dies with the process is not synced after restart until a new publication succeeds.

The repository lock in `refresh_external_disk` is still held only for the refresh. Do not hold it across publication.

## 9. Compatibility and migration

Same-release daemon and frontend. Old clients that deny unknown fields fail closed on a new `DocumentSnapshot`. That is accepted. Do not dual-encode.

New clients read an old daemon: missing `lsp_sync` plus `lsp_degraded: false` is legacy awaited success, not `Pending`.

No configuration flag to restore the old wait. The qualification profiles are the old wait, for qualification callers.

## 10. Required tests

### Focused unit tests

- Missing `lsp_sync` decodes as legacy, not `Pending`.
- `Pending` serializes and does not set `lsp_degraded`.
- `Degraded` and `Unavailable` set `lsp_degraded`.
- Profile constants are unchanged. Reuse the existing rust-analyzer profile assertion rather than copying it.

### Integration tests

- Workspace `DocumentOpen` against a server that never publishes diagnostics returns a snapshot without the profile timeout, with `lsp_sync: Pending`.
- That publication's `didOpen` is not behind `wait_for_first_diagnostics`.
- Change, save, and reload still await sync and do not gain `lsp_sync`.
- Global-agent open, if it awaited publication, returns under the same deadline and still rejects a filename the old path rejected.

### Restart and recovery tests

- A failed publication clears the witness. A later successful publication records the revision it actually pushed.

### Contention and cancellation tests

- An older publication completing after a newer revision does not record the older revision as synced.

### Security and negative tests

- A refused path returns the existing document error and does not start publication for that path.
- The TUI test that opens a document does not read the fixture file itself.

### Migration and compatibility tests

- Old payload decode, as in work package C.

## 11. Required verification commands

Run the new tests in the package that owns each change. Identify the existing family from `docs/test-ownership.toml` before adding a test; do not invent a filter that matches zero tests.

```bash
python3 scripts/check_tui_editor_text_authority.py
scripts/verify.sh scoped
```

If the daemon document arm or the TUI editor task changes, the scoped sweep is the claim. A green `cargo check` is not the claim.

## 12. Documentation updates

- `architecture/document.md`: open-returned is document authority. `lsp_sync` is the server witness. `lsp_degraded: false` on a new open response is not "synced."
- `architecture/lsp.md`: interactive `didOpen` does not use the qualification diagnostics wait. Profile constants are unchanged.
- Do not rewrite the closed editor roadmaps.

## 13. Acceptance criteria

- A diagnostics-silent server does not hold workspace `DocumentOpen` for the profile timeout.
- The TUI paints the returned snapshot while publication is still `Pending`.
- `rust_analyzer_profile()` is still a 30-second diagnostics wait.
- The requested file's interactive `didOpen` is not waiting on that policy.
- A missing `lsp_sync` still means legacy awaited success.
- Change, save, and reload behavior is unchanged.
- `scripts/check_tui_editor_text_authority.py` is green.

## 14. Stop conditions

- ADR-0065's Decision is rejected, or the only way to finish is Option A or Option B.
- Applying the deadline requires a frontend filesystem read, a frontend language server, or a second text buffer.
- Folding the writer acquire in would skip the acquire's authorization.
- A warm open is still multi-second after publication is detached. Report the `refresh_external_disk` / repository-lock measurement. Do not skip the refresh.
- `CoreResponse` cannot gain `lsp_sync` without breaking same-release decode of old payloads, or without a dual-encode. Report that. Do not omit the field on new responses.
- The global-agent path cannot share the deadline without widening its filename rule.

## 15. Closure evidence required

- The diagnostics-silent open test, with the timeout it did not wait.
- The unchanged profile assertion.
- The legacy-payload decode test.
- The text-authority guard command and its result.
- `scripts/verify.sh scoped` result, labeled local.
- A statement that no 800-line wall-clock was claimed, and whether a warm-open lock wait was observed.
- Confirmation that change, save, and reload were not given this deadline.

## 16. Handoff notes

M002 is blocked on this closure. Do not start status copy, underlines, or completion in this change. `apply_editor_opened` may still pull diagnostics and semantic tokens after paint; a `NotReady` result must not be presented as a clean file beyond what the baseline already does. Naming that interval is M002.

Do not edit `scripts/annotate_nextest_junit.py`, `scripts/iter_selection.py`, or `stash@{0}`. Do not reopen ADR-0064.
