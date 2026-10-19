# TUI editor Helix parity Milestone 015 — LSP preview leaves

Status: implemented - closed 2026-10-19
Repository baseline: `a8778e87c` (`feat/auto-20261018-12255c45`)

Source roadmap: `plans/subsystems/tui-editor-helix-parity-roadmap.md` §7
(M015, a new milestone added by the M010-planning amendment)
Long-term requirements: `plans/000-long-term-specification.md` (IDE-grade
language intelligence); `plans/002-long-term-roadmap.md` (editor capability
sequencing)
Applicable ADRs: `plans/adrs/ADR-0011-editor-document-ownership-and-frontend-replication.md`;
`plans/adrs/ADR-0008-lsp-preview-apply-authorization-and-runtime-ownership.md` (accepted — owns the
LSP write path this milestone deliberately does not reach; see §17.15)
Primary class: capability

## 1. Objective

Give the two **already-delivered** LSP previews a key.

M013 shipped `LspReadOperation::FormatPreview` and
`LspReadOperation::RenamePreview` as read-only operations, with daemon
dispatch, warm-client plumbing, protocol bounds and panel rendering. **No
key reaches either of them.** They are capabilities a reader cannot invoke.

This milestone adds:

| Chord | Helix's chord | Effect |
|---|---|---|
| `space r` | `space r` = `rename_symbol` (an LSP **write**) | open the rename **preview** for the symbol under the cursor |
| `=` | `=` = format (an LSP **write**) | show the **format preview** for the document |

and leaves `space a` (code action) **unbound with a visible reason**, because
no code-action *read* exists in the protocol to preview.

## 2. Why this milestone is ready

- **The reads are delivered and tested.** M013's closure records *"Format /
  rename preview | **delivered as read-only previews**"*
  (`plans/closure/tui-editor-helix-parity/013-status.md:434`), and the
  panel renders both (`src/tui/components/editor.rs`).
- **A preview is a read.** The roadmap's M010 boundary gates `space r` /
  `space a` / `=` on *"an LSP write authorization ADR"* — that gate is about
  the **apply**, not the preview. A preview has no write in it, so it needs
  no ADR. **Corrected while implementing — see §17.15:** ADR-0008 *is*
  accepted and owns the write path; the gap is the bridge, not the ADR.
- **The chords are free.** `space r` is not in `SPACE_LEAVES`
  (`src/tui/editor.rs:400-478`), and `grep -n "Char('=')"` finds **no**
  `=` binding in `src/tui/commands/editor.rs` or `src/tui/input.rs` — the
  chord is unbound in the editor route and in both global keymaps.
- **The argument prompt has a precedent.** M008 reused the `:` command line
  for its regex-verb argument rather than adding a second input mode, and
  its closure called that a win. The rename preview needs a **new name**,
  and the same mechanism serves it.

## 3. Current implementation evidence

Verified by reading the tree at `a8778e87c` with the M013 work applied.

### 3.1 The two operations exist and are reachable only by accident

`crates/codegg-protocol/src/lsp.rs`:

| Item | Line | Note |
|---|---|---|
| `LspReadOperation::FormatPreview` | `:122` | **file-scoped** — `is_file_scoped` (`:158`) |
| `LspReadOperation::RenamePreview` | `:125` | **point** — `is_point` (`:145`) |
| `capability()` | `:186-187` | `FormatPreview` → `documentFormattingProvider`; `RenamePreview` → `renameProvider` |
| `LspReadRequestDto::new_name` | `:288` | `Option<String>`, `#[serde(default)]`, **required** for `RenamePreview` (`:326-336`), **rejected on every other operation** (`:341` → `LspReadInvalid::IncoherentFields`) |
| `MAX_LSP_NEW_NAME_BYTES` | — | 200; an over-long name is `LspReadInvalid::NewNameTooLong` (`:333`) |
| `LspFormatPreviewDto` | `:431` | the format diff |
| `LspRenamePreviewDto` | `:462` | `new_name` + `files: Vec<LspRenameFileDto>` |

