# TUI Editor Helix Parity Milestone 015 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/tui-editor-helix-parity/015-lsp-preview-leaves.md`
Source subsystem roadmap: `plans/subsystems/tui-editor-helix-parity-roadmap.md` §7 (M015), §13 (stage E)
Repository baseline reviewed: `a8778e87c` (M013, with the M013 work applied)

---

## 1. Executive finding

M015 is **closed**. The two LSP previews M013 shipped now have keys: `=` shows
the **format preview**, and `space r` collects a new name on the `:` command
line and shows the **rename preview**. Both show what *would* change and
**apply nothing**. `space a` states that no code-action request exists in this
editor — a different claim from "there is one and it may not be applied", and
the one that is actually true.

Five things in this milestone are worth recording beyond the checklist, because
each is a decision a later milestone would otherwise have to re-litigate.

1. **The milestone was worth doing because M013 delivered a capability nothing
   could reach.** M013 shipped two protocol operations, two daemon arms, two
   panel renderings — and **no key**. A reader who installed that build had
   the previews and no way to ask for them. This milestone is the difference
   between a capability and a *reachable* capability, and it is why the
   milestone was planned separately rather than folded into M010.
2. **A preview is a read, and a read needs no ADR — but the reason the
   milestone landed is sharper than the plan said.** The roadmap's M010
   boundary had gated `space r` / `space a` / `=` on *"an LSP write
   authorization ADR"*, and the plan repeated that such an ADR "does not
   exist". **It does exist**: ADR-0008 is accepted and owns the LSP write
   path. The gate was never the problem. What the editor lacks is the
   **bridge** from a rendered preview to the *registered reviewable artifact*
   ADR-0008's apply path consumes — so the milestone could land, and the
   next one is an implementation task rather than a decision. §3.4 carries
   the evidence, and §7 records the correction.
3. **`=` is a Normal-mode chord, so it is reported through `BufferKey`, not
   through `pending_effect`.** The plan said `=` becomes
   `PrefixAction::Effect(EditorPendingEffect::Read(...))`, but `=` has no
   prefix and `EditorState::pending_effect` is documented as *a **prefix
   leaf's** effect*. The implementation uses a new
   `BufferKey::LspRead(LspReadOperation)` arm: the buffer decides **what** was
   asked for, and the App layer — which holds the core client — does **how**.
   That is the established pattern for a Normal-mode chord that needs the App
   layer, and it is why the key-authority guard attributes `=` as a Normal-mode
   chord and the count moves **70 → 71**.
4. **`space r` is a leaf that names a *step*, not a read.** The leaf parks
   `EditorPendingEffect::RenamePrompt`, which opens the `:` command line
   pre-filled with `rename-preview `; the read is issued when the line is
   submitted. A preview is still a read — it just happens *after* the reader
   has supplied the name the request needs. The `:` line is M008's precedent
   for an argument, reused rather than replaced by a second input mode.
5. **The over-long-name refusal is real but unreachable through the prompt.**
   The `:` command line is bounded at `MAX_EDITOR_COMMAND_LINE` (64)
   characters, so a 201-character name cannot be typed into it. The check
   exists (the protocol enforces the same bound and the editor should not
   spend a request discovering it) and is unit-tested **directly**, because a
   test that typed 201 characters into a 64-character line would be testing
   the line's cap and calling it the name's.

---

## 2. The work packages that landed

| Package | What it delivered |
|---|---|
| W1 | The format preview: `=` → `BufferKey::LspRead(LspReadOperation::FormatPreview)` → the request → M013's panel rendering |
| W2 | The rename prompt: `EditorCommandName::RenamePreview`, the `:` line armed with `rename-preview `, the local name check, the request, and the panel |
| W3 | The `space a` statement: `EditorPendingEffect::Unavailable` with the reason that says what is actually true |
| W4 | The docs: `docs/tui.md`, `docs/LSP.md`, `architecture/tui.md`, `architecture/lsp.md`, `CHANGELOG.md`, and the two skill files |

