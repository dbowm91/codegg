# TUI Composer Helix Parity M007 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-composer-helix-parity/007-keymap-consistency-and-guard-coverage.md`

Source subsystem roadmap:

- `plans/subsystems/tui-composer-helix-parity-roadmap.md` (§7, M007)

Repository baseline reviewed: `3cad45929` (branch `feat/auto-20261019-8cc543fb`)

Implementation commits or pull requests:

- *uncommitted at the time of writing* — the working tree carries the derived verb list, the hint
  row, the derived help section, the route-dependent copy row, the two guard rules, this record,
  and the plan. No hosted CI run is claimed.

Applicable ADRs:

- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` — **accepted** 2026-10-19. This
  milestone does not change its decision. It makes the claim set *visible* — one derived list read
  by three surfaces — and it adds two guard rules that police the surfaces rather than the table.
- The ADR's part 4 conflict table still omits `a`; that gap was recorded by M006's closure record
  and is not an M007 finding.

## 1. Executive finding

The three surfaces that describe the composer's verbs now read **one** answer.

At the baseline the composer had a claim table, a help that listed some of its letters, and a hint
row that said nothing about them. Three surfaces, three answers, and no check that they agreed:
the `a` → "Focus sidebar" row sat in the help for a milestone after the composer had taken the
letter. M007 removes the possibility rather than the instance.

- **`composer_verb_entries`** (`src/tui/composer_claims.rs`) is a read-only view over the claim
  table: the live claims, each with its key, its human label, and its condition. It adds no claim
  and changes no table.
- Three surfaces read it, and **none of them keeps a copy**: the **hint row** above the composer,
  the **help**'s composer section, and the **guard**.
- The help's composer section is now **derived**, so a verb that lands in the table lands in the
  help without anyone touching the help.
- The **route**, not the mode, decides whether the copy gesture is listed. `HelpRoute` moved to
  `src/tui/input.rs`, beside the content it selects, and `HelpDialog::with_route` now **rebuilds**
  that content — the route selects content as well as behaviour.
- Two new guard rules police the help: **G10** (the help against the claim table) and **G11** (the
  app's re-spelled verbs). The guard's self-test grew from fourteen cases to **twenty-six**, and
  each of the new classes is proved to be detected.
- The three-surface sweep found **one divergence** — the terminal panel's word geometry — and
  **three deliberate differences**. The divergence is **recorded, not fixed**: the terminal's `e`
  and `b` are a row-scoped reader's motion over a PTY's screen, not a buffer motion, and changing
  them is a decision the milestone's non-goals forbid without one.

**This closes the workstream.** Every milestone is closed, and the roadmap's §11 completion
definition is checked item by item in §4.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| **AC-M1 — the hint row names every Normal-mode verb available right now, and omits the ones that are not** | `tests/tui_composer_keymap_consistency.rs::the_hint_names_every_live_verb_on_a_multi_line_buffer`, `::the_hint_does_not_claim_the_line_motions_on_a_single_line_buffer` | pass | The row's key set is the derived list filtered by the claim's condition against the buffer's live line count. |
| **AC-M2 — the row's presence does not change with the input mode, so the transcript does not resize on `Esc`** | `::the_hints_presence_is_the_same_in_normal_and_insert_mode` | pass | Focus gates the row; the mode changes its text. |
| **AC-M3 — the row names no verb that is not a live composer claim, and no verb another surface holds** | `::the_hint_names_no_verb_that_is_not_a_live_claim`, `::the_hint_names_the_composers_verbs_and_not_the_transcripts` | pass | The check reads the row back out of the text and compares every single-letter part against the claim table. |
| **AC-M4 — the help lists every live composer verb, and the listing is derived from the claim table rather than hand-written** | `::the_help_has_a_row_for_every_live_composer_verb`, `::the_help_section_is_derived_rather_than_written_out`, `::the_help_and_the_claim_table_agree_on_every_key`; `src/tui/input.rs::composer_help_entries` | pass | The rows are computed by `composer_help_entries` from `composer_verb_entries`. The guard's G10.1 checks the derivation is wired at all. |
| **AC-M5 — the `space`-chord box lists the copy gesture; the `/help` box does not** | `::the_copy_gesture_is_listed_only_in_the_chord_routes_box`, `::the_pre_rendered_constructor_keeps_the_command_route` | pass | The composer's own verbs are in **both** routes — they are documentation. The copy gesture is in one, because it is a gesture a route *performs*. |
| **AC-M6 — the three surfaces agree on selection semantics, proved by running the shared machine's rules against more than one point type** | `src/tui/helix_selection.rs`'s test module: the rules are held once, generic over the point type, and run against the terminal's viewport cell **and** the composer's point — twenty rules each | pass | The test existed from M003; M007's sweep **relied** on it rather than writing a second set of tests that says the same thing in different words. |
| **AC-M7 — the word-geometry divergence is fixed or recorded, with the reason** | `architecture/tui.md`'s "The three surfaces' known differences" table; this record's §5 | pass | **Recorded.** The terminal panel's `next_word_end` / `previous_word_start` are row-scoped over a PTY's screen; the composer and the editor share `text_geometry`'s buffer-scoped rule. The terminal's is a **viewport** motion, not a buffer motion, and §3's non-goals protect the terminal panel's keymap. |
| **AC-M8 — the composer and the editor agree on word boundaries, proved by a test** | `src/tui/composer_text.rs::tests::the_composer_and_the_editor_agree_on_words` | pass | The test was extended to cover `word_end` as well as `word_forward` and `word_backward`, at every offset of a sample text. |
| **AC-M9 — the help audit (G10) passes with the complete claim set, and its self-test rejects each of its three violation classes** | `python3 scripts/check_tui_composer_key_authority.py`; `--self-test` cases `detects a help whose composer section is not derived from the claim table`, `detects a live verb with no label`, `detects a help that advertises a claimed key for a different action` | pass | Three further cases cover the reserved-claim rule and the conditional fall-through exception. |
| **AC-M10 — the modifier-chord audit (G11) passes, and its self-test rejects a help that teaches an old bare letter** | `python3 scripts/check_tui_composer_key_authority.py --self-test` cases `detects a help that lists `x` as the stop key`, `allows a help that lists the stop key only as a chord`, `detects a help that omits a re-spelled action's chord` | pass | The chord spelling is derived from the **binding maps**, not from a hand-maintained list, so the guard and the help cannot disagree about what the chord is called. |
| **AC-M11 — the user documentation, the architecture documentation, and the TUI skill describe the composer's keymap, its ownership boundary, the hint row, and the audits** | `docs/tui.md` ("The composer's own keymap in `[NOR]` mode", "The row above the composer", "Four keys moved to chords"); `architecture/tui.md` (the derivation, the two rules, the differences table); `.opencode/skills/tui/SKILL.md` | pass | The skill names the three readers of the derived list and the route/mode distinction. |
| **AC-M12 — no new timer, poller, or animation cadence was introduced** | `git diff --stat` over `src/` | pass | The hint row reads state. The only render change is `render_composer_hints`, which reads a derived row. |
| **AC-M13 — the roadmap's §11 completion definition is checked item by item, and D7 is recorded as still open with its recommendation** | §4 of this record; the roadmap's §10 (D7) and §12 | pass | D7 (the line start/end motion) stays **open**: it is not this workstream's to settle, and the recommendation it already carries is unchanged. |