`LspReadResultDto` has `FormatPreview(LspFormatPreviewDto)` (`:522`) and
`RenamePreview(LspRenamePreviewDto)` (`:524`) payload arms, and the editor's
panel renders both.

**Neither operation is named in `src/tui/`.** `grep` for
`LspReadOperation::FormatPreview` and `LspReadOperation::RenamePreview`
across `src/tui/` returns **no** construction site: there is no leaf, and
no request. The daemon can answer both; the TUI never asks.

### 3.2 The space leaves today

`SPACE_LEAVES` (`src/tui/editor.rs:400-478`) is `y k s S d h f F` — `r` is
**not** among them, and no `space` leaf carries a *typed argument*.

### 3.3 The argument prompt precedent

`EditorCommandName::ALL` (`src/tui/editor.rs:1140-1153`) has twelve
entries, of which four take an argument: `Open`, `SplitRegex`,
`KeepRegex`, `RemoveRegex` (`takes_argument()`, `:1129-1134`).
`parse_editor_command(line) -> Option<(EditorCommandName, Option<String>)>`
(`:1171`) parses the line. One list drives the palette, the parser and the
help (`:1049-1053`).

M008's closure records the decision: the multi-selection regex verbs reused
the `:` command line for their argument **rather than adding a new input
mode**, and that was called a win.

### 3.4 The rename target

`LspReadOperation::RenamePreview` is a **point** operation, so the daemon
resolves the position and the new name. The symbol under the cursor is
already resolved elsewhere in the editor: `space S` (workspace symbols) uses
the token under the cursor, resolved by the App
(`src/tui/commands/editor.rs:2145-2152`).

### 3.5 Containment is already enforced

M013's closure records that the rename preview's targets are re-checked with
`workspace_relative_target` and that `MAX_LSP_RENAME_EDITS` marks the
result `truncated`. A preview that named a file outside the workspace
would be a read that reports a path the editor cannot open — and M013
already refuses it.

## 4. Invariants that must not regress

1. **No LSP write path exists.** A preview reads and displays; it applies
   nothing. `M013`'s acceptance criterion 7 — *"No LSP **write** path
   exists … `space r`/`space a`/`=` unbound"* — is **deliberately changed by
   this milestone**, and the change is to *unbinding*, not to adding a
   write. The criterion's substance survives: **there is still no write
   path**, and the closure must say so explicitly.

   > **Corrected while implementing — see §17.15.** The wording above says
   > "no LSP write path exists", and the plan's §2 and §5 said the gate was
   > "an LSP write authorization ADR" that "does not exist". **Both are
   > imprecise, and the code says so.** ADR-0008 is **accepted** and owns the
   > LSP write path: `CoreRequest::LspPreviewApply`, authorized
   > `via_session` + `file.modify`, applying a *registered, still-current
   > reviewable preview* by id through the change-review surface or the
   > model-facing preview-apply tool. What the editor's chords produce is a
   > **read result**, not such an artifact — no preview id, no patches, no
   > `register_preview_artifact` in the editor path, and no reference to
   > `change_review` in `src/tui/commands/editor.rs` or `src/tui/editor.rs`.
   > So the invariant that actually holds, and that the closure states, is:
   > **the editor's preview chords cannot reach the apply path, and they
   > apply nothing** — the gap is the **bridge**, not an ADR.
2. **The protocol's `new_name` coherence rule holds.** `new_name` is
   present only on a `RenamePreview` request and absent everywhere else
   (`crates/codegg-protocol/src/lsp.rs:337-341`).
3. **The daemon's capability check is unchanged.** `capability()` maps
   `FormatPreview` → `documentFormattingProvider` and `RenamePreview` →
   `renameProvider`; a server without the capability answers
   `LspReadStatus::Unsupported` (`:186-187`), which the panel renders as a
   named missing capability.
4. **Containment holds.** No preview may name a target outside the
   session's workspace.
5. **A preview changes nothing.** The document, the undo history and the
   selection are untouched by a preview.
6. **The pending effect is drained on the next statement**, and a document
   change clears it.
