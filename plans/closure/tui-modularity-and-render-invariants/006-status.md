# TUI Modularity and Render Invariants Milestone 006 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/tui-modularity-and-render-invariants/006-advertised-keys-are-handled.md`

Source subsystem roadmap:

- `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md#milestone-006--every-advertised-key-is-handled`

Repository baseline reviewed: `7642b9e38` (`feat-auto-20261109-f8abad0d`), plus
uncommitted working-tree work — **235 changed or untracked paths** at the time of
review (measured, `git status --porcelain | wc -l`)

Implementation commits or pull requests:

- **None.** This milestone exists only as uncommitted working-tree work on
  `feat-auto-20261109-f8abad0d`, on top of HEAD `7642b9e38`. There is no commit,
  branch or pull request carrying it, and the same tree carries M001–M005,
  M008–M010 and the Helix editor line, so no statement below is attributable to a
  SHA.

Provenance labels used throughout: **measured** means this record's author read
it out of the tree or ran the command at the stated tree state; **parent-run**
means the parent session ran it on this same tree and supplied the transcript,
which this record did not repeat; **source plan** means the statement is the
plan's requirement, not a measurement.

## 1. Executive finding

**The milestone's three named gaps are resolved or correctly retained, and its
generic invariant is not closed.** Those are two different exit conditions and
the record must not merge them.

The three recorded gaps in `tests/tui_render/dialog_dispatch_coverage.inc:5-12`
are now:

1. **Import `Char`/`Backspace` — closed.** `ImportDialog::handle_key` edits its
   input in `ImportState::Input` (`src/tui/components/dialogs/import.rs:312-336`),
   so a typed URL reaches the preview. Four unit tests pass, including
   `typing_reaches_the_input_and_backspace_pops_it`, and the dispatch-level test
   `the_import_dialog_takes_a_typed_url_and_offers_its_preview`
   (`dialog_dispatch_coverage.inc:59`) types a URL through `FocusManager` and
   asserts the input and the `Enter` message.