## 3. Production implementation evidence

### 3.1 The derived verb list

`composer_verb_entries` (`src/tui/composer_claims.rs`) returns the live claims as
`ComposerVerbEntry { key, key_text, label, condition }`.

- The **key** comes from the claim table; `key_text` is its static spelling, produced by a
  mechanical bijection over the ASCII letters, so it cannot spell a key differently from the
  character it stands for.
- The **label** comes from `ComposerVerb::label`, one match arm per verb. The label is the *name*
  of the thing; the enum's own doc comment is the *meaning* of it.
- The **condition** is the `ClaimCondition` **enum**, not its label: the help needs the words
  (`ClaimCondition::label`) and the hint row needs the *verdict* (`ClaimCondition::holds`), and a
  label alone cannot answer "is this verb available right now?".

Four tests pin it: the derived list has one entry per live claim; it carries every claim's
condition; the conditional verbs are exactly `j` and `k`; and every live claim has a label.

### 3.2 The hint row

`App::composer_hint_row` (`src/tui/app/mod.rs`) builds the composer's own row:
`COMP · NOR · a · b · d · e · i · j · k · o · p · u · U · v · w · x · space help`.

**It names the keys, not the descriptions.** That is a measurement, not a preference: at eighty
columns the *grouped* form — "i/a/o insert · u/U undo · w/b/e word · v/x select · d delete · p
paste · j/k line" — runs to ninety-four characters, and the truncation would drop the line motions
and `p paste` from a row that is supposed to list what is available. The keys fit in fifty-three
characters, and the row's job — "which letters are mine right now" — is answered by the keys alone.
The descriptions live in the help, one keypress away, and the row ends with `space help` pointing
at it.