7. **The bounds hold**: `MAX_LSP_NEW_NAME_BYTES` (200),
   `MAX_LSP_RENAME_FILES` (100), `MAX_LSP_RENAME_EDITS` (1000),
   `MAX_LSP_FORMAT_DIFF_CHARS` (64 Ki).

## 5. Scope

### In scope

- `space r` → the rename **preview**, with the new name collected through
  the `:` command line.
- `=` → the **format preview**.
- `space a` → unbound, with a **visible reason** (no code-action read
  exists).
- The `EditorCommandName` entry the rename prompt needs, and the palette's
  help text.
- The doc surfaces in §12.

### Out of scope, each with a named owner

| Deferred | Why | Owner |
|---|---|---|
| **Applying** a rename, a format, or a code action | The editor's preview is not the *registered reviewable artifact* ADR-0008's accepted write path consumes; a preview is a read | A future milestone that builds the **bridge** (§17.15) |
| A **code-action** preview | No code-action read exists in the protocol — the server capability is there, but nothing in `LspReadOperation` asks for it | A protocol extension, then a leaf |
| A **diagnostic-driven code action** | Same as above | Same |
| `space r` as an *apply* | See above | The bridge's milestone |
| A **rename preview that spans a workspace** | The daemon already bounds the file list and the edit count; widening them is not a leaf concern | — |

### The divergence this milestone records

**`space r` and `=` are previews, not actions.** Helix's `space r` renames
and Helix's `=` formats. CodeGG's versions **show what would change and
apply nothing**. This is M013's established principle — *"Previews are
previews"* — and it is the reason the milestone is safe to land before the
bridge exists.

**The transition is a user-visible change and is registered as one.** When the
bridge is built, `space r` and `=` become the **apply** step, and the chord's
meaning changes. The roadmap's M010/M015 entries and the `CHANGELOG.md` must
record that this transition is expected, so the bridge's milestone does not
surprise a reader who learned the preview. **See §17.15** for the corrected
ADR framing this replaces.

## 6. Required production changes

### 6.1 The rename prompt

`space r` needs the **new name** before it can ask anything, because
`LspReadRequestDto::new_name` is required for `RenamePreview` and rejected
elsewhere (`:326-341`).

The prompt reuses the `:` command line:

- `EditorCommandName` gains a `RenamePreview` variant that
  `takes_argument()` reports true for. It appears in the palette's help
  (`:1107-1122`) with its argument shape.
- `space r` **arms** the `:` line with that command pre-selected, so the
  reader types the new name and presses `Enter`.
- Submitting the line issues `LspReadOperation::RenamePreview` with
  `new_name = Some(<typed>)`.
- An **empty** name is refused before any request is issued (the protocol
  would reject it as `LspReadInvalid::MissingNewName` at `:327-330`; the
  editor must not spend a round trip on it).
- A name longer than `MAX_LSP_NEW_NAME_BYTES` (200) is refused locally, with
  the bound named. The protocol enforces it too (`:333`), but the editor
  should not send a request it knows will be rejected.

**Arming the `:` line is the M008 precedent, not a new mechanism.** No new
input mode is introduced.

### 6.2 The format preview

`=` → `EditorPendingEffect::Read(LspReadOperation::FormatPreview)`.

- `FormatPreview` is **file-scoped** (`is_file_scoped`, `:158`), so the
  request is about the open document, not a position.
- It takes **no argument**, so `=` is a single key with no prompt.
- The server capability is `documentFormattingProvider` (`:186`); a server
  without it answers `Unsupported`, and the panel names it.
- The result is a **diff**, and the panel renders it (M013 delivered the
  rendering).

### 6.3 `space a`

Helix's `space a` is `code_action` — an LSP **write** (applying a code
action), and CodeGG has no code-action *read* either. There is nothing to
preview.

`space a` is therefore **bound to a visible statement** — the M013
`space F` precedent — saying that no code-action request exists in this
editor. It is **not** bound to a preview, because there is no preview to
show.

### 6.4 The leaf actions

Both new leaves ride the mechanism M013 built:

- `space r` → a new pending effect that **arms the rename prompt** (not a
  read: the read follows the prompt).