2. **`/shell-show`'s `i`/`a`/`r`/`k`/`e` — closed by removing the
   advertisement.** `InfoDialog::renders_caller_footer` is false for
   `InfoType::ShellShow` (`src/tui/components/dialogs/info.rs:172-177`), so the
   caller's six-verb row is not painted and the box substitutes
   `j/k scroll  |  Esc/Enter close` (`:321-333`). That is the plan's permitted
   alternative ("either implement the five verbs or remove them from the
   footer"), chosen because the verbs are app-layer intents needing a
   shell-detail id the widget is never handed. The decision is recorded in the
   `.inc` header and the test
   `the_shell_show_modal_advertises_only_keys_it_handles` reads the **painted**
   footer and asserts the five verbs are absent, with a scoping test proving
   `/project-init`'s working caller row still paints.
3. **Model dialog's `Tab` — retained, with its ADR-0042 reason.** Verified in the
   code, not just the comment: `FocusManager::handle_key`
   (`src/tui/components/component/focus.rs:164-170`) intercepts `Tab` before the
   top component sees it (`:171-175`), so the dialog's own tabs are unreachable by
   design. The plan puts this out of scope as a key-ownership decision
   (ADR-0042), and the record says so instead of calling it a missing handler.

The generic I4 check exists and is non-vacuous: `advertised_key_is_handled`
drives each advertised key through `FocusManager`, treats a returned `TuiMsg` as
handled, and otherwise requires a frame difference
(`tests/tui_render/invariants/baseline.rs:333-370`); the failing-fixture test
`the_i4_measurement_detects_an_advertised_key_that_does_nothing` (`:656`) is the
non-vacuity proof the plan's work package A requires.

**The milestone's exit condition was met by its corrective, not by this
milestone's own pass.** The plan's work package D and acceptance criterion 5
require the M001 `[advertised_keys]` baseline to shrink to the irreducible
residue. When this record was written the baseline held **61** entries across 18
surfaces; it now holds **four**, each carrying a `reason` the guard requires:
`Model`/`Tab` and `WorkspaceDashboard`/`Tab` (ADR-0042's `FocusManager`
ownership, so the dialog's own arm is unreachable through production dispatch),
and `SecurityReview`/`H` and `SecurityReview`/`h` (the verb writes the system
clipboard, which a render measurement must not do, and arming a hunk also arms
`H`). The reduction is the corrective the plan's own §8 anticipated:

- `plans/implementation/tui-advertised-keys-corrective/001-i4-baseline-to-its-irreducible-residue.md`
  — a fresh fixture per advertised key, a content-bearing fixture table used by
  I4 only, a per-key report, and `reason` as a required manifest field.
- `plans/implementation/tui-advertised-keys-corrective/002-structured-pairs-from-the-producer.md`
  — the measurement consumes the producer's own `(key, verb)` items
  (`HintLine::items()` via `Component::keymap_hint_line()`), with the painted-row
  scan retained as the completeness cross-check.
- `plans/implementation/tui-modularity-and-render-invariants/012-hint-producer-convergence.md`
  — the hint-vocabulary producer convergence that keeps the advertised rows
  honest.

Both corrective records are `conditionally closed`, and the roadmap row for this
milestone records the same status. The guard now reports "4 advertised-key, 0
contrast and 0 hint-truncation entries recorded; every one reproduces and none
was added unbanked", and the harness's per-key report names exactly the four
`INERT` rows — so the residue is measured, reasoned and read back out of the
run rather than counted.

Two further deviations belong in the executive finding rather than in a
footnote:

- **The harness parses rendered strings; the plan said not to.** Plan §2 says I4
  needs the advertised verbs as structured `(key, verb)` pairs because "parsing
  them back out of a rendered string is exactly the fragility this repository
  avoids". `measure_advertised_keys` parses both `Component::keymap_hint()`
  strings and rendered footer rows (`baseline.rs:348-390`), and the baseline is
  keyed by **spelling** — the architecture document itself notes that
  consequence (`architecture/tui.md:4405-4407`). The check works and is
  non-vacuous, but it is the approach the plan rejected, and it is why the
  exception set is spelling-sensitive.
- **`ImportDialog` bounds nothing, on either path, deliberately.**
  `handle_key`'s doc comment (`import.rs:312-315`) states that neither `Char`
  nor `Backspace` bounds the input because `handle_paste` bounds nothing either,
  and `typing_and_pasting_reach_the_input_at_the_same_bound` asserts parity at
  4,096 characters. That is the plan's stop condition handled correctly — report
  the missing bound rather than invent one — but it means the dialog has no
  length ceiling at all, which is recorded as a finding in §10.

Status is **`conditionally closed`**. The in-scope implementation item this
record asked for — work package D and its acceptance criterion — has been
delivered by the corrective named above: the baseline is four reasoned rows
instead of 61 unexplained ones, and it is read structurally from the producer
with the painted scan kept as a cross-check. What remains is not a gate and not
a missing implementation in this milestone: a **commit** carrying the tree, and
the **advertised-keys residue** the corrective itself defers — WP C's footer
conversion and WP E's parser widening (fixture work), both named in
`plans/closure/tui-advertised-keys-corrective/002-status.md` §10. The gate class
is also exhausted: `scripts/verify.sh full` passed on this tree at 2026-11-13
(17,377 workspace tests, 8,441 feature-pass tests, 0 failures; fmt and clippy
clean; corpus banked at 139,245 comment lines), which subsumes the `quick` this
record cited as parent-run.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| A generic I4 assertion: for every advertised `(key, verb)` pair, pressing the key changes the frame or closes the surface | `advertised_key_is_handled` (`tests/tui_render/invariants/baseline.rs:333-346`) over `measure_advertised_keys` (`:348-390`); `the_recorded_i4_baseline_is_exactly_what_the_harness_measures` (`:542`) | **partial** | The check exists and runs over every fixture; it passes only by matching 61 banked exceptions. |
| The check is non-vacuous, proved by a fixture that advertises a key it ignores | `the_i4_measurement_detects_an_advertised_key_that_does_nothing` (`baseline.rs:656`) | pass | Part of the render corpus, measured green (parent-run). |
| `ImportDialog` accepts typed characters and `Backspace` | `import.rs:319-326`; `typing_reaches_the_input_and_backspace_pops_it`; 4 unit tests passed (measured) | pass | Scoped to `ImportState::Input`. |
| `ImportDialog`'s `Enter` still submits a preview | `import.rs:327-333`; `enter_submits_a_preview_then_confirms_it` (in the 4 passed) | pass | |
| The typed path is bounded consistently with paste | `typing_and_pasting_reach_the_input_at_the_same_bound` (4,096 chars, both paths); doc comment `import.rs:312-315` | pass | The shared bound is **none**; the parity is the property. §10. |
| A typed URL reaches the preview through the real dispatch | `the_import_dialog_takes_a_typed_url_and_offers_its_preview` (`dialog_dispatch_coverage.inc:59-82`) | pass | Asserts the input string and `Some(TuiMsg::SubmitImportPreview)`. |
| `/shell-show` advertises only verbs it handles, or handles all it advertises | `renders_caller_footer` (`info.rs:172-177`); footer substitution (`:321-333`); `the_shell_show_modal_advertises_only_keys_it_handles` (`dialog_dispatch_coverage.inc:193`) | pass | Chosen branch: **stop advertising**. |
| The substitution is scoped to `/shell-show` | `a_caller_footer_still_paints_where_its_verbs_are_dispatched` (`dialog_dispatch_coverage.inc:222`) | pass | `/project-init`'s `a: publish` still paints. |
| The Model dialog's `Tab` is retained with an ownership reason, not a TODO | `FocusManager::handle_key` intercepts `Tab` first (`focus.rs:164-175`); header reason and ADR-0042 reference (`dialog_dispatch_coverage.inc:10-12`) | pass (retained) | Explicitly out of scope per plan §5; not a missing handler. |
| `dialog_dispatch_coverage.inc` records two closures and one retained gap with its ADR reference | Header `:5-12` | pass | Import and `/shell-show` closed; Model `Tab` retained. |
| The recorded gaps' tests still pass | `cargo test --test tui_render -- dialog_dispatch_coverage` → **7 passed, 0 failed**, 1220 filtered out | pass | Measured. |
| The M001 `[advertised_keys]` baseline contains **exactly** the retained entry | `docs/tui-render-invariant-baseline.toml`: **4** entries, each with a `reason`; `Model`/`Tab` and `WorkspaceDashboard`/`Tab` are the ADR-0042 rows, and the two `SecurityReview` rows are the clipboard rows | **pass, with a reasoned residue** | Delivered by corrective C001. Plan WP D and acceptance criterion 5 are met in the amended form ("the recorded residue, each entry carrying a reason"). |
| The I4 baseline is driven to empty except the retained entry | Same file; the ratchet guard passes at 4, and the per-key report names exactly those four `INERT` rows | **pass, with a reasoned residue** | Delivered by corrective C001; the four rows are irreducible under ADR-0042 and the no-clipboard-write rule. |
| The two key-authority guards report unchanged chord counts | `check_tui_composer_key_authority.py` **pass**; `check_tui_editor_key_authority.py` **pass** — "2 guard-block, 92 Normal-mode [9 digits claimed], 8 Insert-mode, no shadowed arm" | **partial** | Both pass. The editor guard reports **92** Normal-mode chords; the M005 record recorded **90**, and M006's two production changes add no chord (a dialog-local `Char`/`Backspace` arm and a footer suppression). The delta is not attributable from an uncommitted tree (§10). |
| `check_command_catalog_ownership.py` passes unmodified | **pass** — "command catalog ownership: clean"; no catalog change in this milestone | pass | ADR-0052 untouched. |
| The render-cost shape guard passes and is unmodified | **pass** — "the render path derives each result once"; `git status` shows the guard unmodified | pass | Measured. |
| The render corpus stays green (I1, I2, I3b, I6) | **1227 passed; 0 failed** on this tree (parent-run) | pass | The family's only movement is the tests later lanes added. |
| `scripts/verify.sh quick` is green | **pass** (parent-run): `==> Quick verification passed.` / `QUICK_RC=0`; 94 guards with `--self-test`s, 479 harness tests `OK (skipped=4)`, workspace check with no warnings | pass | |
| `cargo fmt --check`, `clippy -D warnings`, `scripts/verify.sh full` | **pass, 2026-11-13** — `scripts/verify.sh full` on this tree: 17,377 workspace tests, 8,441 feature-pass tests, 0 failures; fmt clean; clippy `-D warnings` clean | pass | §10, §11. |
| The comment ratchet holds | **banked**: 1,523 files / 138,518 lines / 1,233,740 words / ratio 0.1360; guard and `--self-test` pass (parent-run) | pass | Not outstanding for this milestone. |

## 3. Production implementation evidence

- `src/tui/components/dialogs/import.rs:312-336` — `handle_key` gains
  `Char`/`Backspace` in `ImportState::Input`, with a doc comment stating the
  no-bound decision and the reason: a bound on one path alone would refuse text
  the other accepts.
- `src/tui/components/dialogs/import.rs:338-341` — `handle_paste` remains
  unbounded, so the two paths have the same (absent) ceiling.
- `src/tui/components/dialogs/import.rs:516-565` — the three typed-input tests:
  typing/backspace, state scoping, and typed/paste bound parity.
- `src/tui/components/dialogs/info.rs:172-177` — `renders_caller_footer`, whose
  doc names the five keys and why no arm dispatches the *verbs* (each needs a
  shell-detail id the widget is never handed).
- `src/tui/components/dialogs/info.rs:319-333` — the `SectionFrame::Owned`
  footer selection: a caller row is used only when `renders_caller_footer` is
  true, otherwise the box paints `j/k scroll  |  Esc/Enter close`.
- `src/tui/components/dialogs/info.rs:387-412` — `handle_key`: `j`/`k`/arrows
  scroll, `a` is `ProjectInit` only, `Enter`/`Esc` close. No arm dispatches
  `i`/`r`/`e`, and no arm dispatches `a ask` or `k kill`.
- `tests/tui_render/dialog_dispatch_coverage.inc:1-12` — the header record: two
  closures and one retained gap with the ADR reference; `:59-82` the Import
  dispatch test; `:190-217` the painted-footer test; `:219-236` the scoping test.
- `tests/tui_render/invariants/baseline.rs:239-390` — the I4 harness, and
  `:542-560`, `:656-...` its recorded-baseline and non-vacuity tests.

No core/domain, storage, protocol, runtime or concurrency change: the plan's §6
marks all of them as none, and the two production edits are confined to two
dialog modules and one footer rule.

Nothing in the plan's "explicitly out of scope" list was implemented: no key
changed owner, no command or verb was added, and the Model dialog's `Tab` was
left alone.

## 4. Verification executed

### Commands run

Local, in `/Users/davidbowman/projects/codegg/.worktrees/feat-auto-20261109-f8abad0d`,
with `CARGO_BUILD_JOBS=2`:

```bash
cargo test --test tui_render -- dialog_dispatch_coverage
cargo test -p codegg --lib -- components::dialogs::import

python3 scripts/check_tui_render_invariant_baseline.py
python3 scripts/check_tui_composer_key_authority.py
python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_command_catalog_ownership.py
python3 scripts/check_tui_render_cost.py

grep -c '^\[\[advertised_keys\]\]' docs/tui-render-invariant-baseline.toml
grep -A1 '^\[\[advertised_keys\]\]' docs/tui-render-invariant-baseline.toml | grep '^surface' | sort | uniq -c
git status --porcelain scripts/check_tui_render_cost.py
```

### Results

| Command | Result |
|---|---|
| `cargo test --test tui_render -- dialog_dispatch_coverage` | **pass** — 7 passed, 0 failed, 1220 filtered out, 0.03 s |
| `cargo test -p codegg --lib -- components::dialogs::import` | **pass** — 4 passed, 0 failed, 8178 filtered out, 0.01 s |
| `check_tui_render_invariant_baseline.py` | **pass** — "61 advertised-key, 2495 contrast and 0 hint-truncation entries recorded; every one reproduces and none was added unbanked"; exit 0 |
| `check_tui_composer_key_authority.py` | **pass** — "the claim table is closed, single-owner, leaks no key to an app-level action, and every caret motion is bound the same way in both maps" |
| `check_tui_editor_key_authority.py` | **pass** — "2 guard-block chord(s), 92 Normal-mode chord(s) [9 digits claimed by a count arm: 123456789], 8 Insert-mode chord(s), no shadowed arm" |
| `check_command_catalog_ownership.py` | **pass** — "command catalog ownership: clean" |
| `check_tui_render_cost.py` | **pass** — "the render path derives each result once"; guard unmodified (`git status` empty for it) |
| baseline entry count | **61** entries across **18** surfaces; Model alone holds 3 (`Enter`, `Tab`, `↓`), so the plan's "exactly the retained entry" is not met |
| `dialog_dispatch_coverage` member total | 7 tests; 7 + 1220 filtered = **1227**, consistent with the corpus total below |

### Parent-run evidence

| Command | Result |
|---|---|
| `cargo test --test tui_render` | **1227 passed; 0 failed** on this tree |
| `scripts/verify.sh quick` | **pass** — `==> Quick verification passed.` / `QUICK_RC=0`; every guard including its `--self-test`, `python3 -m unittest discover -s dev/harness_lab/tests` → `Ran 479 tests` / `OK (skipped=4)`, `cargo check --workspace --all-targets --locked` with no warnings |
| comment-budget guard and `--self-test` | **pass**, banked at 1,523 files / 138,518 lines / 1,233,740 words / ratio 0.1360 |

### Not run

| Command | Why |
|---|---|
| `cargo test --test tui_render -- tui_render_invariants` | The plan's §11 names it, but the whole family was measured green (parent-run) and this record's bounded allowance was spent on `dialog_dispatch_coverage`. The I4 harness tests are inside that green family; no per-test count for them is claimed. |
| `cargo test --test tui_render -- connect_dialog_input` | Same reason; the `.inc` is part of the green family. |
| `scripts/verify.sh full` | Not run by this record. **Subsequently run and passed on this tree, 2026-11-13** — 17,377 workspace tests, 8,441 feature-pass tests, 0 failures; `run_full()` calls `run_quick()`, so the `quick` this record cited is subsumed. |
| `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` | Not run individually; formatting is inside `quick`, lint is `full`'s. |

**Every result is a property of this working tree at the time it was read**, not
of a committed state. The tree carried 235 changed or untracked paths.

## 5. Invariant review

- **No key changes owner (ADR-0042).** Holds for this milestone's changes: the
  two production edits are a dialog-local `Char`/`Backspace` arm and a footer
  suppression; neither edits `EDITOR_KEYMAP`, the composer claim table or the
  command catalog. Both authority guards and the catalog guard pass. The editor
  guard's Normal-mode count is 92 against the M005 record's 90, but the delta is
  not in this milestone's two production changes and is not attributable from
  the tree (§10).
- **No command catalog change (ADR-0052).** Holds —
  `check_command_catalog_ownership.py` is clean and the milestone adds no
  command.
- **No surface loses a working verb to make its advertisement honest.** Holds for
  the two repaired surfaces: Import *gains* the input it already advertised, and
  `/shell-show` loses only verbs that were never dispatched. The scoping test
  proves `/project-init`'s working `a: publish` row still paints.
- **I1, I2, I3b, I6 stay green.** The render corpus is 1227/0 (parent-run).
- **The render-cost shape guard is unchanged.** Measured: pass and unmodified.
- **The comment ratchet holds.** Banked and green (parent-run); not an
  outstanding item.

## 6. Failure and recovery review

- **The `Option<TuiMsg>` ambiguity is handled, not papered over.** `None` means
  both "handled silently" and "not handled", so the check counts a returned
  message as handled and otherwise requires a repaint
  (`baseline.rs:333-346`). A key whose effect is invisible is treated as broken,
  which is the owner's bar; the 61 banked entries are exactly the findings that
  bar produces.
- **A closing surface is not re-rendered.** Production unmounts it in the same
  pass; `QuestionDialog`'s `Esc` would leave an empty answer list, so re-rendering
  it would panic in a way no user can reach (`baseline.rs:318-330`). The harness
  states this rather than hiding it.
- **No runtime, persistence, restart, cancellation or contention path.** The
  check is synchronous and per-surface. The one hazard the plan names — an effect
  deferred past the frame — is not exempted silently; it is banked as a measured
  entry, which is the honest form and the reason the bank is 61 rather than 1.
- **Duplicate delivery, stale generations, malformed input.** Not applicable: the
  milestone adds no queue, request, durable state or parser. `ImportDialog`'s new
  path consumes one `KeyEvent` at a time.

## 7. Migration and compatibility review

No storage, protocol or configuration migration; no schema, DTO or manifest
change attributable to this milestone.

The user-visible compatibility surface is exactly the defect being repaired:
a reader who typed into the import dialog and saw nothing now sees the
characters, and `Enter` offers the preview. `/shell-show` stops naming five verbs
it never dispatched; the box instead names the scroll and close keys it does
handle. There is no transitional state and no removal criterion.

## 8. Security review

No authorization, path, process, secret or network surface is touched.

- **Bounded input — the one relevant property — is reported rather than
  invented.** `ImportDialog`'s typed path and paste path are both unbounded, and
  the doc comment and a test state that parity (`import.rs:312-315`,
  `typing_and_pasting_reach_the_input_at_the_same_bound`). The plan's stop
  condition for exactly this case is satisfied: the missing bound was reported
  instead of a new limit being invented for one path.
- **The practical exposure is bounded by the surface**: a local dialog field
  holds a string the user typed or pasted; it is not persisted, logged or sent
  before `Enter`, and the preview request is the existing path. A very large
  paste or typing run still grows that string without a ceiling (§10).
- **The check itself is read-only**: it drives `handle_key` and renders to a
  `TestBackend`; it writes no file and opens no socket.

## 9. Documentation and operations

- `tests/tui_render/dialog_dispatch_coverage.inc:1-12` — the header now records
  two closures and the retained gap with its ADR reason. This is the file the
  plan's work package E names, and it is accurate about the *three* recorded
  gaps.
- `docs/tui-render-invariant-baseline.toml` — the long-form record of the 61
  banked advertised-key measurements, with `[totals]` at `:118-124`.
- `architecture/tui.md:4330-4410` — the hint-vocabulary and hosted-verb prose,
  including the statement that the I4 baseline is keyed by spelling
  (`:4405-4407`).
- **Plan §12's documentation item is not satisfied as written**: it asks for
  "the I4 rule and the one recorded exception". There is no single recorded
  exception to describe, and no section states the I4 rule with a 61-entry
  exception set. Recorded as a finding (§10) rather than claimed.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | **The M001 `[advertised_keys]` baseline held 61 entries, not the single retained Model `Tab` that work package D and acceptance criterion 5 require.** | The milestone's stated objective — every advertised key is a key the surface acts on — was not achieved for 18 surfaces at that pass. | **RESOLVED by corrective C001** (`plans/closure/tui-advertised-keys-corrective/001-status.md`): the baseline is **4** rows, each carrying a required `reason`, and the amended criterion ("the recorded residue, each entry carrying a reason") is met. The four rows are measured `INERT` and irreducible under ADR-0042 and the no-clipboard-write rule. |
| medium | **The I4 check parsed rendered strings instead of the structured `(key, verb)` pairs the plan says `HintLine` supplies** (`baseline.rs:348-390`; plan §2). | The check worked and was non-vacuous, but it is the fragility the plan explicitly rejected, and it made the bank spelling-sensitive. | **RESOLVED by corrective C002** (`plans/closure/tui-advertised-keys-corrective/002-status.md`): `HintItem` is public, `keymap_hint_line()` is the trait's declaration, all 14 overrides convert, and the measurement presses declared keys first with the painted-row scan retained verbatim as the completeness cross-check. `architecture/tui.md:4516-4575` states both layers. |
| medium | **I4's `Keybind` verdicts measure a handler the modal path never reaches.** The measurement presses each advertised key through `KeybindDialog::handle_key` via `FocusManager::handle_key` → the component, but production short-circuits before the component: `App::on_key` maps `DialogType::Keybind` to `App::handle_keybind_key` and returns (`src/tui/app/mod.rs:4683-4686`). `handle_keybind_key` (`:5321`) re-implements the mode dispatch (`WaitingForKey`, `Export`, `Import`, `Normal`) and owns the app-coupled effects the component cannot perform — `save_keybinds`, clearing `dialog_state.keybind_dialog`, and `close_dialog`. So for `Keybind` specifically the I4 measurement is a **lower bound**: a key that repaints through the component but does nothing in `handle_keybind_key` would read as handled, and a key only `handle_keybind_key` acts on is never pressed. | The invariant's claim is "pressing the key a surface advertises changes that surface" — and for this one surface the press travels a path a user cannot take, so the verdict is weaker than it reads. The other 13 `keymap_hint_line()` overrides are dispatched through `FocusManager` and are **not** affected; `Keybind` is the sole exception because it is the one dialog whose production handler carries side effects. | **Recorded, not fixed — deliberately a separate milestone.** Routing `App::handle_keybind_key` through `Component::handle_key` is a behaviour-preserving refactor of two dispatch paths with different result shapes (the component returns `TuiMsg`; the app performs the side effects), and it would need its own evidence that no user-visible outcome changed across all four modes. That is a milestone, not a line in this record. What would settle it: a dedicated refactor with the mode-by-mode equivalence proof, after which the I4 verdict for `Keybind` becomes a full measurement rather than a lower bound. |
| low | **The `/shell-show` header comment said "no arm dispatches them", but `k` does reach the scroll arm** (`info.rs:389-394`). The verb `k kill` is genuinely undispatched; the character is not. | A reader checking the comment against `handle_key` found one imprecision. | **FIXED**: `tests/tui_render/dialog_dispatch_coverage.inc:8` now reads "no arm dispatches those five **verbs**", and the test message at `:217` says "which no arm dispatches". |
| low | **`ImportDialog` has no input length bound on either path** (both `Char` and paste), stated in the code and asserted at 4,096 chars. | A very large paste or typing run grows the dialog's string without a ceiling. It is the pre-existing paste behaviour, so the milestone does not widen exposure, but it is not a bound either. | Decide whether the dialog should have a limit; if so, add it to both paths in one change and re-bank the parity test. The plan stopped correctly rather than inventing one. |
| low | **The baseline's 61 entries carried no per-entry reason.** | The plan requires every exception beyond the ownership conflict to be *listed with its reason* (§8). | **RESOLVED**: `SECTION_FIELDS["advertised_keys"]` is `("surface", "key", "reason")`, the guard rejects a missing or blank reason, and all **4** current rows carry one. |
| low | **Nothing is committed.** HEAD is `7642b9e38`; 235 paths are dirty or untracked. | The record describes a working tree; the 61-entry figure cannot be tied to a SHA. | Land the milestone as its own commit. |
| low | **`scripts/verify.sh full` had not run**; `quick` was green. | `quick` does not run clippy `-D warnings`, the nextest sweep or the bounded feature pass, and the plan's §11 names formatting and linting. | **Discharged 2026-11-13**: `scripts/verify.sh full` passed on this tree — 17,377 workspace tests, 8,441 feature-pass tests, 0 failures; fmt and clippy clean; corpus banked at 139,245 lines. |
| low | **The editor key-authority count is 92 Normal-mode chords; the M005 record recorded 90.** | A reader comparing the two records sees a movement that this milestone did not cause (its two production changes add no chord). | Attribute the two chords to the lane that added them when the tree is split into commits; do not record them against M006. |

No critical finding. The medium findings are bounded, mechanical, and named.

## 11. Roadmap disposition

**Conditionally closed.** The three recorded gaps are resolved correctly, and
the two live defects this milestone exists for — Import typing and
`/shell-show`'s false footer — are repaired and tested. The plan's generic
requirement was then delivered by the corrective:

1. The I4 baseline was driven from **61** unexplained rows to **four** reasoned
   ones by `plans/implementation/tui-advertised-keys-corrective/001-…` — the
   two `Tab` rows by ADR-0042 and the two `SecurityReview` rows by the
   no-clipboard-write rule.
2. The string-parsing deviation was resolved in the same corrective family:
   `…/002-structured-pairs-from-the-producer.md` makes the measurement consume
   `HintLine`'s own items and keeps the painted scan as the cross-check.
3. The `k` comment was reworded to "five **verbs**", and
   `plans/implementation/tui-modularity-and-render-invariants/012-hint-producer-convergence.md`
   converged the hint producers that feed the advertised rows.
4. `scripts/verify.sh full` passed on this tree at 2026-11-13 — 17,377 workspace
   tests, 8,441 feature-pass tests, 0 failures; fmt and clippy clean; corpus
   banked at 139,245 lines.

- **Residue, named.** The remaining condition is **the commit**, plus the
  advertised-keys work the corrective itself defers: WP C's footer conversion
  and WP E's parser widening, both recorded in
  `plans/closure/tui-advertised-keys-corrective/002-status.md` §10 as fixture
  work rather than surface defects. `ImportDialog`'s absent input bound (§10)
  also stands, as the plan stopped correctly rather than inventing one.
- **Risk.** Low. The two repaired surfaces are tested at the dispatch level, the
  corpus is green, and the invariant claim is now a four-row reasoned residue
  rather than a 61-row ratchet that a green gate could be mistaken for closure.
- **Exact future evidence.** The commit that carries M006 and its corrective
  chain; no gate evidence is outstanding.

M005's interface dependency is discharged in the sense that a consumer of the
advertised hints exists; it consumes them as rendered strings rather than as the
structured pairs plan §2 expected (§10). The Model dialog's `Tab` remains out of
scope by design, and this record does not amend ADR-0042.

## 12. Registry updates

- `plans/registry.md` line 684 (the M006 row): status
  `**Blocked — hard dependency on M001; interface dependency on M005**` is false
  for the shipped work; it must become **corrective pass required**, with this
  record's path and the 61-entry baseline finding.
- `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md` line 687
  (the M006 row): `blocked` → `corrective pass required`, closure record
  `plans/closure/tui-modularity-and-render-invariants/006-status.md`, blockers
  "the `[advertised_keys]` baseline holds 61 entries against the plan's one;
  the string-parsing/structured-pairs decision; green `full` on the merged tree; a
  commit".
- `plans/registry.md` line 29 (the subsystem row): "M001–M004 implemented,
  M005–M010 in progress or pending" must name M006's record and its corrective.
- `plans/implementation/tui-modularity-and-render-invariants/006-advertised-keys-are-handled.md`
  header: `Status: blocked` → `Status: conditionally closed`. **This record
  did not make that edit**: `plans/closure/README.md` does not require it, and
  this session was scoped to `plans/closure/**` only.
- Record that the three recorded gaps are **two closed, one retained** and that
  the retained one is `FocusManager`'s `Tab` ownership (ADR-0042), so no later
  sweep re-opens it as a missing handler.
- Record the measured figures: baseline **61 / 2495 / 0** (61 advertised-key),
  composer guard pass, editor key authority **2 / 92 / 8**, catalog **clean**,
  render-cost guard pass and unmodified, corpus **1227 / 0**, `quick` green
  (parent-run), `full` not run.

## Correction log (2026-11-13)

- **`Status:` header.** Was `corrective pass required`. Flipped to **`conditionally closed`** with the evidence named below. It matches the roadmap row (which records `conditionally closed` with corrective `012-hint-producer-convergence.md` plus the advertised-keys corrective) and both corrective records, which are themselves `conditionally closed`.
- **§1, "the milestone's own exit condition is nevertheless unmet".** Was "the baseline … contains **61** entries across **18** surfaces". Met by corrective **C001** (`plans/implementation/tui-advertised-keys-corrective/001-i4-baseline-to-its-irreducible-residue.md`): the baseline is **4** rows, each carrying a required `reason` — two `Tab` rows by ADR-0042 and the two `SecurityReview` clipboard rows. Verified in the tree: `[totals] advertised_keys = 4` and four `[[advertised_keys]]` entries, each with a `reason`.
- **§1, the status paragraph.** Was "Status is `corrective pass required` rather than conditionally closed because …". Met by C001 plus **C002** (`…/002-structured-pairs-from-the-producer.md`, which resolves the string-parsing deviation) and by `scripts/verify.sh full` on 2026-11-13 (17,377 + 8,441 tests, 0 failures; fmt and clippy clean; corpus banked at 139,245).
- **§2, the two `[advertised_keys]` rows.** Were `fail` at 61 entries. Met by C001; both now record the four-row reasoned residue.
- **§2 and §4, the `full`/clippy/fmt rows.** Were "not run". Met by the 2026-11-13 `full`; `run_full()` calls `run_quick()`, so the `quick` this record cited as parent-run is subsumed.
- **§10, the 61-entry finding.** Resolved by C001; the row now names the four irreducible rows and their reasons.
- **§10, the string-parsing finding.** Resolved by C002; the measurement presses declared keys first and keeps the painted-row scan as the completeness cross-check.
- **§10, the `k`-comment finding.** Fixed: `dialog_dispatch_coverage.inc:8` reads "five **verbs**".
- **§10, the no-per-entry-reason finding.** Resolved: `reason` is a required manifest field and the guard rejects its absence.
- **§10, the `full` finding.** Discharged by the 2026-11-13 run.
- **§11, the roadmap disposition.** Was "**Corrective implementation plan required**" with four open items. Rewritten as `conditionally closed`: items 1–4 are delivered or discharged, and the residue is named — the commit plus the advertised-keys corrective's deferred WP C (footer conversion) and WP E (parser widening), and `ImportDialog`'s absent input bound.
- **§10, new finding added (2026-11-13, after this record's first correction pass).** I4's `Keybind` verdicts measure `KeybindDialog::handle_key` through `FocusManager`, while production short-circuits at `App::on_key` (`src/tui/app/mod.rs:4683-4686`) into `App::handle_keybind_key` (`:5321`), which re-implements the mode dispatch and owns the app-coupled effects. Recorded as a **medium** row: the measurement is a lower bound for `Keybind` alone; the other 13 `keymap_hint_line()` overrides are unaffected. Not fixed here — routing the app handler through `Component::handle_key` is a behaviour-preserving refactor of two dispatch paths that needs its own milestone.

**No promotion beyond `conditionally closed`.** The commit is outstanding and the advertised-keys residue is real work, so this is not `closed`; every gate condition is met, which is why it is no longer `corrective pass required`.
