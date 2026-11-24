# Editor inline LLM assistance — research artifact

Date: 2026-11-24
Baseline: `b864c78a1f7f725335e8b19588b0fbff7208d60a` (branch `feat/auto-20261109-f8abad0d-artifact-safe`)
Scope: read-only investigation plus one live `tui-test` probe. No product code was changed.

This artifact is the evidence behind
`plans/subsystems/tui-editor-inline-llm-assist-roadmap.md` and
`plans/adrs/ADR-0066-editor-inline-generation-and-pending-region-contract.md`.
It is written for an implementation agent that has no access to the
git-ignored `agentfindings.md`.

## 1. The requested behaviour

A Normal-mode gesture in the TUI editor opens a small prompt. The operator
types an instruction ("write a function"), presses Enter, and the prompt UI
collapses so the operator can move around the file. The generated text appears
as a differentiated region. Moving the cursor into that region raises a menu at
the cursor offering Approve, Deny and Edit. Approve brings the generated text
in; Deny removes it and the menu disappears; moving the cursor out of the
region also dismisses the menu. Edit removes the text and re-opens the prompt
for another attempt. The prompt field carries Helix NOR/INS semantics, moves
around, accepts multi-line and long input, and Esc clears the prompt and
dismisses the field. The field can select a model and a thinking level
separately from the main composer, defaulting to the current ones.

## 2. The substrate that exists

### 2.1 The editor surface

`crates/codegg-tui/src/components/editor.rs` renders, in order: the chrome
row, the document, the `:` command-match overlay, the keymap/jumplist box, the
LSP panel, and the completion popup. `CHROME_ROWS = 1`; the top border carries
no label. The chrome row holds the path on the left and a status cluster on the
right ("modified", "synced lsp...", "[project]", "writable").

The `:` command line and the `/` search line replace the chrome row while open
and own every key while active (`commands/editor.rs:4844-4963`). They are the
closest existing precedent for "a small input that takes the keyboard".

### 2.2 One keyboard axis

`ui_state.input_mode` (Insert/Normal) is the only mode axis. `EditorMode::Select`
is a verb mode on top of it. `EditorState::buffer_focused(mode)` is
`mode == Insert && is_open()` (`app/state/editor.rs:404-406`).

### 2.3 Prompt-text machinery

`crates/codegg-tui/src/composer_text.rs` — `ComposerText` owns the buffer, the
caret, the selection, the word motions, newline insertion and a bounded undo
history. `crates/codegg-tui/src/composer_claims.rs` is the guard-policed
authority for the composer's Helix NOR/INS verbs.

Three precedents show how a surface that cannot own a text buffer borrows the
composer's:

- `PromptState::chat_seed` (`app/state/prompt.rs:68`) — a surface that cannot
  own a text buffer arms a *seed*; the composer still holds the text and the
  next submit is retargeted.
- `TaskModelChoice` (`app/state/work_orders.rs:793`) — a per-surface model
  choice that falls back to the current selection.
- `ComposerMode` (`components/prompt.rs:48`) — a surface-local mode the app
  has no word for.

### 2.4 The LLM call

`src/context/compaction.rs:323-470` — `llm_summarize` is a daemon-side
one-shot: build a `ChatRequest` (`tools: None`, a system prompt,
`max_tokens`, `reasoning_effort`, `thinking_budget`), call
`provider.stream(&request)`, collect `ChatEvent::TextDelta` until `Finish`,
with a 120s timeout. No session, no turn, no tool loop.

`src/agent/title.rs:200-248` — `generate_title_via_provider` is the closer
precedent and states its own rationale: it is "the `title` agent's contract
executed inline rather than as a dispatched sub-agent loop (which would need
scheduler admission and a result round-trip for a single short string)". Bounded
output budget, short timeout, `tools: None`, and it takes `provider` and
`model` directly.

### 2.5 The write path