**Focus gates the row; the mode changes its text.** The row is present in Normal and in Insert
alike, so `Esc` does not reflow the transcript. In Insert mode it reads `COMP · INS · type · Esc
normal`, because the composer's verbs are not available while you are typing — the letters are
text.

**The row is bounded, and a part is never split.** `fit_parts` drops whole parts from the end, in
the claim table's order, and measures in **display columns** rather than bytes, so the `·`
separator counts as one column. A row cut in the middle of a key names a key that does not exist.

**The row's presence is gated on the keyboard, not the mode.** The turn keys still take the row
when a turn is running: stopping a run is actionable now, and the working strip's own status row
names the same key. The composer's verbs are the idle surface's claim, and the composer's row is
where they are finally visible outside the help box.

**The row is reserved in the main buffer, and the working strip does not move.** Because the
composer holds the keyboard in the main buffer, the row is present there — where it used to be
absent — and it costs one row of transcript. The frame is *quieter* for it: the row's presence no
longer depends on a panel, so the carved region of the prompt chunk keeps its height when a panel
takes or loses focus, and the working strip above it stays exactly where it was. §6.4 records the
three tests that said otherwise.

### 3.3 The help's composer section

`composer_help_entries` (`src/tui/input.rs`) maps the derived list to `HelpEntry`s and splices
them at the head of the Normal-mode section, so they read as the composer's own block rather than
as a tail on the app's rows. The section is **derived**, and the guard's G10.1 checks that the
derivation is wired at all — a hand-written list is a second copy of the table, and a second copy
is the drift the table exists to prevent.

### 3.4 The route

`HelpRoute` moved from `src/tui/components/dialogs/help.rs` to `src/tui/input.rs` and is
re-exported from the old path, so every existing import still resolves. The route now sits beside
the **content** it selects, because it selects content as well as behaviour: the chord route's box
lists the copy gesture and the command route's box does not.

`build_help_lines(vim_mode, active_mode, route)` and `searchable_help_rows(vim_mode, active_mode,
route)` take the route. `HelpDialog` remembers the `vim_mode` and the `active_mode` it was built
from, and `with_route` **rebuilds** both the lines and the searchable rows — relabelling the box
without rebuilding it would leave the chord route teaching a gesture it does not have.

The default route is `HelpRoute::Command`, the one that cannot copy, so a box opened without a
route — the pre-rendered constructor, the docked panel — is inert rather than armed.

### 3.5 The two guard rules

**G10 — the help against the claim table.** Four checks:

1. The composer section must be **derived** (the call is present in `src/tui/input.rs`).
2. Every live verb must have a **label**, so the derived row can describe it.
3. **No app-level row may advertise a claimed key for a different action.** The one documented
   exception is a *conditional* claim's fall-through row — the `j`/`k` row, which says what the
   pair does on a single-line composer buffer — and it earns the exception by carrying its
   condition.
4. **No app-level row may advertise a reserved claim.**

The rules are scoped to the **Normal-mode** section, because that is the surface the composer's
claims live on. The Dialog and Command sections describe other surfaces, where the same letters
mean something else and are not the composer's to own. The first cut of G10 was not scoped, and
it reported the command palette's own `d` (detach) and the dialog's own `j`/`k` (navigate) as
violations — the rule was right about the composer and wrong about the help.

**A chord's components are not bare keys.** The guard reads a row's key text and extracts the
letters it *names*. A key spelled with a modifier is a **chord**, and every letter in it is a
chord component: `Ctrl+D/U` is two chords, and neither of them is the bare `d` the composer
claims. The first cut of the rule read `Ctrl+D/U` as naming `U`, and reported the help's "Page
down/up" row as a contradiction; the fix was to recognise the modifier prefix, not to widen the
rule.

**G11 — the app's re-spelled verbs.** Each action in `RESPELLED_ACTIONS` must be documented under
a **modifier chord**, and must **not** be documented under its old bare letter. The chord spelling
is derived from the **binding maps** — `Ctrl+Alt+X` is rendered from the `CONTROL|ALT` + `x`
binding — so the guard and the help cannot disagree about what the chord is called, and there is
no hand-maintained list of help rows for the rule to depend on.

