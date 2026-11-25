# TUI Editor Inline LLM Assist M001 + M002 — Closure Status

Status: closed (implementation and evidence complete; §5 records the deviations
that are recorded rather than silently absorbed, and §9 records the defects
that verification found and this change fixed)

Source implementation plans:

- `plans/implementation/tui-editor-inline-llm-assist/001-inline-prompt-generation-and-accepted-insertion.md`
- `plans/implementation/tui-editor-inline-llm-assist/002-cursor-anchored-review-menu.md`

Source subsystem roadmap:

- `plans/subsystems/tui-editor-inline-llm-assist-roadmap.md#milestone-1--the-inline-prompt-the-generation-and-the-accepted-insertion`
- `plans/subsystems/tui-editor-inline-llm-assist-roadmap.md#milestone-2--the-cursor-anchored-review-menu`

Governing ADR:

- `plans/adrs/ADR-0066-editor-inline-generation-and-pending-region-contract.md` (accepted 2026-11-24)

Repository baseline reviewed: `4e1ba94a64`
(`docs(plans): accept ADR-0066 and register the inline LLM assist line`).

Implementation commits or pull requests: none. The work is uncommitted in the
working tree at the time of this record.

## 1. Executive finding

The editor can now ask a model for an insertion at the cursor. `Ctrl+Alt+a`
opens a floating prompt field with its own Helix NOR/INS mode, its own
multi-line buffer and its own per-request model and thinking level. `Enter`
sends one bounded, tool-less request to the daemon; the daemon makes one
bounded one-shot provider call and answers with the request id, the document
identity and revision, the model, the effort label and the text, or with a
named refusal. The answer is held as a bounded frontend payload anchored at a
document byte offset, at the document's daemon revision and at the document's
identity, and the renderer draws it as a differentiated region spliced into
the line flow. The document's bytes, the file on disk, the language server and
the undo history see nothing until the operator approves. Approval submits
exactly one insertion through `submit_edit`; discard drops the payload with no
edit. A stale answer is neither drawn nor applied.

Moving the cursor onto the region's anchor raises a floating review menu
offering Approve, Deny and Edit. Its visibility is derived from the cursor's
position **and** the region's applicability every frame, never latched, and
the keys it claims are live exactly when it is drawn. It never reserves a row,
so the document does not reflow when it opens.

The whole path — the request, the provider call, the region's rendering, the
menu, the approval, the undo, and the edit verb — is verified **live** in a
terminal against a local OpenAI-compatible stub provider (§4).

## 2. Requirement-to-evidence matrix

### M001 — the inline prompt, the generation, the accepted insertion

| Exit condition | Evidence | Result | Notes |
|---|---|---|---|
| The gesture opens a field with Helix NOR/INS semantics and a multi-line buffer | `the_field_starts_in_insert_mode_and_takes_typed_text`, `enter_submits_and_shift_enter_starts_a_new_line`, `esc_steps_to_normal_mode_and_a_second_esc_dismisses`; live receipt A steps 2 and 4; live receipt B step 2 | pass | Live: the badge renders `inline [INS]` over the document without moving a row |
| Esc clears the prompt and dismisses the field | `dismissing_the_field_clears_its_instruction`; live receipt A step 4 (`the field overlay is gone`, `the instruction was cleared`) | pass | One press steps INS→NOR, the second clears and dismisses — see §5.4 |
| The request is answered by the daemon's bounded one-shot call | `src/core/daemon_editor_inline.rs`; live receipt B steps 2 and 4 | pass | The stub provider received the request and the editor reports the answer |
| The answer appears as a region and the document's bytes are unchanged | `a_current_answer_becomes_the_pending_region`, `inline_region_splices_after_its_anchor_without_changing_document_offsets`, `inline_region_height_is_clamped_to_leave_document_visible`, `a_region_that_no_longer_applies_is_neither_drawn_nor_reviewable`; live receipt B step 2 (`the generated region is drawn`, `the region is not document text on disk`) | pass | The region's bytes never enter the cursor arithmetic, the selection ranges or the undo history; the file on disk is unchanged |
| Approve produces exactly one undoable edit through the existing funnel | `an_approval_takes_the_region_and_a_discard_takes_nothing`, `an_approval_refuses_a_region_whose_revision_moved`, `an_approval_refuses_a_region_that_belongs_to_another_document`; live receipt B step 4 (`the generated region was inserted`, `the insertion is in the document`, `one undo takes the whole insertion back`) | pass | The approval is a Normal-mode verb, so one `u` takes the whole insertion back |
| Discard leaves no residue | `an_approval_takes_the_region_and_a_discard_takes_nothing`; `discard_region`; live receipt B step 3 | pass | No edit, no undo entry, no write |
| A stale answer is neither drawn nor applied | `an_answer_for_a_moved_generation_is_dropped`, `an_answer_for_a_moved_revision_is_dropped`, `a_superseded_answer_is_dropped`, `a_second_request_supersedes_the_first`, `a_region_that_no_longer_applies_is_neither_drawn_nor_reviewable` | pass | The renderer refuses a region that no longer applies |
| The text-authority guard stays green | `python3 scripts/check_tui_editor_text_authority.py` and `--self-test` | pass | One narrow amendment; 10 injected defects rejected |
| The per-request model and effort choice, defaulting to the daemon's selection | `the_field_cycles_the_model_and_the_thinking_level`; live receipt A step 2 (`model inherit`, `effort inherit`) | pass | The field holds no list of strings; the catalog is read at the keystroke |
| The model's resolution follows ADR-0066's precedence | `the_model_precedence_is_request_then_preference_then_configured` | pass | Request override > persisted principal preference > configured default; see `architecture/provider.md` |
| The daemon's bounded budgets | `an_empty_or_oversized_instruction_is_refused_before_any_provider_call`, `an_instruction_inside_the_budget_is_accepted_trimmed`, `a_context_window_over_the_budget_is_refused` | pass | Instruction 4 KiB, context 16 KiB, output 8 KiB, 60 s timeout over the whole call |

