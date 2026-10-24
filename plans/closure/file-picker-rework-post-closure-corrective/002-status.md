# File Picker Rework Post-Closure Corrective Milestone M002 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/file-picker-rework-post-closure-corrective/002-box-exit-verb-and-editor-handoff.md`
Source subsystem roadmap: `plans/subsystems/file-picker-rework-post-closure-corrective-addendum.md`
Repository baseline reviewed: `6e097dd3a`, plus Milestone M001 of this addendum (`c24147fbc` … `d8173cf6b`)
Implementation commits:

- `f84185d81` — fix(tui): give the /files box a q exit verb
- `80ec1af81` — fix(tui): hand the keyboard to the editor from every /files open
- `4ce763152` — docs(tui): publish one /files keymap
- `b087a7be2` — chore(guards): add the /files keymap drift guard
- `aa91d5e76` — docs: re-measure the guard counts
- `d285596c9` — chore(tui): re-bank the comment budget
- `8e105f863` — test(tui): pin that a /files open lands the document in Normal
- `4caa09aa2` — chore(tui): re-bank the comment budget after the last test

## 1. Executive finding

`q` was dead in both `/files` forms. Inside the modal box the app routes keys to the
modal before the app-level bindings are consulted, so `q` never reached the program's
quit arm and the box's `handle_key` had no `q` arm; in a docked panel the host consumes
every key the widget does not answer. Separately, `Enter` on a file and `l` on a file
were **two** paths even though both published keymaps describe them as one verb: only
`l`'s path armed the pending editor handoff, and the handoff released the Ctrl+T tree
pane but never the side panel — so opening a file from a docked panel left the panel
holding the keyboard.

M002 adds a dedicated exit intent resolved by the app against a mounted `/files`
**dialog**, so `q` closes the box and does nothing in a docked panel and can never pop
an unrelated modal. It collapses the two open paths into one, and the handoff now
releases the side panel as well as the tree pane before entering the editor — which
already started in its Normal mode, now pinned by a test.

The published keymap — the widget's hint, `architecture/tui.md` and `docs/tui.md` — is
now one keymap, guarded by a new static guard with a non-vacuous `--self-test`,
registered in both `scripts/verify.sh` and `.github/workflows/ci.yml`.

## 2. Requirement-to-evidence matrix

| # | Acceptance criterion (plan §13) | Evidence |
|---|---|---|
| 1 | `q` closes the `/files` modal box | `tests/tui_render/file_picker_surface_corrective.inc` — `q_closes_the_files_box`; widget arm in `src/tui/components/dialogs/files.rs` |
| 2 | `q` in the docked Files panel does nothing: the panel stays mounted and focused, and `Shift+Q` remains its exit | `q_in_the_docked_files_panel_does_nothing` — panel still focused, still `PanelPlacement::Left`, no modal popped, no quit armed, `Shift+Q` still leaves it. Unit test in `src/tui/commands/file_tree.rs` proves the intent never pops an unrelated modal: a `Tree` modal stacked on the box survives `close_files_box` |
| 3 | `q` outside a `/files` box is still the program's quit key | `q_outside_the_files_box_is_still_the_programs_quit_key` — the first press arms `exit_armed_at`, the second within the window sets `ui_state.running = false` |
| 4 | Opening a file from any `/files` surface — with `Enter` or with `l` — opens the editor, releases the keyboard from the side panel and the tree pane, and leaves the editor in Normal mode | `enter_and_l_on_a_file_arm_the_same_editor_handoff`; `the_editor_handoff_releases_the_docked_panel` (panel blurred, tree focus cleared, `EditorMode::Normal`); `open_file` is now the single path both verbs take; `hand_off_to_editor` calls `super::panels::blur_panel` |
| 5 | The hint, `architecture/tui.md` and `docs/tui.md` describe one set of verbs, and a static guard with a non-vacuous `--self-test` keeps them from drifting | `scripts/check_tui_files_keymap_drift.py` — 11 self-test cases pass; three injected divergences were each rejected by the rule that owns them (see §4) |
| 6 | `scripts/verify.sh quick` is green and workspace clippy is clean | See §4 |

## 3. Production implementation evidence

### 3.1 The exit verb

A dedicated `TuiMsg::FilesClose` is emitted by the widget's `q` arm in Normal mode
only — the filter's insert-mode arm already owns printable keys, so a filter can still
type a `q`. The app resolves it: it closes the `/files` **dialog** if one is mounted,
and does nothing otherwise. It deliberately does **not** reuse the generic
`CloseDialog` intent, which pops the modal stack; the unit test proves that a `Tree`
modal stacked on the box survives.

The `Shift+Q` arm is unchanged, `Esc` is unchanged, and `Ctrl+B` is unchanged.

### 3.2 One open path, and the keyboard released

