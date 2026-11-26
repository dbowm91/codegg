# TUI Editor Inline LLM Assist Roadmap

Status: closed (M001 and M002 closed; the closure record is
`plans/closure/tui-editor-inline-llm-assist/001-status.md`)

Long-term references:

- `plans/000-long-term-specification.md#42-explicit-ownership`
- `plans/000-long-term-specification.md#44-frontends-render-projections`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/000-long-term-specification.md#25-tui-target-behavior`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/001-terminology-and-domain-model.md#6-session-and-interaction-terms`

Related ADRs:

- `plans/adrs/ADR-0066-editor-inline-generation-and-pending-region-contract.md` (accepted 2026-11-24; gates every milestone in this line)
- `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md` (accepted; not renegotiated here)
- `plans/adrs/ADR-0021-native-editor-completion-read-and-local-edit.md` (accepted; the local-edit funnel this line reuses)
- `plans/adrs/ADR-0049-open-provider-scoped-reasoning-control-vocabulary.md` (accepted; the vocabulary the override carries)
- `plans/adrs/ADR-0050-per-model-reasoning-capability-authority.md` (accepted; the capability the override is admitted against)

Research artifact: `research/tui-editor-inline-llm-assist-2026-11-24.md`

Closed predecessors, not reopened:

- `plans/subsystems/tui-editor-ide-presence-roadmap.md` — closed. This line does not reopen its milestones and does not touch the open deadline, the status cluster's server words, or the semantic-token read.
- `plans/subsystems/tui-editor-helix-ux-improvement-roadmap.md` — closed. This line adds a surface beside the editor, not a change to the Helix verbs it delivered.
- `plans/subsystems/tui-editor-helix-capability-parity-roadmap.md` — active. This line does not bind any chord it owns and does not take a letter from its tables.
- `plans/subsystems/editor-document-foundation-roadmap.md` — document ownership stays there.
- `plans/subsystems/provider-reasoning-control-roadmap.md` and `plans/subsystems/provider-reasoning-admission-corrective-addendum.md` — the reasoning vocabulary and admission policy stay there.

Baseline: `b864c78a1f7f725335e8b19588b0fbff7208d60a` on
`feat/auto-20261109-f8abad0d-artifact-safe`. Re-inspect HEAD before each
implementation.

## 1. Purpose and ownership boundary

**This subsystem owns** the editor's inline-assist surface: the gesture that
opens it, the prompt field and its keyboard semantics, the request that asks a
model for an insertion at the cursor, the pending region that shows the answer
before it is accepted, and the review actions that accept, discard or retry it.

**It consumes** the daemon's provider connections, the model profile's
per-model reasoning capability, the editor's document controller, and the
editor's existing local-edit funnel.

**It must not own** the provider, the credential, the model selection, the
reasoning vocabulary, the document's text authority, the file's write path, or
any turn. ADR-0011 and ADR-0049 are not renegotiated here. The daemon's
command catalog and the editor's key tables are not extended by this line
beyond the one gesture and the two field verbs it names.

The canonical long-term specification is **silent** on inline AI assistance.
This roadmap's contract is therefore **interim** under
`plans/003-planning-process.md` §5 until an owner decision promotes it. It
does not amend `plans/000-`.

## 2. Work classification

### Invariants

- The editor holds no second copy of the document, and the generated payload
  is not document text until it is approved.
- The daemon owns the provider, the credential and the model. The frontend
  names a model and never names a provider, an endpoint or a secret.
- A generated insertion reaches the document only through
  `DocumentController::apply_local`, and therefore only through the editor's
  existing edit funnel, undo history and writer authority.
- The request runs with no tools.
- One keyboard axis: the prompt field's mode is a property of the field, not a
  second axis beside `ui_state.input_mode`.
- One spinner and one activity cadence. The "generating" state is content in
  the editor's status cluster, not a new indicator.
- A stale answer is never drawn and never applied.
- The chord family is `{CONTROL, ALT}` only.

### Capabilities

- The operator asks the model for an insertion at the cursor from inside the
  editor, in a prompt field with Helix NOR/INS semantics and multi-line input.
- The prompt field can choose a model and a thinking level for that request
  only, defaulting to the daemon's current selection.
