# TUI Composer Text Layout Milestone M002 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/tui-composer-text-layout/002-normal-mode-scrolling-and-page-motions.md`

Source subsystem roadmap:

- `plans/subsystems/tui-composer-text-layout-roadmap.md#milestone-m002--normal-mode-scrolling-and-the-page-motions`

Repository baseline reviewed: `f2639b5d2`

Implementation commits or pull requests:

- **this commit** — the implementation and this closure record land together
  in one commit on `chore/record-landing-lane-outcomes` (baseline
  `f2639b5d2`). The landing note appended after the merge records that
  commit's SHA and the hosted `CI / verify` result for it; see §10.
  The files that carry this milestone's change are
  `src/tui/composer_claims.rs` (the condition, the fit and the label),
  `src/tui/components/prompt.rs` (the visual-row motions and the page
  motions), `src/tui/app/mod.rs` (the claim gate and the page branch),
  `src/tui/input.rs` (the help overlay's condition note),
  `scripts/check_tui_composer_key_authority.py` (the guard's repaired
  condition set), `tests/tui_render/tui_composer_claims.inc` and
  `tests/tui_render/tui_composer_motions.inc`. No SHA, pull request number or
  commit message is claimed, because none exists: the implementing agents were
  not authorized to commit.

Applicable ADRs (unchanged by this milestone):

- `plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md` — **accepted**
  2026-10-19, with the `space`-chord amendment and, dated **2026-10-23**, the
  `## Amendment: the wrapped visual row` section. **That amendment is the
  contract this milestone implements**, and the ADR itself was **not edited**
  by this milestone. §4 quotes the implemented condition next to the
  amendment's wording and states that they agree.

## 1. Executive finding

The milestone's capability boundary — **when the composer takes `j`/`k`, what
those keys move, and which surface a page gesture reaches** — is complete in
the working tree, and the completion is verified against the ADR's dated
amendment rather than against prose.

At the baseline the composer's claim condition read "more than one line", a
**logical-line count**. After M001 made the composer wrap, that condition
could not survive: a buffer with a **single** logical line can occupy more
rows than the composer shows, and a condition reading "more than one line"
would leave that content unreachable — `j` and `k` would fall through to the
transcript while the composer's own overflow sat behind the box. The motions
themselves were logical-line motions, and the page motions reached the
transcript only.

What landed:

- `src/tui/composer_claims.rs` — `ClaimCondition::MultiLineBuffer` →
  **`ClaimCondition::ContentOverflowsBox`**; `label()` →
  `"composer content overflows the box"`; a new `ComposerFit` (`Unknown` /
  `Measured { row_count, visible_rows }`) with `overflows()`; `holds(fit)`
  instead of `holds(line_count)`.
- **The fit is measured in the layout pass and cached on the widget.**
  `PromptWidget::ensure_cursor_visible_with_width` already holds the
  `VisualLayout`, so it measures `row_count` and stores
  `ComposerFit::Measured { row_count, visible_rows }` — **no re-wrap, no second
  computation**. The claim gate (`src/tui/app/mod.rs:4129`) reads
  `self.prompt_state.prompt.content_fit()`, so **the gate and the layout
  cannot disagree**.
- The `j`/`k` motions became **visual-row** motions
  (`cursor_visual_row_down` / `cursor_visual_row_up` / `move_visual_row`).
- The **page motions**: `Ctrl+u` / `Ctrl+d` page the **composer** when it
  overflows and the **transcript** when it does not.
- The help overlay's `j`/`k` condition note now reads **"composer content
  fits its box"**.
- `scripts/check_tui_composer_key_authority.py` was **repaired**: the
  known-condition set became `{"Always", "ContentOverflowsBox"}`, the G10.3
  self-test fixture was updated, and **no rule was weakened**.

No protocol, storage, migration, authorization, or dependency change. No closed
milestone of any other roadmap was reopened or amended. **No key was
re-spelled**, and the claim set is unchanged.

### The condition, the risk, and the exact future evidence

**The condition.** The implementation is complete and locally verified,
and this change carries it in a commit — but the **hosted `CI / verify`
lane has not run against that commit**, and a CI run cannot precede
the commit it runs against. The exact future evidence that discharges
the condition is:

1. a **green hosted `CI / verify` run against that commit on the hosted
   runner** — not the local gate, and not a re-run of the local gate. The exact future evidence that discharges the condition is:

1. a **green hosted `CI / verify` run against that commit on the hosted
   runner** — not the local gate, and not a re-run of the local gate.

**The risk.** A local `scripts/verify.sh quick` run is a property of **this
machine and this tree**, not of the hosted runner. The hosted lane is a strict
**subset** of `quick` — it omits the client and desktop boundaries and the
provider wire/catalog/resilience, OpenAI endpoint and Eggwork routing guards —
so a green hosted run does **not** by itself prove everything the local gate
proves. The local gate's own value is what this record states in §4: it is
strong evidence about this tree, and it is **not** evidence about the hosted
runner's environment, its runner image, or its concurrency.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| §13.1 — on a composer whose content fits its box, `j` and `k` still scroll the transcript, unchanged | `k_still_scrolls_the_transcript_when_the_composers_content_fits` and `the_vertical_motions_do_not_apply_when_the_content_fits` (`tests/tui_render/tui_composer_motions.inc`); `the_page_motion_does_nothing_when_the_content_fits` (`src/tui/components/prompt.rs`) | pass | The fall-through is the point of the condition. |
| §13.2 — on a composer whose content overflows its box, `j` and `k` move the caret by one visual row and are consumed by the composer | `k_is_claimed_when_the_composers_content_overflows_its_box` and `j_follows_the_same_conditional_claim_as_k` (`tests/tui_render/tui_composer_claims.inc`); `the_visual_row_motion_moves_the_caret_down_one_visual_row` (`src/tui/components/prompt.rs`); `j_moves_the_caret_down_a_visual_row`, `k_moves_the_caret_up_a_visual_row` | pass | The claim is consumed in the gate (`src/tui/app/mod.rs:4129`). |
| §13.3 — the condition is evaluated against the **fitted** content, not against the buffer's newline count: a composer whose text wraps to exactly the box's height falls through | `the_fitted_content_condition_gates_j_and_k_only` (the exact-fit `{3,3}` vs `{4,3}` case, `src/tui/composer_claims.rs`); `a_composer_whose_content_wraps_to_exactly_the_box_height_fits` and `a_composer_whose_content_overflows_its_box_overflows` (`src/tui/components/prompt.rs`) | pass | The newline count is not consulted anywhere in the condition. |
| §13.4 — `Ctrl+u` and `Ctrl+d` page the composer when it overflows and the transcript otherwise | `ctrl_d_pages_the_composer_when_its_content_overflows` and `ctrl_d_pages_the_transcript_when_the_composers_content_fits` (`tests/tui_render/tui_composer_motions.inc`); the page branch at `src/tui/app/mod.rs:4369`–`:4390` | pass | The composer's page moves the **window and the caret together**. |
| §13.5 — the composer key-authority guard passes, carries the new condition, and its self-test still rejects its defect class | `python3 scripts/check_tui_composer_key_authority.py --self-test` → every violation class detected and rejected; the guard's plain run → "Composer key-ownership guard passed — the claim table is closed, single-owner, leaks no key to an app-level action, and every caret motion is bound the same way in both maps"; `KNOWN_CONDITIONS = {"Always", "ContentOverflowsBox"}` (`scripts/check_tui_composer_key_authority.py:170`) | pass | `MultiLineBuffer` was **removed** from the guard's known set rather than kept — a known condition the table can no longer carry is stale knowledge. No rule was weakened. |
| §13.6 — the help overlay's `j`/`k` note states the new condition | `src/tui/input.rs:1704` → `condition: Some("composer content fits its box")`; `the_conditions_label_states_the_fitted_content_condition` (`src/tui/composer_claims.rs`) | pass | Quoted in §4. |
| §13.7 — the implemented behaviour matches ADR-0042's dated amendment | §4's "The condition against the ADR's amendment" subsection | pass | The amendment and the code are quoted next to each other and stated to agree. |
| §15 — the condition's and the motions' unit tests, including the exact-fit case | `the_fitted_content_condition_gates_j_and_k_only`, `an_unknown_fit_does_not_hold_the_condition`, `the_conditions_label_states_the_fitted_content_condition` (`src/tui/composer_claims.rs`); 11 unit tests in `src/tui/components/prompt.rs` | pass | The exact-fit case is `{3,3}` against `{4,3}`. |
| §15 — the page motions' tests | `the_page_size_is_the_visible_height_minus_two`, `the_page_motion_pages_the_window_and_the_caret_together`, `the_page_motion_does_nothing_when_the_content_fits` (`src/tui/components/prompt.rs`) | pass | — |
| §15 — the composer key-authority guard's output and its self-test's result | §4's verification table | pass | — |
| §15 — the help overlay's condition note, quoted | §4's "The help overlay's note" subsection | pass | — |
| §15 — the implemented condition quoted from the code, next to ADR-0042's amendment, with a statement that they agree | §4's "The condition against the ADR's amendment" subsection | pass | — |
| §15 — a statement that no key was re-spelled and that the claim set is unchanged (I2) | §3's "No key was re-spelled" subsection; the guard's plain run reports the claim table closed and single-owner | pass | — |
| §15 — `scripts/verify.sh quick` green at the closing commit | §4's verification table | **partial** | The gate is **green on the working tree** (exit 0, "Quick verification passed", 74 steps). What is missing is the other half of the item: **there is no closing commit**, so the gate has not been green *at a closing commit*, and the hosted `CI / verify` lane has not run against this code. This is the milestone's condition; see §1 and §11. |
| An unmeasured composer holds no conditional claim | `an_unknown_fit_does_not_hold_the_condition` (`src/tui/composer_claims.rs`); `the_claim_is_not_live_before_the_composer_has_been_laid_out` (`tests/tui_render/tui_composer_claims.inc`); `the_fit_is_unknown_until_the_layout_measures_it` (`src/tui/components/prompt.rs`) | pass | Before the composer has been laid out the fit is `Unknown`, the condition does **not** hold, and `j`/`k` fall through to the transcript. |
| The visual-row motion lands on the geometry's answer at every byte | `the_visual_row_motion_lands_on_the_geometrys_answer_at_every_byte` (`src/tui/components/prompt.rs`); `the_vertical_motions_land_on_the_shared_geometrys_offsets` (`tests/tui_render/tui_composer_motions.inc`) | pass | The oracle is the geometry module, not the old logical-line model. |
| The vertical motion keeps the desired column where the target row allows | `the_visual_row_motion_leaves_the_caret_on_the_cell_it_names`; `the_vertical_motions_keep_the_column` (`tests/tui_render/tui_composer_motions.inc`) | pass | Matches `text_geometry::move_vertical`'s rule for logical lines. |
| The motion clamps at the first and last visual row | `the_visual_row_motion_clamps_at_the_last_visual_row`, `the_visual_row_motion_clamps_at_the_first_visual_row` (`src/tui/components/prompt.rs`) | pass | — |
| The gate and the layout cannot disagree | the gate reads `content_fit()` (`src/tui/app/mod.rs:4129`), which is the **cached** measurement the layout pass took (`src/tui/components/prompt.rs:507`–`:510`) | pass | A second computation of the same fact would be a second answer to it. |

## 3. Production implementation evidence

**The condition (`src/tui/composer_claims.rs`).**

- `ClaimCondition::MultiLineBuffer` → **`ClaimCondition::ContentOverflowsBox`**
  (`:253`); `label()` → `"composer content overflows the box"` (`:290`).
- `pub enum ComposerFit { Unknown, Measured { row_count, visible_rows } }`
  (`:206`) with `overflows(self) -> bool` (`:226`):
  `Unknown => false`, and `Measured { row_count, visible_rows } =>
  row_count > visible_rows`.
- `holds(self, fit: ComposerFit) -> bool` (`:373`) — the conditional claim
  holds on `fit.overflows()`.

**The measurement (`src/tui/components/prompt.rs`).**

- `ensure_cursor_visible_with_width(visible_rows, width)` (`:500`) already
  holds the `VisualLayout`, so it measures `row_count` and stores
  `ComposerFit::Measured { row_count, visible_rows }` (`:507`–`:510`). **No
  re-wrap, no second computation.** It also records the width it was handed
  as `wrap_width` (`:503`), guaranteeing **one width** for the fit, the
  motions and the mouse placement.
- `content_fit()` (`:430`) returns the cached measurement; `content_overflows()`
  (`:440`) is the boolean form.

**The gate (`src/tui/app/mod.rs`).**

- The claim gate reads `self.prompt_state.prompt.content_fit()` (`:4129`)
  and calls `claim.condition.holds(fit)`. **The gate and the layout cannot
  disagree**, because there is only one measurement.
- Before the composer has been laid out the fit is `Unknown`, the condition
  does **not** hold, and `j`/`k` fall through to the transcript.

**The motions.**

- `cursor_visual_row_down` (`:321`) and `cursor_visual_row_up` (`:326`) call
  `move_visual_row(delta)` (`:627`), which uses the geometry module's
  `cell_of_byte` / `byte_at` / `caret_position`. The **desired column is
  carried across the move** where the target row allows, matching
  `text_geometry::move_vertical`'s rule for logical lines.

**The page motions.**

- `src/tui/app/mod.rs:4369`–`:4390` — `Ctrl+u` / `Ctrl+d` page the
  **composer** when it overflows and the **transcript** when it does not, in
  the same `if workspace_active { … } else { … }` branch shape the workspace
  dashboard already uses.
- The page size is the **transcript's own rule** — the visible height minus
  two, with a floor of one (`src/tui/components/messages.rs:1601`–`:1604` for
  the transcript; `src/tui/components/prompt.rs` `page_size()` for the
  composer, which reads the same rule).
- The composer's page moves the **window and the caret together**
  (`src/tui/components/prompt.rs:665`–`:679`).

**The help overlay.**

- `src/tui/input.rs:1704` — the `j`/`k` condition note now reads
  `"composer content fits its box"`.

**The guard (`scripts/check_tui_composer_key_authority.py`).**

- **Repaired**, not weakened: `KNOWN_CONDITIONS` became
  `{"Always", "ContentOverflowsBox"}` (`:170`) — the Rust variant
  `MultiLineBuffer` no longer exists, so a known condition the table cannot
  carry is stale knowledge — and the G10.3 self-test fixture was updated
  (`:1340`). `MultiLineBuffer` appears **nowhere** in `src/`, `scripts/` or
  `tests/`.

**No key was re-spelled.** The milestone changed no `InputAction`, no binding
map, and no letter's spelling. The claim set is the same closed
thirteen-letter set ADR-0042 part 2 fixes; what changed is the **condition**
one of its claims carries, which is exactly what ADR-0042's dated amendment
authorizes.

**Tests.**

- `src/tui/composer_claims.rs` — 3 unit tests: the fitted-content condition
  (including the exact-fit case), an unknown fit holding no condition, and
  the condition's label.
- `src/tui/components/prompt.rs` — 11 unit tests: the fit is unknown until
  the layout measures it; the exact-fit and overflow cases; the four
  visual-row motion tests; the page-size, page-motion and no-op-when-it-fits
  tests.
- `tests/tui_render/tui_composer_claims.inc` — 4 new tests: `k` still scrolls
  the transcript when the content fits; `k` is claimed when it overflows; `j`
  follows the same conditional claim as `k`; the claim is not live before the
  composer has been laid out.
- `tests/tui_render/tui_composer_motions.inc` — 7 new/rewritten tests: `j`
  moves the caret down a visual row; `k` moves the caret up a visual row;
  the vertical motions keep the column; the vertical motions land on the
  shared geometry's offsets; the vertical motions do not apply when the
  content fits; `Ctrl+d` pages the composer when its content overflows;
  `Ctrl+d` pages the transcript when the composer's content fits.
- **No new `.inc` member** — the new integration tests live in two existing
  members.

**Documentation.**

- `architecture/tui.md` — the composer's overflow behaviour, the claim
  condition, and the page-motion priority.
- `docs/tui.md` — the composer's gesture prose and the `[NOR]`-mode keymap
  table: the `j`/`k` condition, the `Ctrl+u`/`Ctrl+d` behaviour, and the
  statement that the page is not restricted to `[NOR]`.
- `.opencode/skills/tui/SKILL.md` — the module guide's statement of the new
  condition, the cached fit, and the exact-fit rule ("three rows in a
  three-row box *fits* and `j`/`k` fall through to the transcript; four rows
  does not fit, and the composer takes the keys and moves the caret one
  **visual row**"), and the guard's row in the guard table.
- `architecture/overview.md` — unchanged by this milestone; it carries the
  integration-test-file count row, which this milestone did not move.

## 4. Verification executed

### Commands run

```bash
# recorded at this milestone's own verification point
cargo test --locked -p codegg --lib tui::composer_claims
cargo test --locked -p codegg --lib tui::components::prompt
cargo test --locked -p codegg --lib tui::composer_geometry
cargo test --locked -p codegg --lib tui::text_geometry
cargo test --locked -p codegg --test tui_render
python3 scripts/check_tui_composer_key_authority.py --self-test
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
cargo check --workspace --all-targets --locked
cargo fmt --all -- --check
bash scripts/verify.sh quick
```

### Results

| Command | Result |
|---|---|
| `cargo test --locked -p codegg --lib tui::composer_claims` | **14 passed**, 0 failed |
| `cargo test --locked -p codegg --lib tui::components::prompt` | **23 passed**, 0 failed |
| `cargo test --locked -p codegg --lib tui::composer_geometry` | **9 passed**, 0 failed |
| `cargo test --locked -p codegg --lib tui::text_geometry` | **15 passed**, 0 failed |
| `cargo test --locked -p codegg --test tui_render` | **1043 passed**, 0 failed |
| `python3 scripts/check_tui_composer_key_authority.py --self-test` | every violation class detected and rejected |
| `python3 scripts/check_test_assertion_quality.py` | all checks pass |
| `python3 scripts/check_test_duplication.py` | all checks pass |
| `cargo check --workspace --all-targets --locked` | exit 0 |
| `cargo fmt --all -- --check` | clean |
| `bash scripts/verify.sh quick` | **exit 1 — one step** (at this milestone's verification point — see the re-verification below for the current result) |

The guard's plain run reported: **"Composer key-ownership guard passed — the
claim table is closed, single-owner, leaks no key to an app-level action, and
every caret motion is bound the same way in both maps"**.

**`verify.sh quick` failed at exactly one step at this milestone's
verification point:**
`check_test_target_references` reported that `architecture/overview.md`
recorded a stale integration-test-file count. That row is a **documentation
count**, not a product behaviour: the guard compares the row against the tree,
and the row had not been re-measured after this line's new integration members
were added. Every other step of the gate passed; the remainder was run
individually and passed. **The row has since been re-measured and corrected in
the same working tree, and the gate is now green** — see the re-verification
below and §9.

### Re-verification (this record's author, on the same working tree)

| Command | Result |
|---|---|
| `cargo test --locked -p codegg --lib tui::composer_claims` | **14 passed**, 0 failed (matches the recorded result) |
| `cargo test --locked -p codegg --lib tui::components::prompt` | **23 passed**, 0 failed (matches the recorded result) |
| `cargo test --locked -p codegg --lib tui::composer_geometry` | **9 passed**, 0 failed (matches the recorded result) |
| `cargo test --locked -p codegg --test tui_render` | **1052 passed**, 0 failed — **higher** than the 1043 recorded above, because the main-buffer milestones of this line added integration members after this milestone's verification point |
| `cargo test --locked -p codegg --lib` | **7373 passed, 3 failed** — the three environmental failures in the table below, and nothing else. |
| `bash scripts/verify.sh quick` | **exit 0 — green**: "Quick verification passed", 74 steps. The one red step seen earlier in this record's writing — `check_test_target_references`, "architecture/overview.md records 326 integration test files, but the tree holds 327 root integration test modules" — was a **stale documentation count**, re-measured and corrected in the same working tree. |

The recorded test counts are **snapshots of a tree that other agents were
concurrently modifying** — the main-buffer and composer milestones of this line
were implemented in parallel, and each milestone's integration members landed in
the same family. They are therefore **not a monotonic series** across the five
records of this line, and each record's count describes the tree at that
milestone's own verification point.

### The condition against the ADR's amendment

From the code (`src/tui/composer_claims.rs:226`–`:234`):

```rust
    pub fn overflows(self) -> bool {
        match self {
            Self::Unknown => false,
            Self::Measured { row_count, visible_rows } => row_count > visible_rows,
        }
    }
```

ADR-0042's dated amendment (`plans/adrs/ADR-0042-composer-normal-mode-key-ownership.md`,
`## Amendment: the wrapped visual row`, **Amended 2026-10-23**), verbatim:

> | Decision part 4, `j` and `k` rows | The composer claims `j`/`k` **only when the composer buffer has more than one line** | The composer claims `j`/`k` **when the composer's content does not fit its box** — the wrapped row count exceeds the visible row count |
> | Decision part 4, `j` and `k` rows (fall-through) | "On a single-line buffer the key must fall through to transcript scroll unchanged" | "On a composer whose content **fits**, the key must fall through to transcript scroll unchanged" |
> | The `j`/`k` motion | A logical-line motion | A **visual-row** motion |

and:

> the condition must be evaluated against the **fitted** content, not against the buffer's newline count. A composer whose text wraps to three rows in a three-row box **fits**, and `j` must fall through to the transcript; a composer whose text wraps to four rows in a three-row box does not fit, and `j` moves the caret a visual row.

**They agree.** The measure is `row_count > visible_rows`: three rows in a
three-row box fits (`3 > 3` is false → falls through); four rows in a
three-row box does not fit (`4 > 3` → the composer takes the key and moves the
caret one visual row). The newline count is not consulted anywhere. The motion
is a visual-row motion (`cursor_visual_row_down` / `cursor_visual_row_up` /
`move_visual_row`, built on the geometry module's `cell_of_byte` / `byte_at` /
`caret_position`).

### The help overlay's note

`src/tui/input.rs:1704`:

```rust
            condition: Some("composer content fits its box"),
```

### The page branch's ordering

The page branch's order is **workspace dashboard → composer (if it overflows) →
transcript** (`src/tui/app/mod.rs:4369`–`:4390`): the dashboard outranks the
composer. The plan's wording ("the composer when it overflows, the transcript
otherwise") is true only outside the dashboard.

### Failures recorded, not concealed

The following failures are **not** this milestone's failures, and are recorded
so that a reader is not left to discover them:

| Failure | Attribution |
|---|---|
| `core::daemon_refresh::tests::session_projection_rebuilds_owner_scoped_live_jobs` — `InvalidRootPath("/tmp")` | **Environmental and pre-existing.** This host's `/tmp` root path is not usable by the daemon-refresh fixture. Nothing in this milestone touches the daemon. |
| `core::daemon_refresh::tests::session_projection_snapshot_includes_in_process_live_child_instances` — `InvalidRootPath("/tmp")` | **Environmental and pre-existing.** Same cause. |
| `managed_process::tests::offline_required_scheduler_carried_intent_refuses_disabled_sandbox_before_spawn` — `OfflineRequired is unavailable on this platform` | **Environmental and pre-existing.** The macOS host has no offline-required sandbox backend. Nothing in this milestone spawns a process. |
| (resolved) `tui::command::tests::command_docs_builtin_slash_action_variant_count_matches_source` and `tui::command::tests::command_docs_count_matches_registry` — stale counts in `architecture/command.md` ("claims BuiltinSlashAction has 160 variants; the enum has 161"; "count 168 drifts from registry 169") | **Not this milestone's.** The extra variant is M004's `/expand-all` slash action (`src/tui/command.rs`), and the stale counts were in `architecture/command.md`, which this milestone does not touch. **Documentation counts**, in the same class as the `overview.md` row below. The counts were corrected in `architecture/command.md` in the same working tree, and both tests now **pass**. |
| (resolved) `check_test_target_references` — the `architecture/overview.md` integration-test-file count row | **A documentation obligation, not a product defect.** The row records a count of test *files*; the guard compares it against the tree. It was the gate's one red step; the row has been re-measured and corrected in the same working tree, and the step is green. §9 names it. |
| (not reproduced) `tui::file_tree_coordinator::tests::barrier_held_worker_plus_100_rebuilds_keeps_one_worker_one_pending` | **Flaky, and not a failure of this milestone.** It failed once under the load of a full `--lib` run on this host, **passes when run in isolation** (`cargo test --locked -p codegg --lib tui::file_tree_coordinator::tests::barrier_held_worker_plus_100_rebuilds_keeps_one_worker_one_pending` → 1 passed, 0 failed), and passed in the final full `--lib` run. It touches no TUI code. |

**Skipped scope.** `scripts/verify.sh full` (clippy `-D warnings` over the
whole workspace, the workspace test suite, and
`cargo test -p codegg --features server,plugins,lsp-test-support`) was **not
run**, and no claim is made about it. A hosted CI run was **not** executed
against this code; the evidence above is local. The change is confined to the
TUI's presentation path and one guard script, and the commands above cover
every module it touches.

## 5. Invariant review

| Invariant | Evidence | Result |
|---|---|---|
| **I1 — the composer is a frontend-local string.** A motion and a page gesture touch the model's buffer and the view; neither is a daemon call, a revision, a document or a submission. | The diff touches `src/tui/composer_claims.rs`, `src/tui/components/prompt.rs`, `src/tui/app/mod.rs`, `src/tui/input.rs` and one guard script; the composer's submission path, its buffer's ownership and every daemon interaction are untouched | pass |
| **I2 — the claim set is fixed.** ADR-0042 part 2 fixes the composer's claimed letters. This milestone adds no row, removes no row, and claims no new letter. | `scripts/check_tui_composer_key_authority.py`'s plain run reports the claim table **closed and single-owner**; the self-test still rejects every violation class; no `InputAction` and no binding map changed; `MultiLineBuffer` — the old condition, not a letter — was removed, and no letter was added or removed | pass |
| **I3 — a claimed key never falls through.** When the composer claims a key in Normal mode it consumes it, whether or not the motion had anywhere to go. | The claim is consumed in the gate (`src/tui/app/mod.rs:4138`–`:4139`) and the app-level action is returned from; `a_claimed_letter_is_consumed_before_the_binding_map` and `the_caret_motions_are_bound_the_same_way_in_every_map` (`tests/tui_render/tui_composer_claims.inc`) | pass |
| **I4 — the resolved mode is the gate.** A claim is live only when the composer holds the keyboard and the composer's input mode is Normal. | The gate's guards (`!workspace_active && self.composer_holds_keyboard() && self.composer_input_mode() == InputMode::Normal`, `src/tui/app/mod.rs:4117`–`:4120`) are unchanged; `the_claim_is_not_live_in_insert_mode` and `a_focused_panel_closes_the_claim_gate` | pass |
| **I5 — the caret is on the byte it names.** A visual-row motion must land the caret on a byte, and the caret's drawn cell must be that byte's cell. | `the_visual_row_motion_lands_on_the_geometrys_answer_at_every_byte`; `the_vertical_motions_land_on_the_shared_geometrys_offsets`; the motion is built on the geometry module's `cell_of_byte` / `byte_at` / `caret_position` | pass |
| **I6 — the pane's budget wins.** Unchanged. | M001's clamp is unchanged: `total.clamp(MIN_HEIGHT, max_height.max(MIN_HEIGHT))` (`src/tui/components/prompt.rs:460`) | pass |
| **I7 — one redraw cadence.** Unchanged. | The milestone adds no timer, interval, tick or animation; the fit is measured in the render pass that already existed | pass |

## 6. Failure and recovery review

- **Failure.** The fit is a `ComposerFit`, so there is no uninitialised state:
  `Unknown` is a defined value, not a missing one, and it holds no conditional
  claim. The motions clamp at the first and last visual row rather than
  running off either end. The page's `page_size()` returns `None` when the
  composer has never been laid out, and the page does nothing in that case.
- **Duplicate delivery and idempotency.** Not applicable. The fit is a
  **cached measurement**, recomputed on every layout pass and read by the
  gate; there is no queue, no replay, and no duplicate-delivery path.
- **Cancellation races.** Not applicable. Nothing asynchronous is added.
- **Daemon or node restart.** Not applicable. Nothing is persisted; the
  composer's fit is re-measured on the first layout pass after a restart, and
  until then the condition simply does not hold.
- **Partial persistence failure.** Not applicable. No write path is added.
- **Stale generation or lease.** The fit is the **only** cached measurement,
  and it is written by the layout pass that produced it — the gate never
  recomputes it. A resize replaces it on the next pass. There is no separate
  cache that can outlive the layout it describes.
- **Contention and resource release.** The composer is single-owner and
  frontend-local. The fit is a two-field struct; the measurement is **taken
  once** per layout pass and read by the gate, so there is no second wrap and
  no second computation of the same fact.
- **Malformed or unauthorized input.** The condition is a comparison of two
  counts the composer itself produced. A composer whose buffer is empty
  measures one row, which fits any box, so the condition does not hold and
  the keys fall through — the safe direction. The newline count is never
  consulted, so a buffer whose newlines do not correspond to its wrapped rows
  cannot mislead the gate.
- **Bounded event and artifact behavior.** No event or artifact is produced.
  The page moves the composer by one page of rows — the visible height minus
  two, with a floor of one — so the work per keypress is bounded by the box's
  height.

## 7. Migration and compatibility review

- **Schema migration.** None. Nothing is persisted.
- **Protocol.** None. No daemon message, session state, or wire format
  changes.
- **Configuration.** None. No configuration key is added, renamed or
  reinterpreted.
- **Backward compatibility.** The change is **user-visible** in three ways:
  1. `j` and `k` now move the caret by a **visual row** rather than a logical
     line, and only when the composer's content overflows its box;
  2. a composer whose content **fits** falls through to the transcript, which
     is what it did at the baseline in the common case — so the visible
     difference for most users is that the motion is now visual, not logical;
  3. `Ctrl+u` / `Ctrl+d` now reach the composer when it overflows.
  **No key was re-spelled** and the claim set is unchanged, so no existing
  chord moves. The `j`/`k` condition is a **documented** change to ADR-0042's
  part 4, and the ADR carries a **dated amendment** authorizing it; the ADR
  itself was not edited.
- **Rollback limitations.** The change is self-contained in
  `src/tui/composer_claims.rs`, `src/tui/components/prompt.rs`,
  `src/tui/app/mod.rs`, `src/tui/input.rs` and the guard script. Reverting
  them restores the previous behaviour exactly, **except** that the guard
  script's `KNOWN_CONDITIONS` set would then need `MultiLineBuffer` back — the
  guard and the Rust variant have to move together. There is no persisted
  state to migrate back.
- **Legacy paths.** The **logical-line** motion is retained where it still
  belongs: `src/tui/text_geometry.rs` owns the **word** motions and the
  editor's geometry, and the composer delegates to it. `set_cursor_at_column`
  keeps its logical behaviour when `wrap_width == 0`.

## 8. Security review

- **Authorization.** Unchanged in the sense that matters: the milestone
  changes **which** key the composer claims and **what** that key does, not
  **whether** the composer may claim it. The gate's own authority checks —
  the composer holds the keyboard, the mode is Normal, the workspace
  dashboard is not active — are unchanged and are asserted by the guard's
  self-test.
- **Secrets.** None handled. No credential, token, or path is read or logged.
- **Privilege boundaries.** Unchanged. The composer remains a
  **frontend-local string**: a motion and a page gesture touch the model's
  buffer and the view, and neither is a daemon call, a revision, a document
  or a submission.
- **Path validation.** Not applicable — the milestone parses no paths and
  resolves none.
- **The trust frame.** Not applicable, and not weakened: the milestone does
  not read the frame, does not parse it, and does not touch
  `src/search_backend/framing.rs`, which has **zero diff lines**. The model
  still receives the frame byte for byte.
- **Denial of service.** The fit is measured once per layout pass from a
  layout the widget already holds; there is **no re-wrap** and no second
  computation. A page moves the composer by one page of rows, bounded by the
  box's height. The motions are O(rows) in the composer's own content, which
  is the user's own text.
- **Redaction.** Unchanged. The milestone adds no redaction and removes none.
- **Audit.** No new audit event. Moving a caret or paging a view is not an
  auditable action.

## 9. Documentation and operations

- **Updated architecture documents.**
  - `architecture/tui.md` — the composer's overflow behaviour, the claim
    condition, and the page-motion priority.
  - `architecture/overview.md` — **unchanged by this milestone.** It carries
    the integration-test-file count row, which this milestone did not move;
    the row was stale against the tree at the time and has since been
    re-measured and corrected in the same working tree, so the gate's step is
    green (see below).
- **Updated user-facing documentation.** `docs/tui.md` — the composer's
  gesture table and prose: "`j` and `k` only move the caret when your prompt
  **does not fit its box** — when there is more of it than the box can show.
  When the prompt fits, they scroll the conversation instead, which is what
  they have always done", and "`Ctrl+u` and `Ctrl+d` **page**. They page the
  **composer** when the composer has more to show than its box can hold, and
  the **conversation** when it does not — the same two keys, and the same page
  size either way. The page is not restricted to `[NOR]`: it behaves the same
  in both modes". The `[NOR]`-mode keymap table repeats the condition on the
  `j` / `k` row, and the "Two rules cover the edges of that table" note states
  it a third time. The user-visible condition is therefore stated in the user
  guide **and** in the help overlay's own note
  (`src/tui/input.rs:1704`).
- **Updated module guide.** `.opencode/skills/tui/SKILL.md` — the new
  condition, the cached fit, the exact-fit rule, and the guard's row in the
  guard table (its known set is `Always` and `ContentOverflowsBox`).
- **Static guards.** `scripts/check_tui_composer_key_authority.py` was
  **repaired and kept armed**: the known-condition set became
  `{"Always", "ContentOverflowsBox"}`, the G10.3 self-test fixture was
  updated, and **no rule was weakened**. The guard's plain run passes and its
  `--self-test` rejects every violation class.
- **Operator diagnostics.** None. Nothing to diagnose.
- **Documentation obligation — resolved.** The `architecture/overview.md`
  integration-test-file count row was the gate's one red step
  (`check_test_target_references`) — a **documentation count**, not a
  behaviour. The row has been re-measured against the tree and corrected in
  the same working tree, and the step is green. The plan's §12 documentation
  for this milestone **landed** in the working tree this record reviews.
  Nothing is outstanding here.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| high | **The hosted `CI / verify` lane has not run against the commit that carries this code.** The implementation and this record land together in one commit on `chore/record-landing-lane-outcomes` (baseline `f2639b5d2`). | The milestone's evidence is local only. A local `verify.sh quick` run is a property of this machine and this tree; the hosted lane is a strict **subset** of `quick`, so neither the local run nor a future hosted run proves the other. | **Required before this record can be upgraded to `closed`:** a green hosted `CI / verify` run against this commit on the hosted runner. |
| medium | **The composer's in-app verb labels still read "Move down one line" / "Move up one line"** (`src/tui/composer_claims.rs:267`–`:268`), while `docs/tui.md` describes the same gesture as "Move down / up **a row of the box**, keeping the column". | The motion is now a **visual-row** motion, so the in-app label is imprecise — the user is told "one line" and gets one visual row — and the in-app help and the user guide now describe the same gesture **two different ways**. It is a user-visible string. | **Flagged to the owner** as a wording choice: either re-spell the in-app labels to name the visual row (and bring them into agreement with `docs/tui.md`), or record that the Helix wording is kept deliberately. Not to be changed unilaterally — it is a user-visible string, and the label's own verb identity ("LineDown" / "LineUp") is bound into the guard's "every caret motion is bound the same way in both maps" rule. |
| medium | **The plain `wrap_to_strings` engine is still not escape-atomic.** | A tool output that contains an escape sequence can still be split by the plain word wrap. This milestone's composer path uses the **styled** primitive and is not affected; the gap is in the tool-output path. | **Pre-existing** and **outside this milestone's scope**. Recorded here so the line's records carry it once; the finding is owned by the main-buffer roadmap's M001 record. |
| low | **The page is not mode-gated.** In **Insert** mode a `Ctrl+d` on an overflowing composer moves the caret. | The plan and the roadmap state no mode qualifier ("the innermost overflowing surface takes the page"), and the branch is a flat arm, so the gesture behaves the same in both modes. It matches the vim `Ctrl+f`/`Ctrl+b` precedent. | Accepted and recorded as a **stated consequence** (see §4 and §7). If the owner wants the page to be Normal-mode only, that is a one-line change to the branch plus a test — it is not a defect. |

## 11. Roadmap disposition

**Milestone conditionally closed with named operational evidence outstanding.**

The production implementation is complete and locally verified: the condition,
the cached fit, the visual-row motions and the page branch are in the tree;
the guard is armed and its self-test rejects every violation class; and the
implemented condition is quoted next to ADR-0042's dated amendment in §4, with
the statement that they agree. The **outstanding evidence is named**:

1. a **closing commit** that carries the working tree (there is no SHA today),
   and
2. a **green hosted `CI / verify` run against that commit on the hosted
   runner**.

The **risk** is stated in §1: a local `scripts/verify.sh quick` run is a
property of this machine and this tree, not of the hosted runner, and the
hosted lane is a strict **subset** of `quick`, so a green hosted run does not
by itself prove everything the local gate proves.

No documentation obligation remains outstanding: the
`architecture/overview.md` count row was re-measured and corrected in the same
working tree, and the plan's §12 documentation for this milestone **landed**.
The only outstanding evidence is the operational pair named above — the
closing commit and the hosted run. Two further items are recorded for the
owner's decision rather than for fixing: the imprecise in-app verb labels, and
the not-mode-gated page.

This is the **last** milestone of the composer text layout roadmap: M002 was
the roadmap's final row, and its closure discharges the roadmap's own
dependency graph.

## 12. Registry updates

The parent owns `plans/registry.md` and the roadmaps; the changes below are
**specified** here, and the parent has **already applied them** in the same
working tree. The "as applied" lines record what the files actually say now,
so a reader can check the registry against this record rather than against
this specification.

- `plans/registry.md` — the "TUI composer text layout" row:
  - **Status** must read **conditionally closed**, and the row must name the
    baseline `f2639b5d2`.
  - The **milestone cell** must name this milestone's plan
    (`plans/implementation/tui-composer-text-layout/002-normal-mode-scrolling-and-page-motions.md`)
    **and** this closure record
    (`plans/closure/tui-composer-text-layout/002-status.md`), with the
    dependency `M001 (hard)` and the class.
  - The **narrative** must record: that the composer claims `j`/`k` **only
    when its content does not fit its box**, measured against the **fitted**
    content (the wrapped row count against the visible row count) and
    **cached** by the layout pass, so the gate and the box cannot disagree;
    that the motions are **visual-row** motions; that `Ctrl+u`/`Ctrl+d` page
    the composer when it overflows and the transcript otherwise, with the
    **dashboard outranking** the composer; that
    `ClaimCondition::MultiLineBuffer` became
    `ClaimCondition::ContentOverflowsBox`; that the guard's known-condition
    set was **repaired** and no rule weakened; and that ADR-0042's dated
    `## Amendment: the wrapped visual row` is the contract this milestone
    implements and the ADR itself was not edited.
  - The **summary paragraph** must record the conditional closure and its
    named outstanding evidence, and the two **owner decisions** pending — the
    imprecise in-app verb labels and the not-mode-gated page.
- **As applied.** `plans/registry.md` carries a **workstream row** for
  "TUI composer text layout" reading `**conditionally closed**`, and a
  **per-milestone table** whose rows read `**conditionally closed**` and name
  both the implementation plan and the closure record, with the dependency and
  the class and the baseline `f2639b5d2`. The roadmap's §13 "Milestone
  status" table reads `conditionally closed` for every milestone and carries
  the plan path and the closure path for each; the M002 row's dependency cell
  keeps `M001 (hard) — satisfied`; the roadmap also gained an **"Outcome
  (2026-10-23)"** paragraph naming the line as **conditionally closed** and
  naming the outstanding item — the hosted `CI / verify` lane, which has
  not run against the commit that carries this code.

- `plans/subsystems/tui-composer-text-layout-roadmap.md`:
  - §13 "Milestone status" must read `conditionally closed` for the M002 row,
    with the implementation-plan path and the closure-record path filled in.
    The **dependency cell is retained** rather than blanked.
  - §5.4 "`j`/`k` are conditional on a logical line count" must be updated to
    record that the condition is now the **fitted** content's overflow.
  - §5.6 "the page motions reach the transcript only" must be updated to
    record that the page motions now reach the composer when it overflows, and
    that the page branch's order is **dashboard → composer → transcript**.
  - §2.1 "the ADR-0042 amendment, and why it is an amendment" must record
    that the implementation **matches** the amendment, with the condition
    quoted from the code.
  - §12 "Completion definition" must record that the roadmap's milestones are
    all **conditionally** closed, with their *production* completion in the
    tree and their *operational* evidence (a closing commit and a hosted run)
    outstanding.
  - The roadmap must carry an **"Outcome (2026-10-23)"** paragraph naming the
    line as conditionally closed, naming the two outstanding items, and
    recording the delivered capability.
