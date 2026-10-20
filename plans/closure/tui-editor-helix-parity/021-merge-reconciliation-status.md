# TUI Editor Helix Parity — Merge Reconciliation Closure Status

Status: closed

Record kind: **supplemental reconciliation record**. This is not a new
milestone of the editor line and it opens no new milestone. It records the
reconciliation of the TUI editor Helix-parity workstream with `main` and the
qualification of the merged tree, as the source roadmap's §11 completion
definition requires: the binding bullet is stated over the surfaces that
exist and is checkable.

Source subsystem roadmap:

- `plans/subsystems/tui-editor-helix-parity-roadmap.md`

Historical closure records this record supplements and does **not** rewrite:

- `plans/closure/tui-editor-helix-parity/001-status.md` … `020-status.md`
  (M001–M013, M015–M020, closed)

Repository baseline reconciled: `fe2c1306` on `main`, which is **113 commits**
ahead of this branch's merge-base `84440fe7` (106 non-merge commits plus 7
merges, touching 324 files). The branch head at merge was `681f7d50f`; the
merge into this branch is the commit that carries this record, subject
**"Merge origin/main into feat/auto-20261018-12255c45"**. Its SHA is named
here by subject rather than by hash deliberately: a record that lives inside
the commit it names cannot cite that commit's own hash and stay true after an
amend.

Implementation pull request: **PR #135**
(`https://github.com/dbowm91/codegg/pull/135`), against `main`.

## 1. Executive finding

The editor's key authority, its prefix grammar, its undo model and its guard
set survived the reconciliation with `main` **unchanged**. `main` touched
**zero** editor-owned files across its 113 commits — not
`src/tui/editor.rs`, not `src/tui/commands/editor.rs`, not
`src/tui/editor_lsp.rs`, not `src/tui/components/editor.rs`, not the editor's
plan or closure records, and not any of the seven editor guard scripts — so
the four conflicts were resolved by keeping both sides' semantics rather than
by negotiating the editor's behaviour.

The reconciliation nonetheless found and fixed **three defects**, all of them
documentation defects, and none of them in the editor's runtime. All three
were *surfaced* by the merge rather than caused by it: each was correct, or
nearly so, on the tree that last wrote it, and the merge put the two trees'
claims side by side. The load-bearing one is described in §3.1: sixteen of
the twenty-four rows of `architecture/overview.md`'s Verified-counts table
were stale in the merged tree, and four of the methods behind them were
ambiguous until they were pinned against the commits that originally recorded
each value. Most of that drift is `main`-side; the attribution is in §3.1.