- The answer appears as a differentiated region in the buffer, with a visible
  state while it is being written.
- Moving the cursor into the region raises a menu at the cursor offering
  Approve, Deny and Edit.
- Approve brings the generated text in as one undoable edit; Deny removes it;
  Edit removes it and re-opens the prompt for another attempt.
- Moving the cursor out of the region dismisses the menu.

### Infrastructure

- A one-shot generation request on the `CoreRequest` wire, and its response
  event.
- The daemon's bounded one-shot provider call.
- The editor's inline-assist state: the pending payload, its anchor, its
  revision and its request fence.
- One narrow amendment to the text-authority guard, with a self-test.

### Polish

- Streaming the answer into the region as it arrives.
- A per-request model and thinking level indicator on the prompt field.
- The desktop frontend consuming the same request and event.

Infrastructure and polish are not the capability. M001 is not done because a
request variant exists; it is done when an operator can ask for an insertion
and accept it.

## 3. Non-goals

- A chat, a session, a transcript, or any use of `TurnSubmit` for an inline
  insertion.
- Tool access, retrieval, or any capability the request does not need.
- Editing a selection instead of inserting at the cursor.
- Multi-file edits, multi-region answers, or a diff view for the generated
  text.
- Code actions, refactors, renames, or a code-action menu; `space a` stays
  the language server's.
- A model picker that is not the field's own per-request choice.
- Changing the daemon's stored model selection or its stored reasoning
  selection.
- A second apply path beside `build_apply_request`.
- A second document buffer, a frontend filesystem read, or a frontend
  language-server spawn.
- The desktop frontend's surface.
- Any change to the editor's Helix verbs, to the `/` search line, or to the
  `:` command line's encoding behaviour. The `:` and `/` arms' narrow
  modifier match is recorded in the research artifact as a finding for a
  separate corrective line; this line does not widen it.

## 4. Current state

The editor is a real document surface. `open_editor_in` navigates to
`Route::Editor` and issues one `DocumentOpen`; `apply_editor_opened`
installs the snapshot. The presentation holds no text: it reads the
controller's snapshots and applies edits through `submit_edit` ->
`DocumentController::apply_local` -> `DocumentChange` -> `DocumentSave`.
The chrome row is one row; the path is on the left and the status cluster on
the right. The `:` command line and the `/` search line own the keyboard
while they are open. The keymap box, the jumplist picker, the LSP panel and
the completion popup are floating overlays painted after the document.

There is no way to ask a model for anything from the editor. `TurnSubmit`
requires a session; the editor route has a document and no session. The daemon
does perform one-shot provider calls — `generate_title_via_provider` and
`llm_summarize` — but neither is exposed on the wire. The wire's per-request
model and reasoning override exist on `TurnSubmit`, which is bounded by the
session's durable model selection and writes into the session transcript.

Two facts from the live probe shape the ergonomics. First, a printable the
editor does not claim does **not** reach the composer on the editor route:
`composer_refuses_bare_text()` is true there, so a prompt field cannot lean
on the composer and must own the keyboard as its own surface. Second, the
editor's `:` and `/` arms accept only an unmodified key, so a terminal that
reports the shifted spelling loses them — a new gesture must be spelled so its
encoding is unambiguous, which in practice means a `{CONTROL, ALT}` chord.

## 5. Target architecture

The daemon gains a bounded one-shot generation request. The frontend sends the
operator's instruction, the chosen model, an optional reasoning override and
the editor context; the daemon answers with a new event carrying the request
id, the document identity and revision, the model, the effort label and the
generated text.

The editor holds the answer in an inline-assist payload: a bounded text, a
document byte offset, the document revision it was asked against, and the
request id. The renderer draws that payload as a differentiated region spliced
into the line flow at the anchor. The document's bytes are untouched, so the
language server, the file on disk and the undo history see nothing until the
operator approves.

Approval submits exactly one insertion edit through the editor's existing
funnel. Discard drops the payload. Edit drops it and re-opens the prompt with
the previous instruction.

The prompt field is its own surface with its own NOR/INS mode, its own text,
and its own per-request model and effort choice. It is not the composer and it
does not share the composer's claims; it is a sibling of the `:` modal line
with a larger vocabulary.

## 6. Dependency graph

