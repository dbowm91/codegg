# TUI Modularity and Render Invariants Milestone 005 — Closure Status

Status: conditionally closed

**Implementation, 2026-11-13 — the corrective landed, and criterion 1b is now
true.**

- **The dead state is gone.** `App::context_hint`, `update_context_hint` and its
  call site are deleted; `grep -rn context_hint src/ tests/ apps/` returns
  nothing. Six sites had composed hover strings that nothing read.
- **Both invisible footers now paint.** The painted-row test was written **red
  first**, and the red was stronger than expected: *neither* `/plugin` nor
  `/keybind` painted **any** advertisement row, not merely a missing one.
  `/plugin` now reserves `Section::owned_height(1)` (was `Length(1)` against a
  bordered section, so `Block::inner` gave height 0) and `/keybind` reserves a
  real footer chunk instead of appending its hint to a 59-line body inside a
  13-row interior.
- **`/security-review` truncates at an item boundary** (`render(width)` instead
  of `render(0)`); its geometry — 4 of 8 verbs at 60 columns — is deferred as
  width, not dialect.
- **`/terminal-show` went from an empty row to whole items**, and the caller
  footer seam is typed: `InfoDialog::set_custom_footer(HintLine)` with
  `HintLine::push_line`, five call sites converted.
- **A fourth layout defect was found and fixed**: the tasks overlay's
  `tasks_strip_height` returned the *content* row count while the painter builds
  content **+ 1** lines for its hint, so the hint was the one line always
  clipped. It now reserves the hint row, and the fix is mutation-proven — with
  the `+1` reverted, `tui_main_buffer` fails on `the hint row must paint`.

Corpus **1236 → 1237** (+1: the painted-row test). `invariants` 40 → 41,
`invariants::vocabulary` 5 → 6. **`RECORDED` is byte-identical** — the
declarations were already one vocabulary. Ratchets unchanged: **`[contrast] = 0`,
`[advertised_keys] = 4`, `[hint_truncation] = 0`**. The I4 harness presses
**238** keys (up 2, both `Esc` items from the newly visible rows, both
`claimed`); `INERT` is still exactly the four recorded rows.

**Residue, named:**

| Item | Why it stands |
|---|---|
| `/security-review` geometry | 4 of 8 verbs fit at 60 columns; a budget fixes the truncation *kind*, not the width |
| `/keybind`'s `Widget for &KeybindDialog` | still appends its footer to the body; **no production call site renders it** (state + `modal.rs` construction only), so converting it would be a second untested implementation |
| Export's verb is `Esc back`, not `Esc close` | `app/mod.rs:5361-5366` routes Export's `Esc` to `cancel_mode()`; the prose it replaced ("Press Esc to close") was already false. A content decision, flagged for review |
| `/project-init` keeps `a publish · Esc cancel` | the plan's §5 item 9 specifies exit-last for that bounded row |
| The I4 parser widening | fixture work (list ends, an answer, a second channel, a hunk); changes no bytes |

**The corrective now has a plan: `plans/implementation/tui-modularity-and-render-invariants/012-hint-producer-convergence.md`.** Its
research settled three things this record could not:

1. **`context_hint` is dead state, not a surface.** Six sites compose hover
   strings that **nothing reads** (`app/mod.rs:480,1236,1827,5983,6004,6100`;
   `grep -rn context_hint src/` returns no reader). The right action is deletion,
   not conversion — converting it would have satisfied criterion 1 cosmetically.
2. **Two footers are invisible for layout reasons, not dialect reasons.**
   `/plugin` reserves `Length(1)` and then paints a *bordered* section into it, so
   `Block::inner` gives height 0; `/keybind` appends its hint to the body list,
   where 59 lines go into an inner height of 13 and the row sits 46 lines below
   the last visible one. Both rows were converted correctly and simply had
   nowhere to land.
3. **Criterion 1 as written is unachievable honestly**, so it is restated: a
   *hint row* is a painted row of `<key> <verb>` items, and **1b** adds that every
   hint row must actually be painted — which is what the current scan, counting
   rows rather than surfaces, could not see.

**Implementation, 2026-11-13 — the painted rows are now one vocabulary.**

The scan was widened to read **painted** rows, not only declarations: 49 dialog
fixtures rendered at 100×30, each frame row split at the column rules (`│`,
`┃`, `┆`, `┊`, `╎`), each column split at every separator a hint has ever used,
and a column counted as an advertisement only when **≥2 of its items are
`<key> <verb>`** — which is what separates a hint from a help table or a row of
labelled values. **It went red on 82 findings across 28 distinct rows on 38
surfaces**; that red is the evidence the conversion was incomplete.

**35 drifted rows were converted across ~40 files.** The review table separated
drift from context, and the context was kept:

- **kept (context):** a docked panel names `Shift+← back` / `⇧S panel` where its
  modal names `Esc` — the host pops the stack, so the exit genuinely differs; and
  every per-mode item set (`Esc revert` vs `Esc cancel`, `Enter commit` vs
  `Enter apply`) is preserved. Only the *form* changed.
- **converted (drift):** pipes, double spaces, Title-Case verbs, `Enter:` colons,
  bare-space rows with no separator, parenthesised titles, and rows whose exit
  key sat last. Where a footer named a different alias for the same action than
  the surface's own declaration (`↑/↓ navigate` where it declares `j/k move`),
  the footer adopted the declaration's alias — the declaration is what I4
  measures.
