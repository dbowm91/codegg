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
calls inside `ToolRegistry::with_options()`'s **body** (54 + 13 = 67) — not
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

## Source verification

Verified 2026-10-23 against the tree at `12999178a`, by running
`scripts/measure_overview_counts.py` and comparing every row's output to the
`## Verified counts` table in `overview.md`. All 24 rows agree, including the
rows whose methods were pinned above.

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
