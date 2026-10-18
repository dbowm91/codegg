# TUI Panel Surface Convergence Corrective Milestone M003 — Closure record

Status: **closed**

Milestone: make a docked box's colours a function of the theme in force **at
render time**, and replace the hardcoded selected-row foreground with a derived
theme token. Nothing requires a new key from a Halloy theme author.

Plan: `plans/implementation/tui-panel-surface-convergence-corrective/004-theme-sourcing-and-shared-chrome-geometry.md`
Roadmap: `plans/subsystems/tui-panel-surface-convergence-corrective-addendum.md`

Baseline: `f8286a2849660d1e386a61ceaf663906451961d0`
(`docs(plans): record the tui-panel M001 commit SHA`), plus M002a and M002b
landed in the working tree.

M002a, M002b and M003 were committed together as a single commit, because they
interleave in the same files and were only ever verified as one unit. That
commit was then fast-forwarded onto `origin/main` by rebase — twice, because
`main` was being actively developed while this milestone was verified. The final
base is **`e8a322161`**, and that rebase needed no conflict resolution (the first
one did, in `plans/registry.md`). Every verification result below was re-taken on
the **rebased** tree; see §4.1 and §6, which record the re-verification and a
material change to the pre-existing-failure attribution.

---

## 1. Read this first: the plan's §3.2 was wrong, and the milestone is smaller than it claimed

The plan recorded **"ten stale, four clean"** and named `mcp` as "the single
worst instance of the defect". A render-level test says otherwise, and the
correction runs the opposite way from what the source count suggested.

The §3.2 table counted `self.theme.` occurrences. That measures *how much code
reads a cache*, not *whether the cache is stale*. Two of the highest counts are
correct code.

| Widget | plan said | measured | why |
|---|---|---|---|
| `mcp` | stale, 44 reads, "worst instance" | **not stale** | `McpDialog::render` and `render_in_panel` both began `self.theme = Arc::clone(theme)` — a cache the render overwrites on the frame it reads |
| `research` | stale, 46 reads | **not stale** | `ResearchBrowserDialog::render_body` did the same, over its own `theme` parameter |
| `keybind` | "mixed", 21 reads | **live path already clean** | all 21 reads are in the dead `impl Widget for &KeybindDialog` |
| `tree` | "mixed", 11 reads | **live path already clean** | all 11 in the dead `impl Widget for &TreeDialog` |
| `plan` | "mixed", 7 reads | **live path already clean** | 10 in the uncalled third `render`, 7 in the dead `impl Widget` |
| `theme` | "mixed", 4 reads | **live path already clean** | 5 in the dead `impl Widget`; the 2 remaining `self.theme` reads are `self.theme.id`, which is semantic (which theme is active), not colour |
| `security_review` | stale, 40 reads | **stale — confirmed** | no re-push anywhere |
| `review` | stale, 17 reads | **stale — confirmed** | `render_body` took `_theme` and ignored it |
| `plugin` | stale, 6 reads | **stale — confirmed** | no re-push anywhere |
| `diff` | stale via sub-widget | **stale — confirmed** | `DiffViewer` caches, refreshed only by open-time `set_theme` |

Also wrong: the plan says `review` "carries a second sub-widget with the same
problem". It does not — `ReviewDialog`'s diff view builds its colours from the
passed theme (`review.rs`, `similar::TextDiff::from_lines` inline).

**The genuinely stale set is four, not ten:** `review`, `plugin`,
`security_review`, `diff`/`DiffViewer`.

This is why `tests/tui_theme_source_of_truth.rs` exists. It renders each box
with theme A, then renders the **same widget** with theme B, and asserts that no
interior foreground is a colour A defined. At the pre-change baseline it fails
for exactly those four and passes for the other six — including `mcp` and
`research`, whose source counts led the chart.

W2 converted **five** dialogs anyway. `mcp` and `research` are correct today, but
a cache that a render method overwrites is a hazard rather than a design: any
future reader outside the render pass gets whatever the last frame left there.
The plan's acceptance criterion ("no `set_theme` colour cache remains in any
render path") also covers them.

Two further plan counts corrected against source:

- **"32 `ThemeData` literals" is 31.** `grep -c '^    ThemeData {' src/tui/theme.rs`
  → 31, and 31 `THEMES[n]` accessors. No work changed — the tokens are derived
  in `to_theme()` rather than authored per literal — but the number is recorded
  so the next reader does not re-derive it.