- **`session.rs:1330`** carried **six** `(key, verb)` pairs, not the ten the
  earlier corrective estimated; it is a declared row now.
- **`/info`'s footer** was rendering unbounded and the frame clipped it
  mid-item. It now takes its real inner width as a budget, so `fit_parts` drops
  whole items and the order is the priority: exit → position → verbs.

**`RECORDED` is byte-identical, all 28 entries.** That is the strongest single
result here: the earlier corrective had already unified the *declarations*
correctly, and the drift this pass found was entirely in painted rows. No
declared key was re-spelled and no declared verb changed.

Corpus **1234 → 1236** (+2 vocabulary tests), 0 failures. No advertised key
became measurable-and-inert: the I4 harness presses **236** keys and its `INERT`
set is still the same four recorded rows.

**Criterion 1 is still not met, and the residue is now named and small:**

| Residue | Where |
|---|---|
| Eight producers still in a second dialect | `app/mod.rs:5987-6000` (mouse-hover context hints), `app/render.rs:1207`, `chat_browser.rs:818-837`, `commands/interactive_terminal.rs:1719`, `commands/shell.rs:1555,1683`, `interactive_terminal.rs:2165`, `runtime/command_dispatch.rs:128` |
| Two footers are **never painted** | `/keybind`'s row is pushed past the box and clipped; `/plugin`'s `Length(1)` footer chunk is spent on a `Section::plain` top border, so the text is never written. Both are converted for consistency but the scan cannot verify either — a finding, not a fix |
| One row is clipped by its own box | `/security-review` now leads with `Esc close`, so the loss moved from the exit to the last verb (`Enter preview` is off the end at 100 columns). Making it whole-part-truncated needs a per-dialog budget, the same chrome change as the two above |

The corrective pass this paragraph asked for has since landed and narrowed the
residue: `plans/implementation/tui-modularity-and-render-invariants/012-hint-producer-convergence.md`
is `conditionally closed`, the two invisible footers and the tasks-overlay clip
are fixed and mutation-proven, `help` and `chat` now build their rows through
`HintLine`, and the double-spaced pipe hint dialect is down from 24 dialog files
to **one** (`plan.rs`). What remains is bounded and named in §11 rather than
assumed: `panels/mod.rs`'s `⇧Q leave · …` wrapper and `situational_hints`'
`parts.join(" · ")` still compose rows by hand, and one dialog footer keeps the
old pipe spacing. The status is therefore **`conditionally closed`** — matching
this record's own header and the roadmap row — with that residue named, not
silently dropped.

> **Owner directive, 2026-11-13 — unify the form, keep the context.** Verbatim:
> *"we need to unify this so everything is talking the same language. in some
> cases it may be that the hints are supplying context relevant hints — they may
> be different and relevant to what they're tied to, such as for panels that can
> be shifted between. review that this is the case. in either case, we should
> seek to unify this so it is done in one standard way."*
>
> Two obligations, not one. The **form** is unified: one separator (` · `),
> lowercase verbs, no double spaces, keys spelled as the surface spells them.
> The **content** stays contextual where it genuinely is — a panel docked left
> names a different entry gesture than one docked right, and that difference is
> a feature. The open question this record's criterion 1 turns on is therefore
> narrower than "is `HintLine` the only producer": it is whether every surface
> that advertises keys does so in the one form, with its context carried in the
> items rather than in the spelling.
>
> The rendered-byte stop condition that blocked C002 (16 of 18 footer sites)
> **no longer applies**: the owner has authorised the visible change. The
> conversion is in progress, with the review table that separates contextual
> differences from drift.


Source implementation plan:

- `plans/implementation/tui-modularity-and-render-invariants/005-one-hint-vocabulary.md`

Source subsystem roadmap:

- `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md#milestone-005--one-hint-vocabulary`

Repository baseline reviewed: `7642b9e38` (`feat-auto-20261109-f8abad0d`), plus
uncommitted working-tree work — 239 changed or untracked paths at the
re-verification pass's first reading (254 by its end; 238 at the closeout
pass, 231 at the first pass)

Implementation commits or pull requests:

- **None.** This milestone exists only as uncommitted working-tree work on
  `feat-auto-20261109-f8abad0d`, on top of HEAD `7642b9e38`.
  `src/tui/components/hint_line.rs` is **untracked** (`git status` shows `??`),
  so it has no predecessor version to diff against.

Labels: **measured** is what this record's author read out of the tree or ran;
**reported** is the implementing session's measurement, not repeated here (the
render corpus is expensive and three sessions were compiling concurrently).

## 1. Executive finding

The builder exists and is good. The corrective this record asked for is
implemented and was verified in this pass; the **uniqueness** claim the
milestone is named after is still false, because the producers that remain were
not all named by the earlier pass.

**Delivered and verified in this pass.** `HintLine`
(`src/tui/components/hint_line.rs`, 193 lines) owns the separator
(`HINT_SEPARATOR = " · "`), lowercases every verb on `push`, orders required
items first in `render`, and truncates through a `fit_parts` whose first part is
protected: "a budget that shows any part shows it, and a budget too small for it
renders an empty row rather than a partial key". Measured at the
re-verification pass: **14** dialog modules implement `keymap_hint` and all 14
build it from `HintLine`; **15** dialog modules import the builder
(`ui_node.rs` uses it for a `footer_hint`, not a keymap), with **21**
`HintLine::new()` construction sites across them and the trait default in
`component.rs` as the 15th definition.
**Invariant I6 is closed in the ratchet**:
`docs/tui-render-invariant-baseline.toml` records `hint_truncation = 0` and the
guard reports "61 advertised-key, **0** hint-truncation" as measured when this
paragraph was written. At the re-verification pass the reported advertised-key
count is 4 because M006 is closing that ratchet in the same tree, and that file
belongs to that lane; the figure this milestone owns is the **0**
hint-truncation, which is unchanged. The detector is
still proven by `invariants::baseline::the_i6_measurement_detects_a_lost_exit_key`.