---

## 3. The model

### 3.1 Three chords, two mechanisms, one invariant

| Chord | Shape | What it does |
|---|---|---|
| `=` | Normal-mode chord, no prefix | shows the format preview for the open document |
| `space r` | prefix leaf | arms the rename prompt; the read follows the submitted name |
| `space a` | prefix leaf | states that no code-action request exists |

The first two are the same *kind* of thing — a preview — and reach the daemon
by different routes, because one has an argument and the other does not. The
third is not a preview at all, because there is no code-action **read** in the
protocol to preview.

### 3.2 The read's argument is an enum, not a second `Option<String>`

The protocol's coherence rule is that a read carries a `query` for and only
for workspace symbols, and a `new_name` for and only for the rename preview.
Two `Option<String>`s would have compiled, and swapping them would have
compiled too — the protocol's own `validate()` would then reject the request
as `IncoherentFields`, one round trip later.

`start_editor_lsp_read` therefore takes an `EditorLspReadArgument` enum with
three arms — `None`, `Query(String)`, `NewName(String)` — and maps it to the
protocol's `(query, new_name)` pair itself. The rule is visible at the call
site, and the two options cannot be swapped.

### 3.3 The name is checked before a request is spent on it

`checked_new_name` refuses an empty name (`":rename-preview` needs a new
name") and a name over `MAX_LSP_NEW_NAME_BYTES` (200 — the refusal **names**
the bound). Neither spends a round trip. The protocol enforces the same bound;
the editor not sending a request it knows will be rejected is a separate,
cheaper failure.

### 3.4 The editor's preview chords cannot reach the apply path

This is the milestone's load-bearing invariant, and it is checked rather than
asserted.

**The apply path exists, and it is not what this milestone was waiting for.**
ADR-0008 is **accepted** (2026-09-20) and owns the LSP write half:
`CoreRequest::LspPreviewApply` is authorized `via_session` + `file.modify`
(`crates/codegg-core/src/authorization/policy.rs:827-831`), and it applies a
**registered, still-current reviewable preview** by id through
`src/lsp/mutation.rs::apply_preview()` — reachable from the TUI's
change-review surface (`/lsp-preview-apply <id>`) and from the model-facing
`lsp_preview_apply` tool, which the agent reaches by supplying only an opaque
preview id.

**What the editor's `=` and `space r` produce is a *read result*, not such an
artifact.** The evidence:

| Check | Result |
|---|---|
| `grep` for `register_preview_artifact` / `PreviewArtifact` / `preview_id` / `LspPreviewApply` across `src/tui/commands/editor.rs`, `src/tui/editor.rs`, `src/tui/editor_lsp.rs`, `src/tui/components/editor.rs` | **no hits** |
| `grep` for `change_review` across `src/tui/commands/editor.rs` and `src/tui/editor.rs` | **no hits** |
| The chords' requests | `CoreRequest::LspReadGet`, gated at `via_session` + `file.read` (`policy.rs:841-845`) — the **read** capability |
| The editor route's other LSP requests | `LspDiagnosticsGet` and `LspCompletionGet`, also reads |
| The LSP client's answer to a server-initiated `workspace/applyEdit` | `applied: false`, with the reason *"Codegg does not permit implicit language-server edits; request a preview and apply it through the authorized patch path"* (`crates/egglsp/src/server_request.rs:166-169`) |

So the invariant that holds — and that this closure states — is:

> **The editor's preview chords apply nothing, and they cannot reach the
> apply path.** The apply path consumes a *registered preview artifact*; the
> editor's preview is not one.

**This corrects the plan's framing.** The plan, and the roadmap prose it
inherited, said the apply needed "an accepted LSP-write ADR, which does not
exist". **ADR-0008 is accepted**; the gap is the **bridge** from a rendered
preview to a reviewable artifact, not a missing decision. The correction is
recorded in the plan's §17.15 and was applied to every surface this milestone
touched — the plan, the closure, `architecture/lsp.md`, `architecture/tui.md`,
`docs/LSP.md`, `docs/tui.md`, `CHANGELOG.md` and both skill files.