- **`centered_rect`'s two copies were not "byte-identical"** (one qualified
  `Direction`, the third parameter was `area` in one and `r` in the other), but
  they *are* semantically identical, so de-duplication was still correct.

## 2. What landed

### 2.1 Two derived selection-foreground tokens

A selected row is a filled surface. The colour painted on it must read against
**that fill** — a different question from "what is this theme's body text".
`UiColors` gains two fields, both **derived, never authored**:

- `selection_text` — foreground to draw on `ui.selection`
- `accent_text` — foreground to draw on `ui.accent_primary`

`derive_text_on(body_foreground, body_background, surface)`
(`src/theme/schema.rs`) picks whichever of the theme's own two body colours
contrasts more with the surface. When neither clears `MIN_DERIVED_TEXT_RATIO`
(3.0) it falls back to whichever of black or white contrasts **more**.

That last detail is load-bearing. The first implementation chose by
`surface.is_dark()`, which leaves a surface at relative luminance 0.3 with white
at 2.1:1 — below the bar it was written to enforce. Taking the maximum of the
two is a guarantee (≥ 4.5:1) rather than a guess, and
`the_derivation_picks_whichever_body_colour_reads_on_surface` walks all 256 grey
levels to hold it.

Callers:

- `parse_halloy_theme` (`halloy.rs`) — beside the existing
  `derive_selection_dim`, for the same reason
- `native::resolve` (`native.rs`) — after `base` and `ui` resolve, so a theme
  that overrides `selection` derives against **its own** value rather than
  inheriting a token derived from the fallback's. Accepts optional
  `ui.selection_text` / `ui.accent_text` keys as author overrides; a malformed
  value is dropped, not fatal, because the derivation is already usable.
- `ThemeData::to_theme` (`src/tui/theme.rs`) — for all 31 static palettes.
  Derived rather than authored per literal: it is a property of the palette, and
  31 hand-picked tuples is 31 chances to leave one theme's selected rows
  unreadable — the same defect as the hardcoded white this replaced.

`Theme::selection_style()` and `Theme::highlight_style()` now return
`fg(selection_text).bg(selection)`. They previously returned `fg(primary)`, which
painted the accent as a foreground over a *different* fill, making a selected
row's text the same colour as the accents around it.

### 2.2 Why derived rather than read from Halloy

Halloy declares `buttons.primary.foreground_selected`. Sourcing from it is the
obvious implementation and it is wrong. Measured across all 50 bundled themes:

- `background_selected` — **50/50**
- `selection` — **50/50**
- `foreground_selected` — **0/50**

A read of that slot returns `None` for every real theme and every theme
authored against Halloy upstream: code that looks correct in review and is empty
in the product, and a new *required* key is exactly what the milestone's hard
constraint forbids. Deriving from `text.primary` / `general.background`, which
the format does guarantee, asks no theme author for anything — that is what keeps
"any Halloy theme ingests unchanged" true.

`no_bundled_theme_defines_foreground_selected` is the regression guard. It exists
because the "simplification" is tempting, local, and compiles: swapping the
derivation for the declared slot removes code and breaks no test that does not
happen to be this one. The guard's failure message says exactly that.

### 2.3 Five dockable dialogs and one sub-widget stopped caching

Each shared body now reads the `&Arc<Theme>` the `Component` seam already hands
it. Field, `set_theme`, and constructor theme parameter deleted from each.

| File | Cache reads converted | Notes |
|---|---|---|
| `dialogs/review.rs` | 17 | `render_body` already took `_theme` and ignored it |
| `dialogs/security_review.rs` | 40 | param added to `render_body`/`render_list`/`render_detail` |
| `dialogs/plugin.rs` | 6 | param added to `render_body` |
| `dialogs/mcp.rs` | 46 | both per-frame re-pushes removed; param added to `render_body`, `render_server_list`, `render_server_details` |
| `dialogs/research.rs` | 46 | the re-push *inside* `render_body` removed |
| `components/diff.rs` (`DiffViewer`) | 17 | `theme` is now a parameter of `render`/`render_in` and all four helpers |

No `Arc::clone` of the borrowed theme remains in any of them; there is no renamed
cache.

Because three constructors lost their only argument, clippy's
`new_without_default` fires — `Default` impls were added for `ReviewDialog`,
`SecurityReviewDialog` and `ResearchBrowserDialog`, matching the one `McpDialog`
already had.