`build_apply_request` (`crates/codegg-tui/src/commands/change_review.rs:72-126`)
is the single builder for `CoreRequest::LspPreviewApply`, guarded by
`scripts/check_tui_editor_apply_path.py`. The request carries patch text and
`original_hash`, not a position. The daemon applier
(`src/lsp/mutation.rs:377-603`) validates the request shape, confines paths,
rejects duplicate affected paths, locks clean document paths, recomputes and
checks the preview digest, takes the repository lock, rechecks the checkout
reservation under it, rejects dirty managed LSP documents, and compares each
file's current hash with the patch's `original_hash` before writing atomically.
On success it persists an edit checkpoint, publishes `FileChanged` and syncs
the language-server service.

The editor's own mutation path is separate: `DocumentController::apply_local`
-> `DocumentChange` -> `DocumentSave`. Accepted edits flow through
`submit_edit`, which updates the cursor, the selections and the history only
on acceptance.

### 2.6 Positional insertion is expressible upstream

`crates/egglsp/src/edit.rs:253-298,424-456` — `build_file_preview` takes LSP
`TextEdit`s, and `apply_text_edits` converts line/UTF-16 positions to byte
offsets; a zero-width range is an insertion. The gap is only that the
preview-apply protocol carries a patch rather than a position.

### 2.7 Per-call model and reasoning

`CoreRequest::TurnSubmit` carries `model: String` and
`reasoning: Option<ReasoningOverride>` (`crates/codegg-protocol/src/core.rs:3020-3043`).
The daemon resolves the effective selection as **explicit turn override >
persisted principal preference > agent/model-profile default**
(`src/core/daemon_turns.rs:139-165`) and admits it against the model's
capability. `apply_reasoning_override`
(`src/agent/request_preparation.rs:785`) writes it onto the `ChatRequest`.
The TUI sends `reasoning: None` today so the daemon's stored choice wins.

The turn path bounds the request-level model: the daemon classifies the
**durable** session selection first (`src/core/daemon_turns.rs:435-515`). A
`Live` selection resolves the turn provider against the submitted model while
keeping the pinned connection; an `UnknownModel` durable selection **refuses
the turn** whatever the client sent; `Unselected`, `ConnectionMissing` and
`RevisionMoved` leave the submitted string as the identity. A one-shot request
that carries its own model does not pass through turn admission at all.

### 2.8 The async precedent

The completion flow builds a scope (document id, revision, cursor as an LSP
line/UTF-16 column), spawns a registered task, and answers with
`TuiCommand::EditorCompletionFinished`. The completion state rejects a
mismatched request id, scope, epoch or loading state
(`crates/codegg-tui/src/editor_completion.rs:119-174`). The editor also has its
own generation counter, bumped on a document switch, and `apply_editor_opened`
refuses a result whose generation is no longer current
(`commands/editor.rs:207-230, 600-642`).

## 3. The gaps

1. **No one-shot LLM request exists on the `CoreRequest` wire.** About 120
   variants were scanned; the LLM-bearing ones are `TurnSubmit`,
   `SessionPromptSubmit`, `SessionPromptSubmitWithHumanShellPromotions` and
   `SessionCompact`. `SessionCompact` answers with a session id and a turn
   flag, not generated prose. `llm_summarize` and
   `generate_title_via_provider` are daemon-internal and never exposed.
2. **The response event is session-scoped.**
   `CoreEvent::TurnTextDelta { session_id, turn_id, delta }`
   (`crates/codegg-protocol/src/core.rs:4779`). A one-shot answer needs its own
   event shape.
3. **The preview-apply path refuses a dirty document.** The editor's buffer is
   dirty whenever the operator has typed, so an approval cannot ride
   `LspPreviewApply` while the buffer is dirty. The editor's own
   `DocumentChange` path is the one that can carry an edit into an open
   document.
4. **There is no cursor-anchored overlay.** The completion popup is centred in
   the editor area; the keymap box is anchored to the text area's bottom-right
   corner. The editor's cursor cell is computed per row inside `line_spans`
   (`components/editor.rs:2146-2266`); `composer_geometry::VisualLayout` is
   **not** used by the editor renderer, so a cursor-anchored menu needs the
   editor's own viewport plus display-width arithmetic (tabs, wide characters,
   horizontal scrolling) and must flip above or below at the edges.