- `=` → `PrefixAction::Effect(EditorPendingEffect::Read(LspReadOperation::FormatPreview))`
  — the existing shape, unchanged.

If M010 has already generalized the pending effect into
`EditorPendingEffect` (M010 §6.1), the new arm is added there; if not, it
is added to the LSP effect enum. **M015 is written to work either way**,
because depending on M010's ordering would make this milestone conditional
on another.

### 6.5 The `=` chord's home

`=` is a **Normal-mode** chord with no prefix. It goes into
`handle_normal_key`'s `(m, KeyCode::Char('='))` table, beside the other
Normal-mode chords — **not** into a prefix leaf table.

The key-authority guard will attribute it as a Normal-mode chord, taking
the count from 66 (or 68, after M012) to 67 (or 69). M011 adds **no**
Normal-mode chords — all nine of its arms are Insert-mode — so M011 does
not move the count. The closure
records the number.

### 6.6 The `space a` statement

The reason must say what is actually true: *"no code-action request exists
in this editor"*, not "applying a code action has no accepted ADR" — the
latter would imply a preview exists. If a code-action *read* is added
later, the reason changes with it.

## 7. Ordered work packages

- **W1 — The format preview.** `=` → `FormatPreview` → the panel. The
  smallest possible vertical slice: one key, one request, one panel.
- **W2 — The rename prompt.** The `EditorCommandName` entry, the arming of
  the `:` line, the local validation of the name, the request, the panel.
- **W3 — The `space a` statement.** The visible reason.
- **W4 — Docs and the sweep.** The doc surfaces in §12; the
  `CHANGELOG.md` entry, which must say **preview, not apply**.

## 8. Failure, cancellation, restart, contention semantics

- **Cancelling the rename prompt changes nothing.** No request is issued,
  the document is untouched, and the pending effect is consumed.
- **A refused rename preview** (no scope, a cold server, an ambiguous
  server, an unsupported capability) renders the reason the M013 panel
  already renders: `NotReady`, `Ambiguous` and `Unsupported` each have
  their own line.
- **A rename preview that the server refuses for a naming reason** (the
  new name is invalid for that symbol) renders the server's message; it
  must not fall through to "No results".
- **A preview applies nothing**, so there is no partial-application state
  and nothing to roll back.
- **A document change while a preview is in flight** invalidates it: the
  panel must be dismissed or marked stale, exactly as the diagnostics
  panel's `diagnostics_stale` flag does
  (`src/tui/editor_lsp.rs:104`).
- **No daemon write, no authorization record, no audit entry.** A preview
  is a read.

## 9. Compatibility and migration

- **No protocol change.** `crates/codegg-protocol` is untouched — the
  operations, the bounds and the `new_name` rule are M013's and already
  shipped.
- **No storage change.**
- **No daemon change.** The dispatch arms exist.
- **Two new chords** and one newly-visible unbound chord. **No chord is
  removed or rebound.**
- **`EditorCommandName` gains one variant**, which changes the `:` palette's
  contents. The palette is a help surface; its change is documented.

## 10. Required tests

Every chord is driven through the **real dispatch path** — `App::on_key` →
`handle_editor_key` → `editor_buffer_key` → `handle_normal_key` — not
through a helper.

- **The format preview**:
  - `=` issues a `FormatPreview` read and the panel shows the diff;
  - `=` **changes nothing** in the document, the undo history, or the
    selection;
  - a server without `documentFormattingProvider` answers `Unsupported` and
    the panel names that capability.
- **The rename preview**:
  - `space r` arms the prompt, the typed name is submitted, and the request
    carries `new_name` — asserted on the **request**, not only on the
    panel;
  - an **empty** name issues **no** request;
  - a name over 200 bytes issues **no** request and names the bound;
  - the preview lists the files that would change, and the panel shows
    them;
  - cancelling the prompt issues no request and changes nothing.
- **`space a`** renders the reason, and the reason says that no code-action
  request exists.
