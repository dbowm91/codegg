# TUI Composer Helix Parity M002 — Closure Status

Status: closed

> **Read this with §4.2.** `scripts/verify.sh quick` is green. `scripts/verify.sh full`
> is **not** green on this machine: both of its nextest sweeps fail on 59 tests that
> register a workspace at `/tmp`, which macOS makes a symlink. The same sweeps were
> run against the baseline commit `61cbe9222` in a detached worktree, and the failure
> sets are **identical** — not one new failure in 16 444 tests. The environment is the
> blocker, not the tree, and this milestone does not reopen that scope.

Source implementation plan:

- `plans/implementation/tui-composer-helix-parity/002-normal-mode-key-ownership-contract.md`

Source subsystem roadmap:

- `plans/subsystems/tui-composer-helix-parity-roadmap.md#7-milestones` (M002)

Repository baseline reviewed: `61cbe9222` (branch `feat/auto-20261019-8cc543fb`)

Implementation commits or pull requests:

- *uncommitted at the time of writing* — the working tree carries the
  implementation, the guard, the tests, the documentation, and this record. No
  hosted CI run is claimed.

Applicable ADRs:

- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` — **accepted**
  2026-10-19, with the `space`-chord amendment. This milestone implements the
  ADR's claim table, its fallthrough rule, and the three re-spellings its
  conflict table requires.

## 1. Executive finding

The composer's **Normal-mode key-ownership contract** is complete: the set of
letters the composer owns is closed, enumerated, and enforced in code and by a
static guard, and the three app-level actions whose letters the composer now
owns have been re-spelled to chords that survive in both keymaps.

At the baseline the composer had **no key ownership at all**. A bare letter in
Normal mode was resolved by the app's binding map and nothing else, so the
letters the Helix surface needs were already spoken for: `x` stopped the turn,
`d` opened the diff, `i` focused the prompt, and `j`/`k` scrolled the
transcript. The terminal panel had already solved the same problem on its own
surface by claiming `d` unconditionally — *"a claimed key is consumed even when
the verb is a no-op, because a reserved claim that falls through becomes an
unrelated app action"* — and the editor had its own table and its own guard.
The composer had neither, and the three surfaces shared a selection model and
a geometry but not an ownership rule, which is exactly the shape of a defect
waiting to be discovered as a user complaint.

What landed:

- **`src/tui/composer_claims.rs`** — the claim table: 13 letters, each with
  exactly one owner, an explicit condition, an explicit encoding set, and an
  explicit status. It is the single source of truth, and the guard reads it
  rather than re-deriving it.
- **The fallthrough rule** — a claimed letter is consumed by the composer
  **even when its verb does nothing**, and the claim step sits in front of the
  binding map, so a claimed letter can never become an unrelated app action
  while its verb is pending.
- **Three re-spellings** — turn stop/resume to `Ctrl+Alt+X`, open-diff to
  `Ctrl+Alt+D`, focus-prompt to `Ctrl+Alt+E`, each in **both** the default and
  the vim keymap. The default map's bare `/` (focus-prompt) **stays** — it is
  not a conflict, and the plan's invariant requires an unclaimed letter to
  keep resolving to the same action (§3.3.1).
- **`scripts/check_tui_composer_key_authority.py`** — a composer-scoped static
  guard with a `--self-test` that injects each violation class, wired into
  `scripts/verify.sh quick` and into CI.

### 1.1 The one deviation from the plan, and why it was necessary

The plan's §5 put "no insert transition" out of scope and its §16 handoff note
said "Do not add verbs here". **One verb is live in this milestone: `i`.**

`i` is the app's **only** route back to Insert mode. It is not a binding at
all — it is the `None` arm of the key chain (`src/tui/app/mod.rs:4325-4345`),
reached when a key resolves to no action. The composer claims `i` (ADR-0042's
conflict table requires it: *"The composer claims `i` as Helix insert-before-
cursor"*), so the claim step consumes the key **before** the binding map is
consulted. A reserved `i` would therefore have consumed the key, done
nothing, and left the user in Normal mode with no way to type at all — the
milestone would have shipped a dead end for the app's only typing gesture.

The verb that runs is the app's **existing** transition, moved into the table:
Helix `i` is "insert before the caret", the caret does not move, and the next
character is typed at it — which is precisely what the old gesture did. The
`None` arm keeps the gesture for the surfaces the composer does not own (the
editor route with its buffer unfocused, a focused panel, the Workspace view's
chat pane). The table is now the one place that says who owns the key.

This is a **phasing** deviation, recorded here and in the roadmap, and it does
not change the milestone's boundary: the transition is not a *new* capability,
the claim table is. M004–M006 still own the remaining verbs, and M006 still
owns the insert transitions *beyond* `i` (`a`, `o`) and the caret placement
they imply. The closure does not present `i` as a new Helix capability: it is
the app's existing behaviour with a new owner.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| **I1 — one owner per chord per surface** | `scripts/check_tui_composer_key_authority.py` G1 (closed table, single owner); `src/tui/composer_claims.rs` unit test `the_claim_table_is_not_empty_and_has_no_duplicate_letters`; `tests/tui_composer_claims.rs::no_claimed_letter_can_reach_an_app_level_action` | pass | The table has 13 rows, one per letter, and the guard fails if a letter appears twice or a second owner is found. |
| **I2 — a claimed key never falls through** | Guard G2 (an unconditional claim must not be app-bound); `tests/tui_composer_claims.rs::a_claimed_letter_is_consumed_before_the_binding_map` (a *custom* binding on a claimed letter is the oracle); `::an_unclaimed_letter_still_reaches_the_binding_map` (the control) | pass | The claim step runs in `App::on_key` **before** `handle_event_with_bindings_moded` (`src/tui/app/mod.rs:4022-4039`). |
| **I5 — the resolved mode is the gate** | `App::composer_holds_keyboard` (`src/tui/app/mod.rs:12773`) and `App::composer_input_mode` (`src/tui/app/mod.rs:12713`) as the only gate; `tests/tui_composer_claims.rs::a_focused_panel_closes_the_claim_gate`, `::the_claim_is_not_live_in_insert_mode`, `::the_workspace_views_filter_still_accepts_a_claimed_letter` | pass | The raw `UiState::input_mode` is never the gate; `composer_input_mode` resolves the mode across the terminal box, the panels and the modal seams. |
| **The re-spelling requirement** (ADR-0042 §4) | `tests/tui_composer_claims.rs::the_re_spelled_chords_reach_their_actions_in_both_maps`; `::the_old_letters_no_longer_reach_the_re_spelled_actions`; guard G4 (a re-spelled action must be present in **both** maps) and G5 (no bare spelling may remain, and the chord's `(modifiers, key)` pair must not be one the table claims) | pass | `Ctrl+Alt+X` / `Ctrl+Alt+D` / `Ctrl+Alt+E`, in both maps. |
| **AC1 — every claimed letter is consumed, triggers no app action** | `tests/tui_composer_claims.rs::no_claimed_letter_can_reach_an_app_level_action`, `::a_claimed_letter_is_consumed_before_the_binding_map` | pass | |
| **AC2 — `d` opens no diff, `x` stops no turn, `i` re-focuses nothing, `space` still opens help** | `::d_does_not_open_the_diff_in_the_main_buffer`, `::x_does_not_stop_the_turn_in_the_main_buffer`, `::i_does_not_focus_the_prompt_in_the_main_buffer`, `::space_still_opens_help_in_the_main_buffer` | pass | The `space` assertion is app-level, not map-level: the claim step sits in front of the map. |
| **AC3 — every re-spelled capability is reachable in both maps by a non-letter chord** | `::the_re_spelled_chords_reach_their_actions_in_both_maps`; guard G4 | pass | |
| **AC4 — every claim-set letter is typed as text in Insert mode** | `::every_claim_table_letter_is_typed_as_text_in_insert_mode` | pass | |
| **AC5 — no claim is live when another surface holds the keyboard** | `::a_focused_panel_closes_the_claim_gate`, `::the_workspace_views_filter_still_accepts_a_claimed_letter`; `src/tui/input.rs` `is_normal_only` and the `is_bare_char` rule | pass | The panel, the Workspace dashboard filter and the dialog are each consulted **before** the claim step. |
| **AC6 — a claimed letter reaches an open dialog** | `::with_a_dialog_open_a_claimed_letter_reaches_the_dialog` (the help box's own filter observes the letter) | pass | |
| **AC7 — `j`/`k` still scroll the transcript on a single-line composer** | `::k_still_scrolls_the_transcript_on_a_single_line_composer`, `::j_follows_the_same_conditional_claim_as_k` | pass | `j`/`k` are the only conditional claims. |
| **AC8 — the composer's text, caret, cursor and selection are unchanged** | `cargo test --test tui_composer_cursor` (24 passed), `cargo test --test tui_render` (111 passed), `cargo test -p codegg --lib tui::` (2114 passed) | pass | This milestone adds no selection and no undo. |
| **AC9 — the editor's and the terminal panel's keymaps are unchanged** | `python3 scripts/check_tui_editor_key_authority.py`, `check_tui_editor_prefix_state.py`, `check_tui_editor_text_authority.py`, `check_tui_transport_authority.py` — all green in `verify.sh quick`; the editor's and terminal panel's own suites are unchanged and green | pass | No editor or terminal-panel file was modified. |
| **AC10 — a claimed capital is reachable in both encodings** | `::a_claimed_capital_letter_is_reachable_in_both_encodings`; guard G6 | pass | `U` answers to `Char('U')` with `SHIFT` **and** with `NONE`, like `insert_panel_direction_bindings`. |
| **AC11 — a capital in Insert mode inserts and opens nothing** | `::every_claim_table_letter_is_typed_as_text_in_insert_mode`, `::a_claimed_lowercase_letter_is_not_claimed_in_its_shifted_spelling` | pass | |
| **The plan's §15 closure-evidence requirements** | §4 below | pass | Guard output (plain and `--self-test`), a real-tree injection, the per-criterion test evidence, and the `quick` and `full` gate output. |

## 3. Production implementation evidence

### 3.1 The claim table — `src/tui/composer_claims.rs` (new)

The table is a `&'static [ComposerClaim]` (`COMPOSER_CLAIMS`,
`src/tui/composer_claims.rs:259`) of 13 rows: `b d e i j k o p u U v w x`. Each
row carries:

