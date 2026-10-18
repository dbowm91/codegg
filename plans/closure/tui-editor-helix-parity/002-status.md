# TUI Editor Helix Parity — M002 closure: Unified prefix state machine

Status: **closed**

Repository baseline: `719cfeb5d` (branch `feat/auto-20261017-80a41db7`)

Implementation plan:
`plans/implementation/tui-editor-helix-parity/002-unified-prefix-state-machine.md`

Predecessor closure:
`plans/closure/tui-editor-helix-parity/001-status.md`

Source roadmap:
`plans/subsystems/tui-editor-helix-parity-roadmap.md#7-milestones` (M002)

---

## 1. Outcome

The two ad-hoc prefix mechanisms on `TuiDocumentPresentation` are deleted and
replaced by one typed state machine, `EditorPrefix`. `gg` and `space y` are
expressed through it and behave exactly as they did at the baseline. `[` and
`]` are armed. The pending indicator renders for every pending state and is
bounded by construction rather than by a string clamp.

Both legacy fields — `pending_command: String` and `yank_prefix_pending: bool`
— are **deleted**, not bypassed, together with the two helpers that existed
only to service them (`push_pending`, `bounded_pending`) and the renderer's
byte-truncation of the pending label.

## 2. The chosen representation, and why

```rust
pub enum EditorPrefix { Idle, Goto, Space, OpenBracket, CloseBracket }
```

on `TuiDocumentPresentation::pending_prefix`, with `arm_prefix` / `disarm_prefix`
on the presentation and resolution in one dispatch-layer function,
`resolve_pending_prefix`.

Three properties decided the shape:

1. **An enum, not a string.** `pending_command` was a free-form `String` whose
   only writer pushed one glyph and whose only reader tested for `"g"`. A
   `String` cannot answer "is this prefix still valid?", which is exactly the
   question the stale-prefix hazard asks.
2. **The mismatch policy is a property of the prefix, not of the machine.**
   This is the load-bearing decision, and it is the one place where following
   the plan's §6 sentence literally would have been wrong — see §3.
3. **Resolution lives outside `handle_normal_key`.** The M001 key-authority
   guard scopes itself to that function's first
   `match (key.modifiers, key.code)`; a prefix resolution written like a key
   table at the top of the function would have made the guard parse the wrong
   table. Keeping it a sibling function also means M006's unimpaired leaves get
   a home that is already scanned by hand rather than by regex.

## 3. Deviation from the plan's §6, and why it was necessary

Plan §6 says: *"A non-matching key disarms and is then resolved normally,
matching today's `space y` semantics."*

That is the correct description of `space y` and the **wrong** description of
`gg`. The baseline's two inline branches disagreed:

| Baseline | Unmatched second key | Evidence |
|---|---|---|
| `pending_command` (`gg`) | **consumed** — `return BufferKey::Consumed` | `a_pending_prefix_swallows_the_next_key_and_resolves_only_known_pairs` |
| `yank_prefix_pending` (`space`) | **disarmed and retried** — falls through to the match table | `space_y_reports_a_yank_range_as_a_remembered_prefix` asserts `space` then `w` returns `Consumed` *and* moves the cursor |