**`Widget` cannot carry a theme**, so a widget that must stay a `Widget` has no
way to reach the live one. Three impls became inherent methods, rendering
byte-identically:

- `impl Widget for &DiffViewer` → `DiffViewer::render_in(&self, area, buf, theme)`
- `impl Widget for &PlanDialog` → `PlanDialog::render_legacy(&self, area, buf, theme)`
- `impl Widget for &DiffDialog` → `DiffDialog::render_legacy(&self, area, buf, theme)`

`DiffDialog` and `PlanDialog` lost their `theme` field entirely, because after
the conversion nothing live read it. `PlanDialog`'s remaining two live paths —
`render_body` and the uncalled third `render` — already read the seam's theme.

Deleting the dead legacy paths is **not** this milestone; it stays with the
separate cleanup. Two of them (`connect`, `share`) have test-only callers, so
deletion is not a uniform cut.

### 2.4 Three modal-only dialogs found late, and converted

The plan scoped W2 to dockable boxes. A final audit of `self.theme.` across every
dialog and component — attributing each read to its enclosing `impl`/`fn` rather
than counting the file — found **three more instances of the identical
pattern** that the plan's ten-box set never mentioned, because none of them is a
dockable box:

| File | Live cache reads | Entry point |
|---|---|---|
| `dialogs/run_detail.rs` | **42** (seven tab helpers) | `Component::render` took `_theme` and ignored it |
| `dialogs/source_preview.rs` | 9 | `Component::render` took `_theme` and ignored it |
| `dialogs/ui_node.rs` | 5 | `Component::render` took `_theme` and ignored it |

`run_detail` alone held more live cache reads than any box in the plan's set,
across all seven of its tabs.

They were converted too. The argument for converting is not that they were
observably stale — a modal is constructed from the live theme when it opens, and
the seven sites that reassign `UiState::theme` cannot fire while a modal owns the
keyboard — but that each one is a render body receiving the theme and discarding
it for a copy it can hold stale, which is the exact shape this milestone exists
to remove. Shipping a closure record that said "three more instances of the
defect remain" while claiming the milestone removed it would be worse than
slightly exceeding the stated scope.

`ui_node`'s `test_set_theme` asserted the cache by pointer identity
(`Arc::ptr_eq(&dialog.theme, &theme2)`). There is nothing left to assert, so it
became `the_dialog_holds_no_theme_of_its_own` — the compile-time half of the
contract, since a reintroduced field would fail the constructor. This is the one
test whose intent had to be rewritten rather than preserved.

### 2.5 What the audit found and deliberately left

The same sweep found ~160 further `self.theme.` reads in `editor.rs` (16),
`sidebar.rs` (81), `messages.rs` (49), `help_overlay.rs` (6), `file_tree.rs` (3)
and `change_review.rs` (2). **None is stale, and none is a `Component`.** The
app's render path re-pushes the live theme into them on every frame —
`src/tui/app/render.rs:1213` (`messages`), `:1976` (`prompt`), `:2096`
(`status_bar`), `src/tui/panels/session.rs:42` (`session`) — and `sidebar`,
`file_tree` and `editor` are constructed per frame with `Arc::clone(&theme)`.

That is the "self-correcting" shape, the same one `mcp` and `research` used. It
is correct today, and it is not the `Component` seam this milestone owns.
Converting them is a different, larger unit — they are host-owned surfaces whose
theme ownership is the render path's, not a parameter — and it is recorded here
as a surveyed result rather than silently left.

### 2.6 The hardcoded selected-row foregrounds

Ten `Color::White` style literals across four files, all now tokens:

| File | Sites | Replacement |
|---|---|---|
| `dialogs/plan.rs` | 6 | `theme.accent_text` over `theme.primary` |
| `dialogs/mcp.rs` | 2 | `theme.selection_text` (over `selection`) and `theme.accent_text` (over `primary`) |
| `dialogs/confirm.rs` | 1 | `theme.accent_text` — the selected option, drawn on the accent fill |
| `dialogs/run_detail.rs` | 1 | `theme.foreground` — see note |

Two corrections to the plan's classification:

- **`run_detail.rs:136` was not a selection row.** It is `cell.summary` — plain
  body text with no fill behind it, pinned to white. On a light theme it rendered
  white-on-white. The right token is the theme's own `foreground`, not a
  selection foreground, and saying otherwise would have put a fill-derived colour
  on unfilled text.