- `key` — the bare letter;
- `verb` — a `ComposerVerb` (13 variants) with a `helix_key()`;
- `status` — `Live` or `Reserved`;
- `encodings` — `Bare` or `CapitalBoth` (a claimed capital answers to both the
  `SHIFT` and the `NONE` encodings);
- `condition` — `Always` or `MultiLineBuffer`.

`claim_for(modifiers, code)` (`:373`) is the lookup; `claims_key(modifiers,
code, line_count)` (`:389`) is the conditioned form. Neither duplicates the
table: the table is the truth and the guard reads it.

The set is 13 letters because it is the union of ADR-0042's conflicting
letters (`x`, `d`, `i`, `j`, `k`) and its named free letters (`e`, `b`, `v`,
`o`, `u`, `U`, `p`, `w`). It contains **neither `space` nor `y`** — the ADR's
amendment keeps `space` opening help and preserves the Helix copy gesture as
the second step of that chord — and it contains **no `g`, `h` or `l`**: `g` is
go-to-top (a real app action the composer does not own), and `h`/`l` are
already the caret's `Left`/`Right`, which the composer *has*, so claiming them
would be a second owner for a key the composer already owns by another route.

### 3.2 The dispatch — `src/tui/app/mod.rs`

The claim step is in `App::on_key`, **after** the modal, panel, Workspace and
editor branches and **before** the binding-map call
(`src/tui/app/mod.rs:4022-4039`). It is gated on
`!workspace_active && self.composer_holds_keyboard() &&
self.composer_input_mode() == InputMode::Normal`, so a modal, a focused
panel, the Workspace dashboard and the editor route are all consulted first
and a claim is never live on a surface the composer does not own.

