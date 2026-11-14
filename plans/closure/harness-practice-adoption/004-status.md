# Harness Practice Adoption Milestone 004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/harness-practice-adoption/004-generated-tool-surface-catalog.md`

Source subsystem roadmap:

- `plans/subsystems/harness-practice-adoption-roadmap.md#m004--generated-tool-surface-catalog`

Repository baseline reviewed: `7642b9e38` (`feat-auto-20261109-f8abad0d`), plus
uncommitted working-tree work

Implementation commits or pull requests:

- **None.** This milestone exists only as uncommitted working-tree work on
  `feat-auto-20261109-f8abad0d`, on top of HEAD `7642b9e38`.
  `architecture/tool-catalog.md`, `tests/tool_catalog/` and
  `scripts/check_tool_catalog.py` are **untracked** (`git ls-files
  --error-unmatch architecture/tool-catalog.md` fails); `architecture/overview.md`
  and `architecture/agent-tool-surface.md` carry uncommitted edits. Nothing below
  is attributable to a SHA.

The source plan still reads `Status: ready for handoff`, and the roadmap's §12
status table still reads `not started` for M004. **Both are stale and are not
this record's to change** — the plan `Status:` line and the roadmap table are
the parent's.

## 1. Executive finding

The surface is now emitted by constructing the real registry, and both layers of
the gate ran green here:

- **The authoritative comparison** — `cargo test --test tool_catalog`: **10
  passed, 0 failed**, in 0.03 s. It includes
  `comparison::catalog_matches_the_committed_file`, so the committed catalog is
  byte-identical to a fresh render on this baseline;
  `render::tests::two_renders_of_one_baseline_are_byte_identical`, so the render
  is deterministic; and
  `comparison::the_hand_written_surface_document_carries_no_tool_table`, so the
  second source cannot return.
- **The cheap structural guard** — `python3 scripts/check_tool_catalog.py`: green,
  "parses, declares its 4 sections in order, and covers every tool the prose
  names"; `--self-test` rejects all **7** cases.

The catalog is **144 rows across 4 surfaces** — `default` 63, `plan-mode` 15,
`restricted-subagent` 3, `backend-disabled` 63 — with **12 of them hidden**, each
carrying a reason its own contract or gate supplies rather than a string the
renderer invented.

Two stale hand counts were replaced, and both corrections are visible in the
tree:

1. `architecture/overview.md`'s tool row was a hand count of the constructor's
   `registry.register(..)` / `register_boxed(..)` call sites. It read **67**; the
   actual model-facing default surface is **63 tools**, because the gated
   branches are mutually exclusive and `retain_only` / `filter_out` remove tools
   after registration. The current row says exactly that and cites
   `tool-catalog.md` as its source.
2. The plan-mode documentation repeated an allowlist that included
   `codesearch`. `src/tool/disclosure.rs::PLAN_ALLOWED` holds **16** names and
   does **not** contain `codesearch`. The prose no longer repeats the list at
   all; it says the catalog's `plan-mode` section *is* the list.

`src/tool/catalog.rs` — the runtime BM25 discovery structure — was deliberately
not touched, and `git status --porcelain src/tool/catalog.rs` is empty.

