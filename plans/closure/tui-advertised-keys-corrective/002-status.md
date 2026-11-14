# TUI Advertised Keys Corrective C002 — Closure Status

Status: conditionally closed

Source implementation plan: `plans/implementation/tui-advertised-keys-corrective/002-structured-pairs-from-the-producer.md`
Workstream: `plans/subsystems/tui-advertised-keys-corrective-addendum.md`
Repository baseline: `7642b9e38` on `feat-auto-20261109-f8abad0d` (worktree uncommitted)
Record written: 2026-11-13

**Status rationale.** Work packages A, B, D and F are implemented and green.
Work package **C stopped at §14.1** at this pass — converting a footer changes
rendered bytes — and the owner then authorised the visible change, which is the
authority §14.1 asked for; **35 drifted rows were converted**. Work package
**E still stops at §14.2** by measurement: the parser widening records twelve
rows that are fixture gaps, not surface defects. The milestone's structural goal
— the measurement consumes the producer's own `(key, verb)` items — is met.

## 1. Executive finding

`HintLine` already held the pairs; the **trait boundary discarded them**.
`Component::keymap_hint() -> Option<String>` rendered the items away, and
`HintItem` was private. That is now inverted:

- `HintItem` is public with `key()`, `verb()` and `is_required()`; `HintLine::items()`
  yields items **in painted order** (required first), and `render` consumes
  `items()` instead of duplicating the ordering;
- `keymap_hint_line() -> Option<HintLine>` is the trait's declaration;
  `keymap_hint()` is a **provided** method that renders it. All **14** overrides
  converted — not only the 8 surfaces the I4 baseline measures — so every surface
  in the tree feeds the structured set, which closes the asymmetry where a
  surface could declare nothing structured yet still paint a hint;
- `advertised_hints() -> Vec<HintLine>` gives a surface several rows where it
  paints several.

The harness now presses **declared** keys first (credited `KeySource::Declared`),
then painted-only keys (`KeySource::Painted`), and keeps the rendered-row scan
**verbatim** as the completeness cross-check. A declaration the parser cannot read
is reported (`AdvertisedReport::unreadable`) rather than skipped.

**The recorded set did not move.** `[advertised_keys]` is byte-identical — the
same four rows, with the same reasons.

## 2. Requirement-to-evidence

| Requirement | Evidence | Result |
|---|---|---|
| WP A: expose the items | `hint_line.rs` public `HintItem` + `items()`; `render` consumes it | **pass** |
| WP B: the trait seam | `component.rs` `keymap_hint_line()`; `keymap_hint()` provided; 14 overrides converted, each losing exactly `.render(0)`; no call site changed | **pass** |
| WP C: the 18 footer sites | **converted** on the owner's authority — 35 drifted rows across ~40 files; the double-spaced pipe dialect survives in one dialog file (`plan.rs`); `RECORDED` byte-identical | **pass, under the owner's authorisation** |
| WP D: the harness reads the structure | declared-then-painted press order; painted scan retained; `source` column and `UNREADABLE` rows in the per-key report | **pass** |
| WP E: the parser's blind spot | widening implemented, measured, **reverted** — it records 12 new rows; the hole is now a named, asserted property | **blocked, §14.2** |
| WP F: non-vacuity | `the_i4_cross_check_names_a_pair_painted_outside_the_declaration`; `a_surface_that_advertises_only_in_its_footer_is_still_measured`; `a_declared_spelling_the_parser_cannot_read_is_reported`; `the_i4_measurement_takes_every_declared_key_from_the_declaration` | **pass** |
| The recorded set is unchanged | `sed`/`diff` of the `[[advertised_keys]]` block before and after: empty | **pass** |
| `[contrast]` and `[hint_truncation]` untouched by this milestone | guard reports them unchanged | **pass** |

## 3. WP C — the footers were converted, on the owner's authority

At this record's pass, 16 of the 18 enumerated sites rendered `|`-separated,
double-spaced, Title-Cased rows — `Agent`'s was
`"↑/↓ navigate  |  Enter select  |  Esc close"` — and converting them through
`HintLine::render(0)` rewrites that to
`↑/↓ navigate · Enter select · Esc close`, i.e. **it changes rendered bytes**,
which the plan names as stop condition §14.1. Only `WorkspaceDashboard` already
matched the dialect, and it could not convert either: its declaration names
`type` and `Ctrl+R`, neither of which `key_of` reads, so WP E's criterion would
fire.