The merged tree is green on every step that is not one of the pre-existing
load-sensitive failures: `cargo fmt --all -- --check`, `cargo clippy
--workspace --all-targets --locked -- -D warnings`, the `codegg` protocol and
`egglsp` suites, every focused editor suite, every TUI guard script and
their self-tests, and `bash scripts/verify.sh quick` ("Quick verification
passed."). The whole-`codegg`-lib step reports **5 failures**, all of them in
`tui::commands::interactive_terminal::tests::`, and the module passes **94 /
0** when it is run alone — the same load-sensitive class this workstream's
earlier sweeps recorded, with **five** members failing this run rather than
the two or three that failed before. Which members fail varies between runs;
that variance is the class's signature.

## 2. Reconciliation inventory

Merge commit on the branch: the commit carrying this record. Four conflicts,
resolved without weakening an editor invariant:

| Conflict | Resolution |
|---|---|
| `src/core/daemon.rs` | Both sides independently replaced a hardcoded `/tmp` fixture with `tempfile::tempdir()` — the same fix, for the same reason (a symlinked `/tmp` fails workspace registration with `InvalidRootPath("/tmp")`). This branch's recorded reason is kept; `main`'s non-cloning binding (`directory` rather than `directory.clone()`) is kept with it, because `directory` is not read again in that function. |
| `.opencode/skills/tui/SKILL.md` | The Static Guards table grew on both sides. It now carries **all eleven** TUI guards in `verify.sh quick` — `main`'s composer key-authority guard beside the text-authority guard, then the seven editor guards, then the transport-authority and subagent-name-pool guards that were on disk but in no version of the table. The section's "Both guards" sentence is corrected in §3.3. |
| `architecture/overview.md` | The Verified-counts rows are remeasured against their listed `Source` in the merged tree; sixteen had drifted, and four methods were pinned rather than guessed (§3.1). |
| `plans/registry.md` | Four tables, resolved by union with the side that owns the milestone: the roadmap-status table keeps `main`'s composer row beside this branch's closed editor row; the closed and eligible tables keep both sides' milestone rows, with `main`'s rows widened to the five-column form this branch adopted; the blocked table keeps `main`'s user-prompt-queue row and this branch's ADR-0034 row, and drops `main`'s M009 row, which this branch closed. |

`main`'s composer workstream is a **peer** of the editor line, not a
successor to it: it owns the main composer (the "LLM buffer"), not the
editor's document surface. Both workstreams' guards are registered in
`scripts/verify.sh quick` and in the CI `verify` job, and both pass.

## 3. Defects found and fixed

### 3.1 Sixteen of twenty-four verified counts were stale (both sides)

`architecture/overview.md`'s "## Verified counts" table is the repository's
single source of truth for counts, and `AGENTS.md` requires each row to be
verified against its listed `Source` rather than copied between documents. In
the merged tree, **sixteen** rows disagreed with their `Source`.

Four rows were ambiguous — the table does not say how to count them — and a
plausible-looking count would have been a guess. Each was therefore pinned by
re-running the candidate method at the commits that originally recorded the
table's value, and only a method that reproduced the historical number was
accepted:

| Row | Method | Historical check | Merged value |
|---|---|---|---|
| Tool registrations in `with_options()` | `registry.register` / `registry.register_boxed` call sites inside `with_options()` | **63** at `541a1c84` | 65 |
| Distinct production tool names | distinct literal `fn name()` in `impl … Tool for …` across `src/tool/`, `src/context/`, less the `#[cfg(test)]` mocks | **59** at `541a1c84`, `3f8ecc48`, `ac4d5962` | 61 |
| Authorized operations | distinct `OperationDescriptor::new("…")` names | **226 / 228 / 237** at `541a1c84` / `3f8ecc48` / `ac4d5962` | 252 |
| Database tables (`CREATE TABLE`) | `CREATE TABLE` **occurrences** in `schema.rs` | **76** at `fc53d0af` | 92 |

The remaining twelve rows have unambiguous `Source`s and were measured
directly: architecture docs **87**, tool source files **80**, `CoreRequest`
variants **258** (234 struct + 24 unit), `CoreResponse` variants **168**,
`CoreEvent` variants **92**, identity newtypes **19**, built-in slash
commands **166**, TUI state modules **32** (33 `.rs` files incl. `mod.rs`),
storage-layout version **83**, integration test files **269**, CI guard
scripts **62**. The eight rows that were already correct were left alone.

Two counting traps were found and are worth recording, because both produce
confident wrong numbers:

- The protocol enum counts must be taken **after** stripping comments. A
  doc-comment containing a comma splits a naive comma-delimited parse, and an
  attribute on a variant drops it entirely. Both errors undercount.
- The tool-name count must exclude `#[cfg(test)]` **modules** by brace
  extent, not by proximity to a `#[cfg(test)]` marker: `src/tool/broker.rs`'s
  test tool declares `impl super::super::Tool for TestTool`, which a
  `impl Tool for` pattern does not match at all.

**Attribution.** The drift is mostly not this branch's. Measured at the
merge-base, at this branch's head, and at `main`:

- `main` moved: identity newtypes 18→19, slash commands 164→166, TUI state
  files 32→33, `CREATE TABLE` 88→92, layout version 79→83, authorized
  operations 244→252, `CoreRequest` 250→258, `CoreResponse` 165→168,
  `CoreEvent` 91→92 — **without** updating the table.
- This branch moved: integration test files 262→263 and CI guard scripts
  54→58.
- Both sides inherited stale rows from the merge-base: architecture docs
  (86 vs 87), tool registrations (63 vs 65), distinct tool names (59 vs 61),
  tool source files (78 vs 80), `CREATE TABLE` (87 vs 88), integration test
  files (244 vs 262) and guard scripts (53 vs 54).

The table's own Source-verification section records that this has happened
before — 2026-10-15 and 2026-10-19 each remeasured rows when a workstream
merged `main` — so the fix follows the section's established practice: a
dated paragraph naming the command, the value, and the movement.

### 3.2 The registry's editor row and the roadmap's status line had to agree

`main`'s registry carried the editor roadmap as *active (M001, M002, M003
closed; M004/M005 eligible for fresh planning)*, and its "Milestones
eligible for fresh planning" table carried M004–M012 as eligible. This
branch closed M004–M013 and M015–M020 and had already moved those rows.
Keeping `main`'s rows would have asserted, in the same table, that M004–M012
were both closed and eligible for fresh planning.