```text
ADR-0066 (accepted)
    |
    `--> M001 the inline prompt, the generation, and the accepted insertion
              |
              `--> M002 the cursor-anchored review menu
```

| Edge | Class | Why |
|---|---|---|
| ADR-0066 -> M001 | hard | M001 implements that Decision and no other option. The request shape, the payload's ownership and the per-request override are all decided there. |
| M001 -> M002 | hard | The menu is a surface over M001's payload and M001's actions. Without a payload there is nothing to review, and without the actions there is nothing for the menu to offer. |

No milestone depends on a live TUI receipt from another.

## 7. Milestones

### Milestone 1 — The inline prompt, the generation, and the accepted insertion

Class: capability

Objective: the operator asks the model for an insertion at the cursor from
inside the editor, sees the answer as a differentiated region, and accepts it
as one undoable edit or discards it.

Dependencies: ADR-0066, accepted 2026-11-24.

Deliverable boundary: the gesture, the prompt field, the per-request model and
effort choice, the request and the response event, the daemon's one-shot call,
the pending region and its rendering, and the approve and discard actions.
**Not** the cursor menu and **not** the Edit action.

User or operator value: a model can write into a file the operator is reading,
under the operator's review, without a chat session and without touching the
transcript.

Exit conditions: the gesture opens a field with Helix NOR/INS semantics and a
multi-line buffer; Esc clears the prompt and dismisses the field; the request
is answered by the daemon's bounded one-shot call; the answer appears as a
region and the document's bytes are unchanged; approve produces exactly one
undoable edit through the existing funnel; discard leaves no residue; a stale
answer is neither drawn nor applied; the text-authority guard stays green.

Deferred work: the cursor menu (M002), streaming the answer, the desktop
consumer.

### Milestone 2 — The cursor-anchored review menu

Class: capability

Objective: moving the cursor into a pending region raises a menu at the cursor
offering Approve, Deny and Edit, and moving the cursor out of it dismisses the
menu.

Dependencies: M001 closed.

Deliverable boundary: the cursor-anchored floating menu, the derived
visibility rule, and the Edit action. It adds **no** new lifecycle action;
Approve and Deny are M001's actions reached through a second surface, and Edit
is one new action that discards the region and re-opens the prompt with the
previous instruction.

User or operator value: the decision to accept generated code is made where the
generated code is, rather than by a verb the operator has to remember.

Exit conditions: the menu appears when the cursor is inside the region and not
otherwise; it is anchored to the cursor's screen cell and flips rather than
clipping at the text area's edges; it never reserves a row, so the document
does not reflow when it opens; Approve, Deny and Edit each produce the
document-level outcome they name; Edit restores the previous instruction; a
document change that invalidates the region dismisses the menu.

Deferred work: a menu for a multi-region answer; the region's own scroll.

## 8. Cross-cutting requirements

### Storage and migration

None. No schema change. No `STORAGE_LAYOUT_VERSION` bump. The pending payload
is frontend state and is not durable: an unapproved insertion has no durable
meaning, and a daemon restart discards it. That is stated in the ADR and
repeated in the plan, so it is a fact rather than a surprise.

### Protocol and compatibility

The request and the response event are additive. A daemon that does not know
the request answers `unsupported`, and the frontend renders that as an
explicit refusal rather than as silence. The response event carries no field
an older frontend interprets.

The request carries a model and an optional reasoning override. The daemon
resolves the effective selection as explicit request override > persisted
principal preference > agent/model-profile default, which is the precedence
ADR-0049 and the turn path already implement, and admits it against the
model's capability through the same policy the turn path uses.

### Security and authorization

The request runs in the daemon and uses the daemon's provider connection and
credential. The frontend names a model only. The request runs with no tools.
The editor's context is taken from the open document under its existing scope
and containment; no filesystem path travels on the wire. The request is
bounded by an instruction budget, a context budget, an output budget and a
timeout.

### Concurrency, cancellation, and recovery

One outstanding inline request per editor. A newer request supersedes the
older one. Closing the document, switching documents, or leaving the route
cancels the outstanding request. A late answer is dropped by the request-id
and document-generation fence the editor already uses for completions. The
daemon owns the provider call's timeout; the frontend owns the cancellation.