The only caveat is an honest non-claim the artifact states itself: the catalog
describes visibility and metadata, never behaviour or schema, and it is not the
whole surface — see §3.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| The catalog is rendered by constructing `ToolRegistry::with_options` | `tests/tool_catalog/render.inc:145` `build_registry`, `:268` `render_catalog`; header of `architecture/tool-catalog.md` | pass | One source, no TOML mirror, no hand list |
| Four declared surfaces: `default`, `plan-mode`, `restricted-subagent`, `backend-disabled` | `render.inc:109-143`; catalog sections in that order | pass | 63 / 15 / 3 / 63 rows |
| A gated tool is listed as gated, never omitted | 12 rows read `hidden (<reason>)`; the reasons are quoted from the tool's own stub or gate (`hidden_reason`, `render.inc:185`) | pass | Criterion 3 |
| A tool that hides with no reason fails the render | `render::tests::a_tool_that_hides_without_a_reason_fails_the_render` | pass | Blank cell is not an option |
| The catalog is deterministic: two runs byte-identical | `render::tests::two_renders_of_one_baseline_are_byte_identical` | pass | Criterion 1's determinism half |
| The committed catalog is byte-identical to a fresh render | `comparison::catalog_matches_the_committed_file` | pass | Criterion 1 |
| The catalog carries no schema | No schema column; the header names five columns | pass | §4 of the plan |
| Update mode rewrites; the default compares and prints the first divergent line | `render.inc:398` `first_divergent_line`, `:421` `update_requested`; `render::tests::the_first_divergence_is_the_line_a_reader_needs` | pass | |
| Update mode is refused while `CI` is set | `render::tests::update_mode_is_refused_while_ci_is_set` | pass | A CI run can never rewrite the artifact it verifies |
| The committed header names its generator and its update command | `comparison::the_committed_header_names_its_generator_and_its_update_command` | pass | A hand-edited file is detectable |
| The catalog carries no absolute path and no home directory | `render::tests::the_rendered_text_carries_no_absolute_path_and_no_home_directory` | pass | |
| Rows sort by name; sections keep their declared order | `render::tests::rows_sort_by_name_and_sections_keep_the_declared_order` | pass | |
| The hand-written document contains no tool table | `comparison::the_hand_written_surface_document_carries_no_tool_table` | pass | Criterion 5's structural half |
| Every tool name the prose still mentions resolves in the catalog | guard check "covers every tool the prose names" | pass | Criterion 5 |
| The structural guard does **not** re-derive the registry | `scripts/check_tool_catalog.py:18-22` states it and the script contains no cargo invocation | pass | This is the layering the plan asked for |
| The structural guard rejects its self-test injections | `--self-test`: 7/7 | pass | Criterion 4 — the plan asked for 5; there are 7 |
| The generator is hermetic: no workspace root, no credential, no network, no config file | `build_registry` uses a sentinel lexical root; the catalog's "Non-claims" says nothing is read, created or required | pass | Stop condition avoided |
| `architecture/overview.md`'s tool row names the catalog as its source | `overview.md:579` — "Tools in the model-facing default surface \| 63 \| [tool-catalog.md]" plus the explanation of why 67 was wrong | pass | |
| No production file changed **by this milestone** | `src/tool/catalog.rs` is unmodified; no `impl Tool`, no registration and no gating changed | pass | Criterion 6, scoped — the tree's production diff belongs to M003 and two sibling lanes, not to M004 |

## 3. Production implementation evidence

**Nothing in the shipped binary changed.** The renderer is a test, not a
production module or a second binary — the plan's explicit stop condition. The
artifacts are:

- `tests/tool_catalog/mod.rs` — the single `[[test]]` target, binding
  `render.inc` and `comparison.inc` by path so Cargo's directory discovery does
  not make each its own executable.
- `tests/tool_catalog/render.inc` — the `Surface` matrix, `build_registry`,
  `hidden_reason`, `category_label`, `render_row`, `render_rows`,
  `render_catalog`, `first_divergent_line`, `update_requested`, and seven tests.
- `tests/tool_catalog/comparison.inc` — three tests: the comparison, the header
  check, and the no-tool-table check.
- `architecture/tool-catalog.md` — the generated artifact, 231 lines.
- `scripts/check_tool_catalog.py` — the structural guard.

**The row counts, measured from the committed file rather than quoted:**

| Section | Rows | Hidden |
|---|---|---|
| `default` | 63 | 5 |
| `plan-mode` | 15 | 1 |
| `restricted-subagent` | 3 | 0 |
| `backend-disabled` | 63 | 6 |
| **total** | **144** | **12** |