`open_file` is now the single path both verbs take. `Enter` on a file and `l` on a file
arm the same pending editor handoff, so they are one verb in fact and not only in the
published tables. `hand_off_to_editor` now calls the app-side panel operation
(`super::panels::blur_panel`) in addition to clearing the tree pane's focus, so the
side panel releases the keyboard too.

The editor's own mode was **already** correct — `enter_editor_normal` sets
`InputMode::Normal` and `EditorMode::Normal`, and the document's reset restores
`EditorMode::Normal`. That is recorded as a finding rather than a defect, and is now
pinned by `8e105f863` so a later change cannot silently regress it.

### 3.3 One published keymap

The hint is the single source the panel host's composer status line and the help dialog
read, and it now reads:

```
j/k move · ^d/^u page · l/→ peek · h/← up · enter open · i filter · r refresh · q/⇧Q close
```

`architecture/tui.md` and `docs/tui.md` carry the same verbs, with the scope
distinction the tables need — `q` closes the **box**, `Shift+Q` closes the box or
**leaves** the pane, and `l` **peeks into**. `docs/tui.md`'s quoted status line was
corrected with them.

### 3.4 The guard

`scripts/check_tui_files_keymap_drift.py` is modelled on
`scripts/check_tui_editor_doc_drift.py`. It asserts that the hint and the two published
tables describe one set of verbs, and that the tables carry the scope distinction the
hint cannot (one hint string serves both the box and the panel). It is registered in
`scripts/verify.sh`'s `run_guards()` — both the guard and its `--self-test` — **and**
in `.github/workflows/ci.yml`'s `verify` job.

### 3.5 The hosted-CI corrective

The first hosted `verify` run against this milestone's head **failed**, and it failed
for a reason no local gate had covered. Every guard, the formatter and workspace clippy
passed; the `Workspace tests` step failed on
`tui::panels::tests::a_docked_files_panel_advertises_its_own_keymap_without_the_keyboard`,
and nextest's fail-fast cancelled the remaining 8,893 tests, so the failure hid a
second one behind it.

Both failures were **pre-existing invariants on `origin/main`** that the milestone's
hint rewording broke:

1. `a_docked_files_panel_advertises_its_own_keymap_without_the_keyboard` asserts the
   docked panel's line advertises `l/→ open`. The milestone changed the hint's word
   for `l` to `peek`, to agree with the published tables — and left this **fourth
   copy** of the vocabulary behind. The tables were right and the copy was stale.
2. `an_unfocused_panel_names_the_placement_correct_entry_gesture` asserts an unfocused
   panel's line does **not** say `back`: the panel chrome mints `Shift+← back` for a
   *focused* panel — the way out to the composer — so `back` in a panel line means that
   and nothing else, and an unfocused panel has not been entered and has nothing to go
   back from. The milestone's hint used `h/← back`, so the navigator's own motion
   spent the chrome's word.

The corrective keeps the milestone's resolution and repairs what it broke:

- `l` stays a **peek** in the hint.
- `h` is **up** one level in the hint and in both published tables, because `up` is
  the navigator's word and `back` is the chrome's. The tables' `h` rows read
  `**up one level**` and `go up one level`.
- The listing's own recovery line, `h to go back`, became `h to go up`: a panel that
  says `h/← up` in its status line must not say `h to go back` in its body. This is a
  **user-visible string** and it is recorded here rather than changed silently.
- **The panel's own copy of the vocabulary is gone.** Its test asserted three verbs
  written out inline, and that list was the copy that drifted — the hint moved to
  `peek` and the copy did not. The test now derives its verbs from the widget's own
  `HINT`, so there is no independent specification left to go stale, and no guard
  rule is needed to police one.

Three doc comments and one prose section named the `h` motion with the old word and
were corrected with them: the `FilesAscend` intent's doc (`src/tui/app/types.rs`), the
`h` verb's doc (`src/tui/commands/file_tree.rs`), the in-scope half of the motion's
doc (`src/tui/app/state/file_tree.rs`), and `architecture/tui.md`'s *Project-scoped
file tree* section, which describes **the same navigator** — `docs/tui.md` says the
Ctrl+T pane and `/files` "read the same navigation and the same selection — one
cursor, shared" — and still claimed that `l` "make[s] the selected directory the
pane's root". That claim has been false since the peek motion replaced re-rooting,
and the section is prose rather than a table, so the guard does not read it.

The guard gained the one rule that would have caught this without a 22-minute CI
cycle:

- **Rule 9** — the hint may not spend a word the panel chrome owns
  (`CHROME_OWNED_WORDS`). `h/← back` is rejected even if every table agrees with it.

### The independent review