Resolved by union with the owning side: this branch's editor rows win for
the editor workstream, `main`'s rows are kept for the workstreams `main` owns,
and `main`'s editor M009 row in the "Blocked work" table is dropped because
this branch closed M009. The registry's roadmap-status row, the roadmap's
own status line, and the roadmap's §12 milestone rows now agree.

The registry's *eligible-for-fresh-planning* table is five columns wide on
this branch (`Implementation plan` split out of the handoff note) and four on
`main`. `main`'s three new rows were widened to this branch's form rather
than the table narrowed, because the split is what lets a row cite its plan
and its dependency separately.

### 3.3 The skill's guard table had outgrown its own sentence

`.opencode/skills/tui/SKILL.md` introduces its Static Guards table with "Both
guards fail `verify.sh quick` and the CI `verify` job". The sentence was
accurate at the merge-base, where the table listed **two** rows. This branch
grew the table to **eight** (six editor guards: key authority, prefix state,
leaf authority, the apply path, the completion triggers, and the doc-drift
guard) and `main` grew it to **three** (adding the composer key-authority
guard). The merged table lists **nine**, and the sentence undercounts all of
them.

Two further guards were on disk and in `verify.sh quick` but in **no**
version of the table: `check_tui_transport_authority.py` and
`check_tui_subagent_name_pool_isolation.py`. Both rows are added, so the
table now carries **eleven** — every TUI guard in the routine set — and the
sentence is corrected to "Every guard in the table below". A guard table that
undercounts itself is the same class of drift as the counts table.

## 4. Qualification evidence