- **Precedence**: `=` does not collide with any existing Normal-mode chord
  (the key-authority guard is the evidence), and `space r` does not collide
  with `space R` (the clipboard replace) — a capital and its lower-case
  counterpart are different leaves, and the `SHIFT` reconciliation that
  M013 built for `space S` / `space F` must be exercised for `space r`.
- **Guard non-vacuity**: an injected duplicate `=` arm makes the
  key-authority guard fail. Prove it by injection before citing the guard
  as evidence.

## 11. Required verification commands

```bash
cd /Users/davidbowman/projects/codegg/.worktrees/feat-auto-20261018-12255c45

cargo fmt && cargo fmt --check
CARGO_BUILD_JOBS=2 cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=2 cargo build --locked --bin codegg-sandbox-helper
CARGO_BUILD_JOBS=2 cargo test -p codegg --lib --no-fail-fast

CARGO_BUILD_JOBS=2 cargo test -p codegg --features lsp-test-support \
  --test lsp_read_delivery_m013 --test lsp_read_surface_trajectory \
  --test lsp_completion_trajectory

python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_tui_editor_prefix_state.py
python3 scripts/check_tui_editor_text_authority.py
bash scripts/verify.sh quick
```

The three LSP integration suites are named because this milestone is the
**first** to drive `FormatPreview` and `RenamePreview` from the TUI; M013's
suites covered them only at the protocol and daemon layers.

`verify.sh full` is the wider sweep; the pre-existing 69 failing `tests/`
integration binaries are **not** this milestone's and are not fixed here (see
`plans/registry.md`, "Residual registered, not discharged by M013"). A
closure that claims a green full sweep must have one.

## 12. Documentation updates

| File | Section | What changes |
|---|---|---|
| `docs/LSP.md` | `### Previews change nothing` | the two preview chords and what they show |
| `docs/tui.md` | the editor language-features table | `space r` and `=`; `space a`'s reason |
| `docs/tui.md` | the `:` palette's command list | the new `RenamePreview` entry |
| `architecture/lsp.md` | the read-operation and preview sections | the reachable previews, and the preview/apply distinction |
| `architecture/tui.md` | the editor keymap section | the two chords |
| `CHANGELOG.md` | `## [Unreleased]` → `### Added` | the two previews, **worded as previews** |
| `.opencode/skills/lsp-ide/SKILL.md` | the leaf table | the two preview leaves and the `space a` reason |
| `.opencode/skills/tui/SKILL.md` | the editor-grammar bullet | the two chords — **required**, because this plan edits `architecture/tui.md` and `AGENTS.md` pairs the two |

`AGENTS.md` requires the skill and the `architecture/` doc to move together.

`src/tui/components/help_overlay.rs` is **unreferenced dead code** and is
not a doc surface; it is not edited.

## 13. Acceptance criteria (externally observable)

1. `=` shows the format preview and **changes nothing**.
2. `space r` collects a new name, shows the rename preview, and **changes
   nothing**.
3. A rename preview names the files that would change.
4. An empty or over-long new name issues no request and says why.
5. A server without the capability answers `Unsupported` and the panel
   names the missing capability.
6. `space a` renders a reason that says no code-action request exists.
7. **There is still no LSP write path.** No preview applies anything.
8. The `space r` and `space R` leaves are distinct and both reachable, in
   both terminal spellings of the capital.
9. The key-authority guard reports the new chord count and fails on an
   injected duplicate.
10. `scripts/verify.sh quick` passes, and the three LSP integration suites
    are green.

## 14. Stop conditions

Stop and report rather than continue if any of these holds.

- **The rename preview would need a second input mode.** The `:` command
  line is the established mechanism; a new prompt surface is a different
  milestone.
- **The preview's apply step becomes reachable by accident.** If any code
  path can apply a rename or a format from an editor chord, stop — that is
  the invariant this milestone exists to hold. (The ADR wording this
  replaced was wrong; see §17.15.)
- **A preview cannot be shown without a stale-answer hazard.** A preview
  that reports a result for a document that has since changed is worse
  than no preview; the `diagnostics_stale` pattern is the model.
