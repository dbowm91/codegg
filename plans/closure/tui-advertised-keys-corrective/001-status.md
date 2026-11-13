# TUI Advertised Keys Corrective C001 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/tui-advertised-keys-corrective/001-i4-baseline-to-its-irreducible-residue.md`

Source subsystem roadmap:

- `plans/subsystems/tui-advertised-keys-corrective-addendum.md` (C001), whose parent is
  `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md` §2 (I4) and §12 row 006
- corrected milestone: `plans/closure/tui-modularity-and-render-invariants/006-status.md`
  (`corrective pass required`)

Repository baseline reviewed: `7642b9e381d9` (`feat-auto-20261109-f8abad0d` HEAD) plus
**uncommitted working-tree work** — 254 changed or untracked paths at the time of review
(measured, `git status --porcelain | wc -l`).

Implementation commits or pull requests:

- **None.** This milestone exists only as uncommitted working-tree work on
  `feat-auto-20261109-f8abad0d`, on top of HEAD `7642b9e381d9`. No commit, branch or pull
  request carries it, and the same tree carries C002 and the M006–M010 TUI milestones, so no
  statement below is attributable to a SHA.
- **The I4 measurement itself is untracked.** `docs/tui-render-invariant-baseline.toml`,
  `scripts/check_tui_render_invariant_baseline.py` and the whole `tests/tui_render/invariants/`
  tree report `??` in `git status`: they do not exist at HEAD. That has one consequence this
  record must carry everywhere it matters — **the 61-row before-state is not in git and cannot
  be re-derived from the tree**; it exists only as the plan's §3.1 measurement and the
  manifest's own header. Every "before" number below is therefore labelled.

Provenance labels: **measured** means this record's author ran the command or read the value
out of the tree at the state above; **parent-reported** means the assignment supplied the
figure and this record did not repeat the measurement; **read** means source reading only.

## 1. Executive finding

The measurement was made state-fresh and inspectable, and the `[advertised_keys]` section was
driven from **61 rows to 4**, each carrying a `reason` the guard now requires.

**The measurement is now a reading rather than a count.** `measure_advertised_keys`
(`tests/tui_render/invariants/baseline.rs:602`) mounts **one fresh fixture per advertised key**
(`mount`, `:560`), seeds content from an I4-only table (`fixtures::dialogs_with_content()`,
`fixtures.rs:899`, whose only readers are `baseline.rs:605`, `:906` and `:1176` — measured by
grep), and applies a declared position prelude only for an up-direction key on a surface
`PARKED_SURFACES` names (`fixtures.rs:508`, one step, `PARKED_ROW: usize = 1` at `:495`).
`the_i4_per_key_report_names_every_announced_key_and_its_verdict` (`baseline.rs:894`) prints one
line per press. Measured today: **225 presses, and exactly four `INERT` lines** —

```text
I4 per-key report — 225 presses:
  Model                Tab      INERT
  SecurityReview       H        INERT
  SecurityReview       h        INERT
  WorkspaceDashboard   Tab      INERT
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1228 filtered out
```

The four `INERT` lines are the four recorded rows. Nothing else in the announcement set is
inert, which is the property the section is supposed to state.

**The residue is four rows, not the two the plan predicted, and the two extra are a genuine
find.** The plan's stop condition (§14) expected exactly the two `Tab` rows and an ADR-0042
amendment as the only way to zero. The implementation additionally retained
`SecurityReview`/`H` and `SecurityReview`/`h`. I read both arms before accepting the reasons:

- `H` (`security_review.rs:339-363`) copies the selected hunk's text to the system clipboard via
  `crate::util::clipboard::copy_to_clipboard`, and there is **no test hook**: `clipboard.rs:4`
  is the real body and `:12` is the `#[cfg(not(feature = "arboard"))]` no-op, so a default-
  feature test run performs the host write. A render measurement must not mutate the operator's
  pasteboard, and the effect is not a frame the invariant can read.
- `h` (`security_review.rs:334-338`) calls `jump_to_hunk()`, which early-returns while
  `item.hunk.is_none()` (`:148-154`). The fixture therefore carries findings with
  `hunks: Vec::new()` (`fixtures.rs:755-797`), and that is not a shortcut: any receipt that gives
  `h` a destination simultaneously arms `H` for the same press set, so `h` cannot be measured
  without the clipboard write.