Every number below is from the sweep against the **merged** tree
(`CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0`).

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | pass |
| `cargo build --locked --bin codegg-sandbox-helper` | pass |
| `cargo test -p codegg-protocol --locked` | **269 passed / 0 failed** |
| `cargo test -p egglsp --features lsp-test-support --locked` | **1012 passed / 0 failed** |
| `cargo test -p egglsp --features lsp-test-support --locked --test code_action_preview` | **2 passed / 0 failed** |
| `cargo test -p codegg --lib --locked` (whole lib) | **7035 passed / 5 failed / 4 ignored** (1 filtered out) |
| the 5 failures, all in `tui::commands::interactive_terminal::tests::` | the module alone: **94 passed / 0 failed** |
| the filtered asset-refresh test, alone | **1 passed / 0 failed** |
| `cargo test -p codegg --lib --locked tui::` | **2457 passed / 0 failed** |
| `cargo test -p codegg --lib --locked tui::commands::editor::tests::` | **231 passed / 0 failed** |
| `cargo test -p codegg --lib --locked tui::components::editor::tests::` | **57 passed / 0 failed** |
| `cargo test -p codegg --lib --locked tui::commands::change_review::tests::` | **9 passed / 0 failed** |
| `cargo test -p codegg --lib --locked tui::editor::tests::` | **182 passed / 0 failed** |
| `cargo test -p codegg --lib --locked tui::editor_lsp::tests::` | **12 passed / 0 failed** |
| `cargo test -p codegg --lib --locked core::daemon_lsp::` | **17 passed / 0 failed** |
| `cargo test -p codegg --locked --test change_review_trajectory` | **7 passed / 0 failed** |
| `lsp_read_delivery_m013` + `lsp_read_surface_trajectory` + `lsp_completion_trajectory` | **12 + 7 + 14 = 33 passed / 0 failed** |
| the TUI guard scripts, and the four that carry `--self-test` | pass — the sweep ran nine of them explicitly; `verify.sh quick` ran all eleven |
| `bash scripts/verify.sh quick` | **"Quick verification passed."** |

### 4.1 The five whole-lib failures

All five are members of `tui::commands::interactive_terminal::tests::`:

- `a_hidden_terminal_surface_does_not_arm_the_loop`
- `a_live_terminal_surface_repaints_with_no_further_user_input`
- `panel_rendered_buffer_shows_live_pty_output`
- `reporting_the_mode_never_moves_the_keyboard`
- `the_badge_returns_to_normal_after_esc_even_though_the_composer_forgot`

Run as a module, the same file passes **94 / 0**. This workstream's sweeps
before the merge recorded the same class with **two** or **three** members
failing, and recorded that *which* members fail varies between runs — the
variance is the class's signature, not a new defect. The merged tree's
`core::daemon::poolless_standalone_human_shell_uses_registered_workspace`,
which failed in some earlier sweeps, passed here.

These are **recorded, not repaired**: repairing them is not this
reconciliation's work. The class is a known one — this workstream's own
closure records each prove it, run the same members alone, and record it as
a low-severity finding: `007-status.md` §10 finding 2 (which cites its §4),
`008-status.md` §4 ("The one `tui::` failure is a known load-sensitive class,
proven so"), `009-status.md` §6.3 ("The two failures are the known
load-sensitive pair, and they pass alone"), `010-status.md` §5.2 ("The
load-sensitive class, in isolation"), and `012-status.md` §6.5 ("The
full-library sweep was **not green**, and why"). It is deliberately **not**
in the registry's "Deferred unregistered product work" section: that section
holds product work, and a timing-sensitive test is not a product.


## 5. Known limits and non-claims

- This record does **not** claim that `main` was green before the merge. The
  pre-existing failures it inherits are recorded, not repaired.
- The whole-lib step's load-sensitive `interactive_terminal` failures are
  **not** claimed to be fixed by this merge, and are not claimed to be a new
  defect: the same class is proven in this workstream's earlier closure
  records, and the module passes **94 / 0** alone. The merged tree's run
  failed **five** members where earlier sweeps failed two or three; *which*
  members fail varies between runs.
- **M014 remains blocked** on ADR-0034 acceptance. ADR-0034 is `proposed`;
  no option is selected by that record. The merge did not touch it, did not
  advance it, and did not discharge it.
- The four residuals this workstream registered remain registered in the
  registry's "Deferred unregistered product work" section. The merge did not
  close any of them.
- The two file-count rows (integration test files, CI guard scripts) are
  counts of a **tree** and move with every merge; the code-count rows move
  only when code moves.

## 6. Historical records preserved

Nothing in this record rewrites a prior closure record. The twenty records
`plans/closure/tui-editor-helix-parity/001-status.md` …
`020-status.md` stand as they were written, with the counts they measured
against the tree they measured. This record adds the merged-tree
qualification and the defect history; it does not restate them.