- **The server's capability check cannot distinguish "no formatting
  support" from "not ready".** M013's three-status split is load-bearing;
  collapsing it here would undo M013's work.
- **The rename preview's file list cannot be bounded** within
  `MAX_LSP_RENAME_FILES` / `MAX_LSP_RENAME_EDITS` without losing the
  `truncated` marker.

## 15. Closure evidence required

- A closure record at
  `plans/closure/tui-editor-helix-parity/015-status.md` with: the baseline
  SHA, the work packages landed, the **new** key-authority chord counts,
  the guard outputs, the test names and counts for every suite in §11, and
  the exit conditions with their evidence.
- An explicit statement that **no LSP write path exists**, and that the
  M013 acceptance criterion 7 was changed in its *unbinding* half only.
- The **request-level evidence**: a test that asserts the rename request
  carries `new_name`, not only that the panel shows something.
- The **preview/apply transition** recorded in the roadmap and the
  `CHANGELOG.md`, so the future bridge's milestone does not change a chord's
  meaning silently.
- The guard-injection proof from §10, with the command and the observed
  failure.

## 16. Handoff notes

- **This milestone exists because M013 delivered a capability nothing could
  reach.** Two protocol operations, two daemon arms, two panel renderings —
  and no key. A reader who installed this build has the previews and no
  way to ask for them.
- **The preview/apply split is the whole design.** Helix's `space r`
  renames and Helix's `=` formats. Binding them to previews is a
  divergence, and it is the right one: a preview is a read, a read needs no
  ADR, and a preview that shows the user exactly what would change is
  strictly better than a chord that does nothing.
- **The rename prompt reuses the `:` line** because M008 established that
  reuse as a win. A second input mode for one more argument would be a
  worse editor.
- **The transition to apply is a real, registered change.** When the bridge
  lands, `space r` and `=` change meaning. That is recorded now, while the
  preview is what a reader learns — so the later change is a documented
  upgrade rather than a surprise.

## 17. Implementation decisions and recorded deviations (2026-10-19)

Everything below was decided while implementing, against the code as it stood.
Each is a place where the plan was wrong, incomplete, or silent, and where the
implementation had to answer the question rather than guess. They are recorded
here because **a plan that disagrees with the code is a defect**, and because
the next reader of this milestone deserves to know which decisions were open
and how they were settled.

### 17.1 `=` is a **Normal-mode chord**, so it does not ride `pending_effect`

§6.2 and §6.4 both said `=` becomes
`PrefixAction::Effect(EditorPendingEffect::Read(LspReadOperation::FormatPreview))`.
That shape does not exist for `=`, and cannot: `=` is a **Normal-mode** chord
with no prefix, and `EditorState::pending_effect` is documented as *a **prefix
leaf's** effect* — a slot the prefix resolver fills and the next statement
drains.

So `=` reports through a new `BufferKey::LspRead(LspReadOperation)` arm: the
buffer decides **what** was asked for, and the App layer — which holds the core
client — does **how**. That is the same shape the other Normal-mode verbs use,
and it is why the key-authority guard attributes `=` as a Normal-mode chord and
the count moves 70 → 71.

The `Read` effect arm itself was left alone: M013's `EditorPendingEffect::Read`
already existed and still carries the reads that *are* prefix leaves.

### 17.2 The read's argument is an **enum**, not a second `Option<String>`

The plan spoke of `new_name = Some(<typed>)` without saying how the call site
would express "the query" versus "the new name". Two `Option<String>`s would
have compiled, and swapping them would have compiled too — the protocol's own
`validate()` would then reject the request as `IncoherentFields`, one round
trip later.

`start_editor_lsp_read` therefore takes an `EditorLspReadArgument` enum with
three arms — `None`, `Query(String)`, `NewName(String)` — and maps it to the
protocol's `(query, new_name)` pair itself. The protocol's coherence rule is
visible at the call site, and the two options cannot be swapped.

### 17.3 The over-long-name check is **not reachable through the `:` line**

