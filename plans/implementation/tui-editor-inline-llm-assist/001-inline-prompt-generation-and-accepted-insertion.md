# TUI Editor Inline LLM Assist Milestone 001 — The inline prompt, the generation, and the accepted insertion

Status: ready for handoff

Repository baseline: `b864c78a1f7f725335e8b19588b0fbff7208d60a` on
`feat/auto-20261109-f8abad0d-artifact-safe`

Applicable decision: `plans/adrs/ADR-0066-editor-inline-generation-and-pending-region-contract.md`,
**accepted 2026-11-24**. This milestone implements that Decision and no other
option.

Source roadmap:

- `plans/subsystems/tui-editor-inline-llm-assist-roadmap.md#milestone-1--the-inline-prompt-the-generation-and-the-accepted-insertion`

Long-term requirements:

- `plans/000-long-term-specification.md#42-explicit-ownership`
- `plans/000-long-term-specification.md#44-frontends-render-projections`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#6-session-and-interaction-terms`

Applicable ADRs:

- `plans/adrs/ADR-0066-editor-inline-generation-and-pending-region-contract.md` (accepted 2026-11-24; this milestone implements its Decision)
- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md` (accepted)
- `plans/adrs/ADR-0021-native-editor-completion-read-and-local-edit.md` (accepted)
- `plans/adrs/ADR-0049-open-provider-scoped-reasoning-control-vocabulary.md` (accepted)
- `plans/adrs/ADR-0050-per-model-reasoning-capability-authority.md` (accepted)

Primary class: capability

Evidence: `research/tui-editor-inline-llm-assist-2026-11-24.md`

## 1. Objective

The operator can ask the model for an insertion at the cursor from inside the
editor, in a prompt field with Helix NOR/INS semantics, and accept the answer
as exactly one undoable edit — or discard it, leaving no trace.

## 2. Why this milestone is ready

- **ADR-0066 is the only hard dependency, and it is accepted** (2026-11-24).
  This milestone is ready: it needs no closed predecessor, because the
  editor, the document controller, the local-edit funnel, the provider
  connections and the reasoning capability registry all exist and are closed
  lines.
- **Interface dependencies are stable.** The editor's local-edit funnel
  (`submit_edit` -> `DocumentController::apply_local` -> `DocumentChange` ->
  `DocumentSave`) is the accepted ADR-0021 path. The reasoning vocabulary and
  the admission policy are the accepted ADR-0049 and ADR-0050 contracts. The
  daemon's one-shot provider-call shape is the existing
  `generate_title_via_provider` implementation, not a new idea.
- **The editor's dispatch and rendering surfaces are known.** The live probe
  and the source map establish the key chain, the overlay order, the chrome
  row and the span layers.

## 3. Current implementation evidence

- The editor route is `Route::Editor`. `App::on_key` consults it after the
  modals, the tasks overlay, the timeline, command mode, the completion
  popup, the change review and the focused panel, and **before** the app's
  global binding map and the composer's claim table.
- `handle_editor_key` wraps `dispatch_editor_key`, which dispatches in this
  order: the file tree, the jumplist picker, the LSP panel, the completion
  popup, then Esc, then the buffer. `handle_normal_key` resolves the pending
  prefix first, then matches on an exact `(modifiers, code)` pair; the table
  has `NONE`, `SHIFT`, `ALT`, `ALT|SHIFT` and bare `CONTROL` arms and
  **no `CONTROL|ALT` arm today**.
- `EditorMode` is `Normal | Insert | Select`; `EditorState::mode()` reads
  the presentation's mode, and the app's `ui_state.input_mode` is the single
  keyboard axis (`buffer_focused(mode)` is `mode == Insert && is_open()`).
- The editor's presentation holds no text. `TuiDocumentPresentation` holds a
  cursor byte, selections, the undo history and the modal lines; the document
  text lives in the controller's snapshot.
- `line_spans` resolves four layers — syntax, `/` search, selection and
  cursor — and the cursor is painted last.
- `render` paints the chrome, the text, the command matches, the keymap
  overlay, the LSP panel and the completion popup, in that order.
- The composer's `ComposerText` owns a buffer, a caret, a selection, word
  motions, newline insertion and a bounded undo history. The composer's
  claim table is a **separate** authority and does not apply to the editor.
- On the editor route in Normal mode a printable the editor does not claim
  does **not** reach the composer: `composer_refuses_bare_text()` is true
  there, and the app's `InputAction::Char` arm turns such a character into a
  colon-prefixed command-palette query.
- `generate_title_via_provider` is the daemon's existing one-shot provider
  call: one `ChatRequest`, `tools: None`, a bounded output budget, a
  bounded timeout, and `provider.stream` collected to `Finish`.
- The completion flow is the editor's async precedent: a scope of document id,
  revision and cursor, a registered task, a completion command, and a state
  that rejects a mismatched request id, scope, epoch or loading state.
- The editor bumps its own generation on a document switch, and
  `apply_editor_opened` refuses a result whose generation is stale.

## 4. Invariants that must not regress

- No second document buffer, no frontend filesystem read, no frontend
  language-server spawn. `scripts/check_tui_editor_text_authority.py` stays
  green, with the one narrow amendment this milestone makes and its self-test.
- Every edit reaches the document only through `submit_edit` and
  `DocumentController::apply_local`.
- The editor route keeps exactly one keyboard axis. The prompt field's mode is
  a property of the field.
- The editor's Helix verbs, the `:` line, the `/` search line and the
  existing overlays keep their behavior.
- One spinner, one activity cadence. The "generating" state is content in the
  editor's status cluster.
- No provider, endpoint, credential or absolute path crosses the wire from
  the frontend.
- The daemon's stored model selection and its stored reasoning selection are
  unchanged by a request that carries its own choices.
- The chord family is `{CONTROL, ALT}` only; no `CONTROL|SHIFT` binding is
  added anywhere.
- The comment budget ratchet is not re-banked by this milestone; rationale
  lives in `architecture/*.md`.

## 5. Scope

### In scope

- One new Normal-mode gesture in the editor that opens the prompt field.
- The prompt field: its own surface, its own NOR/INS mode, its own text, its
  own per-request model and thinking-level choice, and an Esc that clears the
  text and dismisses the field.
- The new `CoreRequest` variant and the new `CoreEvent` variant.
- The daemon's bounded one-shot generation, its refusals, and its per-request
  model and reasoning resolution.
- The editor's inline-assist state: the pending payload, its anchor, its
  document revision and its request fence.
- The pending region's rendering, its differentiated style and its visible
  "generating" state.
- The approve action (one insertion edit) and the discard action.

### Explicitly out of scope

- The cursor-anchored review menu and the Edit action. They are M002.
- Streaming the answer into the region as it arrives.
- The desktop frontend.
- Any change to the `:` or `/` arms' modifier matching. The narrow match
  is a recorded finding for a separate corrective line, named in the research
  artifact.
- Any change to the daemon's stored selections.
- A model picker that is not the field's own per-request choice.
- Editing a selection, replacing a range, or generating for a selection.

## 6. Required production changes

### Core/domain

- The daemon gains one bounded one-shot generation operation, owned by the
  daemon. Its shape follows `generate_title_via_provider`: it resolves the
  model against the connection's catalog, resolves the effective reasoning
  selection, builds a `ChatRequest` with `tools: None`, a system prompt that
  names the insertion contract, a bounded output budget and a bounded
  timeout, and collects `ChatEvent::TextDelta` until `Finish`.
- The daemon reads the **context** from its own document replica. The request
  carries the document identity, the anchor and the context extent; it does
  **not** carry document text, and the frontend does not read the file to
  supply it.
- The effective reasoning selection is resolved as explicit request override >
  persisted principal preference > agent/model-profile default, and is
  admitted against the model's capability through the same policy the turn
  path uses. The request's override never mutates the stored selection.

### Storage and migrations

None. No schema change, no `STORAGE_LAYOUT_VERSION` bump. The pending
payload is frontend state and is not durable.

### Protocol and DTOs

- One `CoreRequest` variant, additive, carrying: the project and workspace
  identity, the document id and the document revision the request was asked
  against, the insertion anchor, the context extent, the instruction, the
  optional model, and the optional reasoning override.
- One `CoreEvent` variant, additive, carrying: the request id, the document
  id and revision, the model, the effort label actually used, and the
  generated text.
- The request is classified in the authorization policy with its own
  capability, and it is a **read-and-generate** effect: it consumes provider
  budget but mutates no workspace state. It must not be classified as a
  writer, and it must not be admitted by the write path.
- A daemon that does not know the request answers `unsupported`.

### Runtime and concurrency

- The frontend issues the request through the registered-task pattern: spawn a
  registered task, answer with a completion command, and apply the completion
  only when the request id and the editor's generation both match.
- One outstanding inline request per editor. A newer request supersedes the
  older one, and the older one's completion is dropped.
- The document's generation counter fences the answer: a document switch
  invalidates an outstanding request, and an answer for a superseded revision
  is neither drawn nor applied.

### Frontend or operator surface

- **The gesture.** A `{CONTROL, ALT}` chord in the editor's Normal table.
  The app's existing chord letters are `b d e f n o p r s t x y`, and `i`,
  `j` and `m` are undeliverable because their control bytes are Tab and
  Enter, so a free letter is available; the plan's intent is `Ctrl+Alt+A`
  ("ask"). The implementer may choose another free letter, and must record
  the choice. The chord must be bound in the editor's Normal table **only** —
  it is an editor verb, not an app-level action — and the key-authority guard
  must be taught the new `CONTROL|ALT` shape before the arm lands.
- **The prompt field.** A floating overlay drawn over the text area, in the
  same manner as the keymap box and the completion popup. It must **not**
  reserve a row: an allocated row changes the text area's height, which
  re-resolves the viewport and can move the cursor's screen row under the
  reader's first keystroke.
- **The field's keyboard.** Its own Normal and Insert modes with Helix
  semantics — word and line motions, selection, select mode — and a
  multi-line buffer. It is a sibling of the `:` modal line with a larger
  vocabulary, and it does **not** share the composer's claim table: on the
  editor route the composer refuses bare text, so the field must own the
  keyboard outright.
- **Esc.** One press clears the field's text and dismisses the field, and the
  editor's keyboard returns in Normal mode. The operator's wording admits a
  two-step reading; this plan takes the single-press reading, because a
  surface that needs two presses to leave is the dead-key shape the editor's
  own rules reject.
- **The per-request choices.** The field carries an optional model and an
  optional reasoning override. Both default to *inherit*, which is what the
  daemon's stored selection already means. The model's ladder is the one the
  daemon publishes for that model, so the field's choices are bounded by the
  same capability the `/models` box reads.
- **The pending region.** The renderer draws the payload as a differentiated
  region spliced into the line flow at the anchor. The region's bytes are not
  document bytes: the cursor's byte arithmetic, the selection ranges, the
  search matches and the document's own undo history must not see them. The
  region's height reduces the number of document lines the viewport can show,
  and the scroll offset stays in document lines so the scroll itself is
  unaffected.
- **The "generating" state.** Content in the editor's status cluster, in the
  transient-disclosure slot the cluster already has. No new spinner, no new
  cadence.
- **Approve.** One insertion edit at the anchor, submitted through
  `submit_edit`, so the editor's undo, cursor and history semantics apply
  unchanged. The anchor is revalidated against the document's current
  revision first; an anchor that no longer resolves refuses the approval with
  a stated reason.
- **Discard.** Drop the payload. No edit, no undo entry, no write.

### Security and authorization

- The request runs in the daemon against the daemon's provider connection and
  credential. The frontend names a model; it never names a provider, an
  endpoint or a secret.
- A model the connection cannot serve is **refused**, naming the model. It is
  never silently substituted.
- The request runs with no tools. It cannot read the filesystem or run
  commands.
- The instruction, the context extent, the output budget and the timeout are
  all bounded.
- The daemon logs neither the instruction's surrounding document text nor the
  generated text.

### Documentation and static guards

- `scripts/check_tui_editor_text_authority.py` is amended with **one**
  narrow allowance for the bounded pending payload, and its self-test is
  extended to prove the amended guard still rejects a real second document
  buffer and rejects an over-budget payload.
- `scripts/check_tui_editor_key_authority.py` is taught the editor's
  `CONTROL|ALT` dispatch shape, and an arm it cannot resolve must remain an
  error rather than a pass.
- `scripts/check_tui_editor_doc_drift.py` must pass with the new gesture
  documented in `docs/tui.md` and listed in the editor's keymap overlay.
- `architecture/tui.md` gains the editor's inline-assist contract;
  `architecture/provider.md` gains the request's model and reasoning
  resolution. Neither may be duplicated into this plan.

## 7. Ordered work packages

### Work package A — The wire contract

Intent: the request and the response exist, are authorized, and are refused
cleanly, before any UI depends on them.

Required changes:

- The `CoreRequest` and `CoreEvent` variants, with their DTOs and their
  serde shapes. The request is additive and carries no field an older daemon
  misreads.
- The authorization descriptor for the request, with its own capability and
  its own `unsupported` answer for a daemon that does not know it.
- The daemon's dispatcher arm, answering `unsupported` until the operation
  is wired.

Acceptance evidence: a protocol round-trip test that serializes and
deserializes both variants; a dispatcher test that an un-wired daemon answers
`unsupported` rather than hanging or dropping the request.

### Work package B — The daemon's bounded one-shot generation

Intent: the daemon can answer the request with generated text, or refuse it.

Required changes:

- The operation described in §6. It resolves the model against the
  connection's catalog, resolves the effective reasoning selection, builds the
  `ChatRequest`, calls the provider with no tools, and collects the text.
- The refusal matrix: an unknown model, a model the connection cannot serve,
  a reasoning label the model's capability does not admit, an over-budget
  instruction, an over-budget context, a provider error, an empty answer, and
  a timeout. Each produces a distinct, named outcome.
- The answer is empty-checked: an empty generation is a failure, not an empty
  insertion.

Acceptance evidence: a daemon test per refusal, each asserting the named
outcome and asserting that **no** answer is produced for it; a test that the
request's override does not change the stored selection; a test that a request
with no override inherits the stored one.

### Work package C — The frontend's request path and its fence

Intent: the editor can issue the request and can only ever apply an answer
that is still current.

Required changes:

- The completion command pair, the registered task, and the application of
  the completion.
- The fence: the request id, the editor's document generation and the
  document revision the request was asked against. A completion that fails
  any of them is dropped, and the drop is reported rather than silent.
- The outstanding-request bookkeeping, including cancellation when the
  document closes, the document changes, or the route is left.

Acceptance evidence: a test that a completion for a superseded request id is
dropped; a test that a completion for a superseded document generation is
dropped; a test that a completion for a superseded document revision is
dropped; a test that a second request supersedes the first.

### Work package D — The prompt field

Intent: the operator can open the field, type an instruction with Helix
semantics, choose a model and a thinking level for this request only, and
dismiss the field.

Required changes:

- The field's state: its text, its caret, its selection, its mode, its
  per-request model choice and its per-request reasoning choice.
- The gesture, bound in the editor's Normal table.
- The field's own key handling, with the Helix motions, the selection verbs
  and the multi-line buffer, and the Esc that clears and dismisses.
- The field's rendering as a floating overlay, with its mode badge, its
  per-request choices, and its own scroll when the instruction is longer than
  the field.
- The "generating" state in the editor's status cluster while the request is
  outstanding.

Acceptance evidence: a rendering test that the field paints its text, its
caret and its mode badge, and that the document's rows are unchanged when the
field opens; a test that the caret can move within and across the field's
lines; a test that Esc clears the text and dismisses the field in one press;
a test that the field's mode is independent of the app's `input_mode`.

### Work package E — The pending region

Intent: the answer is drawn as a differentiated region that is visibly not
document text.

Required changes:

- The inline-assist state on the editor: the bounded payload, the anchor, the
  document revision and the request id.
- The renderer's region layer, spliced into the line flow at the anchor, with
  a style that is distinct from the selection, the search match and the
  syntax, and that never hides the cursor.
- The viewport's visible-line computation, reduced by the region's height.
- The bounded fallback for a region taller than the remaining text area.

Acceptance evidence: a rendering test that the region is drawn at the anchor
and that the document's lines below it are displaced by exactly the region's
height; a test that the cursor remains visible inside the region; a test that
the region's bytes do not appear in the document's byte arithmetic — the
selection range, the search matches and the undo history are unaffected; a
test that the region's payload over its budget is refused by the state, not
drawn.

### Work package F — Approve and discard

Intent: the operator accepts the answer as one edit, or drops it.

Required changes:

- The approve action: revalidate the anchor against the current revision,
  build one insertion edit, submit it through `submit_edit`, and clear the
  payload only when the submission is accepted.
- The discard action: drop the payload, with no edit and no write.
- The region's lifecycle: a payload is invalidated by a document change that
  moves its anchor, and an invalidated payload is not drawn and not applied.

Acceptance evidence: a test that approve produces exactly one edit with the
generated text at the anchor, that the edit lands through `submit_edit`, and
that the editor's history holds one entry for it; a test that a refused
submission leaves the payload in place and the document unchanged; a test that
discard produces no edit; a test that an anchor invalidated by an intervening
edit refuses the approval with a stated reason.

### Work package G — The guards, the docs and the keymap

Intent: the change is discoverable and the guards police it.

Required changes:

- The text-authority guard's one narrow amendment and its extended self-test.
- The key-authority guard's new dispatch shape.
- The editor's keymap overlay row for the new gesture.
- `docs/tui.md`'s entry for the new gesture, in the same change, because the
  doc-drift guard compares the two.
- `architecture/tui.md`'s and `architecture/provider.md`'s accounts of the
  new contract.

Acceptance evidence: each guard's self-test, and a `verify.sh quick` run that
is green.

## 8. Failure, cancellation, restart, and contention semantics

- **A refused request** produces a named refusal in the field and the status
  cluster. The field keeps the operator's instruction so it can be retried.
  Nothing is written.
- **A provider error** produces a named failure. The field keeps the
  instruction. Nothing is written.
- **An empty answer** is a failure, not an empty insertion.
- **Cancellation.** Closing the document, switching documents, or leaving the
  editor route cancels the outstanding request. A late answer is dropped by
  the request-id and generation fence. The drop is reported in the status
  cluster rather than being silent, because a silent drop reads as a hang.
- **Restart.** The daemon's timeout bounds the provider call. A daemon restart
  discards the request; the frontend's generation fence drops any answer that
  arrives afterwards. The pending payload is frontend state and is **not**
  restored by a daemon restart; that is a stated fact, not a defect, because
  an unapproved insertion has no durable meaning.
- **Contention.** One outstanding request per editor. A second request
  supersedes the first; the first's answer is dropped. The daemon serves the
  requests independently; the provider's own rate limits are the provider's
  business, and a rate-limited refusal is surfaced as such.
- **A document change while a request is outstanding** invalidates the
  answer. The answer is dropped and the region is never drawn. The operator is
  told the document changed.
- **An approve that races an edit** revalidates the anchor against the
  current revision first and refuses with a stated reason if it no longer
  resolves. It does not guess an offset.

## 9. Compatibility and migration

No storage migration. The request and the event are additive. A daemon that
does not know the request answers `unsupported` and the frontend renders that
as an explicit refusal. A frontend that does not know the event ignores it.

The request's model and reasoning override are new fields on a new variant, so
no existing request's shape changes and no existing client's parsing changes.

## 10. Required tests

### Focused unit tests

- The prompt field's text model: insert, delete, newline, caret motions, word
  motions, selection growth and drop, undo and redo.
- The field's mode transitions and the Esc that clears and dismisses.
- The per-request model and reasoning choices, including the inherit default.
- The inline-assist state's payload budget, its anchor, and its invalidation
  rule.
- The region's style resolution, distinct from the selection, the search
  match and the syntax.
- The daemon's refusal matrix, one case per named outcome.

### Integration tests

- The editor route: the gesture opens the field; the field's text is typed,
  moved around in and submitted; the request is issued; the answer is drawn;
  approve produces one edit; discard produces none.
- The full daemon path: a real provider stub answers the request and the
  editor renders the region.
- The wire round trip: the request reaches the daemon and the event reaches
  the editor.

### Restart and recovery tests

- A daemon restart while a request is outstanding discards the answer and the
  editor reports it.
- A document switch while a request is outstanding drops the answer.
- The pending payload is not restored after a daemon restart, and the editor
  says so rather than showing an empty region.

### Contention and cancellation tests

- A second request supersedes the first and the first's answer is dropped.
- A completion for an unknown request id is dropped.
- A completion for a superseded document generation is dropped.
- A completion for a superseded document revision is dropped.

### Security and negative tests

- The request carries no provider, endpoint, credential or absolute path.
- The request runs with no tools: the provider request carries `tools: None`.
- The daemon's provider call carries no document text beyond the bounded
  context extent it read itself.
- A model the connection cannot serve is refused and not substituted.
- An over-budget instruction, context, output and timeout are each refused.
- The generated text and the instruction are absent from the logs.

### Migration and compatibility tests

- A daemon that does not know the request answers `unsupported`.
- An older frontend's event parsing is unaffected by the new event.

## 11. Required verification commands

```bash
# focused tests first
cargo nextest run -p codegg-core --profile ci -E 'test(/inline_assist/) or test(/model_profile/)'
cargo nextest run -p codegg-tui --profile ci -E 'test(/inline_assist/) or test(/editor/)'

# static guards
python3 scripts/check_tui_editor_text_authority.py --self-test
python3 scripts/check_tui_editor_key_authority.py --self-test
python3 scripts/check_tui_editor_doc_drift.py
python3 scripts/check_comment_budget.py

# formatting and linting
cargo fmt --all -- --check
CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets --locked -- -D warnings

# broader suite appropriate to the change
scripts/verify.sh scoped
```

Do not claim a command in the closure record that was not actually run. Record
a live `tui-test` run separately from the automated suite, and label it.

## 12. Documentation updates

- `architecture/tui.md` — the editor's inline-assist contract: the gesture,
  the field's ownership and keyboard model, the pending region and its
  relation to document text, and the approve and discard actions.
- `architecture/provider.md` — the request's model and reasoning resolution
  and the fact that the request's override never mutates the stored selection.
- `docs/tui.md` — the new gesture, in the same change as the binding.
- The editor's keymap overlay — the new gesture's row.
- The skill's editor section, if a rule changed that a future editor change
  must know.

## 13. Acceptance criteria

- An operator presses the gesture in an open document and a prompt field
  appears over the text, with a visible mode badge, and the document's rows
  do not move.
- The operator types a multi-line instruction, moves the caret within and
  across the field's lines with Helix motions, selects text and deletes it.
- Esc clears the instruction and dismisses the field in one press, and the
  editor's keyboard is back in Normal mode.
- The operator can choose a model and a thinking level for this request only;
  both default to the daemon's current selection, and neither choice changes
  the daemon's stored selection.
- On submit, the editor's status cluster shows that a generation is
  outstanding.
- When the answer arrives, it is drawn as a differentiated region at the
  anchor; the document's bytes are unchanged; the language server has not
  been told about it; the file on disk has not changed.
- Approving produces exactly one undoable edit through the editor's existing
  funnel, and one `u` takes the whole insertion back.
- Discarding leaves the document, the history and the file untouched.
- A stale answer is neither drawn nor applied, and the editor says so.
- The text-authority guard, the key-authority guard and the doc-drift guard
  are green, and each amended guard's self-test proves it still rejects the
  defect it exists to catch.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- **ADR-0066 is superseded.** This milestone implements that Decision and no
  other option. A superseding ADR changes the pending-region ownership and
  this plan does not apply as written.
- The pending payload cannot be drawn without a second document buffer or a
  second copy of the text. That is the defect ADR-0011 forbids, and the answer
  is to report it, not to widen the guard.
- The text-authority guard's amendment cannot be made narrow, or its
  self-test cannot prove the amended guard still rejects a real second
  document buffer.
- The region's height cannot be reconciled with the viewport without changing
  the scroll's meaning, or the closed draw-path milestone's one-row-per-line
  assumption proves load-bearing in a way this milestone may not change.
- The key-authority guard cannot resolve the new `CONTROL|ALT` dispatch
  shape. An arm the guard cannot resolve is an error, and the guard is taught
  the shape before the shape lands.
- The daemon cannot issue the request without a session, or without binding
  the request's model to the session's durable selection. That would make the
  request a turn, which this milestone refuses.
- A refusal cannot be produced for any of the named outcomes in the refusal
  matrix.
- The work would expand into the desktop frontend, into the `:`/`/`
  encoding correction, or into the editor's Helix verbs.

## 15. Closure evidence required

The closure record must contain:

- The requirement-to-evidence matrix for every exit condition in the roadmap's
  Milestone 1.
- The command-by-command verification record, with local and hosted results
  labelled truthfully, and the nextest counts.
- The three guard runs, including each amended guard's self-test output.
- The live `tui-test` record: the gesture, the field's Helix semantics, Esc,
  the region, the approve and the discard, against an isolated daemon with an
  isolated `HOME` and an isolated `CODEGG_DATA_HOME`, and the confirmation
  that the operator's own daemon and configuration were untouched.
- The measured refusal matrix, one line per named outcome.
- The before-and-after measurements of the comment budget, with the manifest
  unchanged.
- An explicit statement of what was **not** verified.

## 16. Handoff notes

- **Re-inspect HEAD before starting.** The baseline is recorded above; the
  editor's files are large and active.
- **Do not re-bank `docs/comment-budget.toml`.** Put the rationale in
  `architecture/*.md` and shorten the inline comments instead.
- **The probe setup is reproducible.** An isolated `HOME`, an isolated
  `CODEGG_DATA_HOME` (which appends a `codegg/` segment), `--standalone`,
  and a scratch Git repository. The operator's own daemon must not be touched,
  and the temporary directories must be removed afterwards.
- **The gesture's encoding is the first thing to get right.** A bare shifted
  punctuation character is undeliverable, and the editor's own `:` and `/`
  arms are a live example of that failure.
- **The renderer is the risky part.** Work package E is where a regression
  would be hardest to see; give it its own focused rendering tests before
  touching the approval path.
- **Do not start a session turn to implement this.** The daemon's one-shot
  provider call is the whole point of ADR-0066.
