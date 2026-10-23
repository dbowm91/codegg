# File Picker Rework Post-Closure Corrective Milestone M002 — The box's exit verb and the editor handoff

Status: ready for handoff

Repository baseline: `6e097dd3a` (`Merge pull request #180 from dbowm91/fix/repository-audit-corrective-implementation`, 2026-10-23), plus Milestone M001 of this addendum

Source roadmap:

- `plans/subsystems/file-picker-rework-post-closure-corrective-addendum.md` — §5.4, §5.5, §7 M002

Long-term requirements:

- `plans/000-long-term-specification.md` — TUI-first product; correctness before transparent magic.
- `plans/001-terminology-and-domain-model.md` — workspace-relative path identity; document attachment.

Applicable ADRs:

- ADR-0022 (daemon-authorized workspace directory listing) — consumed unchanged.

Primary class: capability

Corrective plan. It corrects the keymap contract published by
`plans/implementation/file-picker-rework/001-panel-keymap-authority-and-listing-hierarchy.md`
and its closure record `plans/closure/file-picker-rework/001-status.md`, which made the
panel's keymap *visible* without making `Enter` and `l` the same verb or making the
panel release the keyboard.

## 1. Objective

Make `q` close the `/files` modal box — and nothing else — and make opening a file from
any `/files` surface hand the keyboard to the editor, which starts in its Normal mode,
instead of leaving the docked panel holding it.

## 2. Why this milestone is ready

- **Hard dependency**: M001 of this addendum. It establishes the single authority and
  the intent plumbing that M002's handoff depends on: the handoff's correctness rests
  on the surface's snapshot being current when the file is opened.
- **Interface dependencies are stable and in-tree**: `FocusManager`, `Component`,
  `SidePanelHost`, `open_editor`, `enter_editor_normal`, `TuiMsg`.
- **No unresolved architectural decision.** No ADR is required.

## 3. Current implementation evidence

Baseline `6e097dd3a`. Paths below are repository-relative; line numbers are the
baseline's and are evidence, not a contract.

### 3.1 `q` is dead in both forms today

`q` is bound to `InputAction::Quit` — the program's exit arm, which requires a second
press within its window.

- In the **modal box**, the app routes keys to the open modal before the app-level
  bindings are consulted, so `q` never reaches the quit arm. The box's `handle_key` has
  no `q` arm, so the key is swallowed and nothing happens.
- In the **docked panel**, the panel host consumes every key the widget does not
  answer, so `q` is swallowed there too.

The widget's own code comment records why `Shift+Q` was chosen: "It is spelled with the
shift modifier so the plain `q` stays the program's quit key." In the modal case that
rationale protects nothing, because the modal consumes the key first.

`Shift+Q` closes the box, and in a docked panel it is the panel's exit.

**This is a closed contract, and this milestone honours it.**
`plans/subsystems/tui-buffer-chrome-and-panel-navigation-roadmap.md` M004 and its
closure record `plans/closure/tui-buffer-chrome-and-panel-navigation/004-status.md` §2
established: *"`is_exit_key` matches `Shift+Q` only — `q` is the program's quit key, and
one press that meant both 'leave this panel' and 'quit the program' was two meanings for
one key."* The same record states that *"a bare `q` inside the chat panel is inert"*.

So: `q` outside the `/files` box remains the program's two-press quit key; `Shift+Q`
remains the panel's exit and the box's close; `Esc` remains the mode switch; and a bare
`q` in a **focused panel** is inert, exactly as it is in the chat panel. The only
addition is that a bare `q` inside the `/files` **modal box** closes that box — a
surface the modal already owns the keyboard for, and a key that today reaches nothing
at all there. The program's quit key is not taken away from anywhere it works today.

### 3.2 `Enter` and `l` are two paths, not one verb

Both published keymaps state that `Enter` is the same motion as `l`.

- `l` on a file emits `TuiMsg::FilesDescend`, which reaches `descend_to`'s
  `PeekStep::Open` arm. That arm sets the pending editor handoff **and** calls
  `open_editor`.
- `Enter` on a file emits `TuiMsg::FilesOpenSelected`, which calls `open_editor`
  directly and **never** arms the handoff.

So the two verbs differ in exactly the way the documentation says they do not.