### M002 — the cursor-anchored review menu

| Exit condition | Evidence | Result | Notes |
|---|---|---|---|
| The menu appears when the cursor is inside the region and not otherwise | `the_review_menu_opens_at_the_cursor_and_closes_when_the_cursor_leaves_the_region`, `a_region_that_no_longer_applies_is_neither_drawn_nor_reviewable`; live receipt B step 2 | pass | Derived from the cursor byte every frame; never latched |
| It is anchored to the cursor's screen cell and flips rather than clipping | `the_review_menu_flips_above_the_cursor_when_the_row_below_leaves_the_area`, `the_review_menu_clamps_to_the_left_edge_rather_than_leaving_the_area`; `inline_menu_placement` | pass | The anchor is the cursor's display column on its screen row, from the editor's own viewport |
| It never reserves a row, so the document does not reflow | `inline_region_splices_after_its_anchor_without_changing_document_offsets` (`the review menu does not move the document's rows`) | pass | The menu is an overlay, like the keymap box and the completion popup |
| The menu's keys are live exactly when the menu is drawn | `inline_menu_is_open` and the renderer share one applicability predicate (`region_is_reviewable` / `applies_to`); live receipt B step 3 | pass | A menu that is not drawn claims no keys |
| Approve, Deny and Edit each produce the document-level outcome they name | live receipt B steps 3, 4 and 5 | pass | Deny drops the region and the file; Approve inserts one undoable edit; Edit re-opens the field with the previous instruction |
| Edit restores the previous instruction | `reopen_field_with`; live receipt B step 5 (`the field is open again`, `the previous instruction is restored`) | pass | An in-flight request for the discarded region is superseded |
| A document change that invalidates the region dismisses the menu | `inline_region_is_valid` gates both the region and the menu on the document's daemon revision; `an_approval_refuses_a_region_whose_revision_moved` | pass | The menu's visibility is derived, so an invalidated region cannot leave one on screen |

## 3. Production implementation evidence