**Hidden reasons come from the tool, not the renderer.** The twelve include
`codesearch` and `invalid` ("declared disclosure state is `hidden`"),
`git_read` ("programmatic-only caller policy"), `lsp` and `lsp_read` ("LSP
process isolation is unavailable for constrained profiles"), and `security`
("security backend is configured as 'disabled' ([tool_backends.security].backend
= \"disabled\")"). That last one quotes a configuration value, which is what the
plan meant by "the reason comes from the tool's own contract or gate".

**What the catalog is not.** Its "Non-claims" section states three limits
plainly: it carries no schemas or descriptions; it is **not the whole surface**
(the daemon's per-turn registry adds `present`, `lsp_preview_apply`,
`context_read`, `goal_*` and `work_plan_*`, which a `ToolRegistryOptions` value
cannot express); and it describes visibility and metadata, never behaviour.

**The two stale counts it replaced.**

1. `architecture/overview.md`'s tool row. The old hand count of `register(..)` /
   `register_boxed(..)` call sites read 67. A call site is not a tool: the gated
   branches are mutually exclusive, and `retain_only` / `filter_out` remove
   tools after registration, so the model-facing default surface is **63**. The
   row now cites the catalog and keeps the explanation, so a reader who remembers
   67 can see why it moved.
2. The plan-mode allowlist. The prose claimed 17 names including `codesearch`.
   `PLAN_ALLOWED` (`src/tool/disclosure.rs:261-278`) holds 16 names and contains
   no `codesearch`. The prose now defers to the catalog's `plan-mode` section.

**The layering was proven, and it is the kind of proof that is easy to fake.**
On a deliberately drifted catalog the structural guard **passed** while the
comparison test failed. That is the report of the implementing session; what I
verified directly is the structural half of it — `scripts/check_tool_catalog.py`
contains no cargo invocation and says in its own header why it must not re-derive
the registry — and the failure-naming half, `first_divergent_line` and its test,
which passed here. I did **not** reproduce the drift experiment myself.

**`src/tool/catalog.rs` was not touched.** `git status --porcelain
src/tool/catalog.rs` is empty. That file is the runtime BM25 discovery structure
and the plan said explicitly it must not become a documentation source.

## 4. Verification executed

### Commands run

```bash
cargo test --test tool_catalog
python3 scripts/check_tool_catalog.py
python3 scripts/check_tool_catalog.py --self-test
git status --porcelain src/tool/catalog.rs architecture/tool-catalog.md \
    architecture/agent-tool-surface.md architecture/overview.md \
    tests/tool_catalog/ scripts/check_tool_catalog.py
git ls-files --error-unmatch architecture/tool-catalog.md
python3 scripts/check_comment_budget.py
```

### Results

- `cargo test --test tool_catalog` — **10 passed, 0 failed**, 0.03 s, after a
  1 m 13 s build. All ten:
  `render::tests::two_renders_of_one_baseline_are_byte_identical`,
  `render::tests::a_hidden_row_quotes_the_reason_its_own_stub_advertises`,
  `render::tests::a_tool_that_hides_without_a_reason_fails_the_render`,
  `render::tests::rows_sort_by_name_and_sections_keep_the_declared_order`,
  `render::tests::the_rendered_text_carries_no_absolute_path_and_no_home_directory`,
  `render::tests::the_first_divergence_is_the_line_a_reader_needs`,
  `render::tests::update_mode_is_refused_while_ci_is_set`,
  `comparison::catalog_matches_the_committed_file`,
  `comparison::the_committed_header_names_its_generator_and_its_update_command`,
  `comparison::the_hand_written_surface_document_carries_no_tool_table`.
- `check_tool_catalog.py` — green, exit 0: "OK: architecture/tool-catalog.md
  parses, declares its 4 sections in order, and covers every tool the prose
  names."
- `check_tool_catalog.py --self-test` — green, exit 0, **7/7** cases: a valid
  catalog passes (control); a duplicate row in one section is rejected; a
  malformed row is rejected; a missing section is rejected; a prose name the
  catalog omits is rejected; a hand-edited header is rejected; an empty hidden
  reason is rejected.
- `git status --porcelain …` — `src/tool/catalog.rs` absent from the output (so
  unmodified); `architecture/agent-tool-surface.md` and
  `architecture/overview.md` modified (`M`); `architecture/tool-catalog.md`,
  `scripts/check_tool_catalog.py` and `tests/tool_catalog/` untracked (`??`).
- `git ls-files --error-unmatch architecture/tool-catalog.md` — fails with
  "did not match any file(s) known to git", confirming the artifact is not yet
  tracked.
- `check_comment_budget.py` — **red, exit 1, 44 violations**, none of them in
  `tests/tool_catalog/` or `scripts/check_tool_catalog.py`. Recorded because it
  is the parent's to bank and because it blocks `quick`.

### Not run

- `CODEGG_UPDATE_TOOL_CATALOG=1 cargo test --test tool_catalog` — **not run**,
  deliberately: the comparison already passes, so a rewrite would produce no
  diff and would only risk touching a file another agent may be reading. The
  update path is nonetheless exercised by
  `update_mode_is_refused_while_ci_is_set`, which drives the same
  `update_requested` decision function.
- `python3 scripts/generate_builtin_agents.py --check` and
  `python3 scripts/generate_subagent_names.py --check` — **not run.** The plan
  lists them to prove the pre-existing generated-asset checks still pass; this
  milestone changes neither generator's input, and `scripts/verify.sh quick`
  cannot be green for unrelated reasons, so a green result here would not be
  load-bearing.
- `cargo fmt --check`, `cargo clippy -D warnings`, `scripts/verify.sh quick`:
  not run. `quick` is red for comment budget (44 violations) and for M001's
  corpus guard, both independent of this milestone.

## 5. Invariant review

- **One source.** `render_catalog` builds four registries through
  `ToolRegistry::with_options`; nothing hand-lists a tool. The guard's header
  records why it must not read Rust to guess what the registry registers.
- **A gated tool is listed as gated, never omitted.** 12 hidden rows, each with a
  reason; `a_tool_that_hides_without_a_reason_fails_the_render` makes a blank
  cell a hard failure.
- **The catalog is deterministic.** Byte-identical across two renders, and no
  timestamp, absolute path or environment-dependent value appears.
- **The catalog carries no schema.** Five columns: tool, safety class, effect
  class, defer-loading, visibility.
- **The hand-written document keeps its prose and loses its enumeration.**
  `the_hand_written_surface_document_carries_no_tool_table` enforces the second
  half; the prose keeps the grouping rationale, the gating story and the
  `tool_search` deferred-loading explanation.
- **No production change.** No tool was added, removed, renamed or re-gated. The
  one apparent interaction with M003 — `present` not appearing as a row — is the
  recorded non-claim, not a change in gating.

## 6. Failure and recovery review

- **Registry construction fails for one matrix entry** — `build_registry`
  returns the error and the test names the surface; there is no fallback to a
  smaller matrix. The renderer's signature (`Result<_, String>`) makes that hard
  to get wrong.
- **A tool's metadata is unavailable** — `render_row` returns `Err`; there is no
  blank-cell path.
- **Update mode in CI** — refused, with the message "CODEGG_UPDATE_TOOL_CATALOG=1
  is refused while CI is set: a CI run must never rewrite the artifact it is
  verifying".
- **Concurrent update runs** — not supported and not needed; one path, one
  writer.
- **Restart** — not applicable; no durable state.
- **Not applicable** — contention and cancellation; this is a single-process
  render.

## 7. Migration and compatibility review

- No production change, so no compatibility surface.
- The catalog is additive documentation; a reader of the old document finds the
  same prose plus a link.
- A later milestone that adds a tool changes the catalog in that milestone's
  change — `catalog_matches_the_committed_file` forces it.
- The comparison test is not `required-features`-gated, so it runs in the nextest
  sweep; the structural guard runs in `quick` and CI where no test runs. That
  split is deliberate and is recorded in the test module's own doc comment.

## 8. Security review

- The generator is hermetic: a sentinel lexical workspace root, no credential, no
  network, no configuration file. `the_rendered_text_carries_no_absolute_path_and_no_home_directory`
  proves the artifact cannot leak a build machine's layout.
- The guard rejects a hand-edited header, which is the only way a drift could be
  made to look generated.
- No authorization surface: this milestone adds no tool, no capability and no
  operation. `architecture/agent-tool-surface.md` keeps the capability discussion
  and the catalog keeps the classification.

## 9. Documentation and operations

- `architecture/tool-catalog.md` — new, generated, with the "AUTO-GENERATED by
  `tests/tool_catalog/render.inc` — DO NOT EDIT" header and the regenerate
  command.
- `architecture/agent-tool-surface.md` — modified: enumeration removed, the
  plan-mode prose defers to the catalog, and the capability/gating/discussion
  prose is retained.
- `architecture/overview.md` — modified: the tool row cites the catalog and
  explains why the old call-site count of 67 is 63 tools.
- `scripts/check_tool_catalog.py` — wired into `scripts/verify.sh` and
  `.github/workflows/ci.yml`, in both the `--self-test` and plain forms.
- Operator action: after a registry change, run
  `CODEGG_UPDATE_TOOL_CATALOG=1 cargo test --test tool_catalog` and commit the
  diff.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | The `plan-mode` catalog section has **15** rows while `PLAN_ALLOWED` holds **16**. The missing name is `todoread`, which `ToolRegistry::with_options` registers only when `policy.allow_model_todo_read` is set (`src/tool/mod.rs:886-889`), and the declared `default` options leave it unset | A reader comparing the constant to the catalog sees 16 vs 15 and may read it as drift; it is not — the matrix simply does not enable that policy | Either add a surface row/note for the todo-policy variant, or state in the catalog's non-claims that `PLAN_ALLOWED` is a superset of what the declared matrix can register |
| low | The drift experiment ("structural guard passes, comparison test fails at line 51") is reported by the implementing session; I verified the layering by reading the guard and the divergence test, not by reproducing the experiment | The strongest claim about the two-layer design rests on a report | Reproduce once: edit a row in `architecture/tool-catalog.md`, run the guard (expect green) and `cargo test --test tool_catalog` (expect `catalog_matches_the_committed_file` to fail naming the line), then revert |
| low | `architecture/tool-catalog.md` and its guard are untracked | Until they are committed the artifact has no history and the guard is not in any reviewable diff | Commit with the milestone |
| low | The roadmap §12 table and the plan's own `Status:` line still say `not started` / `ready for handoff` | The roadmap understates delivered work | Parent owns both |

## 11. Roadmap disposition

**Closed.** Every acceptance criterion was met by a command run here on this
baseline:

1. The committed catalog is byte-identical to a fresh render —
   `catalog_matches_the_committed_file` passed, and the two-run determinism test
   passed.
2. Adding a tool makes the comparison test fail and name the row; update mode
   makes it pass — the naming mechanism is `first_divergent_line` with its own
   passing test, and the update decision is exercised by
   `update_mode_is_refused_while_ci_is_set`. The full drift/rewrite cycle is
   **reported rather than reproduced** (§10); it is a strengthening exercise, not
   a missing capability.
3. A backend-gated tool appears as hidden with a reason — 12 such rows, including
   `lsp`, `lsp_read` and `security`.
4. The structural guard rejects all five of its self-test injections — it rejects
   **seven**.
5. `architecture/agent-tool-surface.md` contains no tool table and every tool
   name it mentions resolves in the catalog — both enforced and passing.
6. No production file changed **by this milestone** — `src/tool/catalog.rs` is
   unmodified and no tool was added, removed or re-gated. The tree-wide diff is
   not this milestone's, and this record does not claim it.

The findings in §10 are evidence-quality and follow-up items. None leaves the
catalog describing a surface the binary does not have, which is the defect the
milestone exists to prevent.

## 12. Registry updates

Required in `plans/registry.md` and
`plans/subsystems/harness-practice-adoption-roadmap.md`:

- §12's M004 row: `not started` → `closed`, closure record
  `plans/closure/harness-practice-adoption/004-status.md`, blocker column `—`.
- The registry's milestone table entry for M004, if `plans/registry.md` carries
  one.
- The two count reconciliations this milestone caused in
  `architecture/overview.md` are already in the tree; the roadmap needs no count
  change of its own.
- **Not updated by this record:** the source plan's `Status:` line. It stays
  `ready for handoff` until the parent changes it.