### 3.6 The three-surface sweep

The sweep compared the composer, the editor, and the terminal panel on seven dimensions. The
verdicts are in `architecture/tui.md`'s "The three surfaces' known differences" table:

- **Agree** — character selection includes the head character; line selection spans whole lines,
  one line per press; a deliberate movement extends a character selection and disarms line growth.
  All three are the shared machine (`helix_selection.rs`), held once and generic over the point
  type.
- **Diverge, deliberately** — word boundaries. The terminal panel's `e` and `b` are row-scoped
  over a PTY's screen; the composer and the editor share `text_geometry`'s buffer-scoped rule. The
  terminal's is a **viewport** motion over a **reader**, not a buffer motion.
- **Diverge by design** — vertical motion keeps the column the target line allows in the
  composer and the editor; the terminal scrolls at the edges, because it is a viewport.
- **Known difference** — paste (roadmap D5) and undo/redo exist in the composer and the editor but
  not in the terminal, whose scrollback is read-only.

The divergence was **recorded, not fixed**. The roadmap's §3 non-goals protect the editor's and the
terminal panel's keymaps, and a change to the terminal's word motion is a decision, not an
implementation detail.

## 4. The roadmap's §11 completion definition, item by item

| §11 item | Verdict | Evidence |
|---|---|---|
| The composer supports horizontal, vertical and word motions; character and line selection; delete, yank and paste; undo and redo; and insert transitions | **met** | M004 (motions), M005 (selection and clipboard), M006 (undo, redo, insert transitions). |
| The composer shows a blinking cursor in Insert, a solid cursor in Normal, and no cursor when it does not hold the keyboard, and the cursor always agrees with the badge | **met** | M001's cursor-presence work; both surfaces read the same `composer_input_mode()` derivation. |
| The claim table is closed, single-owner, non-leaking, and policed by a static guard with a self-test | **met** | `composer_claims.rs`: no reserved claim, one owner per key. The guard: G1–G11, with a twenty-six-case self-test. |
| The re-spelled app-level chords are present in both binding maps, are reachable, and are documented | **met** | `Ctrl+Alt+X`, `Ctrl+Alt+D`, `Ctrl+Alt+E`, `Ctrl+Alt+S` in both maps; the guard's G4, G5 and G11; the help's chord rows. |
| Every claimed letter is verified by a dispatch-reachability test through the real key path | **met** | `tests/tui_composer_claims.rs`, `tests/tui_composer_motions.rs`, `tests/tui_composer_selection.rs`, `tests/tui_composer_undo.rs`, `tests/tui_composer_insert_transitions.rs`, `tests/tui_composer_keymap_consistency.rs` — all through `App::on_key`. |
| The composer's selection and word boundaries are the shared assets' behaviour, not a second implementation | **met** | `helix_selection.rs` and `text_geometry.rs`; `the_composer_and_the_editor_agree_on_words` covers all three word motions at every offset. |
| The user documentation, the architecture documentation, and the TUI skill describe the composer's keymap, its cursor rules, and its ownership boundary | **met** | `docs/tui.md`, `architecture/tui.md`, `.opencode/skills/tui/SKILL.md`. |
| No new timer or animation cadence was introduced | **met** | The hint row reads state; no scheduler, timer, or poller was added. |
| No milestone remains open, blocked, or conditionally closed in `plans/registry.md` without a recorded reason | **met** | M001–M007 are all closed; the registry carries their closure records. |

## 5. Verification

| Check | Command | Result |
|---|---|---|
| Formatting | `cargo fmt --all --check` | clean |
| Type check | `cargo check --workspace --all-targets --locked` | clean |
| Clippy (1.89, the repository minimum) | `cargo clippy --workspace --all-targets --locked -- -D warnings` | clean |
| Clippy (1.99, the CI toolchain) | `cargo +stable clippy --workspace --all-targets --locked -- -D warnings` | clean |
| The composer guard | `python3 scripts/check_tui_composer_key_authority.py` | passed |
| The guard's self-test | `python3 scripts/check_tui_composer_key_authority.py --self-test` | passed — 26 cases, every violation class detected and rejected |
| The M007 suite | `cargo test --test tui_composer_keymap_consistency` | 14 passed |
| The canonical gate | `bash scripts/verify.sh quick` | passed |