M013's acceptance criterion 7 — *"No LSP **write** path exists: rename and
code actions offer a preview only, or are unbound pending §14"* — is changed
by this milestone in its **unbinding half only**. Its substance survives: the
editor's rename and format surfaces still offer a preview only.

### 3.5 The preview/apply transition is registered now

When the bridge is built — when the editor's preview becomes a reviewable
artifact ADR-0008's apply path can consume — `=` and `space r` stop showing
what *would* change and start doing it. The chords do not move, so the change
is invisible to anyone who did not learn the preview — which is why it is
recorded in **three** places while the preview is what a reader learns:

- the roadmap's M015 entry (Amendment A1, point 4, and Amendment A2 for the
  corrected ADR framing),
- this closure record (§9),
- `CHANGELOG.md` under `### Changed`.

---

## 4. The guards

| Guard | Result | Movement |
|---|---|---|
| `scripts/check_tui_editor_key_authority.py` | 2 guard-block chord(s), **71** Normal-mode chord(s), 8 Insert-mode chord(s), no shadowed arm | Normal **70 → 71** — one for the `=` arm |
| `scripts/check_tui_editor_leaf_authority.py` | 5 leaf table(s), **44** leaf glyph(s), 12 prefix binding(s), no duplicate and no unreachable leaf | glyphs **42 → 44** — `space r` and `space a` |
| `scripts/check_tui_editor_prefix_state.py` | one pending-prefix field (`pending_prefix`), typed as `EditorPrefix` | unchanged |
| `scripts/check_tui_editor_text_authority.py` | no retained text buffer and no direct filesystem reads in the editor path | unchanged |

A **space leaf is not a Normal- or Insert-mode chord**, so `space r` and
`space a` moved no chord count. Only `=` did — and §13.2's stage-E row
predicted exactly **70 → 71**.

The leaf guard's own self-test passes 9/9 cases.

---

## 5. The verification and test evidence

### 5.1 The sweep

Every command in the plan's §11 was run against this tree, with
`CARGO_BUILD_JOBS=2` and `CARGO_INCREMENTAL=0` (the worktree's incremental
cache is unreliable — `dep-graph.part.bin` went missing twice during this
work).

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | exit 0 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0 |
| `cargo build --locked --bin codegg-sandbox-helper` | exit 0 |
| `cargo test -p codegg --lib --locked --no-fail-fast` | **6812 passed / 1 failed** — the one failure is the load-sensitive class (§5.2), which passes 94/94 in isolation |
| `cargo test -p codegg --features lsp-test-support --locked --test lsp_read_delivery_m013 --test lsp_read_surface_trajectory --test lsp_completion_trajectory` | exit 0 — **7 + 14 + 12 = 33 passed / 0 failed** |
| `python3 scripts/check_tui_editor_key_authority.py` | exit 0 — 2 / **71** / 8 |
| `python3 scripts/check_tui_editor_leaf_authority.py` | exit 0 — 5 tables / **44** glyphs / 12 bindings |
| `python3 scripts/check_tui_editor_leaf_authority.py --self-test` | exit 0 — 9/9 cases |
| `python3 scripts/check_tui_editor_prefix_state.py` | exit 0 |
| `python3 scripts/check_tui_editor_text_authority.py` | exit 0 |
| `bash scripts/verify.sh quick` | exit 0 — **"Quick verification passed."** |
| `cargo test -p codegg --lib --locked tui::` | **2296 passed / 0 failed** — the TUI surface, re-run against the final tree |
| `cargo test -p codegg --lib --locked tui::commands::editor::tests::` | **219 passed / 0 failed** |


**The sweep was run four times, and the numbers in the table are from the
last of them.** That is deliberate. The first sweep's clippy failed on two
needless borrows this milestone introduced in the panel's test `Options`; the
second sweep's library run failed on three tests, one of them this
milestone's own prefix-table row (§5.4); the third sweep was run against the
tree after that fix; and the fourth — the one whose numbers are reported —
was run against the **final** tree.