- **`plan.rs` has six, not the split the plan implied.** Two are in the live
  `render_body`; four are in dead paths (ten in the uncalled `render`, two in the
  legacy `impl Widget`). All six were converted, since the guard covers the file
  rather than the call graph.

`Color::White` remains in exactly one place under `dialogs/`: `theme.rs:314`,
`Color::White => "ffffff"`, which is a **theme-name mapping** in the picker's
colour table, not a style. The guard allowlists that file with that reason.

Two adjacent cases were looked at and left alone, recorded so the next reader
knows they were not missed:

- `src/tui/components/toast.rs:266` — `fg(Color::White)` on a toast message over
  the toast's own fill. Same readability class, not a selection row, not a dialog.
- `dialogs/run_detail.rs:620,630` — `fg(Color::Yellow)` for the active tab and the
  tab-row highlight. A hardcoded colour that ignores the theme, but not a
  selection foreground, so outside the milestone's stated defect. A follow-up
  should decide whether an accent token serves there.

### 2.7 `centered_rect`

One definition, now `crate::tui::components::component::centered_rect`, used by
`src/tui/app/render.rs` (3 sites) and `dialogs/run_detail.rs` (1). The two
duplicates in `app/mod.rs` and `dialogs/run_detail.rs` are deleted.

It lives in `component.rs` because that is the one module both `app` and the
dialogs already depend on; a dialog reaching into `app::centered_rect` would
invert the dependency, which is how the duplication started.

### 2.8 Guards and docs

- `scripts/check_no_hardcoded_selection_colours.py`, wired into
  `scripts/verify.sh quick`. Scoped to `src/tui/components/dialogs/`, forbidding
  `.fg(Color::White)` and `.fg(Color::Black)`, ignoring `#[cfg(test)]` modules and
  allowlisting `theme.rs` for the name-mapping reason. **Verified to fail**: a
  probe file with two live literals exits 1 with both reported and the test
  fixture ignored; removing it returns exit 0.
  Its own docstring states the limitation plainly — it catches the literal, not
  the bug it causes; a token read from the wrong surface passes it. The
  behavioural half is `tests/tui_theme_source_of_truth.rs`.
- `docs/themes.md` — "Selected-row text is derived, not configured", stating that
  you need to add nothing to a Halloy theme.
- `architecture/theme.md` — token list, two new invariants, and a dated
  verification entry (including the 0-of-50 measurement and the sweep margins).
- `architecture/tui.md` — the live-theme rule, next to the `Chrome` seam, and the
  source-count-vs-rendered-buffer distinction.
- `.opencode/skills/tui/SKILL.md` — the selection-token rule and the
  read-don't-cache rule.

## 3. Tests

**`tests/theme_selection_tokens.rs`** — 9 tests, all passing:

| Test | Asserts |
|---|---|
| `every_bundled_theme_derives_a_legible_selection_foreground` | the sweep: both tokens ≥ 3.0:1 on their own fill, across all 50 bundled themes |
| `no_bundled_theme_defines_foreground_selected` | 0-of-50, so the derivation is not "simplified" into a dead read |
| `every_bundled_theme_still_ingests_with_no_new_required_key` | all 50 parse and derive their id from their file stem — the hard constraint, in parse form |
| `the_derivation_picks_whichever_body_colour_reads_on_the_surface` | the four cases that separate the rule from a guess, incl. all 256 greys |
| `a_native_theme_may_pin_the_tokens_but_never_has_to` | omitted → derived; pinned → wins |
| `a_malformed_selection_override_falls_back_to_the_derivation` | a bad optional key is dropped, not fatal |
| `every_static_palette_derives_a_legible_selection_foreground` | all 31 static palettes |
| `selection_style_paints_the_derived_foreground_not_the_accent` | the style helper, not just the field |
| `the_tokens_actually_vary_by_theme` | the sweep is not reading one constant |

Measured margins, printed by the sweep: worst `selection_text` **3.36:1**
(`macaw-dark`), worst `accent_text` **3.07:1** (`broc`). Both above the 3.0 bar
— thin, and the reason 3.0 rather than 4.5 was chosen.

**`tests/tui_theme_source_of_truth.rs`** — 9 tests, all passing. Baseline before
any production change: **4 failed, 5 passed.**