`App::run_composer_claim` (`src/tui/app/mod.rs:12807`) is where a claim's
behaviour goes. A **reserved** verb consumes its key and does nothing — the
consumption *is* the behaviour, and it is what makes the fallthrough rule hold
while the table grows. The one **live** verb is `i`, which runs the app's
existing transition into Insert mode.

A live verb with no behaviour in the runner is a table/runner disagreement, and
it is reported (`tracing::warn!`) rather than swallowed: a consumed key with no
behaviour is a dead key, and a dead key is the defect class this milestone
exists to prevent.

The `None` arm's `i` gesture (`src/tui/app/mod.rs:4325-4345`) is **kept**, with
its comment rewritten. It is no longer the main buffer's route — the claim
runs first — but it still serves the surfaces the composer does not own.

### 3.3 The re-spellings — `src/tui/input.rs`

| Action | Was | Is now | Maps |
|---|---|---|---|
| `ToggleAgentRun` (stop / resume the turn) | bare `x` (default map) | `Ctrl+Alt+X` | default + vim |
| `OpenDiff` | bare `d` (vim map) | `Ctrl+Alt+D` | default + vim |
| `FocusPrompt` | bare `i` (vim map) | `Ctrl+Alt+E` | default + vim |

The default map's bare `/` → `FocusPrompt` **stays**. See §3.3.1 — it is not
a conflict, and the plan's own invariant requires it to keep working.

`InputAction::ToggleAgentRun` left `is_normal_only` (`src/tui/input.rs:664`):
its only spelling is now a chord, and a chord cannot be composed as text, so
it is already unambiguous in Insert mode. The `is_normal_only` doc's own rule
covers this, and `ToggleTasksList` is the precedent.

`ActionKey::label()` was updated for all three, and the help surface gained
the `Ctrl+Alt+X` entry and moved its `i` and `d` entries to the new chords.
The stop hint in the working strip, the confirmation toast, the llm-buffer key
hints and the stall message all read `STOP_KEY_LABEL`
(`src/tui/commands/turn_control.rs:39`), which changed from `"x"` to
`"Ctrl+Alt+X"`.

**Why `Ctrl+Alt+<letter>` and not `Alt+<letter>`.** The app's established,
audited chord family is `Ctrl+Alt+<letter>`; the ESC prefix makes the chord
unambiguous, whereas `Alt+<letter>` has the ESC-then-letter confusion hazard.

