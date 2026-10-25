# Count Audit History

The measurement history behind the `## Verified counts` table in
[`overview.md`](overview.md). This file records **why each row's counting
method is what it is**, so a later re-measurement reproduces the same numbers
instead of a different set.

The table in `overview.md` remains the single source of truth for counts.
This file is its provenance.

## How to re-measure

```bash
python3 scripts/measure_overview_counts.py
```

The script applies each row's `Source` method from the `overview.md` table and
prints `label<TAB>value<TAB>source` per row. A row that disagrees with the
table means the tree moved — re-measure it, decide whether the *method* or
only the *value* changed, and record that decision here and in `overview.md`.

The method per row is pinned below. Two of them are non-obvious and were
calibrated against the commits that originally recorded their values:

- **Enum variants** — attributes and comments are stripped first (otherwise
  `#[derive(Clone, Debug)]` reads as a variant), then struct variants and unit
  variants are counted separately. The method reproduces this file's published
  values exactly on the pre-merge tree — `CoreRequest` 260 = 236 struct + 24
  unit, `CoreResponse` 169, `CoreEvent` 92, `ProjectionEvent` 47,
  `AppEvent` 60 — before it was applied to any merged tree.
- **Production tool names** — an `impl Tool for` block inside a `#[cfg(test)]`
  module is a test-only mock, not a production tool. The trait path may be
  written `Tool`, `crate::tool::Tool`, or `super::super::Tool`; a scan that
  only matches `impl Tool for` misses `impl crate::tool::Tool for
  ContextReadTool`, whose literal name `context_read` is in the tool catalog.

## Method decisions and their reasons

### Authorized operations

The row's measure has changed twice, and both changes were corrections rather
than drift.

- It first measured the `CoreRequest` variants reachable in
  `operation_descriptor` — which is why it tracked the `CoreRequest` row
  exactly. `operation_descriptor` is the per-request *mapper*; its arm count is
  not the operation count.
- It then measured `representative_requests()`. That list is a *sample* of
  operations, not the operation set: its doc comment claims "one representative
  request per `CoreRequest` operation", but 10 operations have no
  representative request at all (`document_read`, `document_create`,
  `document_delete`, `document_modify`, `document_capabilities`,
  `project_delete`, `project_init_draft`, `project_init_publish`,
  `user_prompt_queue_send_now`, `workspace_directory_list`).
- The row now measures the **distinct operation names in
  `operation_descriptor`** — 257 — which is what
  `architecture/authorization.md` already calls the source of truth for the
  operation set. The representative list's 247 is recorded beside it.

### Tool registrations

The measure is the `registry.register(..)` / `registry.register_boxed(..)`
calls inside `ToolRegistry::with_options()`'s **body** (53 + 14 = 67) — not
the number of lines that merely mention `with_options`. The old measure counted
unrelated methods whose names end in `with_options` — `subscribe_with_options`,
`probe_with_options`, `new_with_options` — which is why it read 63 on one
branch and 65 on `main` for the same tree.

### Production tool names

The row's measure was **scoped too narrowly**, and the 2026-10-23 pass
corrected it. The published value measured `impl Tool for …` `name()` across
`src/tool/` and `src/context/` only — the directories the registry lives in.
That scope silently excluded `python_script`: a production `Tool` impl in
`src/python_script/tool.rs`, registered unconditionally by
`ToolRegistry::with_options()`, whose implementation lives with the
subsystem that owns the tool's behaviour rather than with the registry.

A production `Tool` impl can live anywhere in `src/`, so the row now measures
the **whole `src/` tree**. The corrected value is **62** distinct production
tool names (67 production `impl Tool for` blocks, 5 of which resolve their
name at runtime), excluding the 8 test-only mock impls under `#[cfg(test)]`:
3 in `program_manifest.rs`, plus `catalog.rs`, `tool_search.rs`,
`disabled.rs`, `broker.rs` and `src/scheduler/tool_program_executor.rs`.

This is the third method correction on this row, and each was a real
undercount rather than drift:

1. The original scan matched only the `impl Tool for` shape, so it missed
   `impl crate::tool::Tool for ContextReadTool` — whose literal name is
   `context_read`.
2. It did not exclude `impl Tool for` blocks inside `#[cfg(test)]` modules,
   which are test-only mocks rather than production tools.
3. It scoped the scan to `src/tool/` + `src/context/`, which excluded
   `python_script`.

The lesson generalises: **a measure scoped to where you expect to find
something will not find the thing that lives somewhere else.** A row's
`Source` must name the set the claim is about, not the set where the claim
was first noticed.

### Database tables

95 `CREATE TABLE` statements over 92 distinct names. `migration_version`,
`origin_attribution` and the transient `origin_attribution_new` rebuild each
appear twice, so the table count is 91 — less the transient rebuild name.

### Integration test files

The measure is the root integration test *modules*: `tests/*.rs` plus the
`.inc` members inside each family directory, with the `tests/common/` helper
tree excluded because it holds no test. It is pinned in the row's `Source`
column and checked by `scripts/check_test_target_references.py`, so a later
consolidation that moves or adds a module cannot leave the recorded count
stale.

The measure changed when the CI/test-throughput recovery M004 milestone
consolidated the root package's integration-test executables into families.
The previous measure counted `tests/*.rs` only and read 286. The new measure
reads **321**: `tests/*.rs` is now 4 (every root integration test file moved
into a family) plus `tests/*/*.inc` (317). The move from 286 to 321 is
counted rather than estimated — one file was deleted
(`tests/test_util.rs`, a helper with no test functions and no reference), and
the `.inc` members that M003 had already moved out of `tests/*.rs` became
visible to the new measure.

### Storage layout version

This row moved when the writer permit and session launch migrations were
renumbered. Both sides had independently allocated v84 and v85: `main`
published v84 (the additive network-intent preference) and v85 (the private
index publication journal), while the writer permit and session launch took
v84 and v85 as well. The branch yields the lower numbers to `main` — the same
rule its ADR renumbering follows — so the writer permit is v86 and the session
launch is v87. The renumbering is additive and changes the meaning of no
published database: every version `main` can produce is still below the
branch's two.

## Reconciliation record

Each entry is one point where `main` advanced and the branch was reconciled
with it. The file-count rows are the union of both sides' contributions; the
code-count rows moved only when the advance actually changed code.

