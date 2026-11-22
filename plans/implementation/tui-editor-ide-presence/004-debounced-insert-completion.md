# TUI Editor IDE Presence Milestone 004 — Debounced Insert completion

Status: closed

Closure record:

- `plans/closure/tui-editor-ide-presence/004-status.md`

Repository baseline: `d585e75f895cf41cd2241512d8d7e7c37fcece99` (re-inspect at M001's closure SHA before editing)

Source roadmap:

- `plans/subsystems/tui-editor-ide-presence-roadmap.md#milestone-4--debounced-insert-completion`

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/001-terminology-and-domain-model.md#6-session-and-interaction-terms`

Applicable ADRs:

- `plans/adrs/ADR-0021-native-editor-completion-read-and-local-edit.md`
- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`
- `plans/adrs/ADR-0012-native-lsp-read-surface-delivery-and-authorization.md`

Primary class: capability

This plan does not reopen `plans/implementation/tui-editor-correctness-usability/004-editor-completion-presentation-and-qualification.md`. That milestone closed explicit accept, the revision witness, and the exclusion of automatic trigger. This milestone adds a debounced request. It does not add an accept path.

## 1. Objective

After a short idle in Insert mode, the editor schedules one `LspCompletionGet` at the cursor. The popup is the existing one. Nothing is inserted until the user accepts, through the existing revision witness.

## 2. Why this milestone is ready

It is not ready. Hard dependency: M001 closed, so an open is not still blocked on publication when the first Insert edits happen. M002 is soft: the status word should already explain `NotReady` before an idle popup appears. If M002 has not closed, the idle request must still show the existing warming / not-ready popup state rather than an empty success. Do not wait on M003.

## 3. Current implementation evidence

`start_editor_completion` runs from `F8`, from Insert `Ctrl-x`, and from `:complete`. Those three stay. The closed M004 made acceptance a revision-checked local edit through `DocumentController::apply_local`. A stale response is not actionable. `Esc` dismisses the popup and does not quit the editor. There is no idle timer and no trigger-character request.

The request needs a bound attachment, a daemon revision, and a UTF-16 cursor from the bounded rope walk. An over-budget line is refused and the popup stays closed. That refusal stays.

ADR-0021 forbids routing editor completion through the model-facing read-only completion helper that drops `textEdit`. This milestone uses the existing editor completion request, not that helper, and not `LspPreviewApply`.

## 4. Invariants that must not regress

- Arrival does not insert. Accept is an explicit key on the existing popup (`Enter` today) or the existing command. No second accept path.
- The revision witness still rejects a response for a later revision. A debounce that fires after another edit is a new request, not an insert of the old one.
- `F8`, Insert `Ctrl-x`, and `:complete` still start a request. The idle schedule does not swallow them.
- Insert-mode text integrity: the debounce must not consume the character that was typed. Printable input still reaches the document.
- `Esc` dismisses the popup and does not quit. `Ctrl-c` remains the two-press exit arm and is not the completion key.
- No snippet expansion, `additionalTextEdits`, `executeCommand`, or auto-import.
- No frontend language server. The request goes through the daemon.
- A `NotReady` or unsupported result uses the existing popup states. It is not an empty applicable list and it is not an insert.

## 5. Scope

### In scope

- One idle debounce after Insert-mode document edits, scheduling the existing completion request.
- Cancelling that schedule on mode switch, document switch, and a newer edit.
- Keeping the three explicit triggers.

### Explicitly out of scope

- Changing accept, the revision witness, or the popup widget beyond what a scheduled request needs to open it.
- A user setting for the debounce interval. Pick one bounded interval in the plan's implementation and name it in the closure. A starting point is 150–300 ms. Do not make it configurable in this milestone.
- Trigger-character insertion. The server's trigger characters may reset the debounce. They must not accept an item.
- Hover, signature help, and code actions.
- Desktop completion.

## 6. Required production changes

### Core/domain

None. The daemon completion read is unchanged.

### Storage and migrations

None.

### Protocol and DTOs

None.

### Runtime and concurrency

The debounce is a TUI task on the existing editor task channel, keyed by the editor generation and the document revision at schedule time. A newer edit, a mode change, a document switch, or a generation bump cancels it. A response whose revision does not match the cursor's revision is dropped, as the closed M004 already requires.

Only one idle request is in flight. A second schedule supersedes. `F8` / `Ctrl-x` / `:complete` supersede the idle schedule and start immediately, which is the existing explicit path.

The timer must not run on the render thread and must not block the input loop. Use the existing TUI task spawn. A timed arm needs the animation interval, or the timer will not fire until the next key. That is the same rule as the exit arm.

### Frontend or operator surface

Insert-mode edits that change the document schedule the debounce. Navigation keys do not. A refused over-budget line does not schedule.

The popup opens when the response is actionable, using the existing presentation. Warming, unsupported, and error use the existing distinctions. The user accepts with the existing key. Typing further dismisses or supersedes, as a pending request already must.

Do not bind a new global chord. Do not take `Ctrl-c`.

### Security and authorization

The request uses the existing completion capability and the open document's scope. A schedule without a bound attachment is not sent. No path is read from the frontend.

### Documentation and static guards

`docs/tui.md` says Insert mode requests completions after a short idle and that accept is still explicit. Name `F8`, `Ctrl-x`, and `:complete` as the immediate triggers. `scripts/check_tui_editor_text_authority.py` stays green. The editor key-authority guard stays green: the idle path adds no chord.

## 7. Ordered work packages

### Work package A — Schedule and cancel

Intent: an Insert edit arms one bounded timer; a newer edit or a mode switch disarms it.

Required changes: timer on the editor task channel, generation-keyed. No request is sent until the timer fires, and not if the generation or revision changed.

Acceptance evidence: a test that two edits inside the interval produce one request, and a mode switch produces zero.

### Work package B — Reuse the explicit request

Intent: the fired timer calls `start_editor_completion` or the same function it uses. It does not build a second request shape.

Required changes: one call path. Explicit triggers cancel the timer and call that path immediately.

Acceptance evidence: `F8` during a pending debounce sends the explicit request and does not also send the idle one.

### Work package C — Accept stays explicit

Intent: a response that arrives, including one the user did not ask for with `F8`, does not insert.

Required changes: the popup opens. Insert happens only on the existing accept key, with the existing revision check. A stale idle response is counted as dropped, not applied.

Acceptance evidence: a test that an idle response with a matching revision opens the popup and leaves the document text unchanged until accept, and a stale revision does not open an actionable popup.

## 8. Failure, cancellation, restart, and contention semantics

`NotReady` shows the existing not-ready state and does not retry on a loop. The next edit may schedule again. A transport failure shows the existing error state and does not insert.

Closing the document, switching documents, or leaving Insert cancels the timer and dismisses the popup. Daemon restart invalidates the in-flight request through the existing connection generation. No completion work is persisted.

Two frontends editing the same document keep the controller's revision check. This milestone does not add a cross-client lock.

## 9. Compatibility and migration

No protocol change. The three explicit triggers remain, so a user who never waits out the idle still has the closed M004 behavior. There is no config to disable the idle request in this milestone. If that becomes necessary, it is a later plan, not a flag added in passing.

## 10. Required tests

### Focused unit tests

- Two edits, one request. Mode switch, zero requests. Explicit trigger supersedes the timer.

### Integration tests

- Idle response does not change document text.
- Accept of that response applies one revision-checked edit.
- Stale revision is not actionable.
- Over-budget line does not send a request.
- `Ctrl-x` in Insert still starts completion and does not insert the typed character as a swallowed binding. Confirm the existing Insert binding still owns `Ctrl-x` before adding the timer.

### Restart and recovery tests

- A connection-generation change drops the in-flight idle response.

### Contention and cancellation tests

- A second edit while a request is in flight invalidates the first response.

### Security and negative tests

- No request is sent without a bound attachment.
- The idle path does not call `LspPreviewApply` or the model-facing completion helper.

### Migration and compatibility tests

- `F8`, `:complete`, and Insert `Ctrl-x` still reach `start_editor_completion`.

## 11. Required verification commands

```bash
python3 scripts/check_tui_editor_text_authority.py
scripts/verify.sh scoped
```

Run the TUI test family that owns editor completion, from `docs/test-ownership.toml`. If a key-authority guard covers editor chords, run it. Do not add a chord that the guard does not know about in order to make the idle path reachable.

## 12. Documentation updates

- `docs/tui.md`: idle request, explicit accept, the three immediate triggers, and the interval that shipped.
- `architecture/tui.md`: the debounce is a request schedule, not an accept path.
- Do not amend the closed M004 plan. Cite it.

## 13. Acceptance criteria

- An Insert edit burst produces one completion request after the idle interval.
- The document text is unchanged when that response arrives.
- Accept still inserts exactly the approved local edit, or refuses a stale revision.
- `F8`, Insert `Ctrl-x`, and `:complete` still work.
- `NotReady` does not insert and does not spin.
- No new accept path and no `LspPreviewApply` call were added.

## 14. Stop conditions

- M001 has not closed.
- The idle path cannot call the existing completion request without a second accept path.
- Making the timer fire requires a new key, or requires taking `Ctrl-c` or a printable character.
- The bounded cursor walk cannot build a position for an ordinary line. An over-budget line stays a refusal. Do not materialize the line to schedule a request.
- ADR-0021's revision witness cannot validate the idle response. Do not ship a label-insertion fallback.

## 15. Closure evidence required

- The one-request burst test and the no-insert-on-arrival test.
- The three explicit triggers still reach the existing function.
- The interval that shipped, named.
- The text-authority guard and scoped verify, labeled local.
- A sentence that the closed M004 was not reopened.

## 16. Handoff notes

M002's status word is the right place for a not-ready idle result. If M002 has not closed, use the popup's existing warming state and do not draw a new status row.

Do not start M005 or M006 in this change. Do not edit the protected untracked scripts. Do not reopen ADR-0064.