5. **A generated region is a new span layer.** `line_spans` resolves syntax,
   search, selection and cursor. A pending generated region is a fifth layer
   that must not bury the cursor.
6. **A region that is not document text changes the row layout.** The editor
   draws one screen row per logical line. A pending region of N lines spliced
   into the flow displaces the lines below it, so the viewport and scroll
   arithmetic must account for it. The scroll offset stays in document lines, so
   the scroll itself is unaffected, but the visible-line computation is not.

## 4. Live findings

One `tui-test` run, with an isolated `HOME`, an isolated
`CODEGG_DATA_HOME`, `--standalone` and a scratch Git repository. The
operator's own daemon and configuration were untouched, the scratch file was
never saved, and the temporary directories were removed afterwards.

- The editor opens in **Normal**; the badge row reads `[NOR] [build][LLM]`.
- The chrome row shows `src/main.rs` on the left and `synced lsp... [project]
  writable` on the right; a dirty buffer adds `modified`.
- `space ?` opens the keymap box as a floating overlay in the text area's
  bottom-right corner.
- The `:` command line takes the chrome row and its match list floats on the
  row above it.
- **Encoding gap.** `key press ':'` — SHIFT plus the colon — does **not** open
  the command line: the character became document text and the editor dropped
  into Insert mode. The raw byte (`write ':'`) opens it correctly. The arms
  are `(m, KeyCode::Char(':')) if m == none` (`commands/editor.rs:6525`) and
  the same for `/` (`:6540`). `register_name_char` in the same file already
  accepts both spellings, so the codebase knows about the two encodings; the
  two modal lines never adopted the rule. **A new gesture must be spelled so
  the encoding is unambiguous: a `Ctrl+Alt` chord or a `space` leaf, never
  bare shifted punctuation.**
- In the editor's **Normal** mode a printable that no editor verb claims does
  **not** reach the composer. `composer_refuses_bare_text()` is true on
  `Route::Editor` when the buffer is unfocused (`app/mod.rs:9772-9776`), and
  the gate turns such a character into a colon-prefixed command-palette query
  (`app/mod.rs:4532-4540`). A prompt field therefore cannot lean on the
  composer: it must own the keyboard as a distinct surface.
- Escape needed **two** presses to leave Insert in the live probe; the first
  press was absorbed by the composer branch (`app/mod.rs:4365`) before the
  editor's own Esc arm ran. Recorded as observed, not fully explained.

## 5. The chord family

The app's binding map expresses multi-modifier chords as
`KeyModifiers::CONTROL | KeyModifiers::ALT`. The letters already taken are
`b d e f n o p r s t x y`. `Ctrl+Alt+I` is unusable — the byte is `0x09`,
which arrives as `Tab` — and `Ctrl+Alt+J`/`Ctrl+Alt+M` are unusable for
the same reason (`0x0a`/CR is Enter). The editor's own Normal-mode table
(`handle_normal_key`) has **no** `CONTROL|ALT` arm today, so a chord there is
a new dispatch shape and
`scripts/check_tui_editor_key_authority.py` must be taught it before the shape
lands.

## 6. What the plans must not do

- Do not give the editor a second text buffer or a second copy of the
  document. `scripts/check_tui_editor_text_authority.py` stays green, or is
  amended with one narrow, self-tested allowance for a bounded pending-region
  payload.
- Do not start a session turn for an inline insertion. It would hijack the
  session transcript and inherit the session's durable model selection.
- Do not write to the file outside `DocumentController::apply_local`.
- Do not introduce a second apply path beside `build_apply_request`.
- Do not add a second spinner or a second activity cadence.
- Do not use a `Ctrl+Shift` chord; that encoding is indistinguishable from the
  bare `Ctrl` twin in a terminal that has not enabled keyboard enhancement.
