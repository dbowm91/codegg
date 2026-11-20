# TUI Advertised Keys Corrective Milestone 001 — The I4 baseline holds only what cannot become real

Status: conditionally closed

Repository baseline: `7642b9e38` on `feat-auto-20261109-f8abad0d`

**The tree this plan was written against is uncommitted.** HEAD is `7642b9e38`; the
working tree carries **235 changed or untracked paths** (measured,
`git status --porcelain | wc -l`), including the whole M006 work and the render
harness this plan repairs. `scripts/verify.sh full` was green on that tree at
**2026-11-12** — 17,326 workspace tests and 8,400 feature-pass tests, 0 failures,
render corpus 1227 passed / 0 failed (**parent-run**, not repeated by this
session). Every number below is a property of that tree, not of a SHA, and none
of it is committed.

Source closure record:

- `plans/closure/tui-modularity-and-render-invariants/006-status.md` — status
  `corrective pass required`; §1 executive finding, §10 unresolved findings, §11
  roadmap disposition.

Source implementation plan:

- `plans/implementation/tui-modularity-and-render-invariants/006-advertised-keys-are-handled.md`
  — work package D, acceptance criterion 5, the deferred-work rule in §14.

Source roadmap:

- `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md` §2 (I4) and
  §12 row 006.

Long-term requirements:

- `plans/000-long-term-specification.md#25-tui-target-behavior` — the interaction
  concepts a surface advertises are the concepts it must deliver. The canonical
  spec is silent on *presentation*; I4 is an interim invariant under
  `plans/003-planning-process.md` (roadmap §"Authority note").

Applicable ADRs:

- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` — one owner per
  chord. Two of the 61 entries are `Tab`, which `FocusManager` owns for every
  dialog. This milestone **records** that conflict; it does not amend the ADR.
- `plans/adrs/ADR-0052-cross-frontend-command-execution-ownership.md` — no
  catalog change; no command, verb or binding is added.

Primary class: **invariant**. The deliverable is that I4 is *enforced* rather
than *exempted*, and that its baseline means what it says. The single
user-visible behaviour change (Connect's unpainted error line) restores feedback
for a code path that already exists; it is not a new capability.

## 1. Objective

Drive `[advertised_keys]` in `docs/tui-render-invariant-baseline.toml` from the
**61** entries it holds to the **two** entries that cannot become real without a
key-ownership decision — and make the measurement trustworthy enough that the
number means something:

1. **The measurement must be inspectable and state-fresh.** Today one fixture per
   surface is pressed with every advertised key in sorted order, so an earlier
   press changes the state a later key is measured on, and the announced set
   cannot be derived from the source by reading. Both must stop.
2. **The 61 must be decided individually** — repaired, de-advertised, or
   recorded with a concrete reason — which is work package D that the milestone
   plan already defined and did not deliver.
3. **Four advertisements that name keys the surface does not implement** must be
   corrected at the advertisement, and one key whose only effect is scheduled but
   never painted must be made visible.

## 2. Why this milestone is ready

It has no dependency. The measurement exists, is non-vacuous, and is green
(`cargo test --test tui_render -- the_recorded_i4_baseline_is_exactly_what_the_harness_measures`
→ **1 passed, 1226 filtered out**, measured by this session). The 61 entries are
classified in §3 from the source. The ratchet guard and its `--self-test` exist.

The only external dependency is an owner decision that this milestone
deliberately does **not** need: whether ADR-0042 should let one dialog pre-empt
`Tab`. The two `Tab` rows are retained and recorded; §14 names the decision.

## 3. Current implementation evidence

Provenance: **measured** = read out of the tree or run by this session at the
stated tree state; **parent-run** = run by the parent session on the same tree
and supplied in the assignment; **read** = source reading.

### 3.1 The manifest

`docs/tui-render-invariant-baseline.toml` holds **61** `[[advertised_keys]]`
entries across **18** surfaces, with `[totals] advertised_keys = 61` (**measured**,
`tomllib`). Per surface (**measured**):

| Surface | n | Surface | n | Surface | n |
|---|---|---|---|---|---|
| SecurityReview | 9 | Mcp | 5 | ProjectChat | 3 |
| Tree | 7 | Diff | 3 | ResearchBrowser | 3 |
| Files | 6 | Keybind | 3 | Review | 3 |
| Connect | 5 | Model | 3 | Agent | 2 |
| | | | | Permission | 2 |
| | | | | Session | 2 |
| | | | | Template | 2 |
| | | | | Help | 1 |
| | | | | PanelManager | 1 |
| | | | | WorkspaceDashboard | 1 |

The three largest surfaces (SecurityReview, Tree, Files) are 22 of the 61; the
eight list-shaped dialogs (Agent, Connect, Files, Mcp, Model, Session, Template,
Tree) account for 32 of the 61, and **30 of those 32** are the empty-fixture
class below.

### 3.2 Classification

Every entry reproduces as measured — the harness compares in both directions and
the tree is green — so **class (iv) "no longer reproduces" has zero members and is
structurally impossible** while the equality test passes. The classes below are
derived from the source, one surface at a time. A fifth class is added because
53 of the 61 are not defects of the surface at all, and folding them into (ii)
would send an implementer to repair code that is correct.

- **(i) the advertised key has no arm** — **4 entries.**
  `Diff` `^d`, `^u`; `SecurityReview` `^d`, `^u`.
  Evidence: neither `handle_key` has a `CONTROL` arm (`diff.rs:460-491`,
  `security_review.rs:248-269`); both implement `PageUp`/`PageDown`. Each surface's
  *painted* footer names the working spelling — `"j/k/↑/↓ scroll  |  PgUp/PgDn  |  …"`
  (`diff.rs:187`) and `"j/k move | PgUp/PgDn page | …"` (`security_review.rs:507`)
  — while its `HintLine` says `^d/^u page` (`diff.rs:32`, `security_review.rs:41`).
  So the capability exists; **an advertisement is stale and contradicts the same
  surface's other advertisement.** This is the only true hint-versus-footer
  disagreement in the set.
- **(ii) handled, but what happens is not the advertised verb** — **4 entries.**
  - `Model` `Tab` and `WorkspaceDashboard` `Tab`: `FocusManager::handle_key`
  interior `Tab` before the dialog sees it (`component/focus.rs:164-170`) and
  calls `handle_tab`, which rotates a focus index — the dialogs' own `Tab` arms
    (`model.rs:1029`, `workspace_dashboard.rs:183`) are unreachable through
    production dispatch. `WorkspaceDashboard`'s arm would launch
    `WorkspaceDashboardToggleExpand`, i.e. exactly `Tab tasks`.
  - `PanelManager` `K`: the arm runs (`panel_manager.rs:221-228`) but
    `PanelPlacement::named_by(PanelDirection::Up)` returns `None`
    (`panels/mod.rs:305-312`) — there is no top edge — so `?` yields `None` on
    every press. The footer advertises `⇧H/J/K/L move` (`:359-360`) and the module
    doc says so (`:9`), while the code's own comment at `:543` states "Shift+K
    names no edge … so it must do nothing".
  - `Connect` `Enter`: with no providers the arm runs and stores
    `error_message = Some("Select a provider")` (`connect.rs:446`), but the
    `ConnectStep::SelectProvider` render (`:850-935`) never reads that field — the
    only two reads are in text-input branches (`:1108`, `:1626`). The press is
    therefore invisible, which under the owner's bar is broken.
- **(iii) advertisement stale because the surface changed** — **0 entries** beyond
  the (i) group, which is read better as a stale *hint* than as a missing
  capability.
- **(iv) entry no longer reproduces** — **0 entries**, for the reason above.
- **(v) the probe cannot exercise the key** — **53 entries.** Three mechanisms,
  each verified in the source:
  - **empty fixture (46).** The arm exists and cannot change anything on the
    probe's state: no rows, no selection, no messages.
    `Agent` 2 (`agents: Vec::new()`, `selected()` → `None`);
    `Connect` 4 (`providers: Vec::new()`, `navigate` on an empty list);
    `Files` 6 (`rows: Vec::new()`, `selected_row()` → `None`, so `enter`, `h`,
    `←`, `→` all return `None` and `^d`/`^u` page an empty listing);
    `Mcp` 5 (`servers: Vec::new()`, `new()` at `:83-91`);
    `Model` 2 (`models: Vec::new()`, `selected` 0);
    `ProjectChat` 3 (`message_count() == 0`, so `reply`/`edit`/`arm_or_redact`
    have no target);
    `ResearchBrowser` 3, `Review` 3 (empty item lists);
    `SecurityReview` 7 (`receipt: None`, so `render_body` early-returns at
    `:415-430` with "No security review result available" — `enter`, `f`, `h`,
    `j`, `k`, `n`, `p` change only state the empty branch never paints);
    `Session` 2, `Template` 2, `Tree` 7.
  - **boundary no-op (2).** `Diff` `↑` and `Help` `↑`: the view is already at the
    top, `handle_scroll(-1)` / `scroll_by(-1)` clamp to 0, and the frame does not
    change. `↓` on the same surface does move, which is why it is not recorded.
  - **ordering artefact (5).** The probe presses one surface's keys in sorted
    order on **one shared state**, so an earlier press changes the state a later
    key is measured on: `Keybind` `j`, `k`, `r` — `e` then `enter` then `i`
    leaves the dialog in `KeybindMode::Import`, whose body is the import box, so
    `j`/`k` move an unpainted cursor and `r` is guarded on
    `mode == Normal` (`keybind.rs:636-641`); `Permission` `3`, `4` — `2` arms
    `confirm_persistent` (`permission.rs:346-351`), after which `options().len()`
    is 2 and digits 3 and 4 fail the bounds check at `:408-410`. In a fresh
    fixture, `3` = "Deny Once" and `4` = "Always Deny" both work.

**The classification is not a prediction the current harness confirms per key.**
Two absences are explained by §3.3's design: `SecurityReview`'s `Enter` and `H` and
`Review`'s `Enter`, `↑`, `↓` never reach the announced set because those fixtures
take a render early-return that hides the footer (`review.rs:263-271`,
`security_review.rs:415-430`), so only the `keymap_hint` is scanned. **One
asymmetry is not explained by the source at all:** `Agent`, `Session`, `Template`,
`Mcp` and `Tree` each paint `↑/↓` in a footer the probe reads, and on an empty
list neither `select_up` nor `select_down` can change a cell — yet `↓` is
recorded and `↑` is not. This session could not derive that from the code. That is
the evidence for work package A: **a set that cannot be re-derived from the code
is not yet a measurement, it is a checked-in observation.** The corrective must
make it derivable before anyone promises a number.

### 3.3 The measurement's own limits

- **One state per surface, sorted order.** `measure_advertised_keys`
  (`tests/tui_render/invariants/baseline.rs:348-390`) mounts one fixture and
  presses every announced key against it in sequence. §3.2's ordering class is
  the direct consequence, and the manifest's own header names the same design
  ("the probe runs one fixture per surface, in sorted key order").
- **The key parser is blind to named keys.** `key_of` (`:249-292`) understands
  arrows, `Esc`/`Enter`/`Tab`/`space`, `Shift+X`, `⇧X`, `^X` and single
  alphanumerics. It returns `None` for `Home`, `End`, `PageUp`, `PageDown`,
  `⌫`, `[` and `]`, which are advertised by real hints (`diff.rs:34`,
  `task_view.rs:47`, `queue.rs:64`, `security_review.rs:47`, `research.rs`,
  `info.rs`, `ui_node.rs`). `spellings` also reads only the **first** token of a
  part, so `push("[ ]", "reorder")` (`queue.rs:64`) yields the single token `[`,
  which the parser cannot read — **both** keys of that pair are silently lost.
  A surface can therefore advertise a key the invariant never presses.
- **The guard mirrors the parser.** `scripts/check_tui_render_invariant_baseline.py`
  rejects a recorded spelling the parser cannot read (`rule_measured_surface`,
  R4), so those keys can never even be banked — the 61 undercount the true
  advertisement set.
- **No per-key output.** The measurement's only output is the set, printed solely
  on mismatch (`Measured::to_toml`, `:170-215`). A reviewer cannot ask it what it
  saw for one surface.

### 3.4 What the guards currently say

- `python3 scripts/check_tui_render_invariant_baseline.py` →
  `61 advertised-key, 2495 contrast and 0 hint-truncation entries recorded; every
  one reproduces and none was added unbanked`, exit 0 (**measured by this
  session**; the guard is green by design at any banked count). Its `--self-test`
  prints 12 case lines and `self-test: 12 cases passed`, exit 0.
- The guard's shape rule requires **exactly** `("surface", "key")` on each
  entry (`SECTION_FIELDS`, `rule_shape`), so the record's required action "add a
  reason field" cannot be done without a guard change.

## 4. Invariants that must not regress

- **The render corpus stays green.** 1227 tests, 0 failures. This milestone
  changes fixtures and the harness, so the corpus is the primary regression net.
- **`[contrast]` is untouched at 2,495 rows.** M007 owns that set; the manifest
  itself warns that a content-bearing fixture "would also re-bank `[contrast]`".
  §7 WP A therefore gives I4 its **own** content-bearing fixture table and leaves
  `fixtures::dialogs()` — the table I1, I2, I3a and I5 read — unchanged.
- **One hint vocabulary.** The 32-surface vocabulary scan keeps its floor of 32
  and its recorded hint table; no surface changes its `keymap_hint()` text except
  the two stale `^d/^u` spellings, which are recorded before and after.
- **No key changes owner (ADR-0042).** `check_tui_composer_key_authority.py` and
  `check_tui_editor_key_authority.py` must report the same figures
  (`2 / 92 / 8`, no shadowed arm) as before.
- **No command catalog change (ADR-0052).**
  `check_command_catalog_ownership.py` clean and unmodified.
- **No surface loses a working verb.** Deleting an advertisement is the fallback
  only where the verb cannot be implemented without a capability or ownership
  decision, and the reason is recorded (§7 WP C).
- **The render-cost shape guard is unchanged.**
- **The comment ratchet holds**; any per-file ceiling this milestone moves is
  re-banked in the same diff.
- **The baseline may only shrink, and every removal is matched by the change that
  caused it.** An unbanked reduction is the same drift with the sign flipped; the
  harness's `compare` reports both directions by design.

## 5. Scope

### In scope

- The I4 harness: a **fresh fixture per key**, a **content-bearing fixture table
  used by I4 only**, and a **per-key report** a reviewer can read.
- The four advertisements that name a key the surface does not implement
  (`Diff` and `SecurityReview` `^d/^u`).
- `PanelManager`'s `⇧K`, which names an edge that does not exist.
- `Connect`'s invisible `Enter` failure.
- `[advertised_keys]` driven to the two `Tab` rows, each with a recorded reason;
  the guard taught to require that reason.
- The wording finding the record raises: no arm dispatches these five **verbs**
  (`info.rs:172-174`, `dialog_dispatch_coverage.inc:5-12`).
- Documentation: `architecture/tui.md` states the I4 rule, the two retained
  entries, and the rule that a hint must not contradict the same surface's footer.

### Explicitly out of scope

- **Amending ADR-0042.** The two `Tab` rows are the retained residue. Reaching
  zero requires either that amendment or de-advertising both verbs; §14 names the
  decision and this milestone does not make it.
- **The Import dialog's missing input bound.** The record (§10, low) asks the
  owner to decide whether the dialog should have a limit. It is a decision, not a
  defect this milestone owns; the plan's stop condition already reported it
  instead of inventing a limit.
- **The other hand-formatted footers.** 26 dialog modules paint a `|`-separated
  advertisement row; this milestone touches only the three whose keys are
  defective. Converting the rest to `HintLine` is milestone 002.
- **The structured-pairs conversion** — milestone 002.
- **I1/I2 coverage of the new content-bearing I4 fixtures.** They must render at
  100×30 without panic, which the measurement proves; extending the paint
  invariants to them is a different milestone's scope.
- **Landing the work as a commit and running `full` on the merged tree.** The
  session that owns the merge does both; §15 requires the evidence.

## 6. Required production changes

### Core/domain, storage, migrations, protocol, DTOs, runtime, concurrency

None.

### Frontend or operator surface

- `src/tui/components/dialogs/diff.rs:29-36` — the hint advertises `^d/^u page`
  and no arm answers either. The footer already names `PgUp/PgDn`, and the
  handler implements `PageUp`/`PageDown` (`:473-480`). Correct the hint to the
  keys that exist. (The alternative — adding `CONTROL` arms — would be a new key
  binding on a surface whose footer already names the working keys; the plan
  prefers the advertisement that matches the implementation.)
- `src/tui/components/dialogs/security_review.rs:38-49` — same defect, same
  repair; the footer at `:507` and the handler at `:262-269` already agree on
  `PgUp/PgDn`.
- `src/tui/components/dialogs/panel_manager.rs:359-360` (and the module doc at
  `:9`) — `⇧H/J/K/L move` becomes `⇧H/J/L move`. There is no top edge
  (`panels/mod.rs:305-312`) and `:543` already documents that `Shift+K` must do
  nothing. The verb cannot be implemented without a new placement in
  `PanelPlacement`, which is a capability decision this milestone does not make.
  The reason is recorded in the doc comment.
- `src/tui/components/dialogs/connect.rs` — paint `self.error_message` in the
  `ConnectStep::SelectProvider` branch, beside the existing footer
  (`:920-930`), so `Enter` with no selectable provider says why. One line plus a
  unit test. No new message text: the string already exists.
- `src/tui/components/dialogs/info.rs:172-174` — reword to "no arm dispatches
  these five **verbs**" (the record's low finding: `k` does reach the scroll arm,
  so `k` scrolls rather than kills).

### Security and authorization

None. No path, process, credential, network or authority surface is touched. The
new I4 fixtures are constructed from public API and hold no credential or run
record (`fixtures.rs`'s header already states the four surfaces that cannot be
built and why).

### Documentation and static guards

- `docs/tui-render-invariant-baseline.toml` — `[advertised_keys]` reduced to two
  rows, each with a `reason`; `[totals]` re-banked; the long header's "not yet
  empty" section rewritten to state the residue and why it is irreducible.
- `scripts/check_tui_render_invariant_baseline.py` — allow and **require**
  `reason` on `advertised_keys`; add a `--self-test` case proving an entry
  without one fails.
- `architecture/tui.md` — the I4 rule, the two retained entries, and "a hint and
  a footer on the same surface must not advertise different keys for the same
  verb".

## 7. Ordered work packages

### Work package A — Make the measurement inspectable and state-fresh

Intent: the count must mean something before it is driven down.

Required changes, all in `tests/tui_render/invariants/`:

1. **A fresh fixture per key.** `measure_advertised_keys` mounts a new fixture for
   every advertised key instead of pressing a sorted sequence against one. The
   announcement set is computed once per surface, then each key is pressed
   against its own mount.
2. **A content-bearing fixture table used by I4 only.** Add
   `fixtures::dialogs_with_content()` (or a `Content` argument) that seeds the
   list surfaces with rows, the chat with messages, the review with file entries,
   the security review with a receipt, and the model dialog with models. `Files`,
   `Connect`, `Tree`, `Mcp`, `Session`, `Template`, `Agent`, `Model`,
   `ProjectChat`, `ResearchBrowser`, `Review`, `SecurityReview` and `Keybind`
   each need a real state. **`fixtures::dialogs()` and the contrast measurement
   stay byte-identical.**
3. **A per-key report.** The measurement prints, for each surface, the announced
   spellings and each press's verdict when `--nocapture` is used (or a test that
   asserts the announced set surface by surface). A reviewer must be able to
   derive the recorded rows from the output without changing code.
4. **A scroll-position fixture for the boundary class.** A list surface that
   advertises scroll-up must be measured with the view mid-list, so `↑` can act.

Acceptance evidence: the harness change lands with the recorded set re-banked to
whatever it now measures, and **every movement is explained row by row** in the
commit message and the closure record. The count is expected to *fall*; a row
that appears is a finding about a key that was previously "handled" only because
an earlier press had changed the frame. The i4 test's `--nocapture` output names
every surface and key, so the re-bank is a reading rather than a guess.

### Work package B — The four stale hint spellings

Intent: remove the only hint-versus-footer contradiction in the set.

Required changes: `diff.rs` and `security_review.rs` hints per §6.

Acceptance evidence: `[advertised_keys]` loses `Diff`/`^d`, `Diff`/`^u`,
`SecurityReview`/`^d`, `SecurityReview`/`^u`; no surface's *footer* text changes;
the vocabulary scan's recorded table changes for exactly those two surfaces and
the change is recorded in the plan's closure evidence (`the_recorded_hint…` names
the new strings).

### Work package C — `PanelManager`'s `⇧K`

Intent: stop naming an edge that does not exist.

Required changes: the two Normal-mode footer rows and the module doc; a doc
comment at the handler naming `PanelPlacement` as the reason.

Acceptance evidence: `[advertised_keys]` loses `PanelManager`/`K`; the existing
test that `Shift+K` does nothing still passes (`cargo test --lib --
tui::components::dialogs::panel_manager` → 18 passed, one updated); the vocabulary
scan's floor is still 32.

### Work package D — `Connect`'s invisible failure

Intent: a key whose effect exists must paint it.

Required changes: render `self.error_message` in the `SelectProvider` branch;
one unit test that presses `Enter` with an empty catalog and asserts the painted
row.

Acceptance evidence: `cargo test --lib -- tui::components::dialogs::connect` →
15 passed (one added); `[advertised_keys]` loses `Connect`/`Enter` **only if** the
content-bearing fixture makes the choose path reachable — if it does not, the row
stays with a reason and the plan reports it rather than pretending.

### Work package E — Drive the section to its residue

Intent: the record's central uncompleted item.

Required changes: `[advertised_keys]` contains exactly the two `Tab` rows, each
with `reason = "FocusManager owns Tab for every dialog (ADR-0042); the dialog's
own arm is unreachable through production dispatch"`; `[totals]` re-banked; the
guard's `SECTION_FIELDS` and `rule_shape` accept and require `reason`; a
`--self-test` case proves an entry without a reason is a violation.

Acceptance evidence: the guard prints `2 advertised-key, 2495 contrast and 0
hint-truncation entries recorded; …`; `--self-test` passes with the new case;
`the_recorded_i4_baseline_is_exactly_what_the_harness_measures` passes.

### Work package F — The wording finding

Required changes: `info.rs:172-174` and `dialog_dispatch_coverage.inc:5-12` say
"no arm dispatches these five *verbs*", not "no arm dispatches them".

Acceptance evidence: `cargo test --test tui_render -- dialog_dispatch_coverage` →
7 passed; the two files' prose matches `handle_key`.

### Work package G — Documentation and the closure record

Required changes: `architecture/tui.md` per §6; a closure record at
`plans/closure/tui-advertised-keys-corrective/001-status.md`.

Acceptance evidence: the record cites the before/after counts, the per-key
report, and the remaining two reasons.

## 8. Failure, cancellation, restart, and contention semantics

Not applicable: the measurement is synchronous and per-surface, and the
production edits are dialog-local.

Two hazards are real and stated rather than exempted:

- **The ratchet's two-directional equality.** Removing an entry the harness still
  measures is a failure ("the manifest records … and it no longer reproduces"),
  not a silent success. A repair that does not change the frame will therefore be
  caught, not banked.
- **The harness is the only observer.** A fixture that is *wrong* (too small to
  scroll, too empty to select) manufactures entries; a fixture with content can
  also **remove** entries that a user can still hit at a boundary. When a row
  disappears, the closure record must say which press now changes the frame, not
  merely that the count fell.

## 9. Compatibility and migration

No storage, protocol or configuration migration.

The user-visible surface: `Diff` and `SecurityReview` stop naming `^d`/`^u` and
name the `PgUp`/`PgDn` keys they already implement; `PanelManager` stops naming
`⇧K`; `Connect` shows why `Enter` did nothing. No key changes owner, no binding
is added, and no command exists that did not.

The manifest's own contract changes in one direction only: an entry now carries a
reason, so a later reader has the justification inside the file rather than in a
closure record.

## 10. Required tests

### Focused unit tests

- `connect`: `Enter` on an empty provider catalog paints "Select a provider".
- `panel_manager`: the Normal-mode footer names `⇧H/J/L` and not `⇧K`; the
  existing "Shift+K does nothing" test stays.
- `diff` and `security_review`: `keymap_hint()` names `PgUp/PgDn` for the page
  verb and no `^d`/`^u`; the surfaces' footers are unchanged.

### Integration tests

- The per-key I4 measurement over every surface, with a fresh fixture per key.
- The content-bearing fixture table produces at least one frame-changing press
  for each key it advertises (the property that replaces "it is in the baseline").
- The recorded set equals the measurement in both directions.

### Restart and recovery tests

None.

### Contention and cancellation tests

None.

### Security and negative tests

- An `[advertised_keys]` entry without a `reason` fails the guard (new
  `--self-test` case).
- A fixture that advertises a key it ignores still fails the measurement (the
  existing non-vacuity test, `the_i4_measurement_detects_an_advertised_key_that_does_nothing`).
- A surface whose hint and footer name different keys for one verb is a
  documented finding; §3.2's (i) class must be empty after WP B, and the plan's
  closure evidence names the check (no surface in the baseline advertises a key
  its own footer does not).

### Migration and compatibility tests

- The `[advertised_keys]` section is compared before and after each work package;
  every removed row names the change that removed it in the closure record.

## 11. Required verification commands

Run from the repository root. `CARGO_BUILD_JOBS=2` in every mode, as `verify.sh`
does.

```bash
# The I4 measurement and the whole invariant module. The filter is the MODULE,
# so it selects every test under tests/tui_render/invariants/.
cargo test --test tui_render -- invariants
# expect: test result: ok. 31 passed; 0 failed; 1196 filtered out
#         (31 is this module's test count today; the run must end "0 failed", and
#          if this milestone adds a test the closure record states the new count)

# The one recorded-baseline test, by its full name.
cargo test --test tui_render -- the_recorded_i4_baseline_is_exactly_what_the_harness_measures
# expect: test result: ok. 1 passed; 0 failed; 1226 filtered out

# Non-vacuity: the failing fixture must still be detected.
cargo test --test tui_render -- the_i4_measurement_detects_an_advertised_key_that_does_nothing
# expect: 1 passed; 0 failed; 1226 filtered out

# The one-vocabulary scan (its floor is 32 surfaces; WP B/H/G must not lower it).
cargo test --test tui_render -- vocabulary
# expect: 3 passed; 0 failed; 1224 filtered out

# The recorded dialog gaps.
cargo test --test tui_render -- dialog_dispatch_coverage
# expect: 7 passed; 0 failed; 1220 filtered out

# WP C and WP D, by module path.
cargo test --lib -- tui::components::dialogs::panel_manager
# expect: 18 passed; 0 failed
cargo test --lib -- tui::components::dialogs::connect
# expect: 15 passed; 0 failed
cargo test --lib -- tui::components::dialogs::diff
# expect: 5 passed; 0 failed

# The ratchet guard, and the proof that the guard bites.
python3 scripts/check_tui_render_invariant_baseline.py
# expect: check_tui_render_invariant_baseline: 2 advertised-key, 2495 contrast
#         and 0 hint-truncation entries recorded; every one reproduces and none
#         was added unbanked   (exit 0)
python3 scripts/check_tui_render_invariant_baseline.py --self-test
# expect: self-test: <n> cases passed with no "... cases failed" line and exit 0
#         (n is 12 today, measured; WP E adds one)

# Authority must be unchanged.
python3 scripts/check_tui_composer_key_authority.py
python3 scripts/check_tui_editor_key_authority.py
# expect the editor guard's exact line: 2 guard-block chord(s), 92 Normal-mode
# chord(s) [9 digits claimed by a count arm: 123456789], 8 Insert-mode chord(s),
# no shadowed arm
python3 scripts/check_command_catalog_ownership.py
# expect: command catalog ownership: clean
python3 scripts/check_tui_render_cost.py
# expect: the render path derives each result once

# Formatting, linting, and the two gates.
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
scripts/verify.sh quick
scripts/verify.sh full
```

**Do not cite `cargo test --test tui_render -- tui_render_invariants`.** The
milestone plan's §11 names it and it selects **zero** tests: there is no test
whose id contains `tui_render_invariants`, and a zero-test filter exits 0. The
module filter above is the replacement. This is the documented trap AGENTS.md
describes, and it is why every command here names a filter this session verified
against the tree.

## 12. Documentation updates

- `architecture/tui.md` — I4's rule, the two retained `Tab` entries, and the new
  rule that a hint and a footer on one surface must not advertise different keys
  for one verb.
- `docs/tui-render-invariant-baseline.toml` — the reduced section, its reasons,
  and a header that states what the residue is and why it cannot shrink further
  without ADR-0042.
- `tests/tui_render/dialog_dispatch_coverage.inc` — the verb/character wording.
- `src/tui/components/dialogs/info.rs` — the `renders_caller_footer` doc.
- `plans/closure/tui-advertised-keys-corrective/001-status.md` — the record.

## 13. Acceptance criteria

1. `docs/tui-render-invariant-baseline.toml` holds **exactly two**
   `[[advertised_keys]]` rows, both `Tab` (`Model`, `WorkspaceDashboard`), each
   carrying a non-empty `reason` naming ADR-0042. Checkable by parsing the file.
2. `[totals].advertised_keys` equals the number of rows, and
   `[totals].contrast` is still **2495** with every contrast row unchanged.
3. The guard prints `2 advertised-key, 2495 contrast and 0 hint-truncation
   entries recorded` and exits 0; its `--self-test` passes and includes a case
   that rejects an entry without a `reason`.
4. `cargo test --test tui_render -- invariants` → **0 failed**, and the i4
   measurement test passes with a `--nocapture` per-surface, per-key verdict
   table from which the two remaining rows can be read. (The module holds 31 test
   functions today; if this milestone adds one, the closure record names it and
   states the new count.)
5. `cargo test --test tui_render -- vocabulary` → **3 passed, 0 failed**, with the
   scan enumerating **≥ 32** surfaces.
6. No method in the diff adds a `CONTROL` arm, changes a `keymap` table, or
   touches `EDITOR_KEYMAP`/the catalog: `git diff` shows none, and both authority
   guards print the same figures as before.
7. `Diff` and `SecurityReview` advertise no `^d`/`^u` for paging, and their
   painted footers are byte-identical.
8. `PanelManager`'s footer names `⇧H/J/L move`; `Shift+K` still does nothing and
   a test says so.
9. `Connect`'s `SelectProvider` branch paints `error_message`, proved by a test
   that presses `Enter` on an empty catalog and asserts the row.
10. `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
    `scripts/verify.sh quick` and `scripts/verify.sh full` are green.
11. Every removed entry is named in the closure record with the change that
    removed it.

## 14. Stop conditions

Report rather than improvise when:

- **a verb cannot become real without a key-ownership decision.** The two `Tab`
  rows are exactly this. Reaching **zero** entries requires an ADR-0042
  amendment (letting one dialog pre-empt `Tab`) or de-advertising both verbs.
  Both are decisions about a design intent; this milestone records the conflict
  and does not choose. If the owner wants one entry rather than two, say so —
  that is a different resolution, not a smaller one.
- **a verb cannot become real without a new capability.** `PanelManager`'s
  `⇧K` would need a "top" placement in `PanelPlacement`. The de-advertisement is
  permitted because the verb is not implementable as the surface is shaped, and
  the reason is recorded; inventing a placement is not.
- **a fixture change would move `[contrast]`.** Stop and report. M007 owns that
  set; a re-bank here would hide a contrast change inside an advertisement fix.
- **an entry does not reproduce.** A recorded failure that the harness no longer
  measures is an unbanked reduction; do not delete it quietly and do not keep it
  as a leftover.
- **a repair makes a surface advertise less than it does.** Re-read the plan's
  rule: "making a hint honest by deleting a verb that *could* be implemented"
  needs a recorded reason and is preferred over only when the implementation
  needs a decision above.
- **the per-key report cannot be produced from the harness.** Then the count is
  still an observation, not a measurement, and the section must not be re-banked
  as if it were.

## 15. Closure evidence required

- The `[advertised_keys]` section before and after, with a row-by-row account of
  each removal and the change that caused it.
- The per-key report (`--nocapture`) for every surface.
- The two retained rows with their reasons.
- The guard's two outputs (check and `--self-test`).
- `invariants`, `vocabulary`, `dialog_dispatch_coverage` and the three `--lib`
  module counts.
- Both authority guards' exact lines, the catalog guard and the render-cost
  guard.
- `cargo fmt --check`, `clippy -D warnings`, `scripts/verify.sh quick`,
  `scripts/verify.sh full` (the last run by the session that owns the merge).
- A closure record at `plans/closure/tui-advertised-keys-corrective/001-status.md`.

## 16. Handoff notes

The measurement is the milestone. Repairing 53 fixture artefacts one at a time
would take a week and would teach nothing; making the fixture carry content and
the press start from a fresh mount collapses them, because they were never
surface defects.

Do not treat the 61 rows as a list of bugs. 53 of them are the probe's own
limitation, 4 are stale hints that contradict the same surface's footer, 2 are a
key owned by a stage above the dialog, one is an edge that does not exist, and
one is an error message nobody reads. Read §3.2 before touching any file.

Do not add a `handles_key()` method or a `claims` table to make the measurement
easy. That is a second source of truth for the same question, and the record's
one-line rule stands: assert the observable effect.

If a repair would make `Diff`/`SecurityReview` advertise *fewer* keys, remember
what the user loses: nothing. The footer already named the working key; the hint
was the stale half.

The two `Tab` rows are the whole remaining argument. If a later reader asks why
I4 is not green with an empty file, the answer is in the reason field: one stage
owns `Tab` for every dialog, and deciding otherwise is ADR-0042's job, not this
milestone's.