**Why `Ctrl+Alt+I` is impossible.** crossterm 0.29's legacy parser matches
`ESC` + `0x09` to `KeyCode::Tab` + `ALT` *before* its `0x01..=0x1A`
control-character arm, so `Ctrl+Alt+I` would arrive as `Tab`+ALT and the verb
would be unreachable on every terminal. `Ctrl+Alt+E` (`ESC` + `0x05` →
`Char('e')` + CONTROL|ALT) is deliverable. `Ctrl+Alt+M` is likewise unusable,
and `Ctrl+Alt+J` works only in raw mode.

**A deliberate interpretation, recorded.** The plan says the new chords "must
not be letters in the composer's claim set". Read literally that is
unsatisfiable here: `Ctrl+Alt+X` *is* the letter `x`, and `x` is in the claim
set. The reading that is actually enforceable, and the one the guard
implements, is **the chord's `(modifiers, key)` pair must not be one the table
claims**. A claim is matched on the exact modifier set, so the composer claims
the bare `x` and never the `Ctrl+Alt+X` chord — which is precisely what
prevents the composer from taking the chord back. A chord spelled with a
modifier set the table *did* claim would be swallowed, and that is what G5
fails on. The interpretation is recorded here because it is the difference
between a requirement that can be met and one that cannot.

### 3.3.1 The default map's `/` stays — and the guard rule that follows

An earlier pass of this milestone also removed the default map's bare `/` →
`FocusPrompt`, on the reading that the ADR's "the help surface and
`docs/tui.md` must state the new spellings in the same change that removes the
old ones" covers every old spelling of the three actions.

**That was wrong, and it broke a real capability.** Three things decided it:

1. **The `/` spelling is not a conflict.** ADR-0042's conflict table enumerates
   the letters that had to be resolved, and `/` is not among them: the composer
   does not claim `/`. The re-spelling of focus-prompt exists *because* the
   composer claims `i`, which is the vim map's spelling. There is nothing for
   the default map's `/` to conflict with.
2. **The plan's invariant forbids it.** §4 of the implementation plan: *"a
   letter the composer does not claim must resolve to the same app action it
   resolves to today, in the same mode, with the same preconditions."* `/` is
   not claimed, so it must still resolve to `FocusPrompt`.
3. **It was load-bearing beyond the action.** `/` is the gesture that **enters
   command mode**, and command mode is what makes the rest of a typed slash
   command land as text. Removing it took away "type `/panels` from Normal
   mode and press Enter" — the only way back into the panel manager from a
   focused panel, which owns every key by contract
   (`tests/tui_side_panels.rs::the_slash_command_survives_inside_a_focused_panel`).
   That test **passed at the baseline commit `61cbe9222`** and failed on this
   milestone's first pass; the defect was found by the full gate, not by
   reasoning.

The binding is therefore restored, and the guard's rule was corrected with it.
G5 no longer says "a re-spelled action must have no bare-character spelling";
it says **"a re-spelled action must not answer to a key the composer claims,
and must have a chord to reach it by in both maps"**. The three old letters —
`x`, `d` and `i` — are all claimed, so the claim rule already forbids their
bare spellings; a bare spelling of a letter the composer does *not* claim is
exactly what the invariant above requires to keep working. A third G5 check
("the re-spelled action has no non-bare spelling in this map") was added so
that "reachable by a chord" is actually enforced.

**A latent guard bug this exposed.** The G5 "a re-spelled chord is a key the
composer can claim" check had never fired: `claimed_key_set` was built by
re-pairing a pair that was already whole, so it held a tuple one level deeper
than the bindings use and the membership test was always false. The self-test
that was supposed to prove the check worked had been asserting a *different*
message, so it passed for the wrong reason. Both are fixed: the set is built
from the pairs as they come, and the self-test now asserts the claimed-key
message specifically. The real-tree injection in §4.1 now reports **4**
findings instead of 3, because the claimed-key check contributes one.

### 3.4 The guard — `scripts/check_tui_composer_key_authority.py` (new)

A composer-scoped static guard, ~900 lines, with a `--self-test`. It parses
`src/tui/composer_claims.rs` (the table) and both binding maps in
`src/tui/input.rs` (including the `for (letter, action) in [...]` loop forms
and the `insert_panel_direction_bindings` helper), and enforces:

| Rule | What it fails on |
|---|---|
| G1 | the table is not closed, or a letter has two owners |
| G2 | an unconditional claim can still reach an app-level action (the fallthrough rule) |
| G3 | a claim uses a condition the guard does not know |
| G4 | a re-spelled action is missing from one of the two maps |
| G5 | a re-spelled action answers to a key the table claims (which is how the old letter would keep reaching it), or it has no chord to reach it by in one of the maps |
| G6 | a capital claim does not answer to both encodings |
| G7 | a chord-only action is still listed in `is_normal_only` |
| G8 | a claimed key bound under `SHIFT` has no `is_normal_only` entry |