| File | Change |
|---|---|
| `crates/codegg-protocol/src/core.rs` | `CoreRequest::EditorInlineGenerate`, `CoreResponse::EditorInlineGenerated`, `EditorInlineOutcome`, `editor_inline_refusal` (11 codes), the instruction and context budgets |
| `crates/codegg-core/src/authorization/policy.rs` | the `editor_inline_generate` descriptor: `DirectProject` + `FileRead`, plus a representative request |
| `crates/codegg-core/src/audit_instrumentation.rs` | the bare `editor_inline_generate` entry in `UNINSTRUMENTED_OPERATIONS` |
| `src/core/daemon_family.rs`, `src/core/daemon.rs`, `src/core/daemon_documents.rs`, `src/core/mod.rs` | the request's routing, its project scope, and its dispatch to the new arm |
| `src/core/daemon_editor_inline.rs` | the daemon's bounded one-shot call: the document check, the bounded context read, the model and reasoning resolution, the refusal matrix, and the streamed answer |
| `crates/codegg-tui/src/editor_inline_assist.rs` | the frontend state: the prompt field, the pending region, the request fence, the budget, and the region's document identity |
| `crates/codegg-tui/src/commands/editor.rs` | the chords, the field's keyboard, the request path, the fence application, approve, discard, and the menu's three verbs |
| `crates/codegg-tui/src/components/editor.rs` | the prompt overlay, the region's rendering, the review menu, and the keymap rows |
| `crates/codegg-tui/src/app/render.rs` | the connection between the editor's widget and the inline-assist state, and the region's applicability |
| `crates/codegg-tui/src/app/state/editor.rs` | a detached document clears the inline state |
| `crates/codegg-tui/src/app/commands.rs`, `crates/codegg-tui/src/runtime/command_dispatch.rs` | the answer's command and its dispatch |
| `scripts/check_tui_editor_text_authority.py` | one narrow allowance for `generated_text`, with two new self-test injections |
| `architecture/tui.md`, `architecture/provider.md`, `docs/tui.md` | the gesture, the field, the region, the fence, the approve path, the menu's geometry, the daemon's refusal matrix and the model's precedence |

## 4. Verification record

### Focused tests

- `cargo nextest run -p codegg-tui --profile ci -E 'test(/inline/) or test(/review_menu/)'`
  — **30 passed, 4623 skipped**: the field, the fence, the region, the budget,
  the region's document identity, the applicability predicate, the overlay, the
  region's splice, the region's height clamp, the generating disclosure, the
  menu's open/close, its flip and its clamp, and the stale region's refusal to
  draw.
- `cargo nextest run -p codegg --lib --profile ci -E 'test(/daemon_editor_inline/)'`
  — **8 passed, 5159 skipped**: the instruction budget, the context budget, the
  prompt's shape, the reasoning application, the character-boundary snap and
  the model's three-tier precedence.
- `cargo nextest run -p codegg-protocol --profile ci -E 'test(/inline/)'`
  — **4 passed, 345 skipped**: the request round trip, the request without a
  model or reasoning, the response round trip, and the refusal's named code.

### Static guards

- `python3 scripts/check_tui_editor_text_authority.py` — passed.
- `python3 scripts/check_tui_editor_text_authority.py --self-test` — passed; all 10 injected defects rejected, including the two added for this change (a container payload, and a payload with no declared bound).
- `python3 scripts/check_tui_editor_key_authority.py` — passed; 2 guard-block chords, 95 Normal-mode chords, 8 Insert-mode chords, no shadowed arm.
- `python3 scripts/check_tui_editor_doc_drift.py` — passed; 52 leaf gestures and 103 chords are documented, and every key declared unbound is unbound.
- `python3 scripts/check_tui_editor_apply_path.py` — passed.
- `python3 scripts/check_comment_budget.py` — passed.
- `python3 scripts/check_audit_coverage.py` — passed.
- `python3 scripts/check_work_plan_repository_binding.py` — passed.

### Formatting and linting

- `cargo fmt --all -- --check` — clean.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — see §7.

### Live receipts

Both receipts use an isolated `HOME` and an isolated `CODEGG_DATA_HOME`, a
scratch git repository, and `target/debug/codegg --standalone` in a 120×40
terminal. The editor is opened through the command palette's `/files`
navigator. Neither run touches the operator's own daemon or configuration.

**Receipt A — no provider.** The isolated environment has no provider
connection and no configured model, so the daemon refuses with
`model_unavailable`. **14 assertions, 14 passed, 0 failed:**

1. The document is rendered.
2. `Ctrl+Alt+a` opens the field; the overlay shows `inline [INS]`, `model
   inherit` and `effort inherit`.
3. The instruction lands in the field and the document is untouched.
4. `Esc` steps the badge to `inline [NOR]`; a second `Esc` removes the
   overlay and clears the instruction.
5. `Enter` submits; the editor reports `the inline generation was refused
   (model_unavailable)` and no region is produced.