### 3.3 The handoff releases the tree pane and not the side panel

`hand_off_to_editor` calls `set_focus(app, false)` — which clears the **Ctrl+T tree
pane's** focus flag — and then enters the editor in Normal mode. It never touches the
side panel's focus. `commands::panels::blur_panel` is the app-side operation that
releases the panel and restores the composer's mode, and it is not called.

The consequence: opening a file from the docked Files panel opens the editor and
leaves the **panel** holding the keyboard, so the user's next keystrokes go to the
tree rather than to the document they just opened.

### 3.4 The editor's mode is already correct

`open_editor` → `open_editor_in` → `enter_editor_normal` sets `InputMode::Normal` and
`EditorMode::Normal`, routes to `Route::Editor`, and the document's reset puts the
editor's own mode back to `EditorMode::Normal` (the default).

**The editor already starts in Normal mode.** This is recorded as a finding, not a
defect: the missing half of the requirement is the focus switch. A test must pin the
mode so a later change cannot silently regress it.

### 3.5 The published keymap is not one keymap

Three surfaces publish the navigator's verbs: the widget's `HINT` constant, the
`architecture/tui.md` table, and the `docs/tui.md` table.

- The hint describes `l` as "open" and `Enter` as "open"; the tables describe `l` as a
  **peek** — the directory opens in place and the cursor moves into it.
- The hint says `⇧Q close`; the tables say `Shift+Q` closes the box or leaves the pane.
  After this milestone the hint must also name `q`.

The hint's own doc comment says it is kept in one place "so the box, its hosted panel,
and the help dialog cannot disagree about what the keys do" — and nothing enforces
that.

## 4. Invariants that must not regress

- **I1 — `Esc` remains the mode switch and never an exit.** It leaves the filter's
  insert mode and keeps the filter.
- **I2 — `Shift+Q` remains the panel's exit**, and remains the way out of the docked
  panel.
- **I3 — `Ctrl+B` remains the sidebar toggle.** It is not repurposed and not shadowed.
- **I4 — The program's quit arm is unchanged.** `q` remains the program-exit key
  everywhere outside the `/files` modal box.
- **I5 — The tree pane's keymap is unchanged.**
- **I6 — The editor's own modes and keymap are unchanged.**
- **I7 — The daemon listing authority is unchanged.**

## 5. Scope

### In scope

- The `/files` widget's `q` verb, and its effect (and non-effect) in the docked panel.
- The editor handoff: which surfaces release the keyboard, and on which verbs.
- The published keymap: the widget's hint, `architecture/tui.md`, `docs/tui.md`.
- A static guard that keeps those three from drifting apart.

### Explicitly out of scope

- Any change to the tree pane's keymap or chrome.
- Any change to the editor's modes, keymap or undo.
- `Ctrl+B`, `Ctrl+T`, `Shift+Q` and every other key's meaning.
- The panel host's chrome contract.
- The `/files` box's other verbs — M001 owns them.

## 6. Required production changes

### Core/domain

None.

### Protocol and DTOs

None.

### Runtime and concurrency

None.

### Frontend surface

- **`q` closes the modal box only.** The widget cannot know whether it is modal or
  hosted, so the intent is resolved by the app: a dedicated intent that closes the
  `/files` **dialog** when one is mounted, and does nothing otherwise.
  It must not reuse the generic `CloseDialog` intent in the docked case, because that
  intent pops the modal stack, and popping an unrelated modal would close the wrong
  thing. The `/plan` box already documents that failure mode for a docked box.
- **In the docked panel `q` does nothing.** The panel stays docked and focused;
  `Shift+Q` remains its exit; `Ctrl+B` remains the column toggle. This is the user's
  decision and is recorded so a later reader does not "fix" it back.
- **`Enter` on a file and `l` on a file become one verb.** Both arm the same pending
  editor handoff before opening.
- **The handoff releases every surface that held the keyboard**: the side panel and
  the tree pane, then the editor in Normal mode. The editor's Normal mode is already
  correct and is pinned by a test rather than re-implemented.
- **The published keymap is one keymap.** The hint, the `architecture/tui.md` table and
  the `docs/tui.md` table describe the same verbs, and `l` is described as the peek it
  is.

### Security and authorization

None.

### Documentation and static guards