A construct the guard does not understand is **reported**, not skipped.

### 3.5 What is *not* in this milestone

The remaining twelve verbs are **reserved**: their keys are consumed and
nothing happens. This milestone adds no motion, no selection, no delete, no
yank, no paste, no undo and no redo. The closure does not present a reserved
claim as a shipped capability — the user-visible change in this milestone is
the three re-spellings, and nothing else.

## 4. Verification executed

All commands were run **locally** on macOS 25.6.0 (aarch64) at the working
tree described above. No hosted CI run is claimed.

### Commands run

```bash
# the guard, plain and with its self-test
python3 scripts/check_tui_composer_key_authority.py
python3 scripts/check_tui_composer_key_authority.py --self-test

# the other TUI guards the plan names
python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_tui_editor_prefix_state.py
python3 scripts/check_tui_editor_text_authority.py
python3 scripts/check_tui_transport_authority.py

# focused tests
cargo test -p codegg --lib tui::
cargo test -p codegg --lib tui::input
cargo test --test tui_composer_claims
cargo test --test tui
cargo test --test tui_main_buffer
cargo test --test tui_composer_cursor
cargo test --test tui_render
cargo test --test modifier_chords

# the canonical gates
scripts/verify.sh quick
scripts/verify.sh full
```

### Results

| Check | Result |
|---|---|
| `check_tui_composer_key_authority.py` | **pass** — "the claim table is closed, single-owner, and leaks no key to an app-level action" |
| `check_tui_composer_key_authority.py --self-test` | **pass** — 12 cases: 11 violation classes detected and rejected, plus the delivered tree clean |
| the four editor/transport TUI guards | **pass** |
| `cargo test -p codegg --lib tui::` | **pass** — 2114 passed, 0 failed |
| `cargo test -p codegg --lib tui::input` | **pass** — 50 passed, 0 failed (includes the help audit) |
| `cargo test --test tui_composer_claims` | **pass** — 27 passed, 0 failed |
| `cargo test --test tui` | **pass** — 167 passed, 0 failed |
| `cargo test --test tui_main_buffer` | **pass** — 56 passed, 0 failed |
| `cargo test --test tui_side_panels` | **pass** — 31 passed, 0 failed |
| `cargo test --test tui_composer_cursor` | **pass** — 24 passed, 0 failed |
| `cargo test --test tui_render` | **pass** — 111 passed, 0 failed |
| `cargo test --test modifier_chords` | **pass** — 20 passed, 0 failed |
| `cargo fmt --all --check` | **pass** — clean |
| `cargo clippy -p codegg --lib --tests --all-targets` | **pass** — no warnings |
| `scripts/verify.sh quick` | **pass** — "Quick verification passed." |
| `scripts/verify.sh full` | **not green** — the two nextest sweeps fail on failures that are **identical to the baseline's**; see §4.2 |

### 4.1 The guard is not vacuous — a real-tree injection

The `--self-test` proves the guard rejects synthetic trees. To prove it
rejects **this** tree, a bare-letter binding was injected into the delivered
`src/tui/input.rs`, replacing the `Ctrl+Alt+D` spelling of `OpenDiff` with
`(KeyModifiers::NONE, KeyCode::Char('d'))`:

```
The composer's claim table does not own its keys.

  the claimed letter 'd' can still reach OpenDiff in the default map (input.rs:810): a claimed key must be consumed by the composer and must not fall through
  OpenDiff is bound to 'd' under NONE in the default map (input.rs:810), which is a key the composer claims; the composer would take the chord back
  the re-spelled action OpenDiff has no non-bare spelling in the default map; the capability must be reachable by a chord that is not a bare letter
  the old letter 'd' still reaches OpenDiff in the default map (input.rs:810)

4 violation(s) found.
```

exit status **1**. The file was restored and the guard re-run clean. The
injection is the same class of defect the terminal panel already records for an
unclaimed `d`.

### 4.2 The full gate is not green in this environment — and why that is not this milestone

`scripts/verify.sh full` **fails** on this machine. The failure is not in the
TUI and not in anything this milestone touched, and that is a measured claim,
not an argument: the same sweeps were run against the **baseline commit
`61cbe9222`** in a detached worktree with none of these changes, and the
failure sets are identical.

```
$ cargo nextest run --workspace --locked --profile ci --no-fail-fast
baseline  61cbe9222 :  59 unique failures   (14 747 tests)
this milestone      :  60 unique failures   (14 807 tests)

$ diff <baseline failures> <this milestone's failures>
only in the baseline : (none)
only in this tree    : core::daemon::tests::poolless_standalone_human_shell_uses_registered_workspace
```