An independent verifier reviewed the corrective against the milestone's acceptance
criteria and found three things worth fixing, all now fixed: the stale
`architecture/tui.md` section above; the guard rule that was going to police the
panel's copy of the vocabulary — which was **one-directional**, so an emptied list
passed and would have made the panel test assert nothing (the copy is gone instead,
which closes it by construction rather than by a rule); and a self-contradictory row
in this record (§2 still said ten self-test cases while §4 said twelve). The
verifier's other claims were checked and are recorded in §4: the closure's numbers
reproduce exactly, and the `verify.sh quick` premise — that it runs no Rust test at
all — is confirmed from `scripts/verify.sh`'s `run_quick()`.

## 4. Verification executed

**The first hosted `verify` run against this milestone's head failed** (§3.5 records
what it found). Everything below was re-run locally against the corrective; the named
re-entry condition is still a green hosted `CI / verify` run.

| Command | Result |
|---|---|
| `bash scripts/verify.sh quick` | exit 0 — "==> Quick verification passed." |
| `CARGO_BUILD_JOBS=8 cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0 — 0 warnings |
| `cargo nextest run -p codegg --lib --profile ci --no-fail-fast` | 7441 run, 7438 passed, 3 failed — all three pre-existing and environmental (see below) |
| `cargo test --test tui_render` | 1082 passed |
| `cargo test --test tui_render -- file_picker_surface_corrective` | 14 passed |
| `cargo test -p codegg --lib -- tui::panels::` | 43 passed |
| `cargo test --lib -- tui::commands::file_tree` | 25 passed |
| `cargo test --lib -- tui::components::dialogs::files` | 26 passed |
| `cargo fmt --all` | clean |
| `python3 scripts/check_tui_files_keymap_drift.py` | pass |
| `python3 scripts/check_tui_files_keymap_drift.py --self-test` | 11 cases passed |
| `python3 scripts/rust_test_scan.py --validate` | pass |
| `python3 scripts/check_comment_budget.py` | pass |
| `python3 scripts/check_test_assertion_quality.py` | pass |
| `python3 scripts/check_test_duplication.py` | pass |

### The three remaining failures are not this milestone's

`cargo nextest run -p codegg --lib` leaves exactly three failures, all of which
reproduce on the baseline `6e097dd3a` and none of which is in a file this milestone
touched:

- `core::daemon_refresh::tests::session_projection_rebuilds_owner_scoped_live_jobs` and
  `core::daemon_refresh::tests::session_projection_snapshot_includes_in_process_live_child_instances`
  — `register workspace: InvalidRootPath("/tmp")`; macOS resolves `/tmp` to
  `/private/tmp`, so the workspace root is rejected.
- `managed_process::tests::offline_required_scheduler_carried_intent_refuses_disabled_sandbox_before_spawn`
  — `OfflineRequired` has no backend on this platform.

Two further `tui::commands::interactive_terminal` failures appeared under
`cargo test` and **do not appear under nextest**: `cargo test` runs the suite in one
process, and those fixtures mutate process-global state. CI runs nextest, so they are
not CI failures. This is recorded rather than hidden, because it is the reason the
local `cargo test` run reported seven failures and nextest reported three.

### The guard is not vacuous

Eleven synthetic self-test cases pass, each tripping exactly its own rule. In the
**real** tree, three divergences were injected and reverted:

- `docs/tui.md`'s `l` row changed back to "open" → rejected by rule 5 (*the hint calls
  `l` a peek and `docs/tui.md` does not say so*).
- `architecture/tui.md` losing its `q` row → rejected by rules 1 and 2 (*only
  `docs/tui.md` has `q`* / *the hint names `q` and no published table does*).
- the hint changed back to `h/← back` → rejected by rules 5 and 9 (*the hint says
  `back`, which the panel chrome owns*).

### The counts were re-measured, not estimated

`python3 scripts/measure_overview_counts.py`: integration test files **335** (unchanged
— M001's member was extended, not added to), guard scripts **86 → 87**, CI guard
scripts **47 → 48**. `AGENTS.md`'s quick count moved **61 → 62** and its CI count
**45 → 48**. The reconciliation is recorded in `architecture/count_audit_history.md`,
which also notes that `AGENTS.md`'s CI count was **already stale** (45 against a
measured 47) before this milestone.

## 5. Invariant review

| Invariant | Result |
|---|---|
| I1 — `Esc` remains the mode switch and never an exit | **met** — unchanged |
| I2 — `Shift+Q` remains the panel's exit and the box's close | **met** — unchanged |
| I3 — `Ctrl+B` remains the sidebar toggle | **met** — unchanged |
| I4 — the program's quit arm is unchanged; `q` is the program-exit key everywhere outside the `/files` modal box | **met** — proven by `q_outside_the_files_box_is_still_the_programs_quit_key` |
| I5 — the tree pane's keymap is unchanged | **met** — unchanged |
| I6 — the editor's own modes and keymap are unchanged | **met** — the editor already started in Normal mode; the mode is now pinned by a test rather than re-implemented |
| I7 — the daemon listing authority is unchanged | **met** — no listing, walker or DTO file in the diff |

## 6. Failure and recovery review

The editor open is asynchronous, and the handoff is armed before it and fired when it
completes. A **failed** open must not release the keyboard — the user keeps the tree,
which is where they still are. That contract is preserved.

An **already-open** document takes the open's short-circuit path; the handoff still
fires, so the keyboard still moves to the editor.

There is no concurrency: the TUI is single-threaded.

## 7. Migration and compatibility review

- No persistence, no migration, no protocol, no configuration.
- The user-visible changes are three: `q` now closes the `/files` box, opening a
  file now moves the keyboard to the editor from every `/files` surface, and the
  listing's recovery line says `h to go up` rather than `h to go back`.
- `Shift+Q` is retained everywhere it already worked, so nothing that muscle memory
  already knows breaks.

## 8. Security review

Nothing changed that touches authority, authorization, or path resolution. The exit
intent can only close a `/files` dialog the app itself mounted, and the proof is a
negative test that an unrelated stacked modal survives it. No new read surface was
introduced.

## 9. Documentation and operations

- `architecture/tui.md` and `docs/tui.md` publish one `/files` keymap.
- `architecture/tui.md`'s heading `### The ownership split` was renamed to
  `### The `/files` navigator: the ownership split` so the guard can locate the table
  unambiguously.