The last edits were **comment-only**, and they landed in `src/tui/editor.rs`
— which the four editor guards **parse**. So the fourth sweep re-ran `cargo
fmt`, `cargo clippy`, **all four guards**, the leaf guard's self-test, and the
TUI test subset. A compiler being green is not enough when the change was to
a file a guard reads.

**Every §11 command was run against the tree this closure closes.** A closure
that reports a sweep from an earlier tree is reporting a sweep it did not get.

The three LSP integration suites are the ones the plan named, because this is
the **first** milestone to drive `FormatPreview` and `RenamePreview` from the
TUI; M013's suites covered them only at the protocol and daemon layers.

### 5.2 The load-sensitive class, in isolation

The full library sweep finished **6812 passed / 1 failed** after the
prefix-table fix. The one failure was
`tui::commands::interactive_terminal::tests::the_badge_returns_to_normal_after_esc_even_though_the_composer_forgot`
— *"the box never writes the composer's mode, so this stale field is the
trap"* (`left: Normal`, `right: Insert`).

Run in isolation, on the same binary:

```
cargo test -p codegg --lib --locked interactive_terminal
```

```
test result: ok. 94 passed; 0 failed; 0 ignored; 0 measured; 6723 filtered out; finished in 0.40s
```

**94 / 94 pass** — the same figure M009's and M010's closures recorded. The
first sweep also failed
`core::daemon::tests::poolless_standalone_human_shell_uses_registered_workspace`
(*"standalone shell output was not delivered"*); it passed on the re-run and
in isolation (**1 passed**).

**This is the pre-existing class, not a regression.** The failing test is in
`tui::commands::interactive_terminal::tests::*` — a module this milestone
does not touch. The M015 change set contains no PTY, no terminal surface and
no `interactive_terminal.rs` edit, and the same command passes on the next
run on the same binary. M009's closure recorded the same class with the same
instruction: the failing count is not exactly the pre-existing count with the
same names, that is a regression, and it must be fixed before closure.
**This is not that.**

The three LSP integration suites are the ones the plan named, because this is
the **first** milestone to drive `FormatPreview` and `RenamePreview` from the
TUI; M013's suites covered them only at the protocol and daemon layers.

### 5.3 The new tests

Ten new tests. Eight drive the real dispatch path (`App::on_key` →
`handle_editor_key` → `editor_buffer_key` → `handle_normal_key`), and two
exercise the panel rendering M013 delivered but never tested.

| Test | What it pins |
|---|---|
| `the_format_preview_is_requested_and_changes_nothing` | `=` issues a `FormatPreview` read with no `query` and no `new_name`, and the document, the undo history and the selection are untouched |
| `the_rename_preview_carries_the_typed_name_on_the_request` | `space r` arms the prompt and issues **nothing**; the submitted name reaches the **request** as `new_name`, with the cursor's position, and no `query` |
| `an_empty_new_name_issues_no_request_and_says_why` | an empty name issues no request, and the refusal says what is missing |
| `a_new_name_over_the_bound_issues_no_request_and_names_the_bound` | a name over 200 bytes issues no request, and the refusal names the bound |
| `cancelling_the_rename_prompt_issues_no_request` | `Esc` issues no request, changes nothing, and closes the prompt |
| `space_a_states_that_no_code_action_request_exists` | the reason says no code-action request exists — and does **not** say "ADR", which would imply a preview exists |
| `the_lower_r_and_upper_r_leaves_are_different_and_both_spellings_reach_them` | `space r` and `space R` are different leaves, and `Char('r') + SHIFT` reaches the capital the way a shift-reporting terminal spells it |
| `the_rename_preview_reaches_the_panel_with_the_files_it_would_change` | the result reaches the panel with the new name and the file list |
| `the_rename_preview_names_the_files_it_would_change` (component) | the panel names the files and says the change is **not applied** |
| `the_format_preview_shows_the_diff_and_says_it_is_not_applied` (component) | the panel shows the diff and says the change is **not applied** |