**The workspace sweep, measured against the baseline rather than asserted.** The sweep
(`cargo nextest run --workspace --locked --profile ci --no-fail-fast`) ran **14,993** tests:
**14,933 passed, 59 failed, 1 timed out**, 7 skipped. The baseline was then **measured**, not
assumed: the commit `3cad45929` was extracted to a clean tree (`git archive`) and the same
binaries were run there, so every failure could be attributed by name.

**Three of the 59 were this milestone's doing, and they are fixed.** The hint row made the
composer's verbs visible outside the help box, which **superseded** a contract three
pre-existing tests encoded (*no panel focused → the row is not reserved at all*). The baseline's
two binaries were **82 passed, 0 failed**; this tree's sweep failed three of them. The three
tests now express the M007 contract — and one of them now asserts a *stronger* property than
the one it used to (§6.4):

- `tests/tui_chat_surfaces.rs::no_panel_focused_advertises_no_keys`
- `tests/tui_chat_surfaces.rs::an_unbound_panel_advertises_nothing_and_takes_no_focus`
- `tests/tui_panel_docking.rs::the_terminal_hint_row_only_exists_while_the_panel_holds_the_keyboard`

**The other 56 failures and the 1 timeout reproduce at the baseline, by name.** They are this
machine's environment, not this workstream's code:

| Count | What | Why it fails here |
|---|---|---|
| 33 | `codegg::lsp` | the LSP tool is built without a workspace root and refuses — *"no workspace root was resolved for this call (the tool was constructed without a workspace root); refusing to substitute the process working directory"* |
| 6 | `codegg::identity_m002_live_audit_hooks`, `codegg::identity_live_execution_audit` | they need live credentials and a shell |
| 4 | `codegg::git_execution_origin_matrix` | they need a git identity |
| 3 | `codegg::tool_surface_minimization` | they assert the tool registry's own contents |
| 2 | `codegg::tool_program_search_palette` | the external `eggsearch` backend is not installed |
| 2 | `codegg::session_family` (`control_m004_controller_lease`) | the daemon's root-path checks, which the macOS `/tmp` symlink breaks |
| 2 | `codegg core::daemon::tests` | the same macOS `/tmp` symlink |
| 2 | `codegg::tool_program_git_lsp_palette`, `codegg::workspace_root_execution_authority` | the same class — the temp root under `/var/folders` is a symlink to `/private/var/folders` |
| 1 | `codegg::workspace_isolation` | the same symlink (`InvalidRootPath`) |
| 1 | `codegg tool_advisor::causal_frontier::m003_checked_in_receipt_matches_recomputation` | upstream, present at the baseline |
| 1 (timeout) | `codegg agent::asset_refresh::same_scope_requests_coalesce_to_one_publication` | a load flake — **measured to pass in isolation** |

The baseline run produced **19 load-induced failures of its own** that this sweep did not have —
fifteen `security::workflow::report` tests, two `tool::bash` tests, `core::eggpool`,
`core::daemon_documents` (timeout), `core::transport::daemon_socket` (timeout), and
`tui::commands::interactive_terminal::panel_rendered_buffer_shows_live_pty_output`. They are
timing-sensitive tests that flaked while the baseline run competed with this session's clippy and
other sessions' builds. The diff touches none of them: `git diff --stat -- src/ crates/` is
confined to `src/tui/`.

**So the tree's own failure count is 56 + 1, all of them confirmed at the baseline, and
`scripts/verify.sh full` is therefore not green on this machine.** Hosted CI is where the suite
is expected to pass.

## 6. Deviations and decisions

### 6.1 The guard's G10 needed a scope the plan did not name

The plan's G10 rule 3 — "no row advertises a claimed key for a different action" — is right about
the composer and wrong about the help as a whole. The first cut read every row in every mode, and
reported the command palette's own `d` (detach) and the dialog's own `j`/`k` (navigate) as
violations. The rule is scoped to the **Normal-mode** section, which is the surface the composer's
claims live on. This is a narrowing of the plan's wording, and it is recorded here because a rule
that is quietly wider than the code is worse than no rule.

### 6.2 G10.1 proves the derivation is wired, not that the rows are present

The help's composer rows are computed at runtime, so the guard cannot read them in the source. It
checks that the derivation is **called** — that `input.rs` uses the claim table's verb list — and
that every live verb has a label, which is what makes the derived rows complete. It is a proof
chain rather than a direct observation, and the guard's docstring says so rather than implying
more.

### 6.3 The route moved to `src/tui/input.rs`