§6.1 said a name longer than `MAX_LSP_NEW_NAME_BYTES` (200) is refused
locally. That is implemented — `checked_new_name` refuses it and names the
bound — but it is **unreachable through the prompt**: the `:` command line is
bounded at `MAX_EDITOR_COMMAND_LINE` (64) characters, so a 201-character name
cannot be typed into it in the first place.

The check is kept for two reasons: the protocol enforces the same bound and
the editor should not spend a request discovering it, and any future caller
that reaches `run_editor_command` with an argument the line could not carry
gets the refusal rather than a rejected request. **Its test therefore calls
`run_editor_command` directly**, because a test that typed 201 characters into
a 64-character line would be testing the line's cap and calling it the name's.

### 17.4 `space r`'s leaf names a **step**, not a read

§6.4 already said this — "not a read: the read follows the prompt" — and the
implementation follows it. The leaf parks `EditorPendingEffect::RenamePrompt`,
which opens the `:` line pre-filled; the read is issued when the line is
submitted. Recording it because the leaf table is otherwise uniform: every
other `space` leaf that touches the language server parks a `Read`.

### 17.5 The `:` line carries the **command**, not a bare name

§6.1 said the prompt is "armed with that command pre-selected". The
implementation opens the line with `rename-preview ` — the command and a
trailing space — rather than with the command merely *selected* in the
palette. A selected row would still require the reader to type the argument
into a line that begins with the empty command, which is a second spelling of
the same thing.

The consequence is that `:rename-preview newname` is a line the reader could
have typed by hand, and the chord is a shortcut for it. `EditorCommandName`
gains the `RenamePreview` variant, which is what puts the command in the
palette and in the help.

### 17.6 The `=` arm carries the `none` guard

Every other Normal-mode chord in `handle_normal_key` matches on
`(m, KeyCode::Char(c)) if m == none`, where `none` is the "no modifiers"
pattern. `=` matches the same way, so it cannot be claimed by a modified
spelling of `=`.

### 17.7 The palette's row count moved 12 → 13

`EditorCommandName::ALL` grew by one, so the palette has thirteen rows. The
test `space_question_mark_opens_the_editors_own_command_palette` asserted
twelve and had to change — it asserts an absolute count, which is exactly the
kind of assertion that must be updated when the surface it describes grows.

### 17.8 The leaf table's glyph count moved 42 → 44

`SPACE_LEAVES` gained `r` and `a`, so the leaf-authority guard now reports
**44 leaf glyphs** across 5 tables and 12 prefix bindings. The key-authority
guard's counts are unchanged at **2 / 71 / 8**: a space leaf is not a Normal-
or Insert-mode chord, so `space r` moves no count — only `=` does.

### 17.9 §12 named a doc surface that does not exist

§12's row for `docs/tui.md` said "the `:` palette's command list | the new
`RenamePreview` entry". **There is no such table.** The palette's contents are
generated from `EditorCommandName::ALL`; `docs/tui.md` describes the palette
in prose (the "regex keys" section) and has no enumerated command list to
extend. The new command is therefore documented in the new "The preview keys"
section, which is where a reader would look for it.

### 17.10 `lsp-ide`'s leaf table carried an M010 omission

`.opencode/skills/lsp-ide/SKILL.md` still said "`space f` document symbols" —
a statement M010 made false when it reclaimed `space f` for the file picker,
and which M010 did not correct here. M015 edits that same paragraph, so it
corrected the stale row rather than leaving a false statement in a table it
was rewriting.

### 17.11 The `space a` reason is a **status row**, not a toast

§6.3 bound `space a` to "a visible statement". The mechanism that already
exists for that — `EditorPendingEffect::Unavailable`, M010's — renders the
reason as a **status row**, which stays readable after the keypress. A toast
would have expired before the reader had read it.

### 17.12 The CHANGELOG's M013 bullet was **edited in place**

`CHANGELOG.md` carried an M013 bullet saying the previews were "not yet bound
to a key". M015 makes that sentence false, and a false sentence in the
`## [Unreleased]` section is a defect in the release notes. The bullet was
rewritten in place to describe the bound previews, and a **new** `### Changed`
bullet records the preview/apply transition — because a reader who learned the
preview must be told, in advance, that the chord's meaning changes when the
bridge lands.