| Test | Baseline | Now |
|---|---|---|
| `review_follows_a_theme_change` | **FAIL** (3 stale colours) | pass |
| `plugin_follows_a_theme_change` | **FAIL** (2) | pass |
| `security_review_follows_a_theme_change` | **FAIL** (3) | pass |
| `diff_follows_a_theme_change` | **FAIL** (5) | pass |
| `mcp_follows_a_theme_change` | pass (self-correcting) | pass |
| `research_follows_a_theme_change` | pass (self-correcting) | pass |
| `the_boxes_that_already_read_the_seam_theme_pass_this_check` | pass | pass |
| `this_check_can_detect_a_widget_that_reads_a_stale_theme` | pass | pass |
| `the_theme_picker_shows_each_candidate_palette_rather_than_the_active_one` | pass | pass |

The seventh test is the anti-vacuity control: a widget painted with the old
theme's primary must be *caught*, so four green boxes prove something. The
picker test is the deliberate exception — `/theme` previews candidate palettes,
so its column paints colours that are not the active theme's, and a blanket leak
check would have flagged a feature rather than a defect.

## 4. Verification actually run

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | **PASS** |
| `cargo clippy --workspace --all-targets -- -D warnings` | **PASS** (0 errors, 0 warnings) |
| `scripts/verify.sh quick` | **PASS** — "Quick verification passed", including `check_no_hardcoded_selection_colours.py` |
| `cargo test --test theme_selection_tokens` | **PASS** — 9/9 |
| `cargo test --test tui_theme_source_of_truth` | **PASS** — 9/9 |
| `cargo test --test tui_usability_fixes --test tui_panel_host_chrome --test tui_info_dialog_panel_chrome --test change_review_trajectory --test security_review_receipt --test security_review_panel` | **PASS** |
| `cargo test -p codegg --lib dialogs::` | **PASS** — 292 passed, 0 failed (run by the W2a worker) |
| `cargo nextest run --workspace --locked --profile ci --no-fail-fast` | see §6 |

`--no-fail-fast` is used deliberately: `verify.sh full` stops at the first
failure, and the two known pre-existing failures (`session_create_through_daemon`,
`InvalidRootPath("/tmp")`) would otherwise hide everything behind them. The
full sweep result is recorded in §6 rather than claimed here.

### 4.1 Re-verification after the rebase onto `origin/main`

The pass above was taken on the pre-rebase tree. After each rebase every check
was **re-run**, because a clean merge is not a verified merge — and `main` moved
twice while this milestone was verified. The results below are for the final
base, `e8a322161`:

| Command | Result on the rebased tree |
|---|---|
| `cargo fmt --all -- --check` | **PASS** |
| `cargo clippy --workspace --all-targets -- -D warnings` | **PASS** (0 errors, 0 warnings) |
| `scripts/verify.sh quick` | **PASS** — "Quick verification passed", including `check_no_hardcoded_selection_colours.py` |
| `cargo nextest run -p codegg --test theme_selection_tokens --test tui_theme_source_of_truth --test tui_panel_host_chrome --test tui_info_dialog_panel_chrome --test tui_usability_fixes` | **PASS** — 82 run / 82 passed |
| `cargo nextest run --workspace --locked --profile ci --no-fail-fast` | see §6 |

The rebase itself produced exactly one conflict, in `plans/registry.md`, where
upstream had updated two neighbouring roadmap rows and added a third. It was
resolved by taking this milestone's updated panel row and upstream's neighbours
verbatim. The three Rust files that both sides touched
(`src/tui/app/mod.rs`, `src/tui/app/render.rs`,
`src/tui/runtime/command_dispatch.rs`) auto-merged, and the rebase's diff to
them is **stat-identical** to upstream's own diff to the same files
(35 / 43 / 2 lines, 73 insertions, 7 deletions) — so neither side was disturbed.

Resolving that conflict also surfaced a **pre-existing factual error in the
registry**: the blocked-milestone row still read *"**Blocked (deliberately
unapproved):** … No plan exists."* for M003 after the milestone had been
implemented and closed. Corrected as part of this commit.

## 5. Not done, and deliberately

- **The dead legacy `impl Widget` paths were not deleted.** Two of the ten
  (`connect`, `share`) have test-only callers in
  `widget_and_component_render_paths_are_equivalent` and the connect
  restoration tests, so deletion needs those tests rewritten first. Still a
  separate cleanup milestone.