Both reasons are true of the code, and I agree they cannot be measured safely under the current
contract. They are a second instance of the same class as the `Tab` rows: the verb is real, the
*measurement* is what cannot be allowed to perform it. That is a decision above this milestone
(whether an invariant may write the clipboard), exactly as ADR-0042 is for `Tab`.

**Four stale spellings, one non-existent edge, one invisible failure and one wording claim were
repaired at the surface that advertised them rather than in a handler.** `Diff` and
`SecurityReview` now spell the page verb `PgUp/PgDn` in their `HintLine`, and their footer
literals are **byte-identical to HEAD** (measured: the strings at `diff.rs:195`, `:335`, `:443`
and `security_review.rs:513` do not appear in `git diff HEAD`); `PanelManager`'s footer is
`⇧H/J/L move` and the module doc plus the handler comment name `PanelPlacement` as the reason;
`Connect`'s `SelectProvider` branch paints the `error_message` it already stored; `info.rs` and
`dialog_dispatch_coverage.inc` were reworded to "five **verbs**".

**The guard now requires the reason.** `SECTION_FIELDS["advertised_keys"]` is
`("surface", "key", "reason")` (`scripts/check_tui_render_invariant_baseline.py:78-82`), and the
self-test has two new cases — a missing reason and a blank reason are both rule-1 violations
(`:582-590`). Measured today: the guard prints **`4 advertised-key, 1520 contrast and 0
hint-truncation entries recorded; every one reproduces and none was added unbanked`**, exit 0,
and `--self-test` prints **`self-test: 14 cases passed`** (the plan measured 12 before this
milestone; two were added).

Status is `conditionally closed`, not `closed`, for four reasons, only one of which is a gap in
the milestone's own code:

1. **`architecture/tui.md` is now updated.** Work package G and plan §12 require the I4 rule,
   the retained rows and the rule that a hint and a footer on one surface must not advertise
   different keys for one verb. At this record's pass `architecture/tui.md` contained no such
   section; it now does, at `:4516-4575`: "**I4: an advertised key is handled, and the
   measurement means something**" states the declaration-versus-painted-row rule, "Both
   advertisement sources are authoritative" states the hint-versus-footer rule, and the four
   retained rows are listed with their reasons. This in-scope documentation item is
   **delivered**.