- A static guard asserts that the navigator's published keymap and the widget's hint
  describe one set of verbs. It is modelled on `scripts/check_tui_editor_doc_drift.py`,
  reads the code's own model rather than re-parsing it, carries a `--self-test` that
  proves it is not vacuous, and is registered in **both** `scripts/verify.sh`'s
  `run_guards()` **and** `.github/workflows/ci.yml`.

## 7. Ordered work packages

### Work package A — The exit verb

Intent: `q` closes the `/files` modal box, and provably does nothing in the docked
panel.

Required changes: a dedicated intent, resolved by the app against the mounted
`/files` **dialog**. The widget's `q` arm emits it. The `Shift+Q` arm is unchanged.

Acceptance evidence:

- `q` in the modal box closes it — proven through the real key path.
- `q` in the docked Files panel leaves the panel mounted and focused, and does not
  close an unrelated modal.
- `q` outside a `/files` box still reaches the program's quit arm unchanged.

### Work package B — One verb for opening a file

Intent: `Enter` and `l` on a file are the same motion.

Required changes: the `Enter`-on-a-file path arms the same pending editor handoff the
`l`-on-a-file path arms, so one completion path serves both.

Acceptance evidence: a test asserts that both verbs leave the same pending-handoff
state, and that both end with the editor open, the side panel released, the tree pane
released, and the editor in Normal mode.

### Work package C — Release every holder of the keyboard

Intent: the editor, not the panel, has the keyboard after a file is opened.

Required changes: the handoff releases the side panel through the app-side panel
operation that already exists, in addition to the tree pane it already releases.

Acceptance evidence: an integration test opens a file from the docked Files panel and
asserts that the panel no longer holds the keyboard, that the tree pane does not either,
and that the editor's mode is Normal.

### Work package D — One published keymap

Intent: the hint and the two documents describe one set of verbs.

Required changes: the hint names `q` and describes `l` as the peek; the `architecture`
and `docs` tables name `q` for the box and keep `Shift+Q` for the panel; the hint
stays the single source the panel host and the help dialog read.

Acceptance evidence: the static guard passes, and the guard's `--self-test` proves it
rejects an injected divergence.

### Work package E — The guard is registered

Intent: the divergence cannot come back.

Required changes: the guard is added to `scripts/verify.sh`'s `run_guards()` **and** to
`.github/workflows/ci.yml`'s `verify` job. The counts in `AGENTS.md` and
`architecture/overview.md`'s Verified-counts table are re-measured with
`python3 scripts/measure_overview_counts.py` and the reconciliation recorded in
`architecture/count_audit_history.md`.

Acceptance evidence: `scripts/verify.sh quick` is green with the guard registered, and
the guard's failure mode is demonstrated on an injected divergence.

## 8. Failure, cancellation, restart and contention semantics

- **The editor open is asynchronous.** The handoff is armed before the open and fired
  when the open completes, so a failed open must **not** release the keyboard — the
  user keeps the tree, which is where they still are. This is the existing handoff's
  contract and must be preserved.
- **An already-open document** takes the open's short-circuit path. The handoff must
  still fire, so the keyboard still moves to the editor.
- **No concurrency**: the TUI is single-threaded.

## 9. Compatibility and migration

- No persistence, no migration, no protocol, no configuration.
- The user-visible changes are two: `q` now closes the `/files` box, and opening a
  file now moves the keyboard to the editor from every `/files` surface. Both are the
  requested behaviour.
- `Shift+Q` is retained everywhere it already worked, so nothing that muscle memory
  already knows breaks.

## 10. Required tests

### Focused unit tests

- `q` in the widget's Normal mode emits the exit intent; in its filter insert mode it
  types a `q` into the filter and emits nothing.
- The exit intent closes a mounted `/files` dialog and does nothing when none is
  mounted.
- `Enter` on a file and `l` on a file leave the same pending-handoff state.

### Integration tests

- `q` closes the modal `/files` box.
- `q` in the docked Files panel leaves the panel mounted and focused.
- Opening a file from the docked panel: the editor opens, the panel releases the
  keyboard, the tree pane releases it, and the editor is in Normal mode.
- Opening a file with `Enter` and with `l` produce the same observable outcome.

### Restart and recovery tests

- None.

### Contention and cancellation tests