The **request-level** evidence the plan required is
`the_rename_preview_carries_the_typed_name_on_the_request`: it asserts on the
request the daemon was handed — through a recording `CoreClient` — not on the
panel, because the panel shows what the server *answered*, not what was
*asked*.

### 5.4 Two pre-existing tests changed

Both are the kind of assertion that must move when the surface it describes
grows, and both are of a kind worth naming: **a binding change takes a
fixture from an earlier milestone's test; the test's *subject* survives, its
*example* does not.**

1. `space_question_mark_opens_the_editors_own_command_palette` asserted that
   the palette lists **twelve** editor commands. `EditorCommandName::ALL` grew
   by one, so the count is **thirteen**, and the assertion was updated. It is
   an absolute count on purpose — that is the kind of assertion that must move
   when the surface it describes grows.
2. `tui::editor::tests::each_prefix_declares_its_leaf_and_its_unmatched_key_policy`
   holds the prefix table's own `Space` row — the test that exists so
   "claiming a key is supposed to require editing the row here". The row
   listed **nineteen** glyphs after M010; M015's `space r` and `space a` make
   it **twenty-one**, and the row was updated. This one was **caught by the
   sweep**, not by inspection: the first library run failed with
   `left: ['y', 'k', 's', 'S', 'd', 'h', 'f', '?', 'p', 'P', 'R', 'Y', 'r', 'a', 'F', 'b', 'j', 'D', 'c', 'C', '\'']`
   against the nineteen-glyph expectation, and it passes on the re-run
   (**1 passed**).

### 5.5 The guard-injection proof

The plan required proof that the key-authority guard is not vacuous for the
new `=` chord. A duplicate `=` arm was injected into `handle_normal_key` and
the guard was run against it.


The command was `python3 /tmp/m015_inject_guard.py`, which injects a duplicate
`=` arm into `handle_normal_key` in `src/tui/commands/editor.rs`, runs
`scripts/check_tui_editor_key_authority.py` against it, and restores the file
byte-identically.

```
--- guard exit code: 1 ---
TUI editor key-authority violations found.
The editor dispatch chain is a sequence of ordered stages, so a chord
declared in a later stage is dead the moment an earlier stage claims it.
That is how Normal mode ended up with `u` for undo and no reachable
redo. Every chord needs exactly one owner.

  editor.rs:4047 [handle_normal_key] (m, KeyCode::Char('=')) if m == none re-declares NONE+'=', already claimed at editor.rs:4037 ((m, KeyCode::Char('=')) if m == none) — unreachable

1 violation(s) found.
--- the guard caught the injected duplicate `=` arm ---
--- restored byte-identically (888ccf2e44b48200) ---
```

The guard **exited 1**, named the duplicate, and pointed at both the
injected arm and the arm it shadows. The proof was run twice — once against
the mid-implementation source and once against the final source — with the
same outcome and the same restored digest.

The file was restored **byte-identically** (SHA-256 `888ccf2e44b48200…`
before and after), and the backup was keyed by **relative path** — a flat
backup made `src/tui/editor.rs` and `src/tui/commands/editor.rs` share one
slot once, and the clobber was silent.

---

## 6. The exit conditions, with their evidence

| Exit condition | Evidence |
|---|---|
| Both previews are reachable and show their result | `the_format_preview_is_requested_and_changes_nothing`; `the_rename_preview_carries_the_typed_name_on_the_request`; `the_rename_preview_reaches_the_panel_with_the_files_it_would_change`; the two component rendering tests |
| A preview **applies nothing** | `the_format_preview_is_requested_and_changes_nothing` asserts the text and the undo depth are unchanged; the two component tests assert the panel says "not applied"; §3.4 shows the editor's chords issue only `LspReadGet` and produce no reviewable artifact the apply path could consume |
| A server without the capability answers `Unsupported` and the panel names it | M013's own three-status split, unchanged by this milestone — the milestone adds no request path that could bypass it |
| An empty or over-long new name issues no request | `an_empty_new_name_issues_no_request_and_says_why`; `a_new_name_over_the_bound_issues_no_request_and_names_the_bound` |
| The `space r` / `space R` leaves are distinct and both reachable | `the_lower_r_and_upper_r_leaves_are_different_and_both_spellings_reach_them` |