**The stop was then removed by the owner, not by this record.** The owner
authorised the visible change (the directive recorded in
`plans/closure/tui-modularity-and-render-invariants/005-status.md`: "we need to
unify this so everything is talking the same language … in either case, we should
seek to unify this so it is done in one standard way"). **35 drifted rows were
converted across ~40 files**, with contextual differences preserved in the items
rather than in the spelling: a docked panel naming `Shift+← back` while its modal
names `Esc` stays, because the host genuinely pops the stack. Measured on the
current tree, the double-spaced pipe hint dialect survives in **one** dialog file
(`src/tui/components/dialogs/plan.rs`) rather than 24, and `RECORDED` stayed
byte-identical across all 28 entries — no declared key was re-spelled and no
declared verb changed. §14.1 was a stop about *authority*, and authority was
given; the conversion is recorded in M005's §1 and its residue in M005's §11.

The two measurements that sharpened the problem still stand as the historical
record of why the stop was taken:

- the real site count was **26 advertisement literals across ≥20 paint sites**
  (`Connect` 4, `Session` 4, `PanelManager` 4, `Model` 2, plus one to four each
  for the rest), not 18;
- `src/tui/components/dialogs/session.rs:1330` painted
  `"d delete  a archive  f fork  s sort  h toggle archived  m search messages"` —
  **no separator at all**, so `footer_parts` could not see ten advertised pairs.
  That row is now a `HintLine::from_items` declaration (`session.rs:1330`), so the
  hole is closed by the conversion rather than left to the scan.

## 4. WP E — the twelve rows

Widening `key_of` to read `Home`, `End`, `PageUp`/`PgUp`, `PageDown`/`PgDn`,
`Backspace`, `⌫`, `[` and `]` makes those keys pressable, and the presses are
inert where the fixture has no state to act on. Measured, then reverted:

| Surface | Newly recorded |
|---|---|
| `Diff` | `Home`, `End`, `PgUp`, `PgDn` |
| `SecurityReview` | `PgUp`, `PgDn`, `[`, `]` |
| `TaskView` | `Home`, `End` |
| `ProjectChat` | `[`, `]` |
| `Help`, `Model`, `Question`, `Queue`, `Permission` | one each |

Banking them would trade one honest constraint ("the recorded set must not move")
for twelve rows that exist because the fixtures lack list ends, an answer, a
second channel and a hunk. That is fixture work, not parser work.

**The strongest single argument for the structural read came out of this
package:** `Permission`'s footer, truncated at 100 columns, renders `Esc c` — the
`l` of `cancel` is gone. A parser that reads every leading token therefore
**invents a key that does not exist**. A structured declaration cannot do that.

## 5. Verification executed

| Command | Result |
|---|---|
| `cargo test --test tui_render -- invariants::baseline` | **13 passed, 0 failed** |
| `cargo test --test tui_render -- invariants::vocabulary` | **3 passed**; `RECORDED` table byte-identical, floor 32 |
| `cargo test --test tui_render` | **1234 passed, 0 failed** |
| `cargo test -p codegg --lib -- tui::components::hint_line` | **9 passed** |
| `cargo test -p codegg --lib -- tui::components` | **762 passed, 0 failed** |
| `cargo test -p codegg --lib -- tui::panels` | **51 passed, 0 failed** |
| `python3 scripts/check_tui_render_invariant_baseline.py` | `4 advertised-key, 1448 contrast and 0 hint-truncation entries recorded; every one reproduces and none was added unbanked` |
| `… --self-test` | **15 cases passed** |
| `cargo clippy -p codegg --lib --tests -- -D warnings` | clean |
| `cargo fmt --all --check` | clean |
| `scripts/verify.sh full` | **passed** — 17,353 workspace tests, 8,420 feature-pass tests, 0 failures |

Per-key report: **225 presses — 103 `declared`, 122 `painted`, 12 `UNREADABLE`,
4 `INERT`**. The four `INERT` are exactly the four recorded rows.

## 6. Invariant review

- **The recorded set is the same four rows**, byte-identical; a fifth appearing
  would mean a surface started advertising something it does not do.
- **The painted scan is retained**, so the structural read added a source of
  evidence rather than replacing one. `a_surface_that_advertises_only_in_its_footer_is_still_measured`
  is what proves it.
- **ADR-0042 ownership untouched**; no key's owner changed.
- **`[contrast]` and `[hint_truncation]` are not this milestone's** and did not move
  for it.
- The two key-authority guards and the catalog/render-cost guards pass.

## 7. Migration and compatibility

`keymap_hint()` keeps its signature and is now provided, so any out-of-tree
implementation of `Component` that overrode it still compiles. `HintItem` becoming
public is additive. No protocol, storage or DTO surface.

## 8. Security review

None. No path, process, credential or authority surface is touched. The one
security-adjacent finding is the opposite direction: `SecurityReview`'s `H` writes
the system clipboard, which is why it stays unmeasured.

## 9. Documentation

`architecture/tui.md` gains the two-layer description: the declaration is the
advertisement, the rendered row is one rendering of it, and the measured set is
the residue. It also records the `Permission` truncation that invents `Esc c`,
and the I4 section states both durable rules (both advertisement sources are
authoritative; the measurement must be able to press the key).

## 10. Unresolved findings

| Severity | Finding | Required action |
|---|---|---|
| medium | **WP C was blocked** at this pass: 16 of 18 footer sites change rendered bytes if converted (§14.1), and the real count was 26 literals / ≥20 sites. | A milestone had to decide whether a footer may switch `|` → `·` visibly. | **RESOLVED by the owner's authorisation**: 35 drifted rows were converted across ~40 files with context preserved in the items; measured on this tree the double-spaced pipe dialect survives in one dialog file (`plan.rs`). The fixture work for WP E remains below. |
| medium | **`session.rs:1330` advertised ten pairs with no separator**, so the completeness scan could not see them at all. | The completeness scan's blind spot on that row. | **RESOLVED**: `session.rs:1330` is now a `HintLine::from_items` declaration, so the row is part of the structured set the measurement reads. |
| medium | **WP E blocked**: the parser widening records 12 rows, all fixture gaps rather than surface defects. | Fixture work: list ends for `Diff`/`TaskView`, an answer for `Question`, a second channel for `ProjectChat`, a hunk for `SecurityReview`. |
| low | `Section::hint` stays `&str`; no converted Section-edge caller exists, so `hint_line(HintLine)` would be an unexercised API that forces `Section` to drop `Copy`. | Add it with the first real caller. |
| low | The tree is uncommitted. | Commit; the records' statuses assume an uncommitted worktree. |

## 11. Roadmap disposition

**Conditionally closed.** The structural goal is met and the recorded set is
unchanged. WP C is converted under the owner's authorisation (§3), so what
remains is WP E's fixture work — twelve rows that exist because the fixtures lack
list ends, an answer, a second channel and a hunk — plus a commit. Closing this
outright requires that fixture milestone, which the table above describes.

## 12. Registry updates

- `plans/registry.md` — the advertised-keys workstream row: C002 →
  `conditionally closed` with this record.
- `plans/subsystems/tui-advertised-keys-corrective-addendum.md` — status reflects
  C001 and C002 both `conditionally closed`.
- TUI roadmap §12 row `006` — its corrective now has both records; C and E are
  the named remaining work.

## Correction log (2026-11-13)

- **§1, the status-rationale paragraph.** Said "Work packages **C and E stopped at this plan's own stop conditions**". Met by: the owner authorised the visible footer change — the authority §14.1 asked for — and **35 drifted rows were converted across ~40 files** (recorded in `plans/closure/tui-modularity-and-render-invariants/005-status.md` §1 and §11). WP E still stops at §14.2 by measurement.
- **§2, the WP C row.** Was "not converted / **blocked, §14.1**". Met by the authorisation and the conversion; the row now records 35 converted rows, `RECORDED` byte-identical, and one remaining dialog file carrying the double-spaced pipe dialect (`plan.rs`).
- **§3, the section itself.** Was "WP C — why the footers were not converted". Rewritten as "WP C — the footers were converted, on the owner's authority", keeping the historical measurement (16 of 18 sites, 26 literals across ≥20 paint sites) and naming what changed.
- **§10, the WP C medium finding.** Was "**WP C blocked**". Resolved by the owner's authorisation and the conversion; the fixture half (WP E) is named as what remains.
- **§10, the `session.rs:1330` medium finding.** Was "advertises ten pairs with no separator". Resolved: that row is now a `HintLine::from_items` declaration (`session.rs:1330`).
- **§11, the disposition.** Was "C and E are deferred with their stop conditions measured". Now: C is converted under the owner's authorisation, and the remaining work is WP E's fixture rows plus a commit.

**No promotion.** The status stays `conditionally closed`: WP E's twelve rows are fixture work that has not been done, `Section::hint` remains `&str` with no converted caller, and the tree is uncommitted. No condition was deleted without naming what met it.