2. **The plan's literal numbers no longer describe the tree, for reasons this milestone does not
   own.** Criterion 1 says "exactly two rows" (the tree has four, correctly reasoned), criterion
   2 says contrast is "still 2495" (the tree records 1520, re-banked by M003's 528-row
   reduction, M007's class-(a) 92, the theme-picker re-bank and an ADR-0062 re-bank, per the
   manifest's own headers), and criterion 3 predicts the guard will print "2 advertised-key". The
   amended M006 criterion in the addendum — "the recorded residue, each entry carrying a reason" —
   is met; the plan's arithmetic is stale.
3. **The gate class is now discharged.** `scripts/verify.sh quick`, `full` and
   `cargo fmt --check` were not run by this review; they have since run tree-wide —
   `scripts/verify.sh full` passed on this tree at 2026-11-13 (17,377 workspace tests, 8,441
   feature-pass tests, 0 failures; `cargo fmt --all --check` clean; clippy `-D warnings` clean;
   render corpus 1237 passed / 0 failed). Criterion 10 is met.
4. **The 61-row before-state is not recoverable.** The manifest is untracked and there is no
   commit, so criterion 11's "row-by-row account" can only be a class-level account (§4). The
   plan's classification sums exactly to 61 (`4 + 4 + 46 + 2 + 5`, plan §3.2), which is the
   strongest available check that no row is unexplained; it is not the same as naming each row.

None of the four is a redesign, and none hides a broken surface: the four retained rows are
measured inert for stated reasons, and every other announced key changes the frame.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| WP A: a fresh fixture per advertised key | `baseline.rs:602-630`; `mount` at `:560`; the comment at `:611-615` names the defect the change removes | pass | Measured by running the report: 225 independent presses |
| WP A: a content-bearing fixture table used by I4 only | `fixtures::dialogs_with_content()` (`fixtures.rs:899`); grep shows its only readers are the three I4 sites in `baseline.rs` | pass | `dialogs()` remains the table the contrast measurement reads. **No committed before-state**, so "unchanged" is a structural claim, not a diff |
| WP A: a per-key report a reviewer can read | `the_i4_per_key_report_names_every_announced_key_and_its_verdict` (`baseline.rs:894`); measured 225 presses and four `INERT` lines | pass | The test also asserts its inert lines equal the measurement's inert set (`:921-925`), so the report cannot drift from the record |
| WP A: a scroll-position fixture so a boundary key can act | `PARKED_SURFACES` (`fixtures.rs:508`), `PARKED_ROW = 1` (`:495`), applied by `mount(..., park)` only for `is_up_direction` (`baseline.rs:544-572`) | pass | One step, not a middle row, because a short list (Permission, Plan) must keep both directions reachable — the constant's own doc says so |
| WP B: four stale `^d/^u` spellings corrected | `diff.rs:31-48`; `security_review.rs:39-57` | pass | Criterion 7 |
| Criterion 7: both painted footers byte-identical | `git diff HEAD` on the two files changes no footer literal (`diff.rs:195/335/443`, `security_review.rs:513`) | pass | The only `CONTROL` lines added are the doc comment explaining that no `CONTROL` arm exists |
| WP C: `PanelManager` stops naming `⇧K` | footer `panel_manager.rs:369-370`; module doc `:12`; handler comment `:224-231` | pass | Criterion 8 |
| Criterion 8: `Shift+K` still does nothing and a test says so | `an_upward_directional_moves_nothing` (`panel_manager.rs:589`); footer test asserts `⇧H/J/L move` (`:448`) | pass (read) | The module filter was not re-run; the tests are present and unmodified in meaning |
| WP D: `Connect` paints `error_message` in `SelectProvider` | paint at `connect.rs:925-938`; test `enter_with_no_provider_paints_why_it_did_nothing` (`:1909`) asserts the string is absent before the press and present after | pass (read) | No new message text; the stored string is painted |
| WP E: the section is the reasoned residue | manifest `docs/tui-render-invariant-baseline.toml:166-184` (4 rows, each with `reason`); `[totals]` at `:153-158` | **partial** | Four rows, not two (criterion 1's literal text). Two extra rows are justified in §1 |
| Criterion 2: `[totals].advertised_keys` equals the row count | 4 = 4 (guard's rule 2) | pass | |
| Criterion 2: contrast is still 2495 | the tree records **1520**, not 2495 | **not met as written** | M003/M007 re-banks, per the manifest headers; no C001 contrast account exists, and no before-state exists to prove C001 added none |
| WP E: the guard requires a `reason` | `SECTION_FIELDS` (`check_tui_render_invariant_baseline.py:78-82`); self-test cases at `:582-590` | pass | Measured: guard exit 0, `--self-test` 14/14 |
| Criterion 3: guard prints "2 advertised-key …" and `--self-test` passes | prints **4** advertised-key; `--self-test: 14 cases passed` | pass in substance / **literal count stale** | The count is the measurement's; the plan's "2" was its prediction |
| Criterion 4: `cargo test --test tui_render -- invariants` is 0 failed, with a per-key report | not run as a module; the report member was run alone: 1 passed, 1228 filtered out | **partial** | The parent reports the corpus 1229 passed / 0 failed; the module-wide figure was not re-measured here |
| Criterion 5: vocabulary 3 passed, ≥ 32 surfaces | the test asserts `dialogs.len() + panels.len() >= 32` (`vocabulary.rs:144-150`) | not re-run | Parent reports the corpus green; the floor is in the test, not relaxed |
| Criterion 6: no `CONTROL` arm, no keymap/`EDITOR_KEYMAP`/catalog change | no `CONTROL` arm added (diff grep); composer guard passed; editor guard printed `2 guard-block chord(s), 92 Normal-mode chord(s) [9 digits claimed by a count arm: 123456789], 8 Insert-mode chord(s), no shadowed arm`; catalog guard `clean` | pass (measured) | All three guards were run here |
| Criterion 9: `Connect`'s paint proved by a test | same test as WP D | pass (read) | |
| Criterion 10: `fmt`, `clippy -D warnings`, `quick`, `full` green | `scripts/verify.sh full`, 2026-11-13: 17,377 workspace tests, 8,441 feature-pass tests, 0 failures; `cargo fmt --all --check` clean; clippy `-D warnings` clean; render corpus 1237/0 | **pass, 2026-11-13** | Uncommitted tree; §10 |
| Criterion 11: every removed entry named with its cause | §4's class account, which sums to the plan's 61 | **partial** | The 46 empty-fixture rows are named per surface by count, not per key; the before-state is untracked |
| WP F: the wording finding | `info.rs:172-179`; `dialog_dispatch_coverage.inc:1-14` | pass | The `.inc` is tracked and modified, so its before-text is readable at HEAD |
| WP G: `architecture/tui.md` documents the I4 rule and the hint-versus-footer rule | `architecture/tui.md:4516-4575`: the I4 rule, "Both advertisement sources are authoritative" (the hint-versus-footer rule), and the four retained rows with their reasons | pass | Delivered at that section; §10 |

## 3. Production implementation evidence

- **The harness.** `tests/tui_render/invariants/baseline.rs:544-630` — `is_up_direction`,
  `mount`, `announced_keys` and `measure_advertised_keys`. The announcement set is computed once
  per surface (hint plus rendered footer, `:579-600`), then each key is pressed against its own
  mount; the position prelude is a function of the key alone, never of press order, so no verdict
  depends on another press.
- **The content table.** `fixtures.rs:876-930` — `CONTENT_MAKERS` seeds fourteen surfaces and
  `dialogs_with_content()` overlays them on `dialogs()`. Its doc states the reason it is separate:
  I1, I2, I3a and I5 keep reading the empty table the contrast measurement was banked against.
  `seeded_security_review` (`:755-797`) is where the no-hunk decision is written down.
- **The report.** `baseline.rs:879-926` — one line per press, with the inert lines asserted equal
  to the measurement's recorded set in both directions.
- **The four repairs.**
  - `diff.rs:31-48` — the page verb is `PgUp/PgDn`, and the doc comment states why adding
    `CONTROL` arms instead would have been a new binding on a box whose footer already named the
    working keys.
  - `security_review.rs:39-57` — the same repair, plus the pre-existing reasons `Esc` and `H`
    are absent from the hinted set.
  - `panel_manager.rs:224-231` — `PanelPlacement::named_by(PanelDirection::Up)` returns `None`,
    so `⇧K` yields nothing on every press; the footer now names the edges that exist and the
    test asserts the no-op.
  - `connect.rs:925-938` — the `SelectProvider` footer now appends `Error: …` when the dialog
    has one. This is one render path and one stored string; no message was invented.
- **The manifest.** `docs/tui-render-invariant-baseline.toml:71-126` states the residue, both
  reasons, the five historical classes and the two measurement defects (a bordered box gluing
  its edge to the first advertised part, and a rendered content row posing as an advertisement).
  The header is the long-form record; this record does not restate it.
- **The guard.** `scripts/check_tui_render_invariant_baseline.py:78-88` adds `reason` to the
  required fields while keeping `(surface, key)` as the identity for the duplicate check (a
  reason is justification, not identity), and `:582-590` adds the two self-test cases.

**What did not change.** No `CONTROL` arm, keymap table, command catalog or binding was added;
both painted footers are byte-identical; no ADR was amended; no contrast row is attributed to
this milestone.

## 4. Verification executed

### Commands run by this review

```bash
python3 scripts/check_tui_render_invariant_baseline.py
python3 scripts/check_tui_render_invariant_baseline.py --self-test
CARGO_BUILD_JOBS=2 cargo test --locked --test tui_render -- the_i4_per_key_report --nocapture
python3 scripts/check_tui_composer_key_authority.py
python3 scripts/check_tui_editor_key_authority.py
python3 scripts/check_command_catalog_ownership.py
python3 scripts/check_tui_render_cost.py
git status --porcelain | wc -l
git show HEAD:<path> / git diff HEAD -- <path>     # before-states, read-only
grep -rn 'dialogs_with_content|CONTROL|PARKED_' tests/ src/
```

### Results

| Command | Result |
|---|---|
| `check_tui_render_invariant_baseline.py` | **pass** — `4 advertised-key, 1520 contrast and 0 hint-truncation entries recorded; every one reproduces and none was added unbanked`, exit 0 |
| `… --self-test` | **pass** — 14 cases, including "an advertisement without a reason is rule 1" and "…with a blank reason is rule 1" |
| per-key report | **pass** — `1 passed; 0 failed; 1228 filtered out`, 225 presses, four `INERT` lines quoted in §1 |
| composer key-authority guard | **pass** |
| editor key-authority guard | **pass** — the plan's exact expected line, reproduced |
| command catalog ownership | **pass** — `clean` |
| render-cost guard | **pass** — "the render path derives each result once" |

### Not run

| Command | Why |
|---|---|
| `cargo test --test tui_render -- invariants` / the whole corpus | Another lane is editing `tests/tui_render/invariants/**` and `src/tui/components/**`; this session was restricted to narrow, read-only runs. The corpus figure (1229 passed / 0 failed) is **parent-reported** |
| `cargo test --lib -- tui::components::dialogs::{connect,panel_manager,diff}` | Not run; the tests are present and were read. The plan's expected counts (15/18/5) are a prediction, not evidence this record has |
| `scripts/verify.sh quick` / `full`, `cargo fmt --check` | Forbidden by this session's instructions; criterion 10 is therefore unevidenced here |
| `check_comment_budget.py` | The tree-wide budget is red by a parent-owned margin; running it would report a state this milestone does not own and must not fix. No source comment was added by this review |

**A note on the two `Tab` rows.** `FocusManager::handle_key` intercepts `Tab` before the dialog
(`src/tui/components/component/focus.rs:164-177`) and `handle_tab` (`:179-193`) only rotates a
focus index; neither dialog declares a focus ring (`focusable_count` is the default), so the
press is a total no-op and `ModelDialog`'s own `Tab` arm (`model.rs:1029-1034`, "Tab configure")
and `WorkspaceDashboard`'s (`workspace_dashboard.rs:183`, "Tab tasks") are unreachable. A
focus-ring implementation would not deliver either advertised verb — `handle_tab` can only call
`set_focused` — so the two rows need the ADR decision, exactly as recorded.

## 5. Invariant review

- **The render corpus stays green.** Not re-measured here; parent-reported 1229 passed / 0
  failed on this tree. The one member this review ran is green.
- **`[contrast]` is untouched by this milestone.** The guard reproduces all 1,520 rows in both
  directions and passes. The count is not the plan's 2,495 because M003, M007 and an ADR-0062
  re-bank moved it; the manifest's headers document those reductions and contain no C001 contrast
  account. Holds as far as an untracked manifest can be checked; the before-state gap is §10.
- **One hint vocabulary, floor at 32.** The floor is asserted in
  `vocabulary.rs:144-150`, not lowered; the two repaired hints are `HintLine`s like every other.
  The vocabulary test was not re-run (parent-reported green).
- **ADR-0042 ownership unchanged.** Both key-authority guards pass with the expected figures;
  no `CONTROL` arm, keymap or catalog change exists in the diff.
- **No surface loses a working verb.** The removals are two stale hint spellings on two surfaces
  (`PgUp/PgDn` remain, and are what the handler answers), an edge that does not exist (`⇧K`), and
  an error message that had no paint (now painted). No working verb was deleted.
- **The baseline may only shrink, and every removal is matched by the change that caused it.**
  The guard's rule 2 fails in both directions, so an unbanked reduction is still an error. What
  the tree cannot show is the *identity* of each of the 61 old rows (§4's class account).
- **The comment ratchet.** Not verified. This review added no source comment anywhere.

## 6. Failure and recovery review

No runtime path: the measurement is synchronous and per-surface, and every production edit is
dialog-local. The applicable hazards are the measurement's own; the plan named the first two.

- **Two-directional equality.** Removing a row the harness still measures fails the recorded-set
  test rather than passing silently; the guard's rule 2 catches a count/entry mismatch. That is
  why the residue can only shrink by a change that changes a frame.
- **A fixture can manufacture entries.** The content table and the parked-position prelude are
  declared in one module each (`fixtures.rs`), so a reviewer reads content and position without
  chasing either; a surface absent from `PARKED_SURFACES` is measured from the first row, where
  a user arrives.
- **A fixture can also remove an entry a user can still hit at a boundary.** For the 57 rows that
  disappeared, the manifest's header names what changed — content, a fresh mount, decoration
  trimming, a row-separator rule — rather than asserting that the count fell. For the two extra
  retained rows, §1 states the reason. That is the record's answer to plan §8's requirement.
- **Failure visibility.** A wrong advertisement fails the recorded-set test or the per-key report
  with the surface and spelling in the message; the non-vacuity fixture
  (`the_i4_measurement_detects_an_advertised_key_that_does_nothing`, `baseline.rs:984`) still
  advertises a key it ignores.

## 7. Migration and compatibility review

No storage, protocol, configuration or dependency change. The user-visible surface: `Diff` and
`SecurityReview` stop naming `^d`/`^u` and name the `PgUp`/`PgDn` keys they already implement;
`PanelManager` stops naming `⇧K`; `Connect` shows why `Enter` did nothing. No key changes owner,
no binding is added, and no command exists that did not.

The manifest's contract changes in one direction: an `[advertised_keys]` row now requires a
`reason`, so the justification lives inside the file rather than only in this record. A later
row added without one is a guard failure, not an exemption.

## 8. Security review

Not applicable in the authorization sense: no path, process, secret, network or permission
surface is touched, and the I4 fixtures hold no credential or run record. Two bounded properties
are worth naming:

- **The clipboard.** The residue exists partly because a render measurement must not write the
  operator's pasteboard. The alternative — a fixture with hunk context — would make every test
  run mutate the host, and with the default `arboard` feature there is no injection point. A
  future decision to stub the clipboard would be a decision about the invariant's authority, not
  a test cleanup.
- **Bounded input.** The measurement presses only spellings its own parser reads (`key_of`,
  `baseline.rs:341`), and the guard rejects a recorded spelling the parser cannot read, so a
  surface cannot bank a key the invariant never presses.

## 9. Documentation and operations

- `docs/tui-render-invariant-baseline.toml:71-126` — the residue, both reasons, the five
  historical classes, the two measurement defects, and how to re-bank by reading the report.
- `docs/tui-render-invariant-baseline.toml:160-184` — the section's own rule: every row carries
  a reason, and the guard rejects one without.
- `scripts/check_tui_render_invariant_baseline.py` — the shape rule, the identity rule and the
  two self-test cases.
- `src/tui/components/dialogs/{diff,security_review,panel_manager,connect,info}.rs` — each repair
  carries the reason beside the code it explains.
- **Present:** `architecture/tui.md:4516-4575` — the I4 rule, both advertisement sources being
  authoritative, and the four retained rows with their reasons (work package G / plan §12). The
  manifest header and this record are the operational detail behind it.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | `architecture/tui.md` was not updated with the I4 rule, the retained rows, or the rule that a hint and a footer on one surface must not advertise different keys for one verb (WP G, §12) | The invariant was documented only in a manifest header and a closure record. | **RESOLVED**: the section exists at `architecture/tui.md:4516-4575` — the I4 rule, the hint-versus-footer rule ("Both advertisement sources are authoritative"), and the four retained rows with their reasons. |
| medium | The plan's acceptance criteria 1–3 name "exactly two rows", "contrast 2495" and a guard line of "2 advertised-key"; the tree has four rows and 1520 contrast | A literal reading of the plan says this milestone failed; the amended M006 criterion is met. Without the amendment recorded, a future reader will re-open a closed question or mistrust the record | Amend the plan's §13 criteria to the residue-with-reasons form, or record the amendment against M006 in the registry; name the contrast re-banks' owners (M003/M007) |
| low | `scripts/verify.sh quick`/`full` and `cargo fmt --check` were not run on this tree, and the tree is uncommitted | Criterion 10 had no evidence from this review. | **Discharged 2026-11-13**: `scripts/verify.sh full` passed on this tree (17,377 + 8,441 tests, 0 failures; fmt and clippy clean; render corpus 1237/0). The commit is still outstanding. |
| low | The 61-row before-state is not in git (the manifest, guard and `tests/tui_render/invariants/` are untracked) | Criterion 11's row-by-row account cannot be produced; the class account sums to 61 and is the best available check | Land the milestone as a commit, or record the manifest digest at the measurement's introduction. Not fixable retrospectively |
| low | The contrast delta (2,495 → 1,520) is not attributable from the tree, and no committed before-state exists | A reader cannot mechanically confirm this milestone added no contrast row; the structural argument (I4 reads its own fixture table) is the substitute | None while the tree is uncommitted; the guard's per-row reproduction is the operative property and it passes |
| low | The two `SecurityReview` rows are a decision, not a defect: making `h`/`H` measurable requires either a clipboard test hook or de-advertising a working verb | If an owner later allows the write in tests, two rows can be removed; until then they are the honest residue | Decide whether an invariant may perform the clipboard write. Same class as the ADR-0042 `Tab` decision, and not this milestone's to take |

## 11. Roadmap disposition

**Milestone conditionally closed with one named operational condition outstanding.** The
documentation item that was outstanding is delivered: `architecture/tui.md:4516-4575` states the
I4 rule, the hint-versus-footer rule and the four retained rows with their reasons.

What remains is precise: a **commit** carrying the tree, and — for a fully literal reading of the
plan — acceptance criteria 1–3 amended to the measured residue rather than the plan's predicted
two rows. The gate class is discharged: `scripts/verify.sh full` passed on this tree at
2026-11-13 (17,377 workspace tests, 8,441 feature-pass tests, 0 failures; fmt and clippy clean;
render corpus 1237/0). Neither remaining item is a redesign, and the residue itself is settled:
four rows, four reasons, four `INERT` lines from the measurement, and no other inert key in the
set.

## 12. Registry updates

- `plans/registry.md:29` (the subsystem row): "ready; C001 and C002 planned, not started" must
  become a status naming C001's record and its condition — C001
  `conditionally closed`, `plans/closure/tui-advertised-keys-corrective/001-status.md`; C002
  unchanged.
- `plans/registry.md:681` (the M006 milestone row): its "the I4 baseline holds 61" text is
  stale; the measured residue is four rows with reasons, and the row must name this record and
  the outstanding `quick`/`full` run plus the `architecture/tui.md` item.
- `plans/subsystems/tui-advertised-keys-corrective-addendum.md` header: `Status: ready` →
  `conditionally closed` for C001 (C002 untouched), with the record path and the conditions.
- `plans/implementation/tui-advertised-keys-corrective/001-i4-baseline-to-its-irreducible-residue.md`
  header: `Status: ready` → `conditionally closed`. **Not edited here**: this session's write
  scope is the closure record only, and `plans/closure/README.md` does not require a source-plan
  header edit.
- Roadmap `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md` §12 row `006`:
  remains `corrective pass required` at the milestone level; its corrective pointer should name
  this record as the C001 half that is now measured.

## Correction log (2026-11-13)

- **§1 reason 1, §2's WP G row, §9 and §10's medium row.** Said `architecture/tui.md` had no I4 section, no retained-row statement and no hint-versus-footer rule. Met by the tree: `architecture/tui.md:4516-4575` — "**I4: an advertised key is handled, and the measurement means something**", the "Both advertisement sources are authoritative" rule, and the four retained rows with their reasons. The item is delivered, not outstanding.
- **§1 reason 3, §2's criterion-10 row, §4 and §10's low row.** Said `quick`/`full`/`cargo fmt --check` were not run. Met by: `scripts/verify.sh full` on this tree, 2026-11-13 — 17,377 workspace tests and 8,441 feature-pass tests, 0 failures; `cargo fmt --all --check` clean; clippy `-D warnings` clean; render corpus 1237 passed / 0 failed. `run_full()` calls `run_quick()`.
- **§11, the disposition.** Was "named operational evidence outstanding, plus one bounded in-scope documentation item that a small corrective must close". The documentation item is delivered and the gate class is discharged; the remaining condition is the commit (plus, under a literal reading of the plan, the criteria 1–3 amendment).

**No promotion.** The status stays `conditionally closed`: the tree is uncommitted, and the plan's literal criteria 1–3 still describe two rows, 2,495 contrast rows and a "2 advertised-key" guard line rather than the measured residue. No condition was deleted without naming what met it.