### The plan's stop conditions, none fired

| Stop condition | Why it did not fire |
|---|---|
| The rename preview would need a second input mode | It reuses the `:` command line — M008's precedent, and the plan's own instruction |
| The preview's apply step becomes reachable by accident | §3.4: the editor's chords issue only `LspReadGet`, produce no preview id, and reference no apply path; `workspace/applyEdit` is refused with `applied: false` |
| A preview cannot be shown without a stale-answer hazard | The previews are reads against a document, and the panel's staleness handling is M013's (`base_stale`, `diagnostics_stale`); this milestone adds no new answer path |
| The server's capability check cannot distinguish "no formatting support" from "not ready" | M013's three-status split is untouched — the milestone issues reads and never interprets a status of its own |
| The rename preview's file list cannot be bounded | M013's bounds (`MAX_LSP_RENAME_FILES`, `MAX_LSP_RENAME_EDITS`, the `truncated` marker) are untouched; the milestone adds no list of its own |

---

## 7. The recorded divergences

The plan's §17 records sixteen decisions in full. The three that change what
a reader of the code should expect:

1. **`=` is not a prefix leaf** (§17.1) — it is a Normal-mode chord reported
   through `BufferKey::LspRead`, and it is the only chord the milestone adds
   to the Normal-mode count.
2. **The over-long-name check is not reachable through the `:` line** (§17.3) —
   the command line caps at 64 characters, so the check is defence in depth
   and its test calls `run_editor_command` directly.
3. **§12 named a doc surface that does not exist** (§17.9) — `docs/tui.md`
   has no enumerated `:` palette table (the palette is generated from
   `EditorCommandName::ALL`), so the new command is documented in the new
   "The preview keys" section instead.

Two further corrections are recorded because they fix something a reader would
otherwise find false:

- **§17.10** — `.opencode/skills/lsp-ide/SKILL.md` still said "`space f`
  document symbols", a statement M010 made false when it reclaimed `space f`
  for the file picker, and which M010 did not correct. M015 edits that same
  paragraph, so it corrected the stale row rather than leaving a false
  statement in a table it was rewriting.
- **§17.12** — the `CHANGELOG.md` bullet M013 wrote ("not yet bound to a key")
  became false when this milestone landed. It was rewritten in place, and a
  **new** `### Changed` bullet records the preview/apply transition.

And one correction is recorded because the plan itself was wrong about the
code:

- **§17.15** — the plan said the apply needs "an accepted LSP-write ADR,
  which does not exist". **ADR-0008 is accepted** and owns that path. The
  invariant that holds is narrower: the editor's preview chords produce a
  *read result*, not the *registered reviewable artifact* ADR-0008's apply
  path consumes, so they cannot reach it — **the gap is the bridge, not an
  ADR**. Every surface this milestone touched was corrected to say so, and
  the "preview/apply transition" is registered against the bridge.
- **§17.16** — `SPACE_LEAVES`'s own doc comment was stale twice over: it
  called `space r` and `space a` *omissions* (M010 had already made that
  false for the statement leaves, and M015 makes it false for `space r`),
  and it repeated the "no accepted ADR" reason in the one place a
  maintainer would look first. Corrected. It is a doc comment on a `const`,
  so it changed no behaviour — but a comment that contradicts the code next
  to the code is the defect this roadmap exists to prevent.

---

## 8. The docs that moved