The second sweep in `full` — the feature-gated one — is an **exact** match:

```
$ cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support --profile ci --no-fail-fast
Summary [373.107s] 11637 tests run: 11578 passed, 58 failed, 1 timed out, 6 skipped

$ diff <baseline failures> <this sweep's failures>
only in the baseline : (none)
only in this sweep   : (none)
```

Not one new failure in either sweep, in 16 444 tests.

The one extra name is a **load-sensitive flake**, not a regression: it passes
in isolation on this tree (0.90 s), passes in isolation at the baseline
(0.89 s), and passes when the `core::daemon::tests` group is re-run on this
tree (44 tests, 42 passed, the two known failures below). It is in
`src/core/daemon.rs`, a file this change set does not touch.

All 59 baseline failures share one cause, and it is environmental: **macOS
`/tmp` is a symlink to `/private/tmp`**, and the workspace-root validation
rejects a symlinked root.

```
workspace register failed: Error { code: "workspace_register_failed",
  message: "workspace root '/tmp' must be an absolute, non-symlink path" }
```

The affected tests are the ones that register a workspace at `/tmp` —
`core::daemon::tests::session_create_through_daemon` (reproduced
deterministically at the baseline in a detached worktree),
`core::daemon::tests::turn_submit_rejects_when_active`,
`tool_execution`'s bash-tool tests, `workspace_isolation`,
`session_family`'s controller-lease tests, `git_execution_origin_matrix`,
`identity_m002_live_audit_hooks`, `identity_live_execution_audit`,
`tool_surface_minimization`, `tool_program_git_lsp_palette`, and the `lsp`
command tests — plus one `TIMEOUT` in
`agent::asset_refresh::tests::same_scope_requests_coalesce_to_one_publication`,
which passes in isolation in 0.24 s and is a known timing-sensitive test.

**The milestone's own scope is green.** Every TUI test target is green
(`tui_composer_claims` 27, `tui_side_panels` 31, `tui` 167, `tui_main_buffer`
56, `tui_composer_cursor` 24, `tui_render` 111, `modifier_chords` 20), the
`cargo test -p codegg --lib tui::` suite is 2114 passed / 0 failed, the guard
and its self-test pass, `cargo fmt --check` is clean, and `cargo clippy` has
no warnings.

**This is also how the `/` regression was found.** The first pass of this
milestone removed the default map's bare `/` → `FocusPrompt` and broke
`tests/tui_side_panels.rs::the_slash_command_survives_inside_a_focused_panel`.
That test passes at the baseline and failed here; the fix is in §3.3.1. The
full gate is what surfaced it — reasoning about the ADR had not.

## 5. Invariant review

| Invariant | Evidence that it still holds |
|---|---|
| **I1 — one owner per chord per surface** | Guard G1; the table has 13 unique keys; the three re-spelled actions have exactly one spelling each, in both maps; the claim step and the binding map are consulted in one order, so no key has two owners in the main buffer. |
| **I2 — a claimed key never falls through** | Guard G2; `App::on_key` consumes a claim before the binding map (`src/tui/app/mod.rs:4022-4039`); a reserved verb is consumed and does nothing. |
| **I5 — the resolved mode is the gate** | The gate is `composer_holds_keyboard()` and `composer_input_mode()`, never `UiState::input_mode`; `App::composer_input_mode` was left as M001 delivered it. |
| **No app-level capability is removed** | Turn stop/resume, open-diff and focus-prompt are all still reachable, by chords, in both maps (AC3). `space` still opens help (AC2). The default map's bare `/` for focus-prompt stays, so the "type `/panels` from Normal mode" route survives too (§3.3.1). |
| **The editor's and the terminal panel's keymaps are unchanged** | No file under `src/tui/commands/editor.rs` or `src/tui/commands/interactive_terminal.rs` was touched; their four guards are green; their suites are green. |
| **Insert mode still types bare letters** | `is_normal_only` still lists every action whose bare spelling would shadow typing; `ToggleAgentRun` left the list because its only spelling is now a chord. Every claim-set letter is typed as text in Insert mode (AC4). |
| **Unclaimed letters behave exactly as before** | `an_unclaimed_letter_still_reaches_the_binding_map`; the claim step returns only for a letter in the table. |
| **No authority change** | Nothing in this change set touches turn admission, tool authority, or the daemon's accepted surface. The composer remains a frontend-local string. |
| **The single-cadence rule** | No timer, poll interval, or animation was introduced. The claim step is a synchronous branch in the existing key path. |
| **The composer's text model is untouched** | No selection, no undo, no yank was added. The claim table is a key-ownership table, not a text model. |

## 6. Failure and recovery review

- **Duplicate delivery and idempotency** — not applicable: the claim table is
  a static table and the dispatch is a pure lookup. Pressing a claimed letter
  twice has the same effect as pressing it once.