`HelpRoute` was defined in `src/tui/components/dialogs/help.rs` by M005. It moved to
`src/tui/input.rs` and is re-exported from the old path, because the route selects **content** as
well as behaviour, and the content lives in `input.rs`. Every existing import
(`crate::tui::components::dialogs::help::HelpRoute`) still resolves.

### 6.4 The hint row superseded a pre-existing contract, and three tests said so

WP2 makes the composer's own verbs the row's content whenever the composer holds the keyboard.
That **supersedes** a contract three pre-existing tests encoded: *"no panel focused → the row is
not reserved at all, so the prompt keeps its full height."*

| Test | Before | After |
|---|---|---|
| `tests/tui_chat_surfaces.rs::no_panel_focused_advertises_no_keys` | the row is absent | the row names the composer's own verbs and no panel's |
| `tests/tui_chat_surfaces.rs::an_unbound_panel_advertises_nothing_and_takes_no_focus` | the row is absent | the unbound panel still takes no focus and still says nothing; the row names the composer |
| `tests/tui_panel_docking.rs::the_terminal_hint_row_only_exists_while_the_panel_holds_the_keyboard` | the working strip moves up into the vacated row | the working strip does not move at all — the composer takes the row |

The two `tui_chat_surfaces` tests keep their **names**, because the file-picker workstream cites
them by name for its own contract — *an empty or unfocused panel advertises nothing*
(`plans/closure/file-picker-rework/001-status.md:225`,
`plans/implementation/file-picker-rework/001-panel-keymap-authority-and-listing-hierarchy.md:159`).
That contract still holds, and M007 does not touch it: a panel that does not hold the keyboard
says nothing, and the row's content is the composer's. Renaming the tests would have left those
citations dangling, and editing a closed record of another workstream to chase a rename is worse
than a doc comment on a test whose subject is one word longer than its name.

The third is the one worth reading. That test pinned the **frame geometry** of the row: a panel
that released the keyboard used to leave the row empty, so the carved region of the prompt chunk
shrank and the working strip moved up into it. M007's row is never vacated — the composer takes
it — so the carved region keeps its height and **the working strip does not move at all**. That
is a strictly stronger property than the one the test asserted, and the test now asserts it.

The cost is one row of transcript in the main buffer: the composer's verbs are visible outside
the help box, and a row is what that costs. The input box does not move, and `Esc` does not
reflow the frame, because the row's **presence** is gated on the keyboard and never on the mode
(I5). `architecture/tui.md` records the corrected reading of the layout: the row's *content*
changes with the keyboard, its *presence* does not.

### 6.5 The plan named two test files; the tests landed in three places

The plan's §10 names `tests/tui_composer_discoverability.rs` (the hint row and the help) and
`tests/tui_helix_surfaces.rs` (the three-surface sweep). The implementation put them elsewhere:

| Plan's item | Where it actually lives |
|---|---|
| 1–10, the hint row and the help | `tests/tui_composer_keymap_consistency.rs` — one new integration file, fourteen tests |
| 11, the shared selection machine over both point types | `src/tui/helix_selection.rs`'s own test module, which holds the rules once and runs them against a terminal-style viewport cell and the composer's point |
| 12, the composer and the editor agree on words | `src/tui/composer_text.rs::tests::the_composer_and_the_editor_agree_on_words` |
| 13, the terminal's word motion pinned as it is | `src/tui/interactive_terminal.rs::tests::word_motion_walks_word_boundaries_within_the_cursor_row` |

Every planned item is covered, and the guard's five self-test cases are in the guard as planned.
The **file names** were the plan's prediction and the subjects decided otherwise: a test about the
composer's words belongs next to the composer's words, and a test that pins the terminal's
divergence belongs next to the terminal's motion. A single "surfaces" file would have put all
three in one place that owns none of them.

## 7. Known differences and open items

- **D7 (the line start/end motion) is still open.** The workstream did not settle it: it is a
  decision about what `0` and `$` do in the composer, and the recommendation it already carries
  (bind `Home`/`End` to the buffer's line start/end and leave the letters alone) is unchanged.
- **The terminal panel's word geometry diverges from the shared rule, deliberately.** It is
  recorded in `architecture/tui.md` and here. Changing it is a decision, not an implementation
  detail.
- **Roadmap D5 (paste) is closed in the composer.** The editor's own paste roadmap is a separate
  workstream.
- **The ADR's conflict table still omits `a`.** Recorded by M006; an accepted ADR is immutable.