6. `Ctrl+Alt+u` with no region reports that there is nothing to approve, and
   the document is still untouched.
7. `Ctrl+Alt+v` with no region is a no-op.

**Receipt B — a local stub provider.** A loopback HTTP server speaks the
OpenAI chat-completions streaming protocol and answers with one line of
text; the isolated config points the builtin `openai` provider at it. **16
assertions, 16 passed, 0 failed:**

1. The document is rendered.
2. `Ctrl+Alt+a` opens the field; the instruction lands in the field.
3. `Enter` submits; the daemon calls the provider and the editor reports
   `the model answered — review the generated region`.
4. **The generated region is drawn**, and **the file on disk is unchanged** —
   the region is not document text.
5. **The review menu is raised at the cursor**, offering `a`, `d` and `e`.
6. **Deny** drops the region: the region is gone, the menu is gone, and the
   file on disk is untouched.
7. **Approve** inserts one edit: the editor reports `the generated region was
   inserted`, the insertion is in the document, and the menu is gone.
8. **One `u` takes the whole insertion back**, and the document text returns.
9. **Edit** re-opens the field with the previous instruction: the editor
   reports `the region was discarded — edit the instruction and try again`,
   the field is open again, the previous instruction is restored, and the
   region is gone.

The stub server's log records the request the daemon actually made: one
`POST /v1/chat/completions` with the instruction and the code context, and
no tools.

### Comment budget

The manifest `docs/comment-budget.toml` is unchanged. Every new Rust file
(`crates/codegg-tui/src/editor_inline_assist.rs`,
`src/core/daemon_editor_inline.rs`) carries **zero** comment lines, and the
edited files carry no net new comment lines: the rationale lives in
`architecture/tui.md` and `architecture/provider.md`. The guard passes.

## 5. Deviations — recorded, not absorbed

### 5.1 The answer travels on the request's response, not on an event

ADR-0066 and the roadmap say the daemon answers with a new event. The
implementation answers the **request** with `CoreResponse::EditorInlineGenerated`.

The reason is delivery, and it is structural rather than stylistic: the
editor route has no session, the daemon's event log is session-scoped
(`event_log.publish(session_id: Option<String>, …)` and a filter that needs
`include_global`), and every other editor document flow is request/response
(`DocumentOpen` → `CoreResponse::DocumentSnapshot`, `DocumentChange` →
`CoreResponse::DocumentChanged`). A session-less event would have needed a
new global-event path for one caller.

The Decision's substance is unchanged: one-shot request, no turn, no session,
no tool loop, and a payload carrying the request id, the document identity and
revision, the model, the effort label and the text. Only the channel differs.
The ADR was **not** amended; this record and `architecture/tui.md` state the
deviation. The owner decided on 2026-11-25 to **leave it recorded here**
rather than amend ADR-0066: the Decision's substance is unchanged and only
the delivery channel differs, so amending an accepted ADR would be a larger
step than the change warrants.

### 5.2 The review menu is placed after the region, not on the cursor's next row

The M002 plan says the menu is "drawn below and to the right of the cursor".
The region is spliced into the line flow immediately below the cursor's row,
so a menu on the cursor's next row covers the payload it exists to review.
The menu is therefore placed **after the region's own rows**: it keeps the
cursor's display column, so it stays anchored to the cursor, and it leaves the
generated text readable.

This is the one place where the implementation follows the plan's *intent* (a
cursor-anchored floating menu that never reflows the document) over its
letter. The flip and the clamp are implemented and unit-tested as the pure
function `inline_menu_placement`. The menu is an overlay and covers document
pixels, exactly like the editor's existing floating surfaces.

### 5.3 The plan's "flip above" case is reached through the region's clamp

Because the region's height is reserved from the viewport, the row after the
region usually fits. The flip is reached when the region is clamped to the
area's height, and the flip is covered by a unit test on the placement
function rather than by a full-frame rendering test.

### 5.4 The field's `Esc` is a two-step Helix mode transition

ADR-0066 describes the prompt as having "an `Esc` that clears and dismisses
it". The implementation steps `Esc` from insert mode to the field's normal
mode first, and a second `Esc` clears the instruction and dismisses the
field.

The reason is the same requirement the roadmap states in the same breath: the
field has **Helix NOR/INS semantics**. In Helix, `Esc` is the mode step, and
without it the field's normal-mode verbs — the motions, the selection, the
word operations — would be unreachable and the field would not be a Helix
surface at all. The M001 plan's own acceptance table says the same thing:
"one press steps INS→NOR, the second clears and dismisses".