- A **failed** editor open does not release the keyboard.

### Security and negative tests

- None. No new authority and no new path resolution.

### Migration and compatibility tests

- None.

## 11. Required verification commands

```bash
# narrow tests first
cargo test --test tui_render -- files_exit_verb
cargo test --test tui_render -- files_editor_handoff
cargo test -p codegg --lib tui::components::dialogs::files
cargo test -p codegg --lib tui::commands::file_tree

# static guards
python3 scripts/check_tui_files_keymap_drift.py
python3 scripts/check_tui_files_keymap_drift.py --self-test
python3 scripts/rust_test_scan.py --validate

# formatting and linting
cargo fmt --all
CARGO_BUILD_JOBS=3 cargo clippy --workspace --all-targets --locked -- -D warnings

# the canonical gate
bash scripts/verify.sh quick
```

Do not claim commands that were not actually run in the closure record. The workspace
test sweep and `scripts/verify.sh full` are **not** required for this milestone.

## 12. Documentation updates

- `architecture/tui.md` — the `/files` key table: name `q` for the box, keep `Shift+Q`
  for the panel, describe `l` as the peek, and state that `Enter` and `l` are one verb.
- `docs/tui.md` — the browsing-files table, to the same effect.
- `AGENTS.md` and `architecture/overview.md` — the guard counts, re-measured.
- `architecture/count_audit_history.md` — the reconciliation record.

## 13. Acceptance criteria

1. `q` closes the `/files` modal box.
2. `q` in the docked Files panel does nothing: the panel stays mounted and focused, and
   `Shift+Q` remains its exit.
3. `q` outside a `/files` box is still the program's quit key.
4. Opening a file from any `/files` surface — with `Enter` or with `l` — opens the
   editor, releases the keyboard from the side panel and the tree pane, and leaves the
   editor in its Normal mode.
5. The hint, `architecture/tui.md` and `docs/tui.md` describe one set of verbs, and a
   static guard with a non-vacuous `--self-test` keeps them from drifting.
6. `scripts/verify.sh quick` is green and workspace clippy is clean.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- making `q` close the box would require changing the program's quit key or the
  `Shift+Q` contract;
- the exit intent would have to pop a modal stack that does not contain the `/files`
  box;
- releasing the side panel on the editor handoff would require changing the panel
  host's focus contract or the composer's mode rules;
- the keymap guard would have to parse Rust with a regular expression — the shared
  Rust-aware scanner is the ground truth, not a regex;
- the change would expand into the editor, the tree pane or the panel host's chrome.

## 15. Closure evidence required

- The acceptance criteria, each with the test or command that proves it.
- The guard's `--self-test` output, and a demonstration that it rejects an injected
  divergence.
- The published keymap's final text, quoted, from all three surfaces.
- `scripts/verify.sh quick` and the clippy result, labelled local or hosted truthfully.
- The re-measured guard and test counts and the reconciliation record.
- The unblock audit: whether any registered plan's blocker is now discharged.

## 16. Handoff notes

- **Baseline**: `6e097dd3a` plus Milestone M001 of this addendum on
  `fix/file-picker-surface-corrective`.
- **Closed decisions this milestone must not reopen** — all recorded in the roadmap's
  §14 and in M001's §16:
  - `q` is the program's quit key and `Shift+Q` is the panel exit, established by
    `tui-buffer-chrome-and-panel-navigation` M004. A bare `q` in a focused panel is
    inert; the only addition is the modal box.
  - `Esc` is the mode switch, never an exit.
  - `Enter` and `l` are one motion, established by the same roadmap's M001.
  - The panel's keymap and chrome contract, established by `file-picker-rework` M001.
- **Resource constraint**: use `CARGO_BUILD_JOBS=3`; run the narrow test targets, not
  the workspace sweep.
- **Guard registration is part of the work**, not a follow-up: a guard that is not in
  `scripts/verify.sh` **and** in `.github/workflows/ci.yml` does not exist.
- **The count tables are measured, not estimated**: run
  `python3 scripts/measure_overview_counts.py` and record the reconciliation.
- **Comment budget**: `scripts/check_comment_budget.py` is a ratchet that forbids
  growth; if the change adds comments, lower the budget in the same commit, and
  re-bank after `cargo fmt --all`.