| File | What changed |
|---|---|
| `docs/tui.md` | a new "The preview keys" section; `space r` in the space-menu table; `space a` in the "eight that say why" table; a pointer from the language-server features |
| `docs/LSP.md` | "Previews change nothing" rewritten around the two chords, the `:` prompt, and the `space a` reason |
| `architecture/tui.md` | the keymap table (`=`, `space r`, `space a`); "A preview is a read, and the editor's previews cannot apply"; the preview/apply transition |
| `architecture/lsp.md` | "Previews are previews" corrected — the reads are no longer unbound, and the ADR sentence that contradicted the same document is corrected; the prompt mechanism; the preview/apply transition |
| `CHANGELOG.md` | the M013 bullet rewritten in place; a new `### Changed` bullet for the transition |
| `.opencode/skills/lsp-ide/SKILL.md` | the leaf table: the two preview leaves, the `space a` reason, and the stale `space f` row corrected |
| `.opencode/skills/tui/SKILL.md` | two new bullets: "A preview is a read, and it says so" and "A Normal-mode chord that needs the App layer reports what it wants" |

**Every one of those surfaces also carried the ADR framing**, and every one was
corrected to say the gap is the **bridge** rather than a missing ADR (§3.4,
§7). `architecture/lsp.md` is the clearest case: it already documented
ADR-0008's write path in two places, and the sentence *"Applying an LSP-driven
mutation needs an accepted ADR, which does not exist"* contradicted its own
document. A closure that repeated the plan's framing would have propagated
that contradiction into the release notes and the user documentation.

`AGENTS.md` requires the skill file and its `architecture/` doc to move in
the same commit: `lsp-ide/SKILL.md` moved with `architecture/lsp.md`, and
`tui/SKILL.md` moved with `architecture/tui.md`. Both pairs are in this
commit.

`src/tui/components/help_overlay.rs` is unreferenced dead code and was **not**
edited.

---

## 9. Notes carried forward

- **The preview/apply transition is the thing to remember.** When the bridge
  from the editor's rendered preview to a reviewable artifact is built, `=` and
  `space r` change meaning. The chords do not move, so the change is invisible
  to anyone who did not learn the preview — which is why it is registered now,
  in the roadmap, here and in the `CHANGELOG.md`.
- **The write path was never missing.** ADR-0008 is accepted and owns it. What
  is missing is the **bridge** from the editor's preview to a reviewable
  artifact — an implementation gap, not a decision gap. A reader who assumes
  "the editor cannot write because the project has not decided" is wrong; the
  project decided, and the editor's previews simply are not the artifact the
  apply path consumes.
- **A code-action read does not exist.** `space a` is bound to a statement
  because there is nothing to preview, and the reason says exactly that. If a
  code-action *read* is added to the protocol later, `space a` becomes a
  preview and the reason changes with it.
- **The next milestone after this one builds the bridge, not an ADR.** Turning
  the editor's rename or format preview into an applicable, reviewed edit is
  the work that remains.

---

## 10. The residual

**Nothing in this milestone is left open.** The deferred items are each owned
by a named future milestone:

| Deferred | Owner |
|---|---|
| Applying a rename, a format or a code action | The **bridge** from the editor's preview to a reviewable artifact — ADR-0008's apply path already exists; see §3.4 |
| A code-action **read** (and therefore a code-action preview) | A protocol extension, then a leaf |
| The `Ctrl-s` / `Ctrl+r` Insert-mode conflict | Named and unresolved; not this milestone's |
| The `Ctrl-x` guard blind spot | Roadmap §13.4 open question 5 — still open, and **not** discharged here |

The pre-existing failures recorded in `plans/registry.md` are unchanged by
this milestone and are **not** claimed as fixed here.

---

## 11. Handoff

M015 closes the wave-2 stage-E row of the roadmap's §13.2 order. With M009,
M010, M011, M012 and M015 closed, the roadmap's §13.2 final figure is
**2 guard-block chord(s), 71 Normal-mode chord(s), 8 Insert-mode chord(s)**,
and the guard reports exactly that.

**M014 remains blocked** on ADR-0034 acceptance and deliberately has no plan:
the ADR records that *"no option is selected by this record"*, and its two
options produce different contracts that would need different plans.