The corrective from the previous pass landed:

1. **The four panel producers build through `HintLine`** (measured):
   `panels/hosted.rs:178-190`, `panels/session.rs:53-60`,
   `panels/agents.rs:240` and `panels/terminal.rs:384`, each calling
   `render(0)`. The hosted row passes the widget's own `keymap_hint()` as **one**
   part; `GENERIC_KEYS` (`hosted.rs:112`) survives as a fallback *content*
   string, not as a second producer of the row.
2. **The shadow `fit_parts` is deleted.** `app/mod.rs:66-68` is `fit_row`, which
   delegates to `hint_line::fit_parts`; its three call sites go through it, so
   the required-first-part rule is the only truncation rule in the tree.
   `grep -rn 'fn fit_parts' src/` returns one definition (`hint_line.rs:78`).
3. **The vocabulary scan is 32 surfaces** — 28 dialog rows plus `PanelSession`,
   `PanelAgents`, `PanelTerminal` and `ComposerRow`
   (`tests/tui_render/invariants/vocabulary.rs:79-101`) — with the floor raised
   to the measured count (`>= 32`, `:144-151`) and a per-row `bounded` flag,
   because the exit-key rule is a property of a *truncated* row (`:44-52`). The
   detector still rejects both recorded dialects and a non-first `Esc`
   (`the_vocabulary_check_rejects_both_recorded_dialects`).
4. **I6's row now goes through the rule**, not only through argument order: the
   composer row calls `fit_row(&["Esc normal", "type"], budget)`
   (`app/mod.rs:10424`), and `fit_parts` protects the first part. The exit item
   is still placed first by the caller — that ordering is the plan's §16
   design — but a truncation can no longer *silently* drop it at that call site,
   and `hint_loses_exit_key` remains the harness detector for a reorder.

All four were re-verified at the re-verification pass, independently of the
pass that wrote them: the four panel producers and the one `fit_parts`
definition are present at the cited lines; the scan enumerates 28 dialog rows +
3 panels + `ComposerRow` and asserts `>= 32` with the `bounded` flag; the two
plan-named selectors select tests (§4). The corpus is **1228 passed, 0 failed**
through cargo at 21:42, and `tui::panels` is **51 passed, 0 failed**. A second
corpus run at 21:52 reported **1227/1** on the I5 contrast-baseline test after
another lane rewrote `src/theme/halloy.rs`; that is not this milestone's file
and not this pass's edit, but the green figure is a timestamped state, not the
tree's last word. Nothing in this list is outstanding.

**The four panel rows are byte-identical to HEAD**, which is a stronger
statement than "the tests pass": the HEAD versions of
`src/tui/panels/session.rs`, `agents.rs`, `terminal.rs` and `hosted.rs` hold the
old literals, and the `HintLine` calls produce the same strings branch for
branch. Session renders exactly
`SESSIONS · jk move · ↵ open · space fold · hl collapse/expand · ⇧S panel`;
agents keeps the same verb order and treats `:new global <name>` as one part;
the terminal's three branches and the hosted
`title · keys · Shift+← back` / `title · gesture in · keys` are unchanged.
`primary_action_label` already returns lowercase (`edit`/`override`), so the
builder's lowercasing changes nothing. The unit tests assert content substrings
rather than whole strings, so this HEAD comparison is the byte-identity
evidence, not the tests.

**Still not delivered: `HintLine` is not the only producer.** The earlier §1
enumerated the panel composers and the main buffer row; it measured
`keymap_hint` implementations and did not measure the drawn footers or the other
`composer_hints` implementations. The remaining producers are concrete sites:

| Producer | Site | What it does |
|---|---|---|
| Help panel row | `panels/help.rs:117-121` | two `to_string()` literals |
| Chat panel row | `panels/chat.rs:402` | `format!("CHAT · {normal} · ⇧S panel")` |
| Panel host wrapper | `panels/mod.rs:1553` | `format!("⇧Q leave · {panel}")` composes the row a second time |
| Situational row | `app/mod.rs:10396` | `parts.join(" · ")` |
| Dialog body footers | 24 files under `components/dialogs/` | a pipe-separated hint literal in the render path; the six read in this pass are drawn rows |

The dialog footers are the user-visible half. `TreeDialog` renders
`j/k move · e expand · f fork · enter select` as its panel row while the same
box's modal footer still renders a pipe-separated
`↑/↓ navigate | Enter select | e expand | f fork | Esc close`
(`tree.rs:253`, `:371`); `session.rs:1317`, `mcp.rs:277` and `:486`,
`theme.rs:251-253`, `template.rs:206`/`:354` and `review.rs:322` are the same
shape. A modal-tree corpus fixture therefore renders the old dialect, while the
scan — which reads `Component::keymap_hint()` — cannot see it. The plan's §3
baseline counted exactly these rows ("of 93 hint strings, 50 use a pipe"), so
they are in scope. The earlier §5 sentence "the dialect is uniform today" is
**wrong**.