- **The shared chrome/geometry helper was not built.** No seam demands it and no
  test fails without it, and building it now would run against the
  `Chrome::{Owned,Hosted}` contract M002b just established. Recorded as a
  recommended standalone milestone, not silently dropped.
- **`toast.rs:266` and `run_detail.rs`'s `Color::Yellow` tabs** left unchanged
  (§2.6) — same readability class, neither a selection row.
- **The ~160 `self.theme.` reads in `editor`/`sidebar`/`messages`/`help_overlay`/
  `file_tree`/`change_review`** left as they are (§2.5). They are not `Component`s
  and are not stale — the render path re-pushes the theme every frame — but they
  are the same shape, and converting them is a separate unit.
- **M002a + M002b + M003 land as one commit, not three.** They interleave in the
  same files and were verified as one unit, so hunk-splitting them would have
  produced intermediate states that were never built or tested.

## 6. Full sweep, and why the failure count is not this milestone's

Verified on the pushed base — `origin/main` `e8a322161`, *not* the `f8286a284`
this milestone was written against.

`cargo nextest run --workspace --locked --profile ci --no-fail-fast`:
**14331 run / 14260 passed / 71 failed / 6 skipped** (339 s).

### 6.1 `main` is already red, and all 71 failures are its

This is a genuine change from the 11 failures an earlier draft of this record
reported. That number was taken before the first rebase. `origin/main` picked up
`0c04f79b9` (`fix(agent,tool): make escalation and execution authority
evidence-based` — the W001 fail-closed workspace-root work) plus five more
commits, and its test call sites were never updated with it.

Every failure was attributed by **running it**, not by reading the diff:

| Group | Method | Result |
|---|---|---|
| 69 — `lsp`, `tool_execution`, `git_execution_origin_matrix`, `command_routing_execution_ownership`, `session_family`, `workspace_isolation`, `tool_surface_minimization`, `tool_program_git_lsp_palette`, `identity_live_execution_audit`, `identity_m002_live_audit_hooks`, `bash_background_m002_routes` | the same 11 targets run at **both** `origin/main` `e8a322161` and this commit | **69 failed at `main`, 69 failed here, and the two sorted failing-name sets are identical** — the set difference in this commit's direction is empty |
| 2 — `core::daemon::tests::session_create_through_daemon`, `turn_submit_rejects_when_active` | run individually at `origin/main` `e8a322161` | **2 run / 0 passed / 2 failed** |

69 + 2 = 71, with nothing left over. Supporting fact: `src/tool/`, `tests/lsp.rs`,
`tests/bash_background_m002_routes.rs`, `tests/identity_m002_live_audit_hooks.rs`
and `src/tui/commands/interactive_terminal.rs` are all **byte-identical** between
the two commits — no code path that produces these failures was touched here.

### 6.2 What actually broke them

W001 made an absent or relative workspace root a typed refusal — deliberately —
and did not update its own test call sites. The failures are that intended
behaviour arriving at tests that still construct a rootless `BashTool`:

```
MissingWorkspaceRoot { context: "managed shell dispatch has no canonical workdir" }
Execution("workspace validation failed: workspace root '.' must be an absolute, non-symlink path")
```

and, in `lsp`, an error-variant expectation the fail-closed path no longer meets:

```
assertion failed: matches!(err, ToolError::Execution(ref m) if m.contains("action_index"))
```

The single TUI name,
`tui::commands::interactive_terminal::tests::pressing_i_on_the_open_box_enters_focused_input`,
was on M001's recorded pre-existing list (`001-status.md:239` (FAIL|FAIL),
`002:356`, `003:307`, `file-picker-rework/001-status.md:211`) and failed at the
earlier head too. **It now passes** — upstream `0ed5073aa` (`C002`) landed on
`main` during this work and fixed it, which is itself corroboration that the
failure was `main`'s and not this milestone's.

### 6.3 What this milestone contributed

**+18 run, +18 passed, +0 failed** — exactly the new `theme_selection_tokens` (9)
and `tui_theme_source_of_truth` (9) tests. The five new/affected targets report
**82 run / 82 passed** on the pushed base.

**The thing this record must not bury:** `origin/main` is red, before this
milestone lands. Repairing those 71 is separate work against upstream's W001
call sites and is **not** M003's to absorb. That is why M003's acceptance is
stated as "adds zero failures", not as "the suite is green".