### 17.13 The plan's `space r` / `space R` test needed the **shifted spelling**

§10 required that "`space r` does not collide with `space R`" and that "the
`SHIFT` reconciliation that M013 built for `space S` / `space F` must be
exercised for `space r`". The test drives all three: the lower-case leaf, the
upper-case leaf, and the `Char('r') + SHIFT` spelling that a terminal which
reports the shift separately produces.

### 17.14 Two rendering tests, because a rendering nobody exercises is a claim

M013 delivered the panel's rendering of both previews and **no test** for it.
The milestone's value is that the preview *shows* something, so the rendering
is now exercised: the rename preview names the files it would change, and the
format preview shows the diff. Both assert that the panel says the change is
**not applied**.

To let a test seed a panel, the widget's test `Options` gained an owned
`lsp: EditorLspState` field. It was previously a `Default` the render helpers
constructed themselves, which made the panel unreachable from a test.

### 17.15 The plan's "no accepted write ADR" framing was **wrong**, and the code said so

§2 said the previews needed no ADR because *"only the **apply** does"*, and
§5's divergence section said the apply *"needs an accepted LSP-write ADR"*
that does not exist. §4's invariant 1 said *"No LSP write path exists"*.

**ADR-0008 is accepted** (2026-09-20) and owns exactly that path.
`CoreRequest::LspPreviewApply` is authorized `via_session` + `file.modify`
(`crates/codegg-core/src/authorization/policy.rs:827-831`), and it applies a
**registered, still-current reviewable preview** by id through
`src/lsp/mutation.rs::apply_preview()` — reachable from the TUI's
change-review surface (`/lsp-preview-apply <id>`) and from the model-facing
`lsp_preview_apply` tool. `architecture/lsp.md` documents it in two places
in this same file's subject area.

So "no accepted write ADR" was never true, and the closure must not repeat
it. The invariant that **does** hold is narrower and is the one this
milestone preserves:

> **The editor's preview chords cannot reach the apply path, and they apply
> nothing.** The editor's `=` and `space r` produce a *read result* — no
> preview id, no patches, no `register_preview_artifact` call anywhere in
> `src/tui/commands/editor.rs`, `src/tui/editor.rs`, `src/tui/editor_lsp.rs`
> or `src/tui/components/editor.rs`, and no reference to `change_review` in
> the editor path at all. The apply path consumes a *registered preview
> artifact*, and the editor's preview is not one.

The gap is therefore an **implementation gap** — the bridge from a rendered
preview to a reviewable artifact — and **not** a decision gap. Every surface
this milestone touched was corrected to say so: the plan's §4 and §5, the
closure record, `architecture/lsp.md`, `architecture/tui.md`, `docs/LSP.md`,
`docs/tui.md`, `CHANGELOG.md` and both skill files.

**The "preview/apply transition" is therefore registered against the bridge,
not against an ADR.** When the bridge is built, `=` and `space r` become the
apply step — and that meaning change is still registered now, while the
preview is what a reader learns.

### 17.16 `SPACE_LEAVES`'s own doc comment was **stale in two ways**, and M015 fixed both

The doc comment above the space-menu table is the reader's first account of
what the table is for, and it had drifted twice:

1. It said **`space r` and `space a` are omissions** — *"the omissions are
   each a capability this surface does not have"*. M010 had already made that
   false for the seven statement leaves, and M015 makes it false for `space r`
   as well. The comment now lists `space r` as bound to the rename preview and
   `space a` as bound to a statement, and keeps `space w` / `space W` as the
   real omission.
2. It repeated the *"no accepted ADR"* reason (§17.15), in the one place a
   maintainer would look first. Corrected the same way.

The comment also now says *why* the preview leaves are separated from the
apply — the **bridge**, not an authorization — so the next reader does not
re-derive the same wrong conclusion from the same comment.

**This is a doc comment on a `const`, so it changed no behaviour.** It is
recorded because the milestone *touched* the table those comments describe,
and leaving a comment that contradicts the code next to the code is the
defect this roadmap exists to prevent.