Consequence: the four surfaces the corrective named are converted and the
duplicate rule is gone, but acceptance criterion 1 and roadmap completion item 4
("One hint vocabulary is in force") remain false at the sites above. Nothing is
a regression: every string renders what it rendered before; the gap is the
guarantee.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| A `HintLine` builder with one separator | `hint_line.rs:8` `HINT_SEPARATOR = " · "`; `the_builder_owns_the_separator_and_the_casing` asserts the exact string | pass | |
| One casing policy: keys keep their conventional case, verbs are lowercase | `push` lowercases the verb (`hint_line.rs:33`); `the_builder_owns_the_separator_and_the_casing` feeds `Move`/`SELECT` and expects `move`/`select` | pass | Casing is normalised at `push`, so a caller cannot spell a verb in Title Case. |
| Item form `<Key> <verb>`, and a key-only item for a value | `HintItem::text`; `an_empty_verb_renders_the_key_alone` | pass | |
| The exit item is required and ordered first | `push_required` (`:40`); `render` chains required items before the rest (`:50-56`); `a_required_item_renders_before_an_item_pushed_before_it` | pass | Ordering is the builder's, not the caller's — on converted surfaces. |
| A budget that shows any part shows the required one, at 0, 1, 5, 12, 20, 80 | `a_budget_that_shows_any_part_shows_the_required_one`, exactly those six budgets | pass | The plan's §7 B acceptance list, verbatim. |
| A budget below the required item renders an empty row | `a_budget_below_the_required_item_renders_an_empty_row`: 9 → `""`, 10 → `"Esc normal"`, 17 → both | pass | Boundary is measured to the column. |
| No rendered hint ever ends mid-item | `no_budget_ends_in_the_middle_of_an_item` over every budget 0..=64, and `twenty_verbs_render_within_every_budget_without_a_partial_item` over 20 items × 0..=120 | pass | The plan's §10 negative test, implemented as a sweep. |
| `HintLine` is the **only** producer of a rendered hint string | 15 dialog modules + 4 panel producers import it; `panels/help.rs:117-121`, `panels/chat.rs:402`, `panels/mod.rs:1553`, `app/mod.rs:10396` and 24 dialog footers still compose rows by hand | **fail** | Narrowed from §1's earlier enumeration; the list is now measured. |
| One separator in force across panels and dialogs | the 28 dialog `keymap_hint` rows and 4 scanned panels use `·`; 24 dialog files still contain a pipe-separated hint literal (`tree.rs:253`/`:371` is a drawn example; re-measured 24 at the re-verification pass with `grep -rl ' \|  ' src/tui/components/dialogs/*.rs`) | **fail** | The rendered product has two dialects; the earlier "uniform today" reading was wrong. |
| Panel hints (`composer_hints`, `PanelHints`) adopt the builder | `hosted.rs:178-190`, `session.rs:53-60`, `agents.rs:240`, `terminal.rs:384` build through `HintLine`; `help.rs:117-121`, `chat.rs:402` and the `⇧Q` wrapper at `mod.rs:1553` do not | **partial** | 4 of 7 panel implementations; in scope per work package C. |
| Dialog hints adopt the builder | all 14 dialog modules that implement `keymap_hint`; 15 modules import `HintLine`, 21 construction sites | pass | `ui_node.rs` builds a footer hint, not a keymap row. |
| A vocabulary test across surfaces, failing on both recorded dialects | `every_surface_hint_uses_the_one_vocabulary`; `the_vocabulary_check_rejects_both_recorded_dialects` rejects `\|`, `Enter: cancel`, `j/k Move` and a non-first `Esc` | pass | The negative cases are one test with four fixtures, which is stronger than the plan's two. |
| The advertised-verb set per surface is unchanged from baseline | `the_recorded_hint_of_every_surface_is_what_it_renders` compares 28 measured rows against a `RECORDED` table in both directions | **partial** | It guarantees *stability from now on*; the table was authored with the change, so it cannot prove the change preserved a pre-milestone set. Accepted forward-only (§10). |
| The vocabulary test scans **every** surface that advertises a key | `rendered_hints()` walks `fixtures::dialogs()` (28) plus `panel_and_row_hints()` (3 panels + the composer row); the floor is `>= 32` | **partial** | Help, Chat, the hosted fallback row, `situational_hints` and the drawn dialog footers are outside the scan. |
| I6 is zero and the ratchet cannot grow | guard reports **0 hint-truncation** and the manifest's `[[hint_truncation]]` section is empty with a comment saying why; its advertised-key section is another lane's in-flight edit (4 entries at the re-verification pass, 61 when this row was written) | pass | This milestone rests on the hint-truncation figure, not on the advertised-key count. |
| The I6 detector still bites | `hint_loses_exit_key` (`baseline.rs:499`) plus `the_i6_measurement_detects_a_lost_exit_key` and unit cases at `:726-734` | pass | Extracted, so the harness can test the detector without the private producer — the plan's own §6 constraint. |
| `fit_parts` keeps its whole-part rule | one definition (`hint_line.rs:78`); `fit_row` (`app/mod.rs:66-68`) delegates to it, so there is no second copy | pass | The shadow implementation is deleted. |
| No hint advertises a key the surface does not handle (I4 not widened) | I4's baseline was 61 at this milestone's pass and reads 4 at the re-verification pass because M006 closed the recorded keys; this milestone added no key | pass | The change is not this milestone's. |
| `scripts/verify.sh quick` green; render-cost guard unmodified | `check_tui_render_cost.py` passes and is unmodified; `scripts/verify.sh full` is green on this tree at **2026-11-13** (17,377 workspace tests, 8,441 feature-pass tests, 0 failures; fmt and clippy clean), and `run_full()` calls `run_quick()` | pass | See §4 for this pass's focused runs. |

## 3. Production implementation evidence

- `src/tui/components/hint_line.rs` (untracked, 193 lines) — `HINT_SEPARATOR`,
  `HintItem`, `HintLine::{new,push,push_required,render}`, `fit_parts`, and a
  test module of six tests. Module doc: "A hint describes a key; it never
  decides which stage claims a chord, which is ADR-0042's" — the plan's
  ADR-0042 sentence, in the code rather than only in the plan.
- Fifteen dialog modules under `src/tui/components/dialogs/` — `diff`, `files`,
  `info`, `keybind`, `mcp`, `plan`, `plugin`, `queue`, `research`, `review`,
  `security_review`, `task_view`, `theme`, `tree`, `ui_node` — each builds its
  `keymap_hint` from `HintLine` (measured). `queue.rs` shows the shape the plan
  asked for: `hint.push_required("Esc", "cancel").push("Enter", "next")`.
- `src/tui/components/component.rs:234` — the trait's `keymap_hint` doc points
  at `HintLine` as the builder, with the signature unchanged (`-> Option<String>`).
- **Converted by the corrective** (measured in this pass): `panels/hosted.rs`
  (`HintLine` at `:178-190`; `GENERIC_KEYS` at `:112` is content only),
  `panels/session.rs:53-60`, `panels/agents.rs:240` and
  `panels/terminal.rs:384`, each calling `render(0)`.
- `src/tui/app/mod.rs:66-68` — `fit_row` delegates to
  `crate::tui::components::hint_line::fit_parts`. The three-argument duplicate is
  **deleted**; there is one `fn fit_parts` in the tree (`hint_line.rs:78`).
- **Still not converted**, and this is the record's remaining finding:
  `panels/help.rs:117-121` (two literals), `panels/chat.rs:402` (`format!`), the
  `⇧Q leave · {panel}` wrapper at `panels/mod.rs:1553`,
  `situational_hints`' `parts.join(" · ")` at `app/mod.rs:10396`, and the drawn
  modal footers in 24 dialog files (`tree.rs:253`/`:371` and the files listed in
  §1).

Nothing in the plan's "explicitly out of scope" list was implemented: no key was
rebound, nothing was localised, no hint's *content* was changed to suit the
vocabulary, `/shell-show`'s advertised-but-unhandled verbs were left in place
(M006's evidence), and the completion popup's geometry is unchanged.

## 4. Verification executed

### Commands run

```bash
# first pass (the record that produced the corrective)
python3 scripts/check_tui_render_invariant_baseline.py
python3 scripts/check_tui_render_cost.py
grep -rn 'HintLine|fn fit_parts|hint_loses_exit_key' src/
grep -rln 'HintLine' src/tui | sort
grep -rn 'fit_parts' src/tui
grep -rn 'fn keymap_hint' src/tui --include=*.rs | wc -l
# read: src/tui/components/hint_line.rs (whole)
# read: tests/tui_render/invariants/vocabulary.rs (whole)
# read: src/tui/panels/hosted.rs:100-200, panels/{session,agents,terminal}.rs composer_hints
# read: src/tui/app/mod.rs:40-100 and :10340-10450

# closeout pass (this session)
grep -rn 'fn fit_parts' src/
grep -rl ' |  ' src/tui/components/dialogs/*.rs | wc -l
python3 scripts/check_tui_render_cost.py
python3 scripts/check_tui_render_invariant_baseline.py
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
python3 scripts/check_comment_budget.py
```

The two required test selectors could not be launched through `cargo`: the
shared target directory's build lock was held by other lanes throughout the
pass. They were run against the binaries already built from this tree, which
select and execute the same tests:

```bash
./target/debug/deps/codegg-7f071e724f49550c tui::components::hint_line
./target/debug/deps/tui_render-c222fb5313649f59 invariants
./target/debug/deps/tui_render-c222fb5313649f59 invariants::vocabulary
./target/debug/deps/tui_render-c222fb5313649f59    # whole family
```

### Results

| Command | Result |
|---|---|
| `check_tui_render_invariant_baseline.py` | **pass** — "61 advertised-key, 2495 contrast and **0** hint-truncation entries recorded; every one reproduces and none was added unbanked", exit 0 (measured) |
| `check_tui_render_cost.py` | **pass** — "the render path derives each result once", exit 0 (measured) |
| `check_test_assertion_quality.py` | **pass** — "all checks pass" (measured) |
| `check_test_duplication.py` | **pass** — "all checks pass" (measured) |
| `check_comment_budget.py` | **fail** — 21 violations, corpus 138,825 lines / 1,236,980 words / ratio 0.1362 against the banked 138,582 / 1,234,476 / 0.1361; all in other in-flight files (`managed_process.rs`, `security/sandbox*`, `invariants/*`, `codegg-sandbox-helper.rs`), at this pass's reading; those lanes are still editing. Not re-banked. |
| `fn fit_parts` definitions | **1** — `hint_line.rs:78`; the three-argument copy is deleted (measured) |
| Dialog files with a pipe-separated hint literal | **24** (measured, `grep -rl ' |  ' src/tui/components/dialogs/*.rs`) |
| `tui::panels` (lib) | **51 passed, 0 failed**, 8132 filtered out |
| `tui::components::hint_line` selector | **7 passed, 0 failed**, 8176 filtered out |
| `invariants` selector | **31 passed, 0 failed**, 1196 filtered out |
| `invariants::vocabulary` | **3 passed, 0 failed**, 1224 filtered out |
| Whole render family | **1227 passed, 0 failed**, 0 filtered out (29.23 s) |

### Re-verification pass (this session)

No production or test file was edited by this pass. The two selectors the plan
names were re-run **through cargo** (the closeout pass could only run binaries),
and every selector in the plan's §11 was measured for its filter effect — a
selector that matches nothing exits 0, so "it passed" is not by itself proof it
selected anything:

```bash
cargo test --lib -- tui::components::hint_line
cargo test --test tui_render -- invariants
cargo test --test tui_render -- tui_chat_surfaces
cargo test --test tui_render -- tui_composer_claims
cargo test --test tui_render -- tui_composer_keymap_consistency
cargo test --test tui_render -- tui_info_dialog_panel_chrome
cargo test --test tui_render -- invariants::vocabulary
cargo test -p codegg --lib -- tui::panels
cargo test --test tui_render
```

| Selector / command | Result |
|---|---|
| `cargo test --lib -- tui::components::hint_line` | **pass** — **7 passed, 0 failed**, 8184 filtered out |
| `cargo test --test tui_render -- invariants` | **pass** — **32 passed, 0 failed**, 1196 filtered out |
| `cargo test --test tui_render -- tui_chat_surfaces` | **pass** — **29 passed, 0 failed**, 1199 filtered out |
| `cargo test --test tui_render -- tui_composer_claims` | **pass** — **30 passed, 0 failed**, 1198 filtered out |
| `cargo test --test tui_render -- tui_composer_keymap_consistency` | **pass** — **14 passed, 0 failed**, 1214 filtered out |
| `cargo test --test tui_render -- tui_info_dialog_panel_chrome` | **pass** — **13 passed, 0 failed**, 1215 filtered out |
| `cargo test --test tui_render -- invariants::vocabulary` | **pass** — **3 passed, 0 failed**, 1225 filtered out |
| `cargo test -p codegg --lib -- tui::panels` | **pass** — **51 passed, 0 failed**, 8140 filtered out |
| `cargo test --test tui_render` (whole family) | **pass** — **1228 passed, 0 failed, 0 filtered out** |
| same command, run again after another lane rewrote `src/theme/halloy.rs` (21:48) | **1227 passed, 1 failed** — `invariants::baseline::the_recorded_i5_baseline_is_exactly_what_the_harness_measures`: the I5 contrast baseline and the measurement disagree across many themes. That file, the manifest and `src/theme/**` belong to other lanes and were being edited; this pass edited Markdown only, and the selection is the same 1228 tests. |
| `grep -rn 'fn fit_parts' src/` | **1** definition — `hint_line.rs:78` |
| `grep -rl ' \|  ' src/tui/components/dialogs/*.rs` | **24** files with a pipe-separated row (unchanged) |
| `grep -rn 'fn keymap_hint' src/tui` | **14** dialog implementations, all through `HintLine`, plus the trait default |
| `grep -rn 'HintLine::new()' src/tui/components/dialogs/*.rs` | **21** construction sites across **15** files |
| `check_comment_budget.py` | **fail at that pass** — 30 violations, 138,866 lines / 1,237,430 words / 0.1362 against 138,582 / 1,234,476 / 0.1361; other in-flight files, not re-banked. **Discharged 2026-11-13**: green, banked at 139,245 lines inside `scripts/verify.sh full`. |
| `check_tui_render_cost.py` | **pass** — unmodified |
| `check_tui_render_invariant_baseline.py` | **pass** — 0 hint-truncation; the advertised-key section is another lane's in-flight edit |

### Not run

| Command | Why |
|---|---|
| `scripts/verify.sh quick` / `full` / `scoped` | Not run by this session; both are green on this tree — `full` at **2026-11-13**, with 17,377 + 8,441 tests and 0 failures, which discharges the gate class this record carried. |
| `cargo fmt --all --check` | Attempted: it reports diffs only in other lanes' files (`src/managed_process.rs`, `src/security/*`). `rustfmt --edition 2021 --check` passes on the files this pass touched. |
| `cargo clippy --workspace --all-targets -- -D warnings` | Inside `full`; not run individually. |

## 5. Invariant review

- **No hint advertises a key the surface does not handle (I4 not widened).**
  Holds: the I4 baseline was 61 entries at this milestone's pass (it reads 4 now
  because M006 closed the recorded keys), and this milestone added no
  verb.
- **No hint loses a verb it used to name.** Holds for the 14 converted dialogs
  under the both-direction `RECORDED` comparison, and for the four converted
  panel rows against HEAD (§1: the old literals and the new renders are the same
  strings, branch for branch). For the three unconverted panel rows
  (`help`, `chat`, the `⇧Q` wrapper) and the dialog footers there is still no
  before/after comparison — HEAD has the same hand-formatted string, so no
  conversion could have dropped a verb there, but nothing would catch a later
  edit either. **Not proved** for the unconverted surfaces.
- **Key names stay recognisable.** Holds: the builder never touches the key, and
  the panel rows' keys (`jk`, `↵`, `hl`) are unchanged by this milestone.
- **`fit_parts` keeps its whole-part rule.** Holds in the one implementation
  (`hint_line.rs:78`); the three-argument duplicate is deleted.
- **I1, I2, I3b stay green.** The render corpus is **1228 passed, 0 failed** at
  21:42 (re-verification pass, §4); a 21:52 re-run fails only the I5
  contrast-baseline test after another lane's theme edit, so the statement is
  timestamped rather than a claim about the tree's current last word. The
  baseline guard is clean.
- **The render-cost shape guard is unchanged.** Holds — passes, unmodified.
- **One hint producer per surface.** **Does not hold** at the sites in §1: the
  four named panel producers are converted, but `help`, `chat`, the `⇧Q`
  wrapper, `situational_hints` and the drawn dialog footers still compose rows
  themselves. Re-measured at the re-verification pass: 24 dialog files still
  carry a pipe-separated row.
- **The comment ratchet holds.** **Verified 2026-11-13**: `check_comment_budget.py`
  passes inside `scripts/verify.sh full`, with the corpus banked at 139,245
  comment lines. The 30 violations this record reported belonged to other lanes'
  in-flight files at that tree state, not to this milestone.

## 6. Failure and recovery review

- **No runtime path.** Hint rendering is a pure string build per frame.
- **The one hazard the plan names — a hint cached across a focus change — does
  not exist on the converted surfaces**: `render` builds a fresh string from the
  current item list, and the panel rows are rebuilt inside `composer_hints` per
  call. Nothing caches a rendered hint.
- **Bounded output.** `render(budget)` cannot exceed the budget for full parts,
  and `no_budget_ends_in_the_middle_of_an_item` sweeps every budget up to 64 and
  up to 120, so an adversarial item width cannot produce a partial key.
- **Empty list.** `fit_parts` returns `""` for an empty slice rather than
  panicking.
- **Failure visibility.** The vocabulary test prints the offending surface and
  the finding, so a regression names the surface rather than a diff.

## 7. Migration and compatibility review

No schema, protocol, configuration or dependency surface. The change is a
spelling and structure change inside one crate. A hint's *content* is unchanged
by contract, and the plan's §9 is explicit that a verb the surface does not
handle must be left alone rather than quietly deleted — the `/shell-show` rows
were left in place for M006. Rollback is deleting `hint_line.rs` and reverting
the 15 dialog files; there is no durable state.

## 8. Security review

Not applicable in the authorization sense: no path, process, secret, network or
permission surface is touched. The one bounded-input property that matters is
that a rendered hint cannot grow without bound for a given budget, which is
asserted by the budget sweeps above.

## 9. Documentation and operations

- `architecture/tui.md:4396-4436` — the vocabulary, the required-part rule, the
  empty-row case and the ADR-0042 sentence are **present** (measured). The
  section opens "`src/tui/components/hint_line.rs` is the single producer of
  hint text", which is the claim §1 shows is still false; the corrective should
  restore the statement rather than soften it, so §10 records the mismatch.
- The per-surface corpus diff note and the advertised-verb comparison against
  baseline (plan §15) — the `RECORDED` table is the equivalent artifact, but it
  was authored with the change, so it is a forward-going record rather than a
  baseline comparison. Accepted forward-only; §10.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | **`HintLine` is still not the only producer.** The four panel producers and the duplicate `fit_parts` are fixed (§1), but `panels/help.rs:117-121`, `panels/chat.rs:402`, the `⇧Q` wrapper at `panels/mod.rs:1553`, `situational_hints` at `app/mod.rs:10396`, and the drawn footers in 24 dialog files still compose rows by hand. | Acceptance criterion 1 and roadmap completion item 4 remain false. The panel dialect is currently consistent, but a modal-tree footer already renders a pipe-separated row beside the panel's `·` row — two dialects on one surface. | Convert the remaining producers and add them to the scan; the footer conversion changes visible spelling, so review its corpus rows in that change. Bounded and mechanical. |
| low | **`architecture/tui.md:4396` asserts `HintLine` is "the single producer of hint text"** while the sites above remain. | The architecture document records a guarantee the code does not keep. | The corrective restores the statement by converting the sites; do not weaken the doc to match the gap. |
| low | "The advertised-verb set per surface is unchanged from baseline" cannot be proven: the `RECORDED` table was written with the change. | One acceptance criterion is satisfied forward-only. | **Accepted forward-only** (this pass's decision). Reconstructing a baseline would need a commit that predates the milestone. |
| low | `check_comment_budget.py` was red — 30 violations, 138,866 lines against the banked 138,582 at the re-verification pass (21 / 138,825 when this row was written) — from other in-flight files (`managed_process.rs`, `security/sandbox*`, `invariants/*`, `codegg-sandbox-helper.rs` and dialogs those lanes were editing). | The comment ratchet could not be cited green at that tree state. | **Discharged 2026-11-13 by the re-bank arm**: `docs/comment-budget.toml` records `max_comment_lines = 139245`, and `check_comment_budget.py` passes inside `scripts/verify.sh full`. |
| — | **Fixed in the corrective, verified this pass:** the four panel producers, the single `fit_parts`, the scan at 32 with the floor at the measured count and a per-row `bounded` flag. Re-verified at the re-verification pass against the code and the selectors. | — | — |
| — | **Fixed in the plan:** §11's `cargo test --lib -- tui::app::fit_parts` → `tui::components::hint_line`, and `-- tui_render_invariants` → `invariants`. Both corrected selectors were run **through cargo at the re-verification pass** and select tests: **7 passed / 8184 filtered out** and **32 passed / 1196 filtered out** (§4). | — | — |
| — | **Non-vacuity:** the detector's own test covers a pipe row, a colon row, a Title Case verb and a non-first `Esc`; the parent's injection into the session panel failed `every_surface_hint_uses_the_one_vocabulary` and only that test (parent-run, not repeated). | — | — |

## 11. Roadmap disposition

**Conditionally closed**, with the residue named rather than closed. The
corrective landed in two stages: `012-hint-producer-convergence.md` (the dead
`context_hint` deleted, both invisible footers painted, `/security-review`
truncating at an item boundary, the tasks-overlay hint row fixed and
mutation-proven) and the footer conversion (35 drifted rows across ~40 files,
the owner having authorised the visible spelling change). Read against the
current tree, `help` and `chat` build through `HintLine` and the double-spaced
pipe dialect survives in one dialog file (`plan.rs`). What still keeps
acceptance criterion 1 from being literally true is named, not implied:
`panels/mod.rs`'s `⇧Q leave · …` wrapper and `situational_hints`'
`parts.join(" · ")` compose rows by hand.

This is `conditionally closed` rather than `closed` because that residue is
unfinished implementation, not external evidence — the same reason the earlier
pass refused the conditional status, now applied to a much smaller set. It is
also not `corrective pass required` any more: the corrective this record asked
for exists, its items are delivered and verified, and the remaining two sites
are a bounded mechanical conversion with a scan that already covers 32 surfaces.
The user-visible behaviour is not broken and I6 holds at the ratchet
(`hint_truncation = 0`).

What the next milestone may rely on unchanged: `HintLine`'s own contract and
tests, the 14 converted dialog `keymap_hint` rows (15 modules use the builder),
the four converted panel producers, the single `fit_parts`, and
`hint_truncation = 0`.

## 12. Registry updates

- `plans/registry.md` line 29 (subsystem row): "M001–M004 implemented, M005–M010
  in progress or pending" — M005 needs its corrective status named.
- `plans/registry.md` line 683 (the M005 milestone row): status "**Blocked —
  hard dependency on M001**" → `corrective pass required`, with this record's
  path and the remaining-producer finding.
- `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md` §12 row
  `005`: `blocked` → `corrective pass required`, closure record
  `plans/closure/tui-modularity-and-render-invariants/005-status.md`, blockers
  "the remaining hand-formatted producers (`help`, `chat`, the `⇧Q` wrapper,
  `situational_hints`, the dialog body footers) and the scan coverage for them".
- `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md` §11 item 4
  ("One hint vocabulary is in force, and `fit_parts` cannot drop an exit key") —
  the second half is now true (one rule, one definition); the first half is met
  on the dialog `keymap_hint` rows and four panel producers, and outstanding at
  the sites above. The roadmap should say which half, because a completion
  definition that reads as satisfied is what let this gap stay invisible.
- `plans/implementation/tui-modularity-and-render-invariants/005-one-hint-vocabulary.md`
  header: it already reads `Status: corrective pass required`, so no status edit
  was needed. §11's two bad selectors were corrected in the source plan and both
  corrected commands were re-run **through cargo** at the re-verification pass —
  `tui::components::hint_line` **7 passed / 8184 filtered out**,
  `invariants` **32 passed / 1196 filtered out** (§4).
- Do **not** mark M005 closed or conditionally closed on the strength of this
  record: acceptance criterion 1 is still false, and the closure README reserves
  the conditional status for complete implementations with missing external
  evidence.

## Correction log (2026-11-13)

- **§2 and §4, the `quick`/`full` rows.** Were "parent-run" green as of 2026-11-12. Met by: `scripts/verify.sh full` on this tree, **2026-11-13** — 17,377 workspace tests and 8,441 feature-pass tests, 0 failures, fmt and clippy clean. No gate condition is outstanding.
- **§4, §5 and §10, the comment-budget rows.** Were "30 violations, 138,866 lines against the banked 138,582". Met by the re-bank arm: `docs/comment-budget.toml` records `max_comment_lines = 139245`, and `check_comment_budget.py` passes inside the same `full`.
- **§1, the "status therefore stays `corrective pass required`" paragraph.** Superseded by the record's own `Status:` header and the roadmap row, both of which say `conditionally closed`, and by the corrective itself: `012-hint-producer-convergence.md` is `conditionally closed`, `help` and `chat` now build through `HintLine` (measured at `panels/help.rs:118`, `panels/chat.rs:403`), and the double-spaced pipe hint dialect is down from 24 dialog files to one (`plan.rs`). The paragraph now says `conditionally closed` and names what is still hand-composed: `panels/mod.rs`'s `⇧Q leave` wrapper and `situational_hints`' `parts.join(" · ")`.
- **§11, the disposition.** Was "**Corrective pass required** … deliberately not `conditionally closed`" with "24 pipe-separated dialog footers". Met by the corrective above; the disposition now names the two remaining producers instead of the 24 footer files.

**No promotion.** The status stays `conditionally closed` — the header already said so and the roadmap agrees — and the two remaining hand-composed rows are named. This is not `closed`: acceptance criterion 1 is still not literally true.