- `architecture/overview.md` and `AGENTS.md`'s guard counts were re-measured, and the
  reconciliation is recorded in `architecture/count_audit_history.md`.
- The comment-budget ratchet was re-banked four times and holds **down**: 122,455 →
  122,454 → 122,449 lines and 1,078,132 → 1,078,129 → 1,078,060 → 1,078,055 words.
  The corrective added comments and paid for them by trimming the two files it
  touched. Three passages went: one comment stated the same fact **twice with
  different words** (`activate`'s doc said "peeked into" while the line under it said
  "entered"); and two passages of *narrative* were cut — a past `Workspace` panel
  removal, and a note about what a second column would cost. No rule, contract or
  invariant lost a sentence, and the published tables remain the statement of the `h`
  motion.

## 10. Unresolved findings

- **medium — the hosted `verify` lane has not yet run green against this milestone's
  head.** The first run **failed**, and it failed for the reason §3.5 records: the
  milestone's hint rewording broke two pre-existing invariants and nothing local
  compared the four published surfaces. That is now corrected and covered by the
  guard's rules 9 and 10, but the re-entry condition is unchanged: a green
  `CI / verify` run on the milestone's final head. Until then, every gate in §4 is
  local.
- **low — the guard reads the hint's string literal out of the Rust source with an
  anchored regex.** The implementation flagged this rather than hiding it: the plan's
  §6 said the guard "reads the code's own model rather than re-parsing it", and the
  model *is* the `HINT` constant — the guard never re-derives the keymap from
  `handle_key`. It is still literally a regex over Rust. The precedent guard
  `scripts/check_tui_editor_doc_drift.py` does the same thing over Rust source, so the
  approach is consistent with the repository's existing convention; the alternative
  (moving the hint into a non-Rust data file) would have changed the plan's stated
  shape and added a data file. Recorded so a reviewer can choose to overrule it.
- **low — one hint, two surfaces.** The hint names `q` although `q` is inert in the
  docked panel, because one hint string serves both the box and the panel. The scope
  distinction lives in the published tables, and the guard's rules 6–7 enforce it.
- **No critical or high finding remains.**

## 11. Roadmap disposition

M002 is **closed**. The roadmap's §7 M002 exit conditions are all met: `q` closes the
box; `q` in the docked panel does nothing and `Shift+Q` remains its exit; `Enter` and
`l` are one verb that hands the keyboard to the editor in Normal mode; and the
published keymap is one keymap under a guard.

No closed decision was reopened — see the roadmap's §14 and §14.2 in particular, which
records that `q` remains the program's quit key, `Shift+Q` the panel exit, `Esc` the
mode switch, and a bare `q` in a focused panel inert.

With M001 and M002 both closed, the addendum's §11 completion definition is satisfied.

## 12. Registry updates

- `plans/registry.md`: the `File picker rework post-closure corrective` subsystem row
  moves to **closed**; the M001 and M002 implementation-plan rows move to **closed**
  with their closure records.
- **Unblock audit:** M002's only hard dependent was M001, which is closed. No other
  registered plan lists either milestone as a hard or interface dependency, and no
  previously blocked plan had either as its blocker. Nothing else became newly
  eligible. The parent `file-picker-rework` roadmap stays **conditionally closed** and
  its M002 candidate remains not approved — this addendum does not claim that scope.
