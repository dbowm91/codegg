# TUI editor Helix parity Milestone 017 — the code-action read

Status: ready for handoff
Repository baseline: `bb6642688` (`feat/auto-20261018-12255c45`)

Source roadmap: `plans/subsystems/tui-editor-helix-parity-roadmap.md` §7
(a new milestone added by the M017-planning amendment; see the roadmap's
Amendment A1) and §13.6 (the third wave's order)
Long-term requirements: `plans/000-long-term-specification.md` (IDE-grade
language intelligence)
Applicable ADRs: `plans/adrs/ADR-0008-lsp-preview-apply-authorization-and-runtime-ownership.md`
(the write path a code-action candidate reaches);
`plans/adrs/ADR-0012-native-lsp-read-surface-delivery-and-authorization.md`
(the read family's contract — the one whose "read-only by construction" rule this
milestone refines rather than breaks)
Primary class: capability

## 1. Objective

Give the code action a **read**, so `space a` previews instead of stating
that there is nothing to preview.

`space a` is the last leaf that names a thing it cannot do. It says "no
code-action request exists in this editor, so there is nothing to preview",
and the statement is *true* — the protocol has no operation that asks a server
what code actions it would offer. That is the whole of its reason, and the
reason is a missing read.

This milestone adds that read, and binds the leaf to it.

## 2. Why this milestone is ready

- **The server capability exists.** `egglsp` advertises
  `LspSemanticOperation::CodeAction` and gates on it
  (`crates/egglsp/src/capability.rs:409-410`).
- **The operation exists.** `code_action_summaries` and `preview_code_action`
  are implemented (`crates/egglsp/src/operations/code_actions.rs:297,346`).
- **The preview shape exists.** `CodeActionPreview` carries
  `affected_files: Vec<FileEditPreview>` — the per-file `patch` and
  `original_hash` the apply path needs (`code_actions.rs:83-98`), exactly the
  shape the rename preview uses.
- **The apply path consumes it.** `apply_preview` accepts the kind
  `code_action` (`src/lsp/mutation.rs:566-570`), and M016's bridge already
  carries a candidate of any kind.

## 3. The design, and the two decisions in it

### 3.1 One request, many previews

The code actions are fetched **once** and previewed **locally**.

The alternative — summaries first, then a second request that names the
chosen action — has a hazard the model-facing tool already carries: the
action's *index* is only meaningful for the list the server returned in that
moment. A second `textDocument/codeAction` may return a different list, and
the index would then name a different action than the user picked.

So the daemon asks once and previews each action's own `WorkspaceEdit` with
`egglsp`'s `preview_workspace_edit` — a local computation over the edit the
server already sent. No second round trip, no index to go stale, and the
candidate is computed from exactly the action the user will be shown.

### 3.2 The panel is the picker

A code action is a *choice*, and a choice needs a list. The editor's LSP
panel already has exactly that machinery: `j`/`k` move
`EditorLspState::selected_index`, and `Enter` acts on the selected item
(`handle_lsp_panel_key`, `src/tui/commands/editor.rs:2794-2835`).

So the read returns the actions, the panel lists them, and `Enter` opens the
change review on the selected action's candidate. This is the one flow in the
editor that is two steps, and it is two steps because there is something to
choose.

## 4. Invariants that must not regress

1. **The read family writes nothing.** The code-action read returns summaries
   and previews — a diff, a file list, a candidate. It never applies.
2. **A command-only action is never applied.** An action that carries a
   `Command` payload and no edit has no candidate, and the panel says so.
   `egglsp` never executes commands, and neither does this.
3. **The one apply path.** A code-action candidate reaches the apply through
   `build_apply_request` and nowhere else.
4. **A disabled action is not offered.** `CodeAction.disabled.reason` is
   carried through and shown; a disabled action gets no candidate.
5. **The read result is bounded.** The candidates in one read result share a
   total byte bound, and the daemon stops minting when it is reached — saying
   so, rather than silently offering fewer.

## 5. Scope

**In:**

- the protocol's code-action operation, payload and DTO,
- the protocol's total-candidate bound for one read result,
- `egglsp`'s warm-only code-action preview entry point,
- the daemon's arm and its candidates,
- the editor's leaf, the panel's rendering, and `Enter` opening the review,
- the docs, the CHANGELOG and the skill files.

**Out:**

- **Executing a command-only action.** The surface never executes commands.
- **Choosing an action by name.** The panel selects by position, which is
  what the panel already does.
- **A code-action *picker* as its own surface.** The LSP panel is the picker.
- **The `Ctrl-x` completion-key blind spot** — M018.

## 6. Required production changes

| Area | Change |
|---|---|
| protocol | `LspReadOperation::CodeActionPreview`; `LspReadPayloadDto::CodeActions`; `LspCodeActionDto`; the total-candidate bound |
| egglsp | `code_action_previews_for_warm_client` — one request, many local previews |
| daemon | the arm; a candidate per action that has an edit; the running total |
| TUI | the `space a` leaf becomes a read; the panel renders the actions; `Enter` opens the review on the selected action |
| components | the panel's rendering of the action list |
| docs | `docs/tui.md`, `docs/LSP.md`, `architecture/lsp.md`, `architecture/tui.md`, `CHANGELOG.md`, both skill files |

## 7. Ordered work packages

### W1 — The protocol's code-action surface

`LspReadOperation::CodeActionPreview` is a **point** operation: a code action
is about a position. `LspReadPayloadDto::CodeActions { actions }` carries
`LspCodeActionDto { title, kind, preferred, disabled_reason, has_edit,
candidate }`.

The read family is read-only **by construction**, and this milestone does
not break that rule: it adds a *preview*, which is what the format and rename
previews already are. The protocol's own note — that `CodeAction` is absent
"because its answers are edits" — is about the *raw* action, whose payload
is an edit. A preview's payload is a diff. The note is corrected in place,
not deleted.

*Acceptance:* the operation validates as a point operation; the payload
round-trips; a command-only action round-trips with no candidate.

### W2 — The total-candidate bound

One read result may carry several candidates, so the per-candidate bound is
not enough. `MAX_LSP_READ_CANDIDATE_BYTES` bounds the total patch bytes
across one result. The daemon stops minting when it is reached, and the
payload says that some actions carry no preview because of it.

*Acceptance:* the bound is enforced; the payload distinguishes "no preview
because the action has no edit" from "no preview because the result reached
its bound".

### W3 — `egglsp`'s warm-only entry point

`code_action_previews_for_warm_client(key, file_path, start, end, only,
max_actions, allowed_root)` — capability-gated, one `textDocument/codeAction`
request, then one local `preview_workspace_edit` per action that has an edit.

*Acceptance:* the warm-only invariant holds (the caller resolved the key);
the request is issued once; a command-only action produces no preview.

### W4 — The daemon's arm

The arm mints a candidate per action that has an edit, with the kind
`code_action`, the title the action carries, and a provenance naming the
action. The patch text and base hashes come from `FileEditPreview`, so the
digest binds exactly what the apply path will apply.

*Acceptance:* the minted candidate's digest is recomputable from the
candidate's own fields — the same property M016's daemon test proves for the
rename and format candidates.

### W5 — The leaf, the panel, and the review

`space a` becomes a read effect. The panel renders the actions with their
kind, whether they can be applied, and — for a disabled action — the
server's reason. `Enter` on the selected action opens the change review on
that action's candidate; an action with no candidate says why.

*Acceptance:* the leaf issues the read; the panel lists the actions; `Enter`
opens the review on the selected action's candidate; a command-only action
cannot be applied and says so.

### W6 — The wording, the docs and the guard

The panel's wording, the seven documents, and the key-authority guard's
counts — the leaf table gains no glyph, so the leaf guard's numbers do not
move, and the Normal chord count does not move either. The sweep proves both.

## 8. Failure, cancellation, restart, contention semantics

- **A server with no code-action provider** returns the capability error, and
  the panel says the server offers none.
- **A server that returns no actions** says so in the panel.
- **A cancelled read** leaves no candidate and no review.
- **A command-only action** is listed and cannot be applied.
- **A disabled action** is listed with the server's reason and cannot be
  applied.

## 9. Compatibility and migration

- The protocol change is **additive**: a new operation, a new payload
  variant, and a new DTO. An older peer that never heard of the operation
  cannot ask for it, and an older payload is still readable.
- No storage migration. No new durable state.
- No new capability flag — the code-action capability is the server's own,
  and the daemon gates on it.

## 10. Required tests

| Area | Test |
|---|---|
| protocol | the code-action operation is a point operation; the payload round-trips; a command-only action carries no candidate |
| protocol | the total-candidate bound is enforced |
| egglsp | one request, many previews; a command-only action produces none |
| daemon | the minted candidate's digest is recomputable from its own fields |
| daemon | the daemon stops minting when the result reaches its bound, and says so |
| TUI | the `space a` leaf issues the code-action read |
| TUI | `Enter` on the selected action opens the review on that action's candidate |
| TUI | a command-only action cannot be applied and says so |
| guards | the leaf and chord counts do not move |

## 11. Required verification commands

The same sweep as M016's §11, with `cargo test -p codegg --lib --locked
tui::` and the four guards.

## 12. Documentation updates

- `docs/tui.md` — `space a` in "The preview keys", and out of the table of
  leaves that cannot run.
- `docs/LSP.md` — the code action becomes a preview.
- `architecture/lsp.md` — the read family's rule, corrected in place.
- `architecture/tui.md` — the leaf's row and the panel's role.
- `CHANGELOG.md` — the `space a` change under `### Changed`.
- both skill files.

## 13. Acceptance criteria

1. `space a` asks the server what code actions it would offer.
2. The panel lists them, with their kind and whether they can be applied.
3. `Enter` on one opens the change review on that action's candidate.
4. The candidate reviewed is the candidate applied.
5. A command-only or disabled action cannot be applied, and says why.
6. The read result's candidates are bounded, and the bound is visible.

## 14. Stop conditions

Report rather than improvise if:

- the code action's edit cannot be previewed into patches and base hashes,
- the one-request design would have to execute a command,
- the panel cannot select an item without a new input mode,
- the total-candidate bound would have to be enforced by dropping a
  candidate silently.

## 15. Closure evidence required

- the one-request proof,
- the digest recomputation,
- the command-only refusal,
- the guard counts, unchanged.

## 16. Handoff notes

- The code action is the last leaf that named a thing it could not do. After
  this milestone, every leaf either answers or explains.
- The panel is the picker. A second input mode for one more choice would be
  a worse editor.
- The candidate travels the same way M016's does: carried, never registered.