The ADR was **not** amended. Whether its one-sentence description should be
rewritten to name the two presses is an owner decision.

## 6. What was NOT verified

- **A real provider.** The live run answers from a local stub that speaks the
  OpenAI chat-completions streaming protocol. The request's shape, the
  daemon's resolution, the refusal matrix and the answer's handling are
  exercised end to end, but no real credential was used and no real
  provider's streaming quirks were seen.
- **`provider_error` and `timeout` live.** Both refusals are covered by the
  daemon's unit tests and by the code's own bounds; neither was provoked in
  a live terminal.
- **A hosted CI run on the exact SHA.** No commit was made, so there is no
  SHA to run hosted CI against.
- **The desktop frontend.** Out of scope by the roadmap.

## 7. Status of the broad gates

- `cargo clippy --workspace --all-targets --locked -- -D warnings` — **clean**.
  The only warnings are the workspace's pre-existing manifest warnings
  (unused workspace dependency `codegg-sdk`, unused per-crate dependencies);
  no lint fires on this change.
- `cargo nextest run --workspace --locked --profile ci --no-fail-fast` —
  **17788 tests run, 17788 passed, 11 skipped, 1 leaky** in 399 s. The leaky
  test is a nextest annotation about a test that leaves a thread running, not
  a failure.
- `cargo fmt --all -- --check` — **clean**.

The sweep ran to completion with `--no-fail-fast`, so the result is a
property of the whole tree rather than of the first failure. An earlier sweep
of this tree aborted on `codegg::agent_loop agent_loop_harness::
test_task_tool_denied_tools_passthrough` — a timeout-sensitive harness test
that passes in isolation and that touches none of the changed code. It did
not recur; the machine was shared with another workspace's test run at the
time, and that load is the likeliest cause.

## 8. Handoff notes

- The two new state modules are self-contained: `editor_inline_assist.rs`
  holds no `Vec<String>` field (the text-authority guard forbids it) and
  reads the model and effort catalogs at the moment of the keystroke.
- The daemon's refusal codes are stable and named in
  `codegg_protocol::core::editor_inline_refusal`; a frontend that meets an
  older daemon sees `document_unimplemented` as a typed error rather than
  silence.
- The pending payload is frontend state and is not durable. A daemon restart
  discards it, which is the stated non-durability of an unapproved insertion
  and not a defect.
- The region's revision is the **daemon's** revision, not the frontend's local
  buffer counter. They are different counters with different lifetimes, and
  mixing them retires every region at once. `architecture/tui.md` records
  why.

## 9. Defects that verification found

The first implementation passed every unit test and the no-provider live
receipt. The stub-provider receipt and an independent review of the diff
found five defects, all of which this change fixed.

| Defect | How it was found | Consequence | Fix |
|---|---|---|---|
| The revision fence compared the daemon's revision against the frontend's **local buffer** revision | The stub receipt: the region was never drawn, the menu never appeared, and the approval refused with `revision 1 to 0` | The feature's whole answer path was dead in a real editor | The region, the renderer, the menu and the approval all read the daemon's revision (`attachment_info().revision`), which is the counter the daemon itself compares |
| The review menu's **keys** were live while the menu was **not drawn** | The stub receipt: `d` discarded a region the reader could not see | An invisible menu claimed `a`, `d` and `e` | One applicability predicate (`region_applies_to` / `region_is_reviewable`) now drives the renderer, the key handler and the approval |
| A pending region could cross a **document boundary** | An independent review of the diff | A region generated in one document could be approved into another with a matching revision | The region carries its document id, a detached document clears the inline state, and the answer is fenced against the editor's **current** generation |
| The 60-second timeout covered only **stream creation** | An independent review of the diff | A provider that answered promptly and then stalled hung the request forever | The timeout now wraps the whole call — opening the stream and draining it |
| An approval entered **Insert** mode, so `u` typed a `u` instead of undoing | The stub receipt: `one undo takes the whole insertion back` failed | The roadmap's "one `u` takes the whole insertion back" did not hold, and the editor's two modes could disagree | The approval is a Normal-mode verb, exactly like a paste: `stay_insert` is false, so the editor stays in Normal and the insertion is its own undo step |