- **2026-10-21 — merge with `main` at `cb0e79c0`.** Nine rows moved, and two
  changed their *measure* rather than only their value: tool registrations
  65 → 67 (measure pinned to `with_options()`'s body), authorized operations
  253 → 244 (measure moved to `representative_requests()`, later corrected
  again — see above), distinct production tool names 60 → 61 (the previous
  value was an undercount that missed `impl crate::tool::Tool for
  ContextReadTool`), `codegg-core` modules 47 → 48, `CoreRequest` variants
  259 → 260, built-in slash commands 167 → 168, database tables 88 → 89,
  storage layout version 83 → 85, integration test files 278 → 284.

- **2026-10-21 — merge with `main` at `2a0bae08f`.** `main`'s nine commits
  added and deleted **no** file under any directory a count is measured from —
  `src/tool/`, `src/context/`, `src/tui/app/state/`, `crates/codegg-core/`,
  `crates/egglsp/src/`, `tests/`, `scripts/`, `architecture/` — so no row could
  move. Every row was nonetheless re-measured against its own listed `Source`
  on the merged tree and none differed. The built-in slash-command row is
  unchanged for a second reason: `src/tui/command.rs` is byte-identical to the
  pre-merge tree.

- **2026-10-21 — merge with `main` at `84aa47528`** (the M006 scratch usage
  census). Not count-neutral: it added a `CoreRequest` variant, a
  `CoreResponse` variant, an authorized operation, and an integration test
  file. Four rows moved and each was re-measured against its own listed
  `Source` — `CoreRequest` variants 260 → 261 (237 struct + 24 unit),
  `CoreResponse` variants 169 → 170, authorized operations 244 → 245,
  integration test files 284 → 285.

- **2026-10-21 — merge with `main` at `28724bd83`** (the M007 scratch
  retention sweep). Added one request variant, one response variant, one
  authorized operation, and one integration test file, so four rows moved
  again — `CoreRequest` variants 261 → 262 (238 struct + 24 unit),
  `CoreResponse` variants 170 → 171, authorized operations 245 → 246,
  integration test files 285 → 286.

- **2026-10-21 — M004 family consolidation.** The integration-test-file row
  changed its measure, not only its value; see *Integration test files* above.

- **2026-10-22 — merge with `main` at `f4019a410`** (PR #162, the Linux
  trusted-git C008 contained managed-worktree work). This reconciliation was
  not a rebase: `main` still carries its integration tests as top-level
  `tests/*.rs` files, while the branch consolidated them into 21 families. The
  two sides had to be reconciled as two *shapes*, not two values. `main`'s
  advance added sixteen top-level root integration-test files (286 → 302) and
  no family members; thirteen of the sixteen were absorbed into the families
  that already owned their subject, which moves a file without moving the
  count: 16 + 302 = 318 before the absorption, and 3 + 315 = 318 after it.
  The three that stay top-level are named by a hosted qualification lane —
  `shared_checkout_c006` and `macos_native_shared_checkout_containment` by
  the macOS shared-checkout lane, `contained_worktree_c008` by the Linux M004
  lane — and a target a CI lane names keeps its own target, which is the rule
  M004 already applied to the branch's own six "family of one" targets. The
  CI guard-script row moved 68 → 74 (six guards from `main`:
  `check_blackboard_contract_matrix.py`, `check_network_intent_owner_wiring.py`,
  `check_process_denial.py`, `check_shared_checkout.py`,
  `check_test_filter_execution.py`, `check_writer_disposition.py`), then
  74 → 76 with `check_test_self_reexec_paths.py` and
  `check_test_common_binding.py`.

- **2026-10-22 — M008 blackboard closure.** Architecture docs 90 → 91 (the new
  `blackboard_contract_matrix.md`), integration test files 302 → 300
  (`blackboard_trajectory_m008.rs` and `blackboard_economics_m008.rs`), CI
  guard scripts 74 → 76 (+ `check_blackboard_contract_matrix.py`, + the two
  above). M008 added no production code and no schema migration, so every
  code-count row was unchanged and each was re-checked against its own listed
  `Source` rather than assumed. The shared fixture the two M008 suites use
  lives at `tests/common/blackboard.rs`, which the row's measure excludes.

- **2026-10-22 — C008 contained managed-worktree creation.** One integration
  test file moved (301 → 302) and one production source file was added
  (`src/git_contained_worktree.rs`), but **no** new `codegg-core` module (the
  new items live in the existing `worktree_service` and `worktree` modules),
  **no** schema migration, **no** new `GitOperation` variant, **no** new tool,
  **no** new request/response/event variant and **no** new guard script — so
  every other row was unchanged, each re-checked against its own listed
  `Source` rather than assumed. The architecture-doc row was unchanged at 91:
  C008 extended `git.md` and `session_launch.md` rather than adding a document.

  **Amendment (same row, no count moved).** The contained operation was
  **measured** to be two steps rather than one: `git worktree add <path>
  <existing-branch>` locks and rewrites `refs/heads/<branch>` even when the
  value is unchanged, so it cannot run inside a grant that excludes `refs`.
  ADR-0048 was amended accordingly — the operation is `git worktree add
  --detach <path> <commit>` followed by `git symbolic-ref HEAD
  refs/heads/<branch>`, both inside the same containment. The writable set did
  not widen, no code-count row moved, and no new module, variant, tool or
  guard was added.

- **2026-10-23 — merge with `main` at `6e097dd3a` (fourth reconciliation).**
  Four rows moved, each re-measured against its own listed `Source` in the
  merged tree rather than incremented from either parent. *Integration test
  files* 334 (main) / 330 (branch) → **337**, the union of the two sides'
  members. *Guard scripts (all)* 86 (main) / 84 (branch) → **90**. *CI guard
  scripts* 47 (main) / 46 (branch) → **53**, the union of the two workflow
  files' guard steps — CI remains a strict subset of `quick`, which runs **67**
  guards and runs **14** guards CI does not. *Tool registrations* kept its
  total (67) but changed its **split**, 54 `register` + 13 `register_boxed` →
  **53 `register` + 14 `register_boxed`**, because C009 moved the `commit`
  tool onto the fail-closed `UnrootedTool` wrapper, which registers boxed
  rather than in place. Every other row was confirmed unchanged by
  measurement: architecture docs 92, distinct production tool names 75, tool
  source files 83, LSP server definitions 39, `codegg-core` modules 54,
  `AppEvent` variants 61, `CoreRequest` variants 266, `CoreResponse` variants
  174, `CoreEvent` variants 92, `ProjectionEvent` variants 47, authorized
  operations 260, identity newtypes 19, built-in slash commands 169, built-in
  agents 10, bundled themes 50, TUI state modules 33, database tables 95
  statements / 91 tables, storage layout version 88, Git operation variants 54,
  Git risk classes 11, workspace member crates 11 (+ root).

- **2026-10-23 — merge with `main` at `c2d781319` (fifth reconciliation).**
  `main` advanced again, to the file-picker surface corrective (PR #182).
  Four rows moved, each re-measured against its own listed `Source` in the
  merged tree rather than incremented from either parent. *Integration test
  files* 335 (main) / 337 (branch) → **338**, the union of the two sides'
  members — `main` added `tests/tui_render/file_picker_surface_corrective.inc`.
  *Guard scripts (all)* 87 (main) / 90 (branch) → **91**, the union of the two
  guard sets — `main` added `scripts/check_tui_files_keymap_drift.py`. *CI
  guard scripts* 48 (main) / 53 (branch) → **54**, the union of the two
  workflow files' guard steps — CI remains a strict subset of `quick`, which
  runs **68** guards and runs **14** guards CI does not. `AGENTS.md`'s
  published counts moved with them, 67 quick / 53 CI → **68 quick / 54 CI**.
  Every other row was confirmed unchanged by measurement: architecture docs 92,
  distinct production tool names 75, tool source files 83, LSP server
  definitions 39, `codegg-core` modules 54, `AppEvent` variants 61,
  `CoreRequest` variants 266, `CoreResponse` variants 174, `CoreEvent`
  variants 92, `ProjectionEvent` variants 47, authorized operations 260,
  identity newtypes 19, built-in slash commands 169, built-in agents 10,
  bundled themes 50, TUI state modules 33, database tables 95 statements / 91
  tables, storage layout version 88, Git operation variants 54, Git risk
  classes 11, workspace member crates 11 (+ root). The comment budget was
  re-banked on the merged tree: **123,685** lines, **1,089,562** words, ratio
  **0.1325** — over `main`'s newly banked 122,449 and still below the 124,865
  pre-reduction baseline.

## Source verification

Verified 2026-10-23 against the tree at `12999178a`, by running
`scripts/measure_overview_counts.py` and comparing every row's output to the
`## Verified counts` table in `overview.md`. All 24 rows agree, including the
rows whose methods were pinned above.

Verified 2026-10-23 **again** against the merged tree — `origin/main`
`6e097dd3a` merged with the five post-closure corrective milestones' branch
tip `d5b7efc02` — by running the same script and comparing every row's output
to the same table. All **25** rows agree. The four rows that moved are the
four named in the reconciliation entry above; the other 21 were confirmed
unchanged by measurement rather than by assumption.

Verified 2026-10-23 a **third** time against the merged tree — `origin/main`
`c2d781319` merged with the five post-closure corrective milestones' branch
tip `a5c00ef10` — by running the same script and comparing every row's output
to the same table. All **25** rows agree. The four rows that moved are the
four named in the eighth amendment; the other 21 were confirmed unchanged by
measurement rather than by assumption. The `AGENTS.md` counts were derived the
same way, from `scripts/verify.sh quick`'s `run_guards` block and the CI
workflow's distinct `scripts/check_*` names.

One row's **measure** changed in this pass, and the row's value moved with
it: *Production tool names* was corrected from 61 to **62** after the
cross-check against the catalog table showed a name present in the measured
set but absent from the catalog — `python_script`. The cause was the scope,
not the counting method; see *Production tool names* above. The catalog table
in `overview.md` was corrected to match, and the two sets are now identical
with no name present in one and absent from the other.

### Amendment — 2026-10-23, documentation-surfaces pass

One row's **label** was wrong, and the fix split it into two rows.

The row read `| CI guard scripts | 76 | scripts/check_* |`. The **measure** was
correct — `scripts/check_*` really does hold 76 files — but the label claimed
those were CI's guards. They are not: CI (`.github/workflows/ci.yml`) names
**36** of them. The historical entries above that say "CI guard-script row"
were counting the repository's guard population all along; only the label was
wrong, so every increment they record (68 → 74 → 76) is still correct for
`scripts/check_*`.

The row is now two rows:

- `| Guard scripts (all) | 76 | scripts/check_* |` — the population the
  repository ships.
- `| CI guard scripts | 36 | .github/workflows/ci.yml |` — the subset CI
  actually runs, measured from the workflow file rather than assumed.

`scripts/measure_overview_counts.py` now derives the CI figure by scanning
`.github/workflows/ci.yml` for `scripts/check[-_]…` references, so the row
cannot drift from the workflow again. The relation between the two is a
strict subset with no CI-only guard: quick runs **14** guards CI does not
(client and desktop boundaries, `check_daemon_listing_authority`,
`check_first_party_skills`, `check_network_intent_owner_wiring`,
`check_no_hardcoded_selection_colours`, `check_eggwork_target_routing`,
`check_openai_endpoint_composition`, and six provider guards), which is why a
green CI run does not imply a green `quick`. `AGENTS.md` now carries that
derivation as a `diff` command rather than a hand-copied list.

No other row moved: this pass changed no code, no schema, no tool, and no
module.

### Second amendment — 2026-10-23, tool-name row

One row's **measure** was incomplete, and its value moved with it.

The row read `| Distinct production tool names | 62 |` and the measure scanned
`impl Tool for … name()` for a string literal. That scan cannot see the
`eggsact` deterministic family: `impl Tool for EggsactTool` returns
`self.codegg_name`, so its 13 names live at the `EggsactTool::new()` call
sites in `src/tool/deterministic.rs` instead. Those 13 are real production
tools the model can call — `codegg doctor deterministic-tools` reports them as
**8 always-visible, 5 deferred** — so the published total understated the
production surface by 13.

The row now reads **75** (62 literal + 13 eggsact runtime-named), and the
measure derives the eggsact names from their constructor call sites. Two
runtime-named families were checked and deliberately add nothing: `DisabledTool`
carries the name of the tool it *replaces* (`lsp`, `lsp_read`, `security`), and
`UnrootedTool` forwards its inner tool's name, so neither widens the set.
`PluginToolAdapter` is plugin-contributed and is not part of the built-in
surface.

This is the **third** correction to this row across two passes, and all three
share one root cause: the measure was scoped to where the names *looked* like
they lived rather than to where the tool is actually registered. The lesson is
recorded above and now applies to every count in this table: a count must
measure the thing the row claims to measure, and the row's label must say so
when the measure is narrower than the claim.

Both the `architecture/overview.md` catalog table and `docs/tools.md`'s family
table now name all 75, and the two sets are identical with no name in one and
absent from the other.

### Third amendment — 2026-10-23, repository audit corrective C001

Three rows moved because the milestone added one thing of each kind. No row's
**measure** changed, so no method decision is needed; the values were
re-measured with `scripts/measure_overview_counts.py` and compared to the
table row by row.

| Row | Before | After | What moved it |
|---|---|---|---|
| Integration test files | 327 | 328 | `tests/tui_render/nested_runtime_command_dispatch.inc`, a new `.inc` member of the existing `tui_render` family. The measure (`tests/*.rs` + `tests/*/*.inc`) is unchanged; the family already existed, so no new family directory and no `tests/*.rs` was added. |
| Guard scripts (all) | 80 | 81 | `scripts/check_no_nested_runtime_block_on.py`. The measure (`scripts/check_*`) is unchanged. |
| CI guard scripts | 40 | 41 | The same guard, wired into the hosted CI guard step. The measure (the distinct `scripts/check_*` names named in `.github/workflows/ci.yml`) is unchanged. |

The remaining 21 rows were **confirmed unchanged** by measurement, not by
assumption: architecture docs 92, tool registrations 67 (54 register + 13
register_boxed), distinct production tool names 75, tool source files 83, LSP
server definitions 39, `codegg-core` modules 54, `AppEvent` 61, `CoreRequest`
266, `CoreResponse` 174, `CoreEvent` 92, `ProjectionEvent` 47, authorized
operations 260, identity newtypes 19, built-in slash commands 169, built-in
agents 10, bundled themes 50, TUI state modules 33, database tables 95
statements / 91 tables, storage layout version 88, Git operation variants 54,
Git risk classes 11, and workspace member crates 11 (+ root).

### Fourth amendment — 2026-10-23, repository audit corrective C002 + merge reconciliation

Three rows moved because C002 added one thing of each kind, and the merge of
C001 and C002 needed reconciling. No row's **measure** changed, so no method
decision is needed; the values were re-measured with
`scripts/measure_overview_counts.py` and compared to the table row by row.

| Row | Before | After | What moved it |
|---|---|---|---|
| Integration test files | 327 | 329 | C001's `tests/tui_render/nested_runtime_command_dispatch.inc` and C002's `tests/security_permission/destructive_delete_classification.inc` — a new `.inc` member of each of two existing families. The measure (`tests/*.rs` + `tests/*/*.inc`) is unchanged; neither family directory nor any `tests/*.rs` was added. |
| Guard scripts (all) | 80 | 82 | `scripts/check_no_nested_runtime_block_on.py` (C001) and `scripts/check_destructive_risk_signal_coverage.py` (C002). The measure (`scripts/check_*`) is unchanged. |
| CI guard scripts | 40 | 42 | The same two guards, each wired into the hosted CI guard step. The measure (the distinct `scripts/check_*` names named in `.github/workflows/ci.yml`) is unchanged. |

**Merge reconciliation.** C001 and C002 were developed on sibling branches from
the same base and each re-measured these three rows in isolation, so each
recorded its own single increment. `architecture/overview.md` merged without a
conflict — the two edits landed in different hunks — and the merged table
carried **one** increment where two were owed. The row values above are the
canonical measurement of the merged tree (`329` / `82` / `42`); the count rows
are the only place the merge lost information, and no other file's content was
affected.

The remaining 21 rows were **confirmed unchanged** by measurement, not by
assumption: architecture docs 92, tool registrations 67 (54 register + 13
register_boxed), distinct production tool names 75, tool source files 83, LSP
server definitions 39, `codegg-core` modules 54, `AppEvent` 61, `CoreRequest`
266, `CoreResponse` 174, `CoreEvent` 92, `ProjectionEvent` 47, authorized
operations 260, identity newtypes 19, built-in slash commands 169, built-in
agents 10, bundled themes 50, TUI state modules 33, database tables 95
statements / 91 tables, storage layout version 88, Git operation variants 54,
Git risk classes 11, and workspace member crates 11 (+ root).

### Fifth amendment — 2026-10-23, repository audit corrective C004 + merge reconciliation

Three rows moved because C004 added one thing of each kind, and the merge of
C004 needed reconciling. No row's **measure** changed, so no method decision is
needed; the values were re-measured with `scripts/measure_overview_counts.py`
and compared to the table row by row.

| Row | Before | After | What moved it |
|---|---|---|---|
| Integration test files | 329 | 331 | `tests/core_daemon/job_state_terminality_c004.inc` and `tests/scheduler_jobs/job_status_projection_c004.inc` — a new `.inc` member of each of two existing families. The measure (`tests/*.rs` + `tests/*/*.inc`) is unchanged; neither family directory nor any `tests/*.rs` was added, so the root package's **test-target** count did not move. |
| Guard scripts (all) | 82 | 83 | `scripts/check_job_state_terminality_agreement.py`. The measure (`scripts/check_*`) is unchanged. |
| CI guard scripts | 42 | 43 | The same guard, wired into `scripts/verify.sh quick` and `.github/workflows/ci.yml`; a guard's two `ci.yml` steps — the guard and its `--self-test` — are one distinct script. The measure is unchanged. |

**Merge reconciliation.** C004 was developed on a sibling branch from the same
base and re-measured these three rows in isolation, so it recorded its own
single increment against the pre-merge base. `architecture/overview.md`
conflicted on the two guard rows and on the tree comment; the rows were
re-measured on the merged tree and the tree comment was corrected to the
canonical **331**. The paragraph that summarises the count movement was
merged into one statement covering C001, C002 and C004 rather than left as
three competing versions.

The remaining 21 rows were **confirmed unchanged** by measurement, not by
assumption: architecture docs 92, tool registrations 67 (54 register + 13
register_boxed), distinct production tool names 75, tool source files 83, LSP
server definitions 39, `codegg-core` modules 54, `AppEvent` 61, `CoreRequest`
266, `CoreResponse` 174, `CoreEvent` 92, `ProjectionEvent` 47, authorized
operations 260, identity newtypes 19, built-in slash commands 169, built-in
agents 10, bundled themes 50, TUI state modules 33, database tables 95
statements / 91 tables, storage layout version 88, Git operation variants 54,
Git risk classes 11, and workspace member crates 11 (+ root). In particular the
`codegg-core` modules row is still **54**: C004's new
`codegg-core/src/jobs/evidence.rs` is a **sub**module of `jobs`, not a
top-level `pub mod` in the crate's `lib.rs`.
### Sixth amendment — 2026-10-23, repository audit corrective C005 + merge reconciliation

Two rows moved, and both moved because this milestone **added** something
rather than because a measure was wrong.

| Row | Before | After | Why |
|-----|--------|-------|-----|
| CI guard scripts | 40 | **41** | `scripts/check_tool_broker_boundary.py` was repaired and wired into the CI guard step. The guard file already existed, so the *Guard scripts (all)* row (80) is unchanged — the row counts files, and this milestone changed a file's contents rather than adding one. |
| Integration test files | 327 | **329** | Two new root integration family members: `tests/security_permission/reviewer_broker_boundary.inc` and `tests/tui_render/dialog_dispatch_coverage.inc`. The `tests/common/` helper tree is still excluded, and neither member adds a standalone target. |

Both numbers were re-measured with `python3 scripts/measure_overview_counts.py`
against this tree, each against its own listed `Source`; the measurement
agrees with the published values.

No other row moved: this milestone changed no schema, no tool registration,
no module, and no storage layout. `Storage layout version` stays **88**, and
the "quick runs 14 guards CI does not" note in the `CI guard scripts` row is
unchanged — the broker-boundary guard is now in **both** `quick` and CI, so it
adds one guard to each and does not change the size of the quick-only set.

The measurement was taken on the C005 branch, before the sibling corrective
milestones (C001, C002) are merged. Those two branches re-measured the same
table on their own trees, so this row set is expected to conflict at merge
time; the reconciliation is the union of each side's contribution, re-measured
in the merged tree rather than carried over from either side.

**Merge reconciliation.** C005 was developed on a sibling branch from the same
base and re-measured these rows in isolation, so it recorded its own increments
against the pre-merge base. Its amendment is appended here as the **sixth**,
after the C001, C002 and C004 amendments, and its row values were re-measured
in the **merged** tree. C005 added **two** `.inc` members of existing families
(`tests/security_permission/reviewer_broker_boundary.inc` and
`tests/tui_render/dialog_dispatch_coverage.inc`) and repaired the existing
`scripts/check_tool_broker_boundary.py` in place — the row counts files, so
the *Guard scripts (all)* row did not move, while the *CI guard scripts* row
did, because the repaired guard is now wired into the CI guard step for the
first time.

## 2026-10-23 — C006, the TUI render path's cost

C006 added one test member and one guard, so three rows of the
`architecture/overview.md` count table moved. Every row was re-measured with
`python3 scripts/measure_overview_counts.py` against its own listed
`Source`, not incremented by hand.

| Row | Before | After | What moved it |
|---|---|---|---|
| Integration test files | 331 | 332 | `tests/tui_render/tui_render_cost.inc` — a new `.inc` member of an **existing** family, so the root package's **test-target** count did not move. The measure (`tests/*.rs` + `tests/*/*.inc`) is unchanged. |
| Guard scripts (all) | 84 | 85 | `scripts/check_tui_render_cost.py`. The measure (`scripts/check_*`) is unchanged. |
| CI guard scripts | 44 | 45 | The same guard, wired into `scripts/verify.sh quick` and `.github/workflows/ci.yml`; a guard's two `ci.yml` steps — the guard and its `--self-test` — are one distinct script. The measure is unchanged. |

The remaining 21 rows were **confirmed unchanged** by measurement, not by
assumption: architecture docs 92, tool registrations 67 (54 register + 13
register_boxed), distinct production tool names 75, tool source files 83, LSP
server definitions 39, `codegg-core` modules 54, `AppEvent` 61, `CoreRequest`
266, `CoreResponse` 174, `CoreEvent` 92, `ProjectionEvent` 47, authorized
operations 260, identity newtypes 19, built-in slash commands 169, built-in
agents 10, bundled themes 50, TUI state modules 33, database tables 95
statements / 91 tables, storage layout version 88, Git operation variants 54,
Git risk classes 11, and workspace member crates 11 (+ root).

**Merge reconciliation.** C006 was developed on a sibling branch from the same
base as C001–C004 and re-measured these three rows in isolation, so it
recorded its own single increment against the pre-merge base. The merged tree
carries C005 as well, which adds its own guard and its own test member, so the
merged values are one higher on two of the three rows than the values this
record lists: integration test files **334**, guard scripts (all) **85** and
CI guard scripts **46**. (The *guard scripts* row is one higher, not two: C005
**repaired** the existing `scripts/check_tool_broker_boundary.py` in place
rather than adding a file, so that row counts files and C005 did not move it.) The rows are re-measured on the merged tree when it
is cut, rather than carried over from either side.
## C007 — comment corpus reduction and drift guard (2026-10-23)

| Row | Before | After | Why it moved |
|---|---|---|---|
| Guard scripts (all) | 84 | **85** | `scripts/check_comment_budget.py` is a new guard. It is wired into `scripts/verify.sh quick` and into the CI guard step. |
| CI guard scripts | 45 | **46** | The new guard's two CI steps (the `--self-test` and the guard itself) are one guard script, so the *CI guard scripts* row counts the script once, not the steps. |

Both numbers were re-measured with `python3 scripts/measure_overview_counts.py`
against this tree, each against its own listed `Source`. No other row moved:
C007 changed no schema, no tool registration, no module, no storage layout,
and it added no integration test file (the guard's `--self-test` is the
coverage, by the milestone plan's own placement decision).

The *quick runs 14 guards CI does not* note in the `CI guard scripts` row is
unchanged: the new guard is in **both** `quick` and CI, so it adds one guard
to each and does not change the size of the quick-only set.

The measurement was taken on the C007 branch, before the sibling corrective
milestones (C001–C006) are merged. Those branches re-measured the same table
on their own trees, so this row set is expected to conflict at merge time; the
reconciliation is the union of each side's contribution, re-measured in the
merged tree rather than carried over from either side.

### Seventh amendment — 2026-10-23, M005 writer containment corrective, fourth reconciliation with `main`

Four rows moved, each re-measured against its own listed `Source` in the
merged tree rather than incremented from either parent:

| Row | Before | After | What moved it |
|---|---|---|---|
| Integration test files | 334 | **337** | The union of `main`'s and the corrective branch's `.inc` members. The measure (`tests/*.rs` + `tests/*/*.inc`, with the `tests/common/` helper tree excluded) is unchanged. |
| Guard scripts (all) | 86 | **90** | The union of the two guard sets. The measure (`scripts/check_*`) is unchanged. |
| CI guard scripts | 47 | **53** | The union of the two workflow files' distinct `scripts/check_*` names. The measure is unchanged, and CI remains a strict subset of `quick`. |
| Tool registrations | 67 (54 + 13) | **67 (53 + 14)** | The **split** moved; the total did not. C009 moved the `commit` tool onto the fail-closed `UnrootedTool` wrapper, which registers boxed rather than in place. The measure is unchanged. |

The remaining 21 rows were **confirmed unchanged** by measurement, not by
assumption: architecture docs 92, distinct production tool names 75, tool
source files 83, LSP server definitions 39, `codegg-core` modules 54,
`AppEvent` 61, `CoreRequest` 266, `CoreResponse` 174, `CoreEvent` 92,
`ProjectionEvent` 47, authorized operations 260, identity newtypes 19,
built-in slash commands 169, built-in agents 10, bundled themes 50, TUI state
modules 33, database tables 95 statements / 91 tables, storage layout version
88, Git operation variants 54, Git risk classes 11, and workspace member
crates 11 (+ root).

The *quick runs 14 guards CI does not* note in the `CI guard scripts` row is
unchanged: the merged tree's `quick` runs **67** guards, CI runs **53**, and
the difference is the same 14 — the client and desktop boundaries, the
daemon-listing authority, the first-party skills, the network-intent owner,
the selection colours, the Eggwork target routing, the OpenAI endpoint
composition, and the six provider guards.

## M002 — the `/files` keymap drift guard (2026-10-23)

| Row | Before | After | Why it moved |
|---|---|---|---|
| Guard scripts (all) | 86 | **87** | `scripts/check_tui_files_keymap_drift.py` is a new guard. It is wired into `scripts/verify.sh quick` and into the CI guard step. |
| CI guard scripts | 47 | **48** | The new guard's two CI steps (the `--self-test` and the guard itself) are one guard script, so the *CI guard scripts* row counts the script once, not the steps. |

Both numbers were re-measured with `python3 scripts/measure_overview_counts.py`
against this tree, each against its own listed `Source`. No other row moved:
this milestone changed no schema, no tool registration, no module and no
storage layout, and it added **no** integration test file — the new tests
extend the M001 member `tests/tui_render/file_picker_surface_corrective.inc`,
which the *Integration test files* row already counted at 335.

The *quick runs 14 guards CI does not* note in the `CI guard scripts` row is
unchanged: the new guard is in **both** `quick` and CI, so it adds one guard
to each and does not change the size of the quick-only set.

**A stale number this record also corrects.** `AGENTS.md` carried **45** CI
guard scripts where the measured value was already **47** before this
milestone: two CI guards had been added without the prose count following
them. The count in `AGENTS.md` is now the measured one (**62** for `quick`,
**48** for CI), and the 14-guard difference the same file publishes still
holds (62 − 48 = 14). The *method* did not change — the count is still the
number of distinct `scripts/check_*` files the two files name — only the
value had drifted.

### Eighth amendment — 2026-10-23, M005 writer containment corrective, fifth reconciliation with `main`

`origin/main` advanced again, to `c2d781319` (PR #182, the file-picker
surface corrective). Four rows moved, each re-measured against its own listed
`Source` in the merged tree rather than incremented from either parent:

| Row | Before | After | What moved it |
|---|---|---|---|
| Integration test files | 337 | **338** | The union of the two sides' `.inc` members: `main` added `tests/tui_render/file_picker_surface_corrective.inc`. The measure (`tests/*.rs` + `tests/*/*.inc`, with the `tests/common/` helper tree excluded) is unchanged. |
| Guard scripts (all) | 90 | **91** | The union of the two guard sets: `main` added `scripts/check_tui_files_keymap_drift.py`. The measure (`scripts/check_*`) is unchanged. |
| CI guard scripts | 53 | **54** | The union of the two workflow files' distinct `scripts/check_*` names. The measure is unchanged, and CI remains a strict subset of `quick`. |
| `AGENTS.md`'s published counts | 67 quick / 53 CI | **68 quick / 54 CI** | The same union, counted in `scripts/verify.sh quick`'s `run_guards` rather than in the two files directly. The *quick runs 14 guards CI does not* difference is unchanged at 14. |

The remaining 21 rows were **confirmed unchanged** by measurement, not by
assumption: architecture docs 92, distinct production tool names 75, tool
source files 83, LSP server definitions 39, `codegg-core` modules 54,
`AppEvent` 61, `CoreRequest` 266, `CoreResponse` 174, `CoreEvent` 92,
`ProjectionEvent` 47, authorized operations 260, identity newtypes 19,
built-in slash commands 169, built-in agents 10, bundled themes 50, TUI state
modules 33, database tables 95 statements / 91 tables, storage layout version
88, Git operation variants 54, Git risk classes 11, and workspace member
crates 11 (+ root).

**The comment budget was re-banked on the merged tree.** The corpus is a
ratchet: `main`'s file-picker corrective freed comment lines and the branch
spends its own. The merged tree measures **123,685** comment lines, **1,089,562**
words and a **0.1325** ratio — over `main`'s newly banked ceiling of 122,449
and still **below** the 124,865 pre-reduction baseline, which is the same
relation the fourth reconciliation recorded.
## C003 — the composer's keymap drift guard (2026-10-24)

| Row | Before | After | Why it moved |
|---|---|---|---|
| Guard scripts (all) | 87 | **88** | `scripts/check_tui_composer_keymap_drift.py` is a new guard. It is wired into `scripts/verify.sh quick` and into the CI guard step. |
| CI guard scripts | 48 | **49** | The new guard's two CI steps (the `--self-test` and the guard itself) are one guard script, so the *CI guard scripts* row counts the script once, not the steps. |

Both numbers were re-measured with `python3 scripts/measure_overview_counts.py`
against this tree, each against its own listed `Source`: **88** for
`scripts/check_*` and **49** for the distinct guard scripts `.github/workflows/ci.yml`
names. The guard script itself was written before this milestone and was already
present in the tree, so the *Guard scripts (all)* row had already moved to 88
before the milestone's production change; what this milestone adds is the
registration that makes the row's CI half move with it.

No other row moved: this milestone changed no schema, no tool registration, no
module and no storage layout, and it added **no** integration test file — the
new tests extend the existing members `tests/tui_render/tui_composer_claims.inc`
and `tests/tui_render/tui_composer_motions.inc`, which the *Integration test
files* row already counted at 335. The *Architecture docs* row stays at 92: this
milestone edits existing documents and adds no new one under `architecture/`.

The *quick runs 14 guards CI does not* note in the `CI guard scripts` row is
unchanged: the new guard is in **both** `quick` and CI, so it adds one guard to
each and does not change the size of the quick-only set. The set was re-derived
rather than carried over, by differencing the guards `run_guards()` names
against the guards `.github/workflows/ci.yml` names: **63** in `quick`, **49**
in CI, and the same 14 quick-only guards as before.

**A stale number outside this table.** `AGENTS.md` publishes "62 guard scripts"
for `quick` and "48 guard scripts" for CI. The measured values are now **63**
and **49**. The *method* did not change — the count is still the number of
distinct `scripts/check_*` files the two files name — only the value moved, and
`AGENTS.md` is outside the milestone's file ownership, so the correction is
recorded here rather than made there.

### Ninth amendment — 2026-10-24, M005 writer containment corrective, sixth reconciliation with `main`

`origin/main` advanced again, to `a4bbba81e` (PR #183, the composer's second
post-closure corrective). Four rows moved, each re-measured against its own
listed `Source` in the merged tree rather than incremented from either parent:

| Row | Before | After | What moved it |
|---|---|---|---|
| Guard scripts (all) | 91 | **92** | The union of the two guard sets: `main` added `scripts/check_tui_composer_keymap_drift.py`. The measure (`scripts/check_*`) is unchanged. |
| CI guard scripts | 54 | **55** | The union of the two workflow files' distinct `scripts/check_*` names. The measure is unchanged, and CI remains a strict subset of `quick`. |
| `AGENTS.md`'s published counts | 68 quick / 54 CI | **69 quick / 55 CI** | The same union, counted in `scripts/verify.sh quick`'s `run_guards` rather than in the two files directly. The *quick runs 14 guards CI does not* difference is unchanged at 14. |
| Integration test files | 338 | **338** | Unchanged, and this is worth stating rather than leaving implicit: `main`'s composer corrective added **no** integration test file. It extended the existing members `tests/tui_render/tui_composer_claims.inc` and `tests/tui_render/tui_composer_motions.inc`, and it **rewrote** `tests/tui_render/tui_composer_selection.inc`, which already existed at the merge base — a new-file line in a diff stat is not a new file. The row was re-measured at 338 in the merged tree, not carried over from either parent, and the two sides' file sets were differenced to confirm the union. |

The remaining 21 rows were **confirmed unchanged** by measurement, not by
assumption: architecture docs 92, distinct production tool names 75, tool
source files 83, LSP server definitions 39, `codegg-core` modules 54,
`AppEvent` 61, `CoreRequest` 266, `CoreResponse` 174, `CoreEvent` 92,
`ProjectionEvent` 47, authorized operations 260, identity newtypes 19,
built-in slash commands 169, built-in agents 10, bundled themes 50, TUI state
modules 33, database tables 95 statements / 91 tables, storage layout version
88, Git operation variants 54, Git risk classes 11, and workspace member
crates 11 (+ root). The **Tool registrations** row is unchanged too — still
**67**, still **53 `register` + 14 `register_boxed`** — and it is named here
rather than left implicit, because this is the row the fourth reconciliation
had to qualify with a split note, and a reader should not have to carry that
note forward to know the split still holds.

**`AGENTS.md`'s stale count is now corrected at the source.** The C003
amendment above recorded that `AGENTS.md` published "62 guard scripts" for
`quick` and "48 guard scripts" for CI while the measured values were 63 and
49, and deferred the correction because that file was outside that milestone's
ownership. This reconciliation owns the file, so the correction is made there:
`AGENTS.md` now publishes the merged tree's measured **69** for `quick` and
**55** for CI. The *method* is unchanged — the count is still the number of
distinct `scripts/check_*` files the two files name — and the 14-guard
difference still holds (69 − 55 = 14). The quick-only set was **re-derived**,
not carried over, by differencing the guards `run_guards()` names against the
guards `.github/workflows/ci.yml` names; it is the same 14 as before, because
the composer's new guard is in **both** surfaces.

**The comment budget was re-banked on the merged tree.** The corpus is a
ratchet: `main`'s composer corrective spent comment lines and the branch spends
its own. The merged tree measures **124,310** comment lines, **1,095,646**
words and a **0.1330** ratio — over the branch's prior ceiling of 123,805 and
still **below** the 124,865 pre-reduction baseline, which is the same relation
the fourth and fifth reconciliations recorded. The re-bank raised **only** the
global block: no per-file ceiling moved, because `main` had already banked the
composer's files and the branch's own files were already banked at their
current values.


## 2026-10-24 — C005's diff-consumer runtime target

This is **not** a reconciliation: `main` did not advance, and no row moved
because of an advance. One row moved because of a change on this line — the
standalone integration target `tests/git_consistency_consumers_c005.rs`, which
closes the Linux sandbox C005 closure record's last open finding.

*Integration test files* moved **338 → 339**, re-measured on the tree with
`python3 scripts/measure_overview_counts.py` rather than incremented. The
other 24 rows were confirmed unchanged by measurement. The new target is a
root-package integration test file, so the measure is unchanged; what changed
is the population it counts.

Two things moved with the row and are recorded here because both are gates
rather than prose:

- **`docs/test-ownership.toml` gained the `root_git_consistency_consumers_c005`
  group.** Without it `scripts/check_test_ownership.py` fails with *"test
  target codegg/git_consistency_consumers_c005 exists but no group owns it"* —
  an unowned target can never be selected by a scoped run, so the guard's
  failure is the point rather than an obstacle.
- **The comment budget was re-banked on this tree.** The new target crosses
  the unlisted floor, so it is listed in `docs/comment-budget.toml` with its
  measured ceiling. On the tree this section describes, the corpus measured
  **124,362** comment lines — **below** the 124,865 pre-reduction baseline,
  the same relation the fourth, fifth and sixth reconciliations recorded.

  That relation **does not survive the seventh reconciliation**, and the
  tenth amendment below says so with the numbers. `main`'s subsequent work —
  the desktop parity and M008 lines — took its own tree to **125,161**
  comment lines, which is already **above** the 124,865 reference, and the
  merged tree measures **125,212**. The baseline is a historical reference
  from the C007 comment-reduction workstream, not a guard: no script, the
  budget file, or the CI workflow enforces it. What the ratchet *does*
  enforce is that no recorded ceiling may be exceeded, and the re-bank records
  the merged tree's measurement exactly as measured.

### Tenth amendment — 2026-10-24, M005 writer containment corrective, seventh reconciliation with `main`

`origin/main` advanced again, to `4aa07fa72`, which carried PRs #184, #187, #188, #189 and #190 — the delegated-run line's reconciliation, the desktop parity merge, the M007-A desktop gate guard, and the managed-agent-scratch M008 corrective. The branch had to be reconciled with it, and two files conflicted.

Three rows moved, each re-measured against its own listed `Source` in the merged tree rather than carried over from either parent:

| Row | Before | After | What moved it |
|---|---|---|---|
| Integration test files | 340 (main) / 339 (branch) | **341** | The union. `main` added `tests/frontend_parity/tui_reconstruction.inc` and the managed-agent-scratch tree's `tests/scheduler_jobs/scratch_record_less_tree.inc`; this branch added `tests/git_consistency_consumers_c005.rs`. |
| Guard scripts (all) | 93 (main) / 92 (branch) | **94** | The union. `main` added `scripts/check_scratch_reclaim_invariants.py` and `scripts/check_desktop_gate_wiring.py`; this branch added none. |
| CI guard scripts | 56 (main) / 55 (branch) | **57** | The union of the two workflows' distinct `scripts/check_*` names. Both of `main`'s new guards run in **both** `quick` and CI, so the *quick runs 14 guards CI does not* difference is unchanged at **14**, and the *quick* guard count is **71**. |

`AGENTS.md`'s published counts moved with them — **71 quick / 57 CI** — and were re-derived from `scripts/verify.sh quick`'s `run_guards` block and the CI workflow, not carried over. The quick lane's `==>` lines moved from 113 to **117** (`run_guards` **113**, `run_quick` **4**), because `main` added guard steps and their self-tests.

Every other row was **confirmed unchanged** by measurement, not by assumption: architecture docs 92, distinct production tool names 75, tool source files 83, LSP server definitions 39, `codegg-core` modules 54, `AppEvent` variants 61, `CoreRequest` variants 268 (242 struct + 26 unit), `CoreResponse` variants 176 (175 struct + 1 unit), `CoreEvent` variants 92, `ProjectionEvent` variants 47, authorized operations 262, identity newtypes 19, built-in slash commands 169, built-in agents 10, bundled themes 50, TUI state modules 33 (34 `.rs` files incl. `mod.rs`), database tables 95 statements / 91 tables, storage layout version 88, Git operation variants 54, Git risk classes 11, and workspace member crates 11 (+ root). The **Tool registrations** row is unchanged too — still **67**, still **53 `register` + 14 `register_boxed`** — and it is named here rather than left implicit, because this is the row the fourth reconciliation had to qualify with a split note.

**The comment budget's relation to the pre-reduction baseline changed, and it is worth stating plainly.** The corpus is a ratchet — no recorded ceiling may be exceeded — and the merged tree measures **125,212** comment lines, **1,103,943** words and a **0.1337** ratio, which the re-bank records exactly as measured.

The **124,865** figure is the pre-reduction corpus from the C007 comment-reduction workstream. It is a **historical reference, not a guard**: no script, no entry in `docs/comment-budget.toml`, and no CI step enforces it. The corpus crossed it before this merge — `main`'s own tree banked **125,161** before this change, already **296 lines above** the reference — so this merge did not cross it and did not cause it. The fourth, fifth, sixth and ninth amendments each recorded the corpus *below* the baseline because it *was* below on those trees; that relation simply no longer holds now that `main`'s subsequent lines have spent more comment lines than the reduction freed. Recording it as still-below would be false, so it is recorded as measured.

**`main` and this branch share the fence.** `main`'s `2222697d6` — *"carry a git read's consistency class to the client"* — was written against the guard this line merged, and it **propagates** the Phase 3 git read's class rather than admitting it, because the desktop's git read is a display read rather than a generation-bound consumer. Its commit message notes that the desktop's status and branch reads consult refs and the index rather than the object store, so their composite class is `NoObjectGeneration` — a class the fence refuses outright. That is the same refusal the new standalone integration target asserts from outside the crate, and the two agree.

### Eleventh amendment — 2026-10-24, M005 writer containment corrective, eighth reconciliation with `main`

`origin/main` advanced again, to `6e8a6bb60`, which carried PRs #185 and #191 — the desktop chat region and side panel (M007-B) and the panel layer's authority guard. Nothing conflicted textually, but the auto-merge produced **two** copies of the three count rows in `architecture/overview.md` — this branch's and `main`'s — which is the shape a count table takes when both sides moved the same rows without the same values. The duplicate was collapsed to one row set carrying the merged tree's measurement.

Two rows moved, each re-measured against its own listed `Source` in the merged tree rather than carried over from either parent:

| Row | Before | After | What moved it |
|---|---|---|---|
| Guard scripts (all) | 94 | **95** | `main` added `scripts/check_desktop_panel_authority.py`. The measure (`scripts/check_*`) is unchanged. |
| CI guard scripts | 57 | **58** | The CI workflow gained the same guard and its self-test. The measure is unchanged. |
| Integration test files | 341 | **341** | Unchanged, and worth stating rather than leaving implicit: `main`'s M007-B work added no integration test file. |

The *quick* guard count moved **71 → 72**, and the *quick runs 14 guards CI does not* difference is unchanged at **14**: the new guard runs in **both** `quick` and CI, so it adds no quick-only entry. The quick lane's `==>` lines moved from 117 to **119** (`run_guards` **115**, `run_quick` **4**). `AGENTS.md`'s published counts moved with them, to **72 quick / 58 CI**, and were re-derived from `scripts/verify.sh quick`'s `run_guards` block and the CI workflow rather than carried over.

Every other row was **confirmed unchanged** by measurement, not by assumption: architecture docs 92, distinct production tool names 75, tool source files 83, LSP server definitions 39, `codegg-core` modules 54, `AppEvent` variants 61, `CoreRequest` variants 268 (242 struct + 26 unit), `CoreResponse` variants 176 (175 struct + 1 unit), `CoreEvent` variants 92, `ProjectionEvent` variants 47, authorized operations 262, identity newtypes 19, built-in slash commands 169, built-in agents 10, bundled themes 50, TUI state modules 33 (34 `.rs` files incl. `mod.rs`), database tables 95 statements / 91 tables, storage layout version 88, Git operation variants 54, Git risk classes 11, and workspace member crates 11 (+ root). The **Tool registrations** row is unchanged too — still **67**, still **53 `register` + 14 `register_boxed`** — and it is named here rather than left implicit, because this is the row the fourth reconciliation had to qualify with a split note.

**The comment corpus did not move, and the relation to the 124,865 reference did not change.** The merged tree still measures **125,212** comment lines, **1,103,943** words and a **0.1337** ratio; the re-bank moved no per-file ceiling, because `main`'s new commits added no Rust comment lines. That is still **above** the 124,865 pre-reduction corpus, which the tenth amendment already recorded: the figure is a historical reference, not a guard, and the corpus crossed it before this line's merges.