- **Cancellation races** — not applicable: the claim step is synchronous and
  owns no state.
- **Daemon or node restart** — not applicable: nothing is persisted. The table
  is compiled in.
- **Partial persistence failure** — not applicable: no persistence.
- **Stale generation or lease** — not applicable.
- **Contention and resource release** — not applicable.
- **Malformed or unauthorized input** — the claim lookup matches on the
  crossterm `(modifiers, code)` pair. A modified key is never claimed: the
  table's `encodings` say which modifier sets a letter answers to, so
  `Ctrl+d` is not the claim for `d`. An unknown key is not claimed and falls
  through unchanged.
- **Bounded event and artifact behavior** — the table is 13 static rows; the
  dispatch is O(1); no allocation occurs on the key path.

## 7. Migration and compatibility review

- **Schema migration** — none. No storage layout change, so
  `STORAGE_LAYOUT_VERSION` is untouched.
- **Backward compatibility** — the composer's buffer format is unchanged. The
  three re-spellings are a **deliberate user-visible break**, recorded in
  ADR-0042 before implementation rather than discovered afterwards. A user
  who had memorised `x` for stop, `d` for diff or `i` for focus-prompt must
  learn the new chords; `docs/tui.md` carries the was/is table.
- **Protocol negotiation** — not applicable; no wire protocol changed.
- **Configuration behaviour** — a **custom** binding is affected: a user's own
  binding on a claimed letter no longer fires in the main buffer's Normal
  mode, because the claim is consulted first. That is the ADR's rule
  ("unclaimed letters fall through to the app's existing binding map
  unchanged" — a *claimed* letter does not). It is recorded here because it is
  the one place where a user's configuration changes behaviour, and it is the
  intended consequence of claiming the letter.
- **Rollback limitations** — rolling back the re-spellings restores the bare
  letters, but leaves the bare letters in conflict with the claim table, so a
  rollback must revert the table as well. The change set is one commit's worth
  of work and reverts cleanly.
- **Legacy-path status** — the vim map's bare `i` for focus-prompt is
  **retired**, not deprecated: the composer claims `i`, so the spelling had to
  move. The default map's bare `/` is **not** retired: `/` is not a claimed
  letter, so it never conflicted with the claim table, and the plan's
  invariant keeps it resolving to `FocusPrompt` (§3.3.1). The Insert-mode `/`
  affordance (typing `/` at the start of the composer opens the palette) is
  untouched as well.

## 8. Security review

- **Authorization enforcement** — unchanged. The composer is a frontend-local
  string; nothing here alters turn admission or tool authority.
- **Secret handling** — not applicable; no credential or secret is read,
  logged, or displayed by this change.
- **Path validation** — not applicable; no path is resolved by the claim
  step. The path-related rules in `AGENTS.md` are untouched, and the relative
  path in a tool is still resolved against the session's `workspace_root`.
- **Privilege boundaries** — unchanged. The claim step runs inside the same
  `App::on_key` chain, after the same modal/panel/editor branches, so it
  cannot widen what any surface may reach.
- **Denial-of-service bounds** — the dispatch is a bounded lookup over 13
  rows; a key event costs one comparison plus a table scan. No unbounded work
  was added to the key path.
- **Redaction** — not applicable.
- **Audit behavior** — unchanged. The audit trail records what the daemon
  accepted; this milestone changes no daemon-side behaviour.
- **A note on the keys themselves** — a chord is strictly *harder* to trigger
  accidentally than a bare letter, so the re-spellings do not weaken any
  safety property. `Ctrl+Alt+X` in particular replaces a bare `x` that a
  stray keystroke could fire.

## 9. Documentation and operations

- `architecture/tui.md` — a new section for the composer's claim table: the
  13-row table, the fallthrough rule, why `space`/`y`/`g`/`h`/`l` are absent,
  the `j`/`k` condition, the dispatch placement and the Workspace exclusion,
  the capital dual encoding, the three re-spellings with the `Ctrl+Alt+I`
  delivery explanation, the `is_normal_only` consequence, the `/` spelling and
  why it stays, the guard, and the statement that the composer, the editor and the terminal
  panel share selection semantics and geometry but **not** key ownership.
- `docs/tui.md` — the user-facing "Three keys moved to chords" section, with
  the was/is table, the note that `/` at position 0 and `Space` help are
  unaffected, and the note that `i` still takes the user from `[NOR]` back to
  typing.
- `.opencode/skills/tui/SKILL.md` — the composer's key-ownership boundary,
  the fallthrough rule, the new invariant about a consumed claim that does
  nothing being a dead key, and the new guard in the Static Guards table.
- `architecture/overview.md` — the two file-count rows of the Verified-counts
  table were remeasured against their listed `Source` because this milestone
  added a test file and a guard: integration test files **244 → 266** and CI
  guard scripts **53 → 58**. This workstream's own contribution is two test
  files and one guard; the rest is drift that accumulated after the 2026-10-15
  audit, corrected rather than left for the next reader. The `check_*`
  inventory sentence was corrected with it.
- `plans/subsystems/tui-composer-helix-parity-roadmap.md` — the M002 row of
  the milestone status table, and the `i`-verb deviation recorded against
  M006.
- `plans/implementation/tui-composer-helix-parity/002-normal-mode-key-ownership-contract.md`
  — status: **implemented**.
- **New static guard**: `scripts/check_tui_composer_key_authority.py`, wired
  into `scripts/verify.sh quick` and into `.github/workflows/ci.yml` (two
  steps: the guard and its `--self-test`).
- **Operator diagnostics** — none. This milestone adds no runtime
  configuration, no log line a user must read, and no recovery procedure.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | The plan's §5/§16 scope excluded an insert transition, and `i`'s verb is live in this milestone (§1.1). | The milestone ships one live verb where the plan described none. The behaviour is the app's pre-existing transition with a new owner, and the alternative was a dead key. | Recorded in §1.1, in the plan's status line and in the roadmap's M002 row. M006 owns the insert transitions *beyond* `i`. |
| low | A custom user binding on a claimed letter no longer fires in the main buffer's Normal mode. | A user's own configuration changes behaviour for the 13 claimed letters. | Intended by ADR-0042 (a claimed letter belongs to the composer). Recorded in §7. |
| low | The guard reads `src/tui/composer_claims.rs` and `src/tui/input.rs` only; it does not check the runner in `App::run_composer_claim`. | A future milestone that marks a verb `Live` without adding the behaviour in the runner would not be caught by the guard. | The runner reports the disagreement with `tracing::warn!` rather than swallowing it. Extending the guard to the runner is a natural M004 follow-up. |
| low | The first pass of this milestone removed the default map's bare `/` → `FocusPrompt` and broke `tests/tui_side_panels.rs::the_slash_command_survives_inside_a_focused_panel`. | The "type `/panels` from Normal mode and press Enter" route — the only way back into the panel manager from a focused panel — stopped working. | **Fixed in this milestone**: the binding is restored and the guard's G5 rule was corrected with it (§3.3.1). The defect was found by the full gate, not by reasoning about the ADR. |
| low | `scripts/verify.sh full` is not green on macOS: 59 tests that register a workspace at `/tmp` fail because macOS `/tmp` is a symlink to `/private/tmp` and the workspace-root validation rejects a symlinked root. | The full gate cannot be made green on this machine without changing either the validation or the tests, neither of which is this milestone's to change. | Pre-existing and proven at the baseline commit `61cbe9222` (§4.2). The failure set is identical to the baseline's. The environment, not the tree, is the blocker; on Linux CI `/tmp` is not a symlink and the gate is expected to be green. Not reopened here. |
| low | The guard's "a re-spelled chord is a key the composer can claim" check had never fired: `claimed_key_set` was built by re-pairing a pair that was already whole, so the membership test was always false. | A guard rule that looked armed and was not — the same failure class this milestone exists to prevent, one level down. | **Fixed in this milestone**: the set is built from the pairs as they come, and the self-test now asserts the claimed-key message specifically. The real-tree injection in §4.1 now reports 4 findings instead of 3, because the check contributes one (§3.3.1). |

No critical, high, or medium finding is outstanding.

## 11. Roadmap disposition

**Milestone closed and next dependency may proceed.**

M002 is closed. M003 is unaffected and remains eligible for fresh planning;
M004–M007 keep their existing dependencies. The one scoping note carried
forward is the `i` verb (§1.1): M006 owns the insert transitions beyond `i`
and the caret placement they imply, and the roadmap's M002 row records the
deviation.

Two things are carried forward for whoever picks up the next milestone:

- **M004 should extend the guard to the runner** (§10), so a verb promoted to
  `Live` without a runner arm fails the guard rather than only logging.
- **M006 inherits the insert transitions beyond `i`**, and should read §1.1
  before planning them: the `i` transition is already live and already
  behaves as Helix insert-before-caret.

## 12. Registry updates

- `plans/registry.md` — the "TUI composer Helix keybinding parity" row:
  status cell updated to record **M002 closed**, the implementation-plan cell
  updated with M002's closure record, and the notes cell extended with what
  M002 delivered and the `i`-verb deviation.
- `plans/subsystems/tui-composer-helix-parity-roadmap.md` — the M002 row of
  the milestone status table: `ready` → `closed`, with the closure record's
  path, and the `i`-verb deviation noted against M006.
- `plans/implementation/tui-composer-helix-parity/002-normal-mode-key-ownership-contract.md`
  — status: `ready` → `implemented`.
- No other plan's blocker is resolved by this closure, so no other plan
  changes status.