### Observability and audit

The request is a provider call and is visible in the existing provider
observability. No new audit event is required. The plan must not log document
text, and the generated payload is document-adjacent content: it is not
written to a log.

### Documentation and operations

Each milestone updates the architecture page that owns the contract it changed
(`architecture/tui.md`, `architecture/provider.md`), and `docs/tui.md` gains
the gesture when the code binds it. The editor's own keymap overlay gains the
gesture in the same change, because the doc-drift guard compares the two.

## 9. Verification strategy

Each milestone names a regression that failed on the behavior it replaces: a
prompt field that cannot be moved around in, a generated payload that reaches
the document before it is accepted, a stale answer that is applied anyway, a
menu that opens when the cursor is outside the region, and a region that
displaces the document's rows without the scroll following.

`scripts/check_tui_editor_text_authority.py` runs for every milestone that
touches the editor, with its amended allowance and its self-test.
`scripts/check_tui_editor_key_authority.py` runs for the new chord, after the
guard is taught the new dispatch shape. `scripts/check_tui_editor_doc_drift.py`
runs for the documented gesture. `scripts/verify.sh scoped` is the sweep. No
milestone claims a hosted CI run that was not run, and no milestone claims a
live TUI receipt it did not take.

## 10. Risks and decision points

- **ADR-0066 is accepted** (2026-11-24), on Option A: the pending region that
  is not document text, and a new one-shot request rather than a turn. M001
  implements that Decision and no other option. Option B is rejected, not
  deferred; a later change of mind is a new ADR that supersedes it.
- **The region is not document text, and the renderer must draw it.** The
  visible-line computation changes. The scroll offset stays in document lines,
  so the scroll itself is unaffected, but the row flow is not, and the
  editor's closed draw-path milestone assumed one row per logical line. A
  region that is taller than the remaining text area needs a bounded fallback.
- **The text-authority guard needs one amendment.** It must stay narrow, and
  its self-test must prove the amended guard still rejects a real second
  document buffer.
- **A chord that is not `{CONTROL, ALT}` is undeliverable.** The app never
  pushes keyboard-enhancement flags, so `Ctrl+Shift+X` is indistinguishable
  from `Ctrl+X`. The gesture is a `{CONTROL, ALT}` chord, and the letters
  already taken are `b d e f n o p r s t x y`; `i`, `j` and `m` are
  undeliverable because their bytes are Tab and Enter.
- **The editor's key table has no `CONTROL|ALT` arm today.** The new shape
  must be taught to the key-authority guard before it lands, and the guard
  must treat an arm it cannot resolve as an error rather than as a pass.
- **The per-request model is not the session's durable selection.** That is
  the point, but it means the request's model is refused rather than
  substituted when the operator names a model the connection cannot serve.
- **The canonical specification is silent.** This line's contract is interim.
  Promoting it into `plans/000-` is an owner decision and is not this
  roadmap's work.

## 11. Completion definition

The roadmap is complete when M001 and M002 are closed and:

- an operator can ask the model for an insertion at the cursor from inside the
  editor, in a prompt field with Helix NOR/INS semantics, multi-line input,
  and an Esc that clears and dismisses it;
- the prompt field can choose a model and a thinking level for that request
  only, and neither choice changes the daemon's stored selection;
- the answer appears as a differentiated region with a visible state while it
  is being written;
- the document's bytes, the file on disk and the language server see nothing
  until the operator accepts;
- accepting produces exactly one undoable edit through the editor's existing
  funnel;
- moving the cursor into the region raises a menu at the cursor with Approve,
  Deny and Edit, and moving out of it dismisses the menu;
- a stale answer is neither drawn nor applied;
- the text-authority guard is green, including its amended allowance.

A green `cargo check`, a compiled request variant, or a commit message is not
this definition.

## 12. Milestone status table

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| 1 | closed | `plans/implementation/tui-editor-inline-llm-assist/001-inline-prompt-generation-and-accepted-insertion.md` | `plans/closure/tui-editor-inline-llm-assist/001-status.md` | none |
| 2 | closed | `plans/implementation/tui-editor-inline-llm-assist/002-cursor-anchored-review-menu.md` | `plans/closure/tui-editor-inline-llm-assist/001-status.md` | none |