Applying §6 uniformly would have made `g j` move the cursor down one line,
which is not a refactor. The plan's acceptance criterion 2 (`gg` and `space y`
behave identically to the baseline) and its §14 stop condition ("a behavior
change in either gesture is a user-visible decision") outrank §6's prose.
Both are dialect-faithful independently — Helix's goto mode consumes the second
key, vim's space mode retries it — so neither is a bug to fix here.

`EditorPrefix::swallows_unmatched()` therefore carries the policy, and both
paths are now covered by tests that assert the *observable* difference
(`goto_swallows_an_unmatched_key_while_space_retries_it`,
`a_non_char_terminal_key_follows_the_prefixs_policy`).

### 3.1 A second one: §4's "`:` and `/` remain outranking everything"

Plan §4 states that `:` and `/` outrank "everything, including any prefix
resolution". They do not, at the baseline either. Verified against `719cfeb5d`
directly: with `pending_command == "g"`, the resolution branch returns
`BufferKey::Consumed` for **any** `Char`, and it sits above the `:` and `/`
arms — so `g :` consumed the colon and opened nothing.

This was caught by a test written from the plan's sentence, which failed
against correct code. The test was corrected to assert what the editor
actually does, and the correction is itself evidence:

* a **swallowing** prefix (`g`) consumes `:` and `/`;
* a **retrying** prefix (`space`, `[`, `]`) lets them reach their arms.

Plan acceptance criterion 6 is the accurate statement — *"prefix resolution
never intercepts `:`/`/` **lines**"* — and it holds: once a line is open,
`editor_buffer_key` returns before any prefix code runs, so `y` typed into a
half-typed `:q` is a command name and never a yank. That direction is now
covered by `a_modal_line_outranks_the_pending_prefix`.

§4's sentence and criterion 6 pull in opposite directions; the code, the
baseline, and criterion 6 agree, so they were followed. Making `:` and `/`
outrank a swallowing prefix would be a small, defensible improvement — but it is
a user-visible behavior change to a working binding, and §14 makes that a stop
condition rather than an implementation detail. It is recorded as a residual
opportunity in §15 instead.

## 4. Behavior preservation: existing tests passed unmodified

The plan's strongest available evidence is that the existing `gg` and
`space y` tests pass without being rewritten. That is what happened:

| Test | Status |
|---|---|
| `a_pending_prefix_swallows_the_next_key_and_resolves_only_known_pairs` | passed unmodified |
| `the_pending_prefix_is_bounded` | passed unmodified |
| `space_y_reports_a_yank_range_as_a_remembered_prefix` | passed unmodified |
| `every_normal_mode_motion_is_reachable_through_dispatch` (contains the `gg` block) | passed unmodified |

The `gg` assertions read `assert_eq!(pending(&state), "g")` and
`assert_eq!(pending(&state), "")`. Those pass unmodified because the test helper
was changed to report `pending_prefix.display()` rather than the enum — the
indicator string, which is the same fact the assertions were written against.
This was a deliberate choice: returning the enum would have forced four
assertion rewrites for no gain, and a rewrite is indistinguishable from a
behavior change at the point where a reviewer is trying to prove there wasn't
one.

## 5. Deletion evidence

`scripts/check_tui_editor_prefix_state.py` (new; wired into `scripts/verify.sh
quick` and the CI `verify` job) fails on:

- any occurrence of `pending_command`, `yank_prefix_pending`,
  `MAX_EDITOR_PENDING_COMMAND`, `bounded_pending`, or `push_pending` anywhere
  under `src/`, comments excluded;
- a `TuiDocumentPresentation` declaring anything other than exactly one prefix
  field — the invariant a fixed forbidden-name list cannot express, because the
  next milestone's flag would be a *new* name;
- a missing `EditorPrefix`, `Idle`, `MAX_EDITOR_PENDING_PREFIX`,
  `pub pending_prefix`, `arm_prefix`, `disarm_prefix`, or
  `resolve_pending_prefix`, so it cannot pass against an emptied tree.

It ignores comments deliberately: the type's own doc comment is where the
history of the two deleted fields belongs, and a guard that forbade the word
would have pushed that history out of the code.

## 6. Fix found while migrating

`clear_history` disarmed `pending_command` (reconciliation → `resync`,
`reload_from_disk`, `close`) but the `space` flag lived on the same presentation
and was only cleared by `reset_for_open`. A `space` typed before a reload was
therefore still armed when the reloaded buffer arrived, and `space y` would
yank in it. The merged disarm point removes that asymmetry. Proven by
`a_reconciliation_disarms_the_pending_prefix`, which sweeps reload/resync/close
against all three armed grammars.

This is a behavior change relative to the baseline, and it is a **fix** in the
direction the plan's §4 invariant already named ("a stale pending prefix must
never apply to a newly opened buffer"). It is recorded here rather than
presented as preservation.

## 7. Visible change: the `space` indicator

`space` previously armed a `bool` with no rendering path, so pressing `space`
looked like nothing had happened. Every pending prefix now renders a distinct
label in the status row (`g`, `space`, `[`, `]`).

This is a **visible change to `space y`**, which plan §12 said must not change
visibly. It was made because plan §3 names an invisible pending prefix as a
defect in its own right ("a pending prefix that is invisible is a prefix the
user cannot predict"), because acceptance criterion 3 and work package D both
require the indicator to render for *every* pending state, and because a
one-state exception would have left the renderer's contract untested for `space`.
It is trivially revertable (return `" "` for `EditorPrefix::Space::display` and
re-raise the bound), and it is documented in `docs/tui.md` and `CHANGELOG.md`
rather than left to be discovered.

`gg` is byte-for-byte unchanged: `EditorPrefix::Goto::display() == "g"`.

The bound moved from `MAX_EDITOR_PENDING_COMMAND = 2` to
`MAX_EDITOR_PENDING_PREFIX = 5`, the length of the longest label. The new bound
is asserted against every variant by
`every_pending_prefix_display_fits_the_indicator_bound`, including that it
equals the longest label rather than being a guess.

The old bound was also applied to *bytes* via `str::get(..2).unwrap_or("")`.
That is a latent trap rather than a live bug — every baseline label was
one ASCII byte — but it would have silently blanked any multi-byte indicator.
The renderer now takes the `Copy` enum and asks it for `display()`, so the
byte-boundary question no longer exists.

## 8. Requirements to evidence

| Plan requirement | Evidence |
|---|---|
| §5 single typed pending-prefix type | `EditorPrefix` (`src/tui/editor.rs`), `pending_prefix` on the presentation |
| §5 `gg`/`space y` migrated | `resolve_pending_prefix`; §4 above |
| §5 `[`/`]` armed, leaves in M006 | `[`/`]` arms in `handle_normal_key`; `EditorPrefix::{OpenBracket, CloseBracket}` |
| §5 both legacy fields deleted | §5 above; guard passes on the tree |
| §5 render plumbing updated | `EditorWidget::pending_prefix`, its `Options`, four construction sites, `app/render.rs` |
| §5 indicator preserved and bounded | `every_pending_prefix_renders_distinctly_and_idle_renders_nothing`, `every_pending_prefix_display_fits_the_indicator_bound` |
| §6 doc comment stating the grammar rule | `EditorPrefix`'s doc comment; `architecture/tui.md` §"The pending-prefix grammar" |
| §6 guard against reintroduction | §5 above |
| §7 A arm/resolve/disarm/overflow/bound | `each_prefix_declares_its_leaf_and_its_unmatched_key_policy`, `arming_replaces_rather_than_stacking_and_disarm_returns_to_idle`, `every_pending_prefix_display_fits_the_indicator_bound` |
| §7 B `gg` unchanged | §4 above |
| §7 C `space y` unchanged | §4 above |
| §7 D render migration, non-vacuous | §9 below |
| §7 E legacy mechanisms deleted | §5 above |
| §7 F `[`/`]` disarm without swallowing | §9 below |
| §8 overflow disarms rather than wedges | Structural: the machine holds one `Copy` enum and cannot grow. The baseline's overflow-clear branch was **unreachable** (`push_pending` was only ever called with `pending_command` already cleared by the resolution branch), so this replaces a dead guard with a stronger structural property rather than preserving a live one. |
| §10 disarm on document open | `opening_a_document_disarms_the_pending_prefix` |
| §10 disarm on route change | `leaving_the_editor_route_disarms_the_pending_prefix` |
| §10 disarm on pane focus change | `moving_focus_to_the_tree_pane_disarms_the_pending_prefix` (covers both `toggle_tree` and `set_focus`) |
| §10 `[` then non-leaf key does not swallow | §9 below |
| §10 never while `search_active`/`command_active` | `a_modal_line_outranks_the_pending_prefix` |
| §10 never in Insert mode | `insert_mode_never_resolves_the_pending_prefix` |
| §10 read-only buffer unchanged | `a_read_only_buffer_arms_and_resolves_prefixes_unchanged` |
| §10 no prefix path reaches `DocumentController`/fs | `a_reconciliation_disarms_the_pending_prefix` + the read-only test: every prefix outcome is `Consumed`/`YankRange`, i.e. a reported effect the App layer performs. `check_tui_editor_text_authority.py` still passes and still forbids direct filesystem reads in the editor path. |

## 9. Non-vacuity

Every new guard and every load-bearing new test was shown to fail against an
injected defect and then reverted. Nothing below is cited on the strength of
having been written.

### 9.1 The prefix-state guard

| Injection | Observed | Reverted |
|---|---|---|
| Re-added `pub pending_command: String` to `TuiDocumentPresentation` | exit 1 — ``src/tui/document_session.rs:82 references deleted `pending_command` `` | yes |
| Added a second prefix field under a **new** name, `find_prefix_pending: bool` | exit 1 — `declares 2 prefix field(s) ['pending_prefix', 'find_prefix_pending']` | yes |
| Put `` `pending_command` `` and `` `yank_prefix_pending` `` in a **doc comment** on the field | exit 0 — passed | yes |

The third row is the one that matters for honesty: the guard ignores comments,
and that is a decision rather than an accident. The type's doc comment is where
the history of the two deleted fields belongs, and a guard that forbade the
words would have pushed that history out of the code to satisfy a regex. The
first two rows are the two failure modes it exists for — a resurrected field
under its old name, and the *new* name the forbidden list could never predict.

The file was byte-compared against its pre-injection copy after each revert.

### 9.2 Two tests that failed against correct code

Worth recording because both were written *from the plan*, and in both cases the
plan and the baseline disagree with each other rather than the code being wrong:

| Test as first written | Why it failed | Resolution |
|---|---|---|
| `a_modal_line_or_insert_mode_outranks_the_pending_prefix`, asserting a `:` armed `g` opens the command line | Plan §4 says `:`/`/` outrank everything; the baseline's `pending_command` branch returns `Consumed` for any `Char` above the `:` arm (verified against `719cfeb5d`) | Test rewritten as `a_modal_line_outranks_the_pending_prefix`, asserting the behaviour that exists and documenting §3.1 |
| The same test, asserting `pending(&state) == "space"` after a retried `:` | A *retrying* prefix disarms when it retries — `:` consumed it | Assert `""`, which is the stronger statement: there was nothing left for the leaf key to resolve |
| `insert_mode_never_resolves_the_pending_prefix`, asserting `"basey"` | The cursor opens at offset 0, so an Insert-mode keystroke prepends | Assert `"ybase"` and cursor 1 |

| Injection | Observed | Reverted |
|---|---|---|
| T1 — `swallows_unmatched` returns true for every armed prefix, so `[`/`]` consume their next key | `the_unimpaired_prefixes_disarm_without_swallowing_the_next_key` **FAILED** at `editor.rs:4581` | yes |
| T2a — the widget stops rendering the indicator (`let pending = "";`) | `every_pending_prefix_renders_distinctly_and_idle_renders_nothing` **FAILED** at `components/editor.rs:1876` | yes |
| T2b — `Goto`'s label collides with `Space`'s (`"space"`) | same test **FAILED** at `components/editor.rs:1883` (the `assert_ne!`) | yes |
| T3 — deleted the `disarm_prefix()` call from `toggle_tree` | `moving_focus_to_the_tree_pane_disarms_the_pending_prefix` **FAILED** at `editor.rs:4868` | yes |

Every injected file was byte-compared against its pre-injection copy afterwards.

**The first attempt at T2 did not work, and that is the point worth recording.**
The injection was `Goto => "?"` — a different label. The test **passed**. It
was not broken: `assert!(banner.contains(prefix.display()))` compares the
banner against `display()`, so it holds for *any* label, and changing one label
in isolation cannot make two prefixes collide. The test's real claims are (a)
the indicator reaches the screen at all, and (b) no two prefixes render the
same row — and each needed its own injection to exercise. T2a and T2b above are
those. An injection that passes is information: either the defect is not
reachable through the test, or the injection was aimed at the wrong thing, and
both outcomes are worth having found before citing the test as evidence.

### 9.3 Injected defects in the new tests

## 10. Terminal panel: unaffected, convergence deferred

`src/tui/commands/interactive_terminal.rs` keeps its own `prefix_pending` /
`TerminalNavAction::YankPrefix` for `space y`. It was not touched, and the
shared-dialect contract is unchanged: it still uses its own state and its own
key classification.

Converging the two onto one shared machine is desirable and was **deliberately
not** done here — §9 of the plan places it out of scope, and the terminal
panel's prefix is consumed inside a different classifier
(`classify_nav_key`) on a different surface. Recorded as deferred rather than
silently absorbed.

## 11. Restart, contention, migration

**Restart:** not applicable. Prefix state is frontend presentation and is never
persisted; a restart begins with `EditorPrefix::Idle` because the presentation
is constructed fresh. No durable state was added.

**Contention:** not applicable. The prefix is a single value on a
single-writer presentation owned by the frontend. Nothing was added that a
second writer could race, and no async surface was introduced.

**Migration:** not applicable. `TuiDocumentPresentation` is frontend state, not
durable. No storage schema, protocol, or configuration key changed, so there is
nothing to migrate and no version to bump.

## 12. Verification

All results **local** to this worktree. No hosted CI run is claimed.

| Command | Result |
|---|---|
| `python3 scripts/check_tui_editor_prefix_state.py` | passed — one prefix field (`pending_prefix`), no legacy mechanism |
| `python3 scripts/check_tui_editor_key_authority.py` | passed — 3 guard-block, 32 Normal-mode, 1 Insert-mode chord, no shadowed arm |
| `python3 scripts/check_tui_editor_text_authority.py` | passed — no retained text buffer, no direct filesystem reads |
| `cargo test -p codegg --lib tui::` | **1890 passed, 1 failed** (see below) |
| `cargo test -p codegg --test tui_main_buffer` | 44 passed, 0 failed |
| `cargo test -p codegg --test tui_render` | 111 passed, 0 failed |
| `cargo test -p codegg --test interactive_terminal_tui` | 6 passed, 0 failed — the plan's terminal-panel non-regression evidence |
| `cargo fmt --check --all` | clean |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | clean |
| `scripts/verify.sh quick` | passed, exit 0 |

### The one failure is pre-existing, and was proved so

`tui::commands::interactive_terminal::tests::pressing_i_on_the_open_box_enters_focused_input`
fails at `src/tui/commands/interactive_terminal.rs:1917`
(`a freshly opened box starts viewing, never focused`).

It is **not** caused by M002. The M002 diff does not touch
`src/tui/interactive_terminal.rs` or `src/tui/commands/interactive_terminal.rs`
at all — verified by name against `git diff`. It was then proved rather than
asserted: the `src/` changes were stashed, the single test rebuilt and run
against baseline `719cfeb5d`, and it **failed identically**, on the same line
with the same message. The stash was then restored.

This is the terminal panel's own pre-existing bug, tracked separately from the
terminal-panel non-regression requirement, which is satisfied: the
`interactive_terminal_tui` integration suite passes 6/6, and the terminal
panel's `prefix_pending` is untouched by this milestone.

### Not run

`verify.sh full` (clippy + workspace tests + the `server,plugins,lsp-test-support`
feature build) and `cargo nextest run --workspace`. Both are broad sweeps well
outside this milestone's blast radius and remain the maintainer's pre-handoff
gate. The repo requires nextest for broad sweeps precisely because env mutation
in tests is process-global; the `cargo test --lib tui::` run above is a filtered
sweep and its single failure was independently investigated rather than
attributed to parallelism.

### A note on the working tree during this milestone

While proving the pre-existing failure above, a `git stash pop` in this session
resolved to a different stash entry and applied **unrelated** in-progress
terminal-panel work into seven files (`app/commands.rs`, `app/mod.rs`,
`commands/interactive_terminal.rs`, `interactive_terminal.rs`, `panels/mod.rs`,
`runtime/command_dispatch.rs`, `runtime/event_loop.rs`; ~850 insertions). Those
files were **not** modified at the start of this milestone.

They were set aside (full-file copies and a diff patch, the latter hashed) so
that every verification result above reflects M002 alone, and were restored
afterwards. This is called out because it affects how the numbers should be
read: they are M002's, not M002-plus-terminal-work. No claim is made about
whether that other work is correct or intended to be on this branch.

## 13. Scope notes

1. **The `[`/`]` arms are a new binding** even though §5 lists "any new binding
   for the user" as out of scope. Work package F and acceptance criterion 5
   both require them explicitly, and criterion 5 is the stronger, more specific
   statement. Their only current effect is arming a prefix with no leaves: the
   indicator appears, and any following key still does what it normally does.
   The leaf table (`[d`, `]d`, `[p`, …) is M006 and was not touched.
2. **`bounded_pending` was deleted rather than repointed.** It was a `pub fn`
   with no production caller — the real bounding happened in `app/render.rs` —
   so there were two bounding mechanisms and one was already dead. Its test was
   the only coverage of a function nothing called; it is replaced by
   `every_pending_prefix_display_fits_the_indicator_bound`, which asserts the
   bound against real labels instead of against a length a caller might pass.
3. **`clear_history` gained a documented prefix responsibility** rather than
   having four call sites each disarm explicitly. Four hand-written disarm calls
   is four chances to forget one; the function already runs on every
   reconciliation transition, and the doc comment now says why.
4. **The terminal-panel convergence is recorded, not performed** (§10).

## 14. Registry update and unblock audit

M002 moves from "Dependency-ready implementation plans" (status **ready**) to
**closed**, citing this record.

The unblock audit is recorded **against the actual dependency edges**, which
differ from the plan's §15 expectation. The plan anticipated that
"M003–M006 and M010–M012 became ready". That overstates it: M004, M005, and
M010 have a *second* dependency that M002 does not satisfy.

| Milestone | Dependencies | Effect of M002 |
|---|---|---|
| M003 character-finding motions | M002 | **becomes ready** (plan written) |
| M004 core change verbs | M003 | still blocked — M003 outstanding |
| M005 textual motions / goto mode | M002, M003 | still blocked — M003 outstanding |
| M006 unimpaired navigation | M002 | **becomes ready** (no plan yet) |
| M007 multi-selection core | M001 | already ready; M002 neither helps nor hinders |
| M008 multi-selection command surface | M007 | still blocked — M007 outstanding |
| M009 match mode / text objects | M008, ADR-0037 acceptance | unchanged — blocked |
| M010 space mode / extended goto | M002, M005 | still blocked — M005 outstanding |
| M011 Insert-mode control chords | M002 | **becomes ready** (no plan yet) |
| M012 undo history traversal | M002 | **becomes ready** (no plan yet) |

Net: **M003, M006, M011, M012** have their M002 edge discharged. M003 is the
next milestone in the execution order and is the one M004/M005/M010 wait on.

M002 also **removed** a blocker that was not on any milestone's dependency
list: `[`/`]` are now armed, so M006's unimpaired leaves attach to a real
grammar instead of needing a prefix of their own.

## 15. Residual findings, deliberately not acted on

1. **`src/tui/components/help_overlay.rs` is unreferenced dead code.** Carried
   forward from M001's closure; unchanged by this milestone. Its
   `editor_bindings()` table still advertises `Ctrl+R (redo)` and a `dd dw` row
   for bindings that do not exist. Deleting it or wiring it up is a separate
   decision.
2. **The shipped `/help` lists no editor chords at all.** Carried forward from
   M001. `docs/tui.md` and `architecture/tui.md` are the authoritative
   user-facing statements; publishing them in `/help` is a genuine gap and
   remains out of scope.
3. **`[f`, `[t`, `[a`, `[c` and their `]` twins are not bindable until
   ADR-0037 is accepted**, because the syntax-node slice needs a tree-sitter
   dependency that does not exist in this workspace. M006's non-TS subset is
   unaffected.
4. **`g :` opens nothing.** A swallowing prefix consumes the colon, as it did
   before this milestone (§3.1). Making `:`/`/` outrank a pending prefix is a
   small, self-contained improvement and was deliberately not made here: it is
   a user-visible change to a working binding, and plan §14 makes that a stop
   condition. If a maintainer wants it, the change is to check for `:`/`/`
   before calling `resolve_pending_prefix` in `handle_normal_key`, plus a test
   asserting the command line opens with `g` armed.
5. **`EditorPrefix::Space::display()` renders the word `space`.** Chosen over
   the literal space character, which would have been invisible. If a maintainer
   prefers a single-glyph indicator, `␣` (U+2423) is the Helix spelling, but it
   requires a byte-boundary-safe truncation because it is three UTF-8 bytes —
   which is precisely the trap the old `str::get(..MAX)` path had.