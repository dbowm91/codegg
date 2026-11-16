# TUI Modularity and Render Invariants Milestone 002 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/tui-modularity-and-render-invariants/002-one-panel-chrome.md`

Source subsystem roadmap:

- `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md#milestone-002--one-panel-chrome`

Repository baseline reviewed: `7642b9e38` (`feat-auto-20261109-f8abad0d`), plus
uncommitted working-tree work

Implementation commits or pull requests:

- **None.** This milestone exists only as uncommitted working-tree work on
  `feat-auto-20261109-f8abad0d`, on top of HEAD `7642b9e38`. `src/tui/panels/chrome.rs`
  is still **untracked**, and the panel files that adopt it carry the changes
  uncommitted. Nothing below is attributable to a SHA.

## 1. Executive finding

The primitive is delivered and adopted everywhere it was scoped to.
`src/tui/panels/chrome.rs` owns exactly the four decisions the plan specifies —
whether chrome is drawn (`has_chrome`), the border style, the title string and
the title style — and all six panel surfaces paint their edge through it: Chat,
Terminal, Session, Agents, Help, and the hosted host that serves the remaining
eighteen panels.

Three consequences the plan exists to fix are fixed:

1. A focused Agents panel now changes its border **and** its title style, because
   `Modifier::BOLD` is no longer the signal (it was applied unconditionally and
   carried no information).
2. Session and Chat now signal focus identically — both change border and title.
3. Help has an outer border for its title to sit on, instead of riding the first
   inner box's border.

The control case holds. Chat and Terminal are **byte-identical** to their
pre-adoption rendering, asserted two ways: eight recorded digests measured at
`7642b9e38` (Chat and Terminal at 80×24 and 120×40, focused and unfocused), and a
cell-for-cell comparison of the shared chrome against a reconstruction of the
hand-rolled `Block` those two panels used to build. If adoption had moved either
panel, the plan's stop condition would have fired.

I3b is asserted rather than reviewed: the sweep walks
`SidePanelId::ALL × PanelPlacement::ALL × {20×6, 40×12, 80×24, 120×40}` through
the production panel path and requires, for each, that at least one **edge** cell
keeps its glyph and changes its style. It is proved non-vacuous against a
reconstruction of the pre-adoption Agents panel, which is rejected with the
property that failed named. I3a (whether focus is visible at all) stays in
`focus.rs` unchanged, which is what the plan wanted: M002 must not remove the
only difference while it changes everything else.

**The reviewed growth is 78 contrast rows, and it is a decision, not a leak.**
Standardising the unfocused hosted title on `theme.muted` lowers that title's
contrast below `MIN_DERIVED_TEXT_RATIO` on 13 hosted surfaces across 6 themes.
Those 78 `[contrast]` rows were banked deliberately, in the same diff, with none
removed; `docs/tui-render-invariant-baseline.toml` records the reason in its own
header and names **M007** as the owner of driving them to zero. Matching `muted`
was the acceptance criterion (Chat and Terminal already used it and they are the
byte-identity control), so the growth is the cost of the control case, and the
ratchet now holds it. The banked total moved from 2945 to 3023.

One defect was found in this area and **has since been fixed**: `dock` bound
`two_slots_mut`'s two borrows positionally as `(from, to)`, while
`two_slots_mut` returns them ordered by `PanelPlacement::index()`. Moving a panel
to a lower-index edge therefore moved the wrong panel. `dock` now selects the
borrow pair by index — `let (from_slot, to_slot) = if origin.index() <
placement.index() { (pair.low, pair.high) } else { (pair.high, pair.low) };`
(`src/tui/panels/mod.rs:1370-1381`) — and the regression test
`a_move_to_an_earlier_edge_moves_the_panel_the_gesture_named`
(`src/tui/panels/mod.rs:2170`) asserts `Bottom→Right` and `Left→Right`, that the
origin restores the panel it was showing, and that the selected edge is the one
that received the panel. It passes in the 2026-11-13 `scripts/verify.sh full`
run. The defect was pre-existing and outside this milestone's scope when it was
found; it is recorded here as fixed rather than as an open risk.

Status is conditional for two reasons, neither of them the primitive: §15's
*per-panel corpus diff note* was never produced as an artifact, and the
pre-change frame it would be measured against no longer exists — so the
mechanical substitute needs the owner's acceptance in writing (§10, open
question); and the §14 theme collision is real on 4 bundled themes, with
`Solarized Dark` leaving focus visible only as the `●` glyph — reported here,
owned by M007. The two conditions this record previously carried as outstanding
— the corpus-wide comment ceilings / `verify.sh quick`, and the docking defect
that meant the panel manager was "not in a state a closure record should call
fully sound" — are discharged: `scripts/verify.sh full` passed on 2026-11-13
(fmt and the guards run inside it), the comment corpus is banked at 139,245
lines, and `dock` is fixed with a regression test in place.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| One `PanelChrome` primitive | `src/tui/panels/chrome.rs` — `PanelChrome { label, focused, theme }`, `draw`, `styles`, `title` | pass | 237 lines including tests. |
| It owns exactly four decisions | `has_chrome`, `ChromeStyles::resolve` (border + title), `chrome_title` (string) | pass | Documented as a table in the module header. |
| `draw` returns the inner rect, inset by one on each drawn edge | `chrome.rs` unit tests (`draw` returns the inset; a corner shows) | pass | Callers do not re-derive the inset. |
| Border style: `primary` focused, `border` unfocused | `ChromeStyles::resolve`; `focus_style_changes_the_edge` | pass | |
| Title style: `primary` focused, `muted` unfocused | same | pass | This pairing is where the 78 banked rows come from. |
| Focus marker is a trailing `●` inside the title, always when chrome is drawn | `chrome_title("Agents", true) == " Agents ● "`, `chrome_title("Agents", false) == " Agents "` | pass | The marker cannot be painted anywhere but on the border it names. |
| `Modifier::BOLD` is not part of the focus signal | `ChromeStyles::resolve` sets `fg` only; the pre-adoption fixture that used `BOLD` is what I3b rejects | pass | |
| Chrome predicate: `width >= 12 && height >= 4`, reusing Chat/Terminal's guard | `MIN_CHROME_WIDTH`/`MIN_CHROME_HEIGHT`, `has_chrome`; unit test asserts false below and true at the boundary | pass | |
| Below the threshold a panel paints borderless and I3b is not asserted | `every_panel_below_the_chrome_threshold_paints_no_chrome_border` | pass | |
| Chat adopts the primitive | `src/tui/panels/chat.rs:275` | pass | |
| Terminal adopts the primitive | `src/tui/panels/terminal.rs:203` | pass | |
| Session takes border and title style from the primitive | `src/tui/panels/session.rs:38`, `SidebarWidget::render_with_title` | pass | |
| Agents adopts it, and its title comes from `SidePanelId::Agents.label()` rather than the literal | `src/tui/panels/agents.rs:195`; no `" AGENTS ● "` literal remains | pass | The second source of truth for the identifier is gone. |
| Help gains an outer border and passes the inner area on | `src/tui/panels/help.rs:89`; `HelpDialog::render_into` lost its `title` parameter and no longer draws the panel's outer chrome | pass | `tests/tui_render/tui_panel_host_chrome.inc` was updated to the new call shape. |
| The hosted host builds its chrome through the primitive | `src/tui/panels/mod.rs:114`; `render_hosted` now takes `(label, focused)` instead of a pre-composed title | pass | The title string no longer carries the focus decision. |
| I3b: the focused/unfocused difference is a border- or title-style change, uniformly, wherever chrome is drawn | `tests/tui_render/invariants/chrome.rs::every_panel_marks_focus_with_an_edge_style_change` over all panels × all placements × 4 sizes | pass | Asserts `panel_at(placement) == panel` too, so it measures the panel it names. |
| I3b is not vacuous | `the_i3b_check_rejects_the_pre_adoption_agents_panel` — a reconstruction of the old Agents block (fixed border, title `primary` + `BOLD`, only `●` changing) is rejected, and the message names "edge cell whose style changed" | pass | This is the pre-adoption failure the plan's §15 asks for. |
| Chat and Terminal corpora are byte-identical (the control case) | `chat_and_terminal_corpora_stay_byte_identical_to_the_pre_adoption_baseline`: 8 digests measured at `7642b9e38` | pass — **0 cells changed** | The strongest single fact in this record. |
| …and the primitive itself reproduces the old block | `the_chrome_primitive_reproduces_the_block_chat_and_terminal_hand_rolled`: shared chrome vs the legacy `Block`, differing cells must be empty, 2 sizes × 2 focus states | pass | Localises a failure to the primitive rather than to a panel's content. |
| No panel loses content | Same digests for Chat/Terminal; the I3b sweep requires the difference to be on an **edge** cell, so a body-only change cannot satisfy it | partial | See §10: there is no per-panel pre/post content assertion for the 18 hosted panels. |
| No panel gains a key | The `panels/mod.rs` diff is confined to `pub mod chrome` and the `render_hosted` signature; the panel manager, the directional matcher and the docking gestures are outside it | pass | No chord table was edited. |
| I1 and I2 stay green | `cargo test --test tui_render -- invariants`: 24/24, including I1 containment and the 1×1 grid | pass | |
| The render-cost shape guard is unchanged | `check_tui_render_cost.py` passes with no edit | pass | One chrome producer is a reduction in work. |
| The 78-row contrast growth is banked and attributed | `docs/tui-render-invariant-baseline.toml` header "The banked growth: the unfocused hosted title (M002)" plus `[totals] contrast = 3023`; the harness equality test passes | pass | Reviewed growth, reason recorded, M007 named as owner. |
| Help remains readable at 20×6 with an outer border | Not asserted directly | **not run** | §10 finding. The 20×6 grid is covered by I2 (no panic, non-blank), not by a readability check. |
| The §14 theme interplay is reported, not patched with a fallback colour | `ChromeStyles::resolve` has one rule and no per-theme special case; the collision is recorded in §10 | pass | 4 of 50 themes resolve `primary == border`; `Solarized Dark` also resolves `muted` to the same colour, so focus is glyph-only there. This is the plan's §14 stop condition, and reporting is what it asks for. |
| `architecture/tui.md` states the focus policy and the chrome predicate as a contract | "TUI panel chrome" section: the four decisions, the predicate at 12×4, `draw` returning the inner area, and `Section` as the dialog counterpart | pass | |

## 3. Production implementation evidence

- `src/tui/panels/chrome.rs` — the primitive, with the focus policy and the
  chrome predicate documented as the contract a new panel must follow, and
  `ChromeStyles::resolve` made public because `Section` (M003) reuses it.
- Six adoption sites, all in `src/tui/panels/`: `chat.rs`, `terminal.rs`,
  `session.rs`, `agents.rs`, `help.rs`, `mod.rs` (the hosted host).
- `render_hosted`'s signature changed from taking a fully-formed title to taking
  `(label, focused)`. That is the structural expression of the milestone: the
  focus decision moved out of a string and into the primitive. The one test
  fixture that called it with a title
  (`tests/tui_render/tui_panel_host_chrome.inc`) was updated in the same change.
- `src/tui/components/section.rs` (M003's dialog counterpart, untracked in this
  same tree) already imports `chrome_title`, `has_chrome`, `ChromeStyles` and
  `FOCUS_MARKER`, which is the interface dependency the plan said M002 must fix
  and M003 must be able to reuse. It is present and does so.

No key, command, placement or docking behaviour was changed. No protocol,
storage, configuration or authority surface was touched.

## 4. Verification executed

### Commands run

Local, in `/Users/davidbowman/projects/codegg/.worktrees/feat-auto-20261109-f8abad0d`.

```bash
cargo test --test tui_render -- invariants
cargo test --test tui_render            # whole family
python3 scripts/check_tui_render_invariant_baseline.py
python3 scripts/check_tui_render_invariant_baseline.py --self-test
python3 scripts/check_tui_render_cost.py
python3 scripts/check_comment_budget.py
```

### Results

| Command | Result |
|---|---|
| `cargo test --test tui_render -- invariants` | **pass** — 24 passed, 0 failed, finished in 5.76s. Includes `invariants::chrome::every_panel_marks_focus_with_an_edge_style_change`, the two control-case tests, and the three recorded-baseline equalities |
| `cargo test --test tui_render` (whole family) | **pass** — 1217 passed, 0 failed, 0 filtered out, finished in 7.23s |
| `check_tui_render_invariant_baseline.py` | **pass** — 61 advertised-key, 3023 contrast, 1 hint-truncation; every one reproduces, none added unbanked |
| `check_tui_render_invariant_baseline.py --self-test` | **pass** — 12 cases |
| `check_tui_render_cost.py` | **pass** — unmodified |
| `check_comment_budget.py` | **fail** — 20 violations. **None of the six files this milestone edited is in the per-file list**, and `src/tui/panels/chrome.rs` is new and unlisted. One named file is `src/tui/components/section.rs` (M003's, untracked). Three violations are **corpus-wide**: 136,725 comment lines against the ceiling of 134,015, 1,214,792 comment words against 1,188,348, and ratio 0.1352 against 0.1344 — and `chrome.rs` is part of that corpus. |

**One transient red, and what it proves about the bank.** At 04:22 the
`invariants` member failed on the I5 equality, reporting 330 recorded contrast
rows that no longer reproduced (0 unrecorded) across 15 dialog surfaces. The tree
at that moment carried an in-flight edit to
`src/tui/components/dialogs/info.rs` from another session, which was then
reverted; the same command is green at 04:32. Those 330 rows were dialog pairs,
not the 78 hosted-title rows, so they never bore on this milestone's bank — but
the episode is worth recording because it is the only direct observation in this
review that the ratchet detects an unbanked reduction and names the surfaces.

**The tree moved during this review, so every result above is timestamped.** The
`invariants` runs were taken at 04:22 (red, for the transient reason above) and
04:32 (green); the whole-family run used a binary built at 04:26:10. Immediately
afterwards other sessions changed `src/tui/commands/editor.rs`,
`src/tui/components/section.rs`, `src/tui/components/dialogs/agent.rs`,
`src/main.rs` and `crates/codegg-core/src/session/request_ledger.rs`. A later run
may report something different for a reason that is not this milestone. No file
this milestone edited is among them. Two further probes also failed to build, on
another session's transient `E0599` and then `E0433` in `src/tui/**`; the M001
record of this milestone pair carries the details, and the same caveat applies
here — no measurement after 04:32 was obtainable in this session.

### Not run

| Command | Why |
|---|---|
| `scripts/verify.sh quick` | Not run. It is 84 guards plus a workspace check over a tree several sessions are editing, and `check_comment_budget.py` alone is red for files this milestone did not touch. |
| `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings` | Not run; sibling sessions are editing `src/tui/**`, so a failure in a file this milestone did not write would not be evidence about it. |
| The plan's §11 panel-specific members (`tui_agents_panel`, `tui_chat_surfaces`, `tui_info_dialog_panel_chrome`, `tui_panel_manager_reachability`) individually | **Superseded, not skipped**: the whole family was run (1217/1217), which includes every one of them. Running them one at a time would produce the same binary and the same result. |

## 5. Invariant review

- **No panel loses content.** Chat and Terminal are byte-identical by digest and
  by cell-for-cell comparison against the legacy block. For the other panels the
  mechanical guarantee is weaker: I3b requires the focus difference to be on an
  edge cell, so a body change could not satisfy it, but a body change *alongside*
  an edge change would pass. See §10.
- **No panel loses its title or its focus marker.** `chrome_title` always emits
  the label, and the marker when focused; the I3b sweep asserts an edge cell
  changes style on every panel at every placement and size.
- **No panel gains a key.** `git diff -- src/tui/panels/mod.rs` shows only
  `pub mod chrome` and the `render_hosted` signature change; the panel manager,
  the directional matcher and the docking gestures are outside the diff.
- **I1 and I2 stay green.** Asserted in the same run.
- **The render-cost shape guard is unchanged.** Passes, no edit.
- **The comment ratchet.** No file this milestone edited appears in the 20
  per-file violations, and the new `chrome.rs` is under the unlisted floor.
  `section.rs` does appear, and that file is M003's. The **corpus-wide** ceilings
  are exceeded (136,725 lines against 134,015), and this milestone's new module
  is part of that corpus, so the plan's §4 invariant "the comment ratchet holds"
  is not satisfied at this tree state even though this milestone did not break a
  file budget. See §10 and §11.

## 6. Failure and recovery review

Not applicable in the runtime sense: rendering is synchronous and stateless with
respect to this change, there is no persistence, no async work and no
cancellation path.

The one failure mode the plan names — a paint that reads a theme field it should
not have — is caught by the corpus digests for Chat/Terminal and by the edge-style
requirement for the rest.

The §14 stop condition **did** materialise, and this record is where it is
reported rather than worked around: on 4 of the 50 bundled themes the resolved
`theme.primary` and `theme.border` are the same colour, and on one of them
(`Solarized Dark`) the resolved `theme.muted` is the same colour too, so a
focused and an unfocused panel differ **only** by the `●` glyph — exactly the
state I3b exists to eliminate. See §10. No fallback colour was invented, as §14
requires; the token question belongs to M007.

The I3b sweep itself runs on `Theme::dark()` only, so it does not observe any of
this. That matches the plan — §10 asks for four sizes and two focus states, not
fifty themes — but it means the sweep proves uniformity, not universal
uniformity.

## 7. Migration and compatibility review

No compatibility surface. Panel placement, docking, focus and key ownership are
unchanged, so a user's gestures behave identically. The visible change is
confined to border and title styling plus Help's new outer border.

Rollback is deleting `chrome.rs` and restoring the six inline blocks; the digests
in `chrome.rs` are the rollback check.

## 8. Security review

Not applicable: no key, command, scope, path, process or secret surface changes,
and no authority is created or narrowed. The one indirectly security-relevant
property — that a panel's focus indication cannot be forged by a title string,
because the title is now produced by the primitive from `focused` — is improved,
not weakened.

## 9. Documentation and operations

- `architecture/tui.md` — the panel section gains the focus policy and the
  chrome predicate as a contract, and names `Section` as the dialog counterpart.
- `src/tui/panels/chrome.rs` module header — the policy table, why `BOLD` is
  excluded, why `ChromeStyles::resolve` is public, and why the predicate is
  shared.
- `docs/tui-render-invariant-baseline.toml` — the 78-row growth, its reason and
  its owner (M007), in the file's own header.
- No new guard, as the plan specifies: I3b in the M001 harness is the
  enforcement.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | **On 4 of the 50 bundled themes the resolved `theme.primary` equals `theme.border`, and on `Solarized Dark` it also equals `theme.muted`.** Resolved from the theme files and the documented converters (`accent_primary ← general.unread_indicator`/`highlight_indicator`, `border ← buffer.border_selected` first, `muted ← text.tertiary` in `src/theme/halloy.rs`; `Theme { primary ← ui.accent_primary, border ← ui.border, muted ← text.muted }` in `src/theme/target.rs`): `Cyberpunk` `#00FFD9`, `Nord` `#81A1C1`, `Solarized Dark` `#93A1A1`, `VESPER` `#FFC799` have accent == border, and `Solarized Dark` has accent == border == tertiary == `#93A1A1`. | On those four themes the border channel carries no focus information. On `Solarized Dark` the title channel carries none either, so a focused panel differs from an unfocused one **only** by the `●` glyph — the precise defect I3b exists to remove, surviving in a shipped theme. The I3b sweep cannot see it: it runs on `Theme::dark()` only. This is the plan's §14 stop condition, which requires reporting rather than a fallback colour. | **Reported here, owned by M007** per §14 and the roadmap's contrast ratchet, which is the milestone that owns token questions. M007 must decide whether `border`/`muted` are re-derived for these themes or whether the focus policy gains a second signal; this milestone must not invent one. Separately, extend the I3b sweep to all bundled themes (or to these four as named cases) so the defect is a red test rather than a review finding — and note that the claim above is resolved from the theme sources, not from a render, so the first step is to confirm it with a rendered check. |
| low | The global comment-corpus ceiling was exceeded: 136,725 comment lines against the recorded 134,015, with the word and ratio ceilings over as well. This milestone's new `chrome.rs` (237 lines, heavily commented by design) was part of that corpus. | The plan's §4 invariant "the comment ratchet holds" was not satisfied at that tree state. | **Discharged 2026-11-13 by the re-bank arm**: `docs/comment-budget.toml` records `max_comment_lines = 139245` and `check_comment_budget.py` passes — `scripts/verify.sh full` passed with it. |
| medium | **`dock` bound the two slot borrows positionally while `two_slots_mut` returned them in index order.** `PanelPlacement::index()` is `Right = 0, Left = 1, Bottom = 2`. `two_slots_mut(slots, a, b)` returns `(lowest index, highest index)`; `dock` destructured it as `(from_slot, to_slot)` from the *argument* `(origin, placement)`. When the destination's index was **lower** than the origin's — Left→Right, Bottom→Right, Bottom→Left — `from_slot` was the destination and `to_slot` the origin, so `from_slot.release_to(to_slot)` moved whatever was on the destination edge into the origin slot and then selected an edge that never received the panel. | A user moving a panel to an earlier edge got the wrong panel moved and the wrong edge selected. No test covered it: the I3b sweep visits placements in ascending index order only, so it never took the broken branch. | **FIXED.** `dock` now selects the borrows by index (`src/tui/panels/mod.rs:1370-1381`), and the regression test `a_move_to_an_earlier_edge_moves_the_panel_the_gesture_named` (`:2170`) docks Bottom→Right and Left→Right and asserts the destination holds the moved panel, the origin restores its own previous panel, and the selected edge is the receiving one. It passes in the 2026-11-13 `full`. |
| low | §15's **per-panel corpus diff note was never produced**. §12 asks for "the milestone's corpus diff note, listing each panel and the exact cells that changed", and §15 asks for "the per-panel corpus diffs, Chat and Terminal marked byte-identical". No such artifact exists in the tree, and the pre-adoption frames are no longer renderable, so it cannot now be produced. | The control case is proven mechanically (better than a note). For the other 23 panels the property "the diff is chrome, not content" is asserted by I3b but not *recorded* per panel, and "no content row moved" is not asserted at all for the 18 hosted panels — a body shift accompanied by a correct edge change would pass. | **OPEN OWNER QUESTION — not decided by this record.** The plan's §5 already declares the per-panel corpus diff note out of scope because it "need[s] a pre-change frame that no longer exists", and requires the substitute to be accepted in writing. So: is the mechanical substitute acceptable — the whole `tui_render` family run green, `dialog_dispatch_coverage` unmodified and green, and a hosted content-row assertion — in place of the per-panel corpus diff note? What would settle it: an owner yes/no recorded against this record; a yes converts this finding into a recorded substitution, a no leaves it as missing evidence the tree cannot produce. |
| low | The 78-row breakdown "13 hosted surfaces × 6 themes" is the manifest's own record. The **total** is verifiable (3023 − 2945 = 78, and the harness equality passes), but the per-row attribution is not independently re-derivable: the pre-M002 manifest is not in git. | A reviewer can confirm the bank is exactly what is painted, and can confirm the count, but cannot confirm from the tree that every one of the 78 came from the title change rather than from a different paint change in the same diff. | Land milestones as separate commits, or record the 13 surface names in the manifest header alongside the count. |
| low | Help's readability at 20×6 with an outer border (§13 criterion 6, §14 stop condition) is not directly asserted. | I2 proves Help renders without panicking and paints something at 20×6, which is not the same as "fully readable". | Add a 20×6 assertion on Help's content rows, or record the measured result of a manual render. |

Two medium findings: the theme collision, which the plan's §14 anticipated and
assigned to M007 and which is still open, and the docking defect, which is now
fixed and regression-tested (§10). The earlier note that "neither is fixed here"
applies to the theme collision only.

## 11. Roadmap disposition

**Milestone conditionally closed.** The primitive, its six adoptions, I3b and the
byte-identity control are complete and verified against the production panel
path. Conditions:

- **Condition 1 — evidence: half answered, half red.** The per-panel corpus diff
  note cannot exist (its pre-change frame is gone), and **decision 4 accepts the
  substitute in writing** — see the owner decision below. The other half is no
  longer an open question but a **measured defect**: Help's 20×6 readability is
  now asserted, and it **fails**. `HelpDialog::render_into` lays out
  `Length(3) + Length(3) + Min(1) + Length(3)`, so the box needs ten interior
  rows before a single content row paints; at a 20×6 host rect the interior is
  18×4 and the layout collapses to borders.
  `the_minimum_hosted_size_keeps_a_readable_content_row` in `help.rs` renders the
  hosted path (`PanelChrome` + `render_into`) and asserts at least one interior
  row carries three or more letters. It paints none. **This record therefore
  stays `conditionally closed` on that one condition**, and the fix is a
  degradation policy for the fixed three-row chrome at small heights — a design
  choice, not a mechanical repair.
- **Condition 2 — gates: met 2026-11-13.** `scripts/verify.sh full` passed on
  this tree (17,377 workspace tests, 8,441 feature-pass tests, 0 failures;
  `cargo fmt --all --check` clean; clippy `-D warnings` clean), and the comment
  corpus is banked at 139,245 lines, so the 20 violations this record reported
  are gone. `run_full()` calls `run_quick()`, which calls `run_guards()`, where
  the formatting check lives.
- **Condition 3 — the recorded growth is a decision with an owner.** The 78
  banked contrast rows are not a failure and not a silent success: they were a
  reviewed increase, recorded with its reason and assigned to M007. M007 has
  since driven `[contrast]` to **0** across the 50 bundled themes by the mapping
  (`plans/implementation/tui-modularity-and-render-invariants/011-halloy-projection-mapping.md`),
  with the bundled theme files byte-identical to `HEAD`, so the growth this
  condition banks is retired rather than still owed.
- **Condition 4 — a reported, owned presentation defect.** On `Solarized Dark`
  (and, for the border channel alone, `Cyberpunk`, `Nord` and `VESPER`) the focus
  policy resolves to colours that carry no focus information, so focus is
  glyph-only on that theme. §14 requires reporting it rather than patching it,
  which is done in §10; M007 owns the token question, and the sweep should be
  extended to the bundled themes so it becomes a red test.
- **Risk.** Low on rendering. The docking defect is fixed and has a regression
  test; the residual risk is the reported theme collision, which is owned by
  M007 rather than by this milestone.
- **Exact future evidence.** The `full` transcript of 2026-11-13, and — for the
  open question — the owner's acceptance of the corpus-diff-note substitute,
  each labelled with the commit that carries them.

M003 may proceed: `Section` already reuses `ChromeStyles::resolve`,
`chrome_title`, `has_chrome` and `FOCUS_MARKER`, which is the interface
dependency the plan fixed here.

## 12. Registry updates

- Mark roadmap M002 **conditionally closed** and link this record.
- Record the bank: contrast 2945 → 3023, +78 hosted-title rows, reason and owner
  (M007) in `docs/tui-render-invariant-baseline.toml`.
- Add the docking defect from §10 to the roadmap's unresolved list with the
  required action, so it is not lost between this milestone and M008's
  frame-geometry work.
- Record the theme collision from §10 against **M007** as an open I3/I5 item:
  `Solarized Dark` makes focus glyph-only, and `Cyberpunk`, `Nord`, `Solarized
  Dark` and `VESPER` make the border channel carry nothing. Extending the I3b
  sweep to the bundled theme set is the regression guard.
- Record that §15's per-panel corpus diff evidence was replaced by the I3b sweep
  and the eight control digests, and that the replacement is weaker for the 18
  hosted panels.

## Owner decision (2026-11-14)

**Decision 4 of the seven, taken as A — and C's assertion turned the other half
red.**

- **A: the corpus diff-note substitute is accepted in writing.** The pre-adoption
  frames no longer exist, so the per-panel note cannot be produced; the family
  run plus `dialog_dispatch_coverage` plus a hosted content-row assertion is what
  replaces it. That acceptance is recorded here and in
  `tui-modularity/003`.
- **C: the hosted content-row assertion now exists** —
  `the_minimum_hosted_size_keeps_a_readable_content_row` — and it **fails**. The
  substitute was accepted on the understanding that the panel reads at its
  minimum size; the assertion says it does not. `render_into` reserves
  `3 + 3 + Min(1) + 3` rows, so at an 18×4 interior nothing but borders paints.

This is the substitute doing its job: the note it replaced would have recorded
which cells moved, and the assertion instead found a surface that paints nothing
at the size the panel system permits. The record does **not** close on a
substitute whose third leg is red.

## Correction log (2026-11-13)

- **§1, "One defect was found … deliberately not fixed".** The `dock` borrow-pair defect is fixed: `dock` now chooses the pair by index (`src/tui/panels/mod.rs:1370-1381`) and `a_move_to_an_earlier_edge_moves_the_panel_the_gesture_named` (`:2170`) asserts Bottom→Right and Left→Right. The test passes in the 2026-11-13 `full`.
- **§1, the four-reasons sentence.** Was "the docking defect means the panel manager is not in a state a closure record should call fully sound". Met by the fix above plus its regression test; the sentence now says two conditions remain, not four.
- **§1 / §11 Condition 2, the gate sentence.** Was "`quick` cannot be green today: `check_comment_budget.py` reports 20 violations". Met by: `scripts/verify.sh full`, 2026-11-13 — 17,377 + 8,441 tests, 0 failures; `run_full()` → `run_quick()` → `run_guards()`, where `cargo fmt --check` lives. Comment corpus banked at 139,245.
- **§10, the comment-corpus row.** Met by the re-bank at `max_comment_lines = 139245`; `check_comment_budget.py` passes.
- **§10, the `dock` row.** Was a medium finding with a required fix. Met by the fix and the regression test; the row now records both.
- **§10, the per-panel corpus diff note row.** The required action ("Produce the note, or add a per-panel pre/post content-row assertion") is now an **open owner question**, because the plan itself puts the note out of scope for want of a pre-change frame and requires the substitute to be accepted in writing. The question as recorded: is the mechanical substitute — the family run, `dialog_dispatch_coverage`, and a hosted content-row assertion — acceptable in place of the per-panel corpus diff note?
- **§10, closing note.** "Neither is fixed here" is true of the theme collision only; the docking defect is fixed.
- **§11 Conditions 2–4 and Risk.** The gate condition is met; M007 has since driven `[contrast]` to 0 via the M011 mapping with `assets/` byte-identical to `HEAD`, so Condition 3's owed growth is retired; the risk bullet no longer carries the docking defect.

**No promotion.** The status stays `conditionally closed`: the §14 theme collision is still a reported, M007-owned defect, Help's 20×6 readability is still unasserted, and the corpus-diff-note substitute is an undecided owner question. No condition was deleted without naming what met it.
- **Owner decision, 2026-11-14** — the corpus diff-note substitute accepted in writing (decision 4, A); its content-row assertion added and **failing** for Help at 20×6, which is now this record's only remaining condition.
