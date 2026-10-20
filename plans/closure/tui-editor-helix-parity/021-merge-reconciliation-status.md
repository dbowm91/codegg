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

Repository baseline reconciled: `main` twice. First at `fe2c1306`, which was
**113 commits** ahead of this branch's merge-base `84440fe7` (106 non-merge
commits plus 7 merges, touching 324 files); then again at `6c348f03`, seven
commits further on (PR #134, the permission temp-root redirect, and PR #136,
the M005 merged-head evidence). The branch head at the first merge was
`681f7d50f`; at the second it was the first merge's own commit. The merges
into this branch are the commits that carry this record — the first has the
subject **"Merge origin/main into feat/auto-20261018-12255c45"**, the second
**"Merge origin/main again — the permission temp-root redirect"**. They are
named by subject rather than by hash deliberately: a record that lives inside
a commit it names cannot cite that commit's own hash and stay true after an
amend.

The **second** merge had **no** conflicts: PR #134 touches
`src/permission/`, `src/tool/`, `src/agent/tool_batch.rs` and the desktop
skill and docs, none of which the editor line owns.

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

The reconciliation nonetheless found and fixed **six defects**. Three are
documentation defects, surfaced by the merge rather than caused by it: each
was correct, or nearly so, on the tree that last wrote it, and the merge put
the two trees' claims side by side. The load-bearing one is described in
§3.1: sixteen of the twenty-four rows of `architecture/overview.md`'s
Verified-counts table were stale in the merged tree, and four of the methods
behind them were ambiguous until they were pinned against the commits that
originally recorded each value. Most of that drift is `main`-side; the
attribution is in §3.1.

The fourth, the fifth and the sixth are **this branch's own code**, and all
three were found by the hosted CI runs rather than by the merge.

The fourth is §3.3: the M013 LSP read-delivery test target was missing the
`required-features` gate its three sibling fake-LSP tests carry, so a
feature-less CI sweep built the test and ran it against a binary that
`required-features` had left unbuilt. It is fixed, and both directions of the
gate are checked.

The fifth is §3.5: M011 changed the completion popup's `Enter` contract —
`Enter` accepts the candidate in Normal mode and inserts a newline in Insert
mode — and five of `main`'s integration tests still asserted the old
contract. It survived because this branch's sweeps ran the **library** unit
tests and never the `tests/` integration suite, and the hosted run reported
only **one** of the five because `cargo nextest` cancels the run on the first
failure. All five are fixed, a sixth test pins the Insert-mode half of the
contract through a real answer, and the whole-workspace sweep is now part of
the qualification evidence.

The sixth is §3.6: the same missing-gate class again, this time on the
branch's own `egglsp` code-action preview, which had **no** gate at all — not
the manifest declaration and not the in-file `#![cfg(feature = …)]` its
directory-mate uses. It survived even a feature-less *local* sweep, because a
stale feature-enabled `egglsp-test-server` was still on disk and satisfied the
spawn. It is fixed with the same remedy as §3.3, both directions are checked,
and §3.6 also names the guard that would have caught both instances and is
not in this pull request.

The merged tree is green on every step that is not one of the pre-existing
load-sensitive failures: `cargo fmt --all -- --check`, `cargo clippy
--workspace --all-targets --locked -- -D warnings`, the `codegg` protocol and
`egglsp` suites, every focused editor suite, every TUI guard script and
their self-tests, and `bash scripts/verify.sh quick` ("Quick verification
passed."). The **whole workspace** — CI's own `Workspace tests` command with
`--no-fail-fast` added, so that nothing hides behind the first failure —
reports **15243 passed / 1 failed / 1 timed out / 7 skipped** of 15245; the
failure is this workstream's known load-sensitive `interactive_terminal` class
(green on the hosted run) and the timeout is a contention artifact of the
local runner (also green on the hosted run, at the same position in the run)
(§4.1, §4.3). The whole-`codegg`-lib step reports **1 failure**, in
`tui::commands::interactive_terminal::tests::`, and the same file passes
**94 / 0** on a re-run — it fails one member on roughly one run in three,
with a *different* member each time. An earlier sweep of the same tree
failed **five** members of that file and an earlier sweep of the pre-merge
tree failed two or three; which members fail varies between runs, and that
variance is the class's signature.

## 2. Reconciliation inventory

Merge commit on the branch: the commit carrying this record. Four conflicts,
resolved without weakening an editor invariant:

| Conflict | Resolution |
|---|---|
| `src/core/daemon.rs` | Both sides independently replaced a hardcoded `/tmp` fixture with `tempfile::tempdir()` — the same fix, for the same reason (a symlinked `/tmp` fails workspace registration with `InvalidRootPath("/tmp")`). This branch's recorded reason is kept; `main`'s non-cloning binding (`directory` rather than `directory.clone()`) is kept with it, because `directory` is not read again in that function. |
| `.opencode/skills/tui/SKILL.md` | The Static Guards table grew on both sides. It now carries **all eleven** TUI guards in `verify.sh quick` — `main`'s composer key-authority guard beside the text-authority guard, then the seven editor guards, then the transport-authority and subagent-name-pool guards that were on disk but in no version of the table. The section's "Both guards" sentence is corrected in §3.4. |
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

### 3.3 The M013 LSP test target was missing its feature gate (this branch's own code)

The first hosted CI run of this branch failed, and the cause was **this
branch's**, not `main`'s. Four tests in `tests/lsp_read_delivery_m013.rs`
died with:

```
warm the Rust server: LaunchFailed("failed to spawn
  '/home/runner/work/codegg/codegg/target/debug/codegg-lsp-test-server'")
```

The path was right and the file was not there. The reason is the shape of
the root package's manifest: `codegg-lsp-test-server` is a `[[bin]]` with
`required-features = ["lsp-test-support"]`, and the three sibling
fake-LSP integration tests are declared as `[[test]]` targets with the same
`required-features`. `tests/lsp_read_delivery_m013.rs` was **not** declared
at all, so Cargo auto-discovered it, built it during the CI job's
feature-less `cargo nextest run --workspace --locked --profile ci`, and ran
it — while the binary it spawns was never built. `CARGO_BIN_EXE_…` is set
at compile time regardless, so the test resolved a path to a file that did
not exist.

The fix is the one the three siblings already use: a `[[test]]` declaration
for `lsp_read_delivery_m013` with `required-features = ["lsp-test-support"]`.
The test is then built and run by `verify.sh full`'s
`--features server,plugins,lsp-test-support` pass, where the binary exists,
and skipped by the feature-less CI sweep — the same bargain the other three
fake-LSP tests take. Both directions are checked: without the feature Cargo
now refuses the target (`requires the features: lsp-test-support`), and with
it the suite still passes **12 / 7 / 14 = 33 / 0**.

This defect was invisible locally because the local sweep ran the target
*with* the feature, where the binary is built. **A test that needs a
feature-gated binary must be gated the same way; a local run with the
feature proves nothing about a feature-less CI run.** That is the lesson,
and it is the same class as the §3.1 counts: a thing that is true in one
configuration and false in another, checked in only one.

### 3.4 The skill's guard table had outgrown its own sentence

`.opencode/skills/tui/SKILL.md` introduces its Static Guards table with "Both
guards fail `verify.sh quick` and the CI `verify` job". The sentence was
accurate at the merge-base, where the table listed **two** rows. This branch
grew the table to **eight** (six editor guards: key authority, prefix state,
leaf authority, the apply path, the completion triggers, and the doc-drift
guard) and `main` grew it to **three** (adding the composer key-authority
guard). The merge resolution produced **nine**, and the sentence undercounts
all of them.

Two further guards were on disk and in `verify.sh quick` but in **no**
version of the table: `check_tui_transport_authority.py` and
`check_tui_subagent_name_pool_isolation.py`. Both rows are added, so the
table now carries **eleven** — every TUI guard in the routine set — and the
sentence is corrected to "Every guard in the table below". A guard table that
undercounts itself is the same class of drift as the counts table.

### 3.5 The completion popup's `Enter` contract changed in M011, and five `main`-side tests still asserted the old one (this branch's own code)

The second hosted CI run of this branch failed at the `Workspace tests` step, in
`tests/tui_completion_trajectory.rs`:

```
thread 'emoji_cursor_binds_and_applies_exactly' panicked at tests/tui_completion_trajectory.rs:1023:5:
assertion `left == right` failed
  left: "a😀b\n\n"
 right: "a😀B\n"
```

The left is the document with a **newline** where the candidate should have
been applied; the right is the candidate applied. The test is **`main`'s** — it
came with PR #114 (`8875aa66`) and has been on `main` since — and it asserted
that `Enter` accepts the completion candidate **with the popup open in Insert
mode**.

This branch's M011 changed that contract, deliberately and in four places: the
plan's §6.3 table (`Enter` | accept the candidate | **insert a newline**), the
M011 closure record's §5 divergence table, `docs/tui.md`, and
`architecture/tui.md`. The implementation is one arm in
`src/tui/commands/editor.rs`:

```rust
// Accepting a candidate rewrites the text, so it is a normal-mode
// action. In insert mode `Enter` is a newline — always.
KeyCode::Enter if normal_mode => {
    accept_selected_completion(app);
    true
}
```

`8875aa66` wrote that arm **without** the mode guard; this branch's `51957763`
("Helix insert-mode chords, mode-aware panels, and undo coalescing", the M011
commit) narrowed it to Normal mode. The contract is the branch's, and the plan
owns it; the **test** was `main`'s, and still encoded the contract as it was
before M011. A contract change on a shared surface has to carry the tests that
encode it, and this one did not.

**Five** tests in that file pressed `Enter` with the popup open in Insert mode,
and so encoded the old contract:

| Test | What it pressed with the popup open |
|---|---|
| `ready_answer_applies_one_undo_unit_with_redo` | `Down`, `Enter`, `Up`, `Enter` — selection and acceptance |
| `identity_mismatch_is_an_error_not_a_suggestion` | `Enter` on an error state |
| `not_ready_and_error_states_refuse_with_words` | `Enter` on `NotReady`, then `Enter` on an error |
| `lost_writer_lease_refuses_without_side_effects` | `Enter` on a lost writer lease |
| `emoji_cursor_binds_and_applies_exactly` | `Enter` to apply the candidate |

The hosted run reported **one** of them. `cargo nextest` **cancels the run on
the first failure**, so the rest of that binary and **4324 of the workspace's
15286** tests never ran — the log ends with `warning: 4324/15286 tests were not
run due to test failure`. One failure understated five, and the sweep had to
be re-run with `--no-fail-fast` to see the rest.

**Why it survived locally.** This branch's sweeps ran `cargo test -p codegg
--lib` — the **library** unit tests — plus a handful of named integration
targets. The `tests/` integration suite was never run in full on this branch.
The contract change was verified against the branch's own unit tests, which were
updated to it (`the_completion_popup_does_not_steal_enter_in_insert_mode` in
`src/tui/commands/editor.rs`), while `main`'s integration tests — which encode
the *same* contract from the other side — were not run at all. **A contract
change is verified by the suite that encodes it, and here there were two
suites.**

**The fix.** The five tests now run in **Normal** mode, which is where
acceptance lives under the M011 contract, and each says so in a comment. The
one that had to leave Insert mode to reach `u` —
`ready_answer_applies_one_undo_unit_with_redo` pressed `Esc` to leave Insert
for Normal before undoing — now starts in Normal mode and does not need the
`Esc`. A new test, `enter_is_a_newline_in_insert_mode_and_never_applies_the_candidate`,
pins the *other* half of the contract through a real answer rather than a
synthetic popup: in Insert mode `Enter` inserts a newline, dismisses the popup,
and never applies the candidate. The file therefore proves the whole rule —
accept in Normal, newline in Insert — instead of only the half that was failing.

### 3.6 The egglsp code-action preview was missing its gate too — the second instance of §3.3 (this branch's own code)

The third hosted CI run of this branch failed at the `Workspace tests` step, in
`crates/egglsp/tests/code_action_preview.rs`:

```
thread 'a_mixed_action_carries_a_command_and_gets_no_preview' panicked at crates/egglsp/tests/code_action_preview.rs:335:10:
the client starts: LaunchFailed("failed to spawn
  '/home/runner/work/codegg/codegg/target/debug/egglsp-test-server []")
```

**This is the second instance of the §3.3 class**, and it is this branch's
code: the file is new here, added by the code-action-read milestone
(`8df5dbb3`), and `git log --diff-filter=A` shows it is not on `main` at all.

The shape is the one §3.3 describes. `egglsp-test-server` is a `[[bin]]` with
`required-features = ["lsp-test-support"]`, and the package's manifest already
declares **four** sibling test targets with the same gate
(`production_protocol_stdio`, `production_semantic_stdio`,
`production_service_stdio`, `scenario_engine`). `code_action_preview` had
**no gate at all** — neither a `[[test]]` declaration with `required-features`
nor the in-file `#![cfg(feature = "lsp-test-support")]` idiom that
`empty_diagnostics_readiness.rs` in the same directory uses. Cargo therefore
auto-discovered it, built it during the feature-less CI sweep, and ran it
against a binary `required-features` had left unbuilt.

**Why even the feature-less local sweep missed it.** The local sweep ran this
target *without* the feature and it **passed** — because
`target/debug/egglsp-test-server` was on disk, left over from an earlier
feature-enabled build. The harness resolves the binary by path
(`option_env!("CARGO_BIN_EXE_egglsp-test-server")`, which Cargo sets at compile
time whether or not the binary was built), so on a developer machine a
**stale artifact** satisfies the spawn, and on a clean CI runner the same code
fails. A local run proves nothing here in either direction: with the feature
the binary exists, and without it the binary may still exist from yesterday.

**The fix** is §3.3's fix, in the manifest that owns the target:

```toml
[[test]]
name = "code_action_preview"
path = "tests/code_action_preview.rs"
required-features = ["lsp-test-support"]
```

Both directions are checked: without the feature Cargo refuses the target
(`error: target 'code_action_preview' in package 'egglsp' requires the
features: 'lsp-test-support'`), and with it the suite passes **2 / 0**. The
package's ten test targets are now audited in one place — every target that
can reach the fake-server spawn carries a gate (six by the manifest
declaration, one by the in-file `#![cfg]`), and the two that cannot reach it
(`lsp_cache_benchmark`, `phase6_regression`) reference neither the binary nor
the harness.

**The same bargain §3.3 takes applies to lints.** CI's clippy step runs
without the feature, so a gated target is not linted there; it is covered by
`cargo clippy -p egglsp --features lsp-test-support --all-targets --locked
-- -D warnings`, which this tree passes.

**What is not in place.** A mechanical guard for this class would have to
decide, for each test target, whether it *can reach* a feature-gated binary —
through a `mod` tree, not a literal string — and it would have to accept
**either** gate idiom (the manifest declaration or the in-file
`#![cfg(feature = …)]`). That is a real guard, it is not hard, and **it is not
in this pull request**: this record states the limit rather than pretending a
handful of one-off fixes is a check. The class has now bitten twice here, so
the guard is the right follow-up.

## 4. Qualification evidence

Every number below is from the sweep against the **merged** tree — both
merges applied, the `Cargo.toml` gate and the completion-contract fix included
(`CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0`).

| Command | Result |
|---|---|
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | pass |
| `cargo build --locked --bin codegg-sandbox-helper` | pass |
| `cargo test -p codegg-protocol --locked` | **269 passed / 0 failed** |
| `cargo test -p egglsp --features lsp-test-support --locked` | **1012 passed / 0 failed** |
| `cargo test -p egglsp --features lsp-test-support --locked --test code_action_preview` | **2 passed / 0 failed** |
| `cargo test -p codegg --lib --locked` (whole lib) | **7042 passed / 1 failed / 4 ignored** (1 filtered out) |
| the 1 failure, in `tui::commands::interactive_terminal::tests::` | the module alone: **94 passed / 0 failed** |
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
| `cargo nextest run --workspace --locked --profile ci --no-fail-fast` (the whole workspace, 292 binaries) | **15243 passed / 1 failed / 1 timed out / 7 skipped** of 15245 — see §4.1 and §4.3 |
| the TUI guard scripts, and the four that carry `--self-test` | pass — the sweep ran nine of them explicitly; `verify.sh quick` ran all eleven |
| `bash scripts/verify.sh quick` | **"Quick verification passed."** |

### 4.1 The load-sensitive `interactive_terminal` class

One member of `tui::commands::interactive_terminal::tests::` failed:
`a_hidden_terminal_surface_does_not_arm_the_loop`. The same file, run alone
by the `interactive_terminal` filter (94 tests across two modules), passes
**94 / 0** on a re-run — and fails one member on roughly one run in three,
with a *different* member failing each time. Three consecutive runs of that
one command on this tree gave **93 / 1**, **94 / 0**, **94 / 0**.

An earlier sweep of the same merged tree — before `main`'s second batch
landed — failed **five** members of the same file, and an earlier sweep of
the pre-merge tree failed **two** or **three**. *Which* members fail varies
between runs; the variance is the class's signature, not a new defect. This
workstream's closure records each prove it and record it as low severity
(the list is in the paragraph below).

**It is not one runner's fault.** The whole-workspace nextest sweep in §4
reproduced it once — one member, in the second of the two sweeps — while the
first sweep of the same tree did not. The hosted run is green for it: this
branch's CI log shows `PASS … a_hidden_terminal_surface_does_not_arm_the_loop`
in **0.060 s** at position 5562, and `main`'s own whole-workspace nextest CI
is green (`e52695b9`, the PR #134 merge). It is a property of the tests, not
of a runner, and it is why the two sweeps are reported separately here rather
than as one number.

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

### 4.2 The desktop E2E's `permission-list` timeout is caused by `main`'s PR #134

The Desktop E2E job has failed on this branch, twice, with:

```
Error: element ("[data-testid=\"permission-list\"]") still not existing
       after 120000ms
```

The element renders only when the projection carries a pending permission, and
the spec waits 120 s for it. The cause is **`main`'s**, and it is now
*identified* rather than asserted:

- `main`'s PR #134 — "answer the model with a temp-root redirect before the
  human", commit `39e1445b`, merged as `e52695b9` — added
  `PermissionRegistry::temp_root_redirect_offer`. When the pending decision is
  the **implicit default** `Ask`, the tool is the structured **`write`**, and
  the target is an **absolute path under a known host temp root** outside every
  authorized root, the offer **answers the model** with an actionable scratch
  path instead of raising a human decision. `src/agent/tool_batch.rs`'s
  `PermissionResult::Ask` arm returns `ToolPermissionOutcome::Denied` with
  that guidance, so **no `PermissionPending` is ever published** and the
  desktop has nothing to render.
- The E2E's fixture writes to exactly such a path.
  `apps/desktop/src-tauri/src/bin/desktop_e2e_fixture.rs` builds it as
  `home.join("e2e-mock-write.txt")` — inside the temp-scoped phase home,
  **outside** the probe workspace root — and the fixture's own comment says
  the write is "not workspace-auto-allowed and deterministically pends". That
  comment describes the behaviour **before** PR #134.
- Every condition of the offer is met for that path: the tool name is
  `write`, `relative_to_known_temp_root` recognises it (it is under
  `std::env::temp_dir()`), it is outside every authorized root, and the
  operator has not spoken about it.
- **The timing fixes the order.** The redirect commit is dated 2026-10-20
  04:37Z. The last green Desktop E2E runs are `56a81432` (2026-10-19 23:04Z)
  and `main`'s `a5d88eb7` (2026-10-20 00:02Z) — both **before** it, and
  `39e1445b` is **not an ancestor** of either. No green Desktop E2E run exists
  after the redirect landed, on `main` or on this branch.
- **Why `main` never saw it.** The Desktop E2E workflow is **path-gated**.
  PR #134 changed `src/permission/`, `src/tool/` and `src/agent/tool_batch.rs`
  — **none** of which is in the gate's path list — so the job did not run on
  `main`'s `e52695b9` merge at all (`main`'s runs for that SHA are the `CI`
  job alone, and it succeeded). It runs on this pull request because the
  branch does touch `src/core/daemon.rs`, which the gate watches. So the
  redirect changed a behaviour the spec exercises without the job being
  scheduled, and this pull request is the first to schedule it after the
  change. **The gate's path list is missing the permission-admission code the
  spec drives** — that is a second, separate gap, in the gate rather than in
  the fixture.

This branch's role in it: it changes **nothing** under `apps/` — `git diff
origin/main -- apps/` is empty — and it does not touch `src/permission/` or
`src/agent/tool_batch.rs` beyond what `main` already contains.

So the honest statement is not "this branch is not the cause" but "**the cause
is `main`'s PR #134, and `main` is unverified on this path**". Repairing it
belongs to the workstream that owns the redirect offer: the fixture's premise —
that an out-of-workspace write under the phase home *deterministically pends* —
is what PR #134 changed. This record does not repair it and does not claim that
`main` is green after PR #134.

An earlier version of this section described the timeout as a documented flake
of that job, on the strength of the desktop workstream's own closure record
§4 ("succeeding on the same SHA after rerunning its fail"). That is true of the
occurrence that record describes and it is **not** an explanation of this one:
the job has failed twice on this branch with the same error, and the redirect
commit post-dates every green run. The weaker explanation is withdrawn.

### 4.3 The whole-workspace sweep's one timeout is contention, and it is green on CI

The whole-workspace sweep — CI's own `Workspace tests` command, with
`--no-fail-fast` added so that nothing hides behind the first failure —
reports:

```
Summary [ 584.722s] 15245 tests run: 15243 passed (2 leaky), 1 failed, 1 timed out, 7 skipped
TIMEOUT [ 300.004s] (  19/15245) codegg agent::asset_refresh::tests::same_scope_requests_coalesce_to_one_publication
FAIL    [   0.113s] ( 5548/15245) codegg tui::commands::interactive_terminal::tests::a_hidden_terminal_surface_does_not_arm_the_loop
```

The one **failure** is the load-sensitive class of §4.1 — the same member the
shared-process sweep fails, and green on the hosted run.

The one **timeout** is not a defect of this tree:

- This branch changes **nothing** under `src/agent/` —
  `git log --no-merges 6c348f03..HEAD -- src/agent/` is empty.
- Run **alone**, under the same `ci` profile and the same slot count, the test
  **passes in 0.15 s**.
- The `ci` profile in `.config/nextest.toml` gives that test
  `threads-required = "num-cpus"` and a 300 s timeout, and says why: *"This
  test parks one Tokio worker while coordinating a same-scope refresh. Running
  it alongside other test processes makes its bounded yield spin
  scheduler-sensitive; reserve the runner so it can complete
  deterministically."* The reservation is what keeps it deterministic.
- On the hosted runner the reservation **holds** — 4 vCPUs and
  `test-threads = 4` — and the test **passes**: this branch's own CI log
  shows `PASS … ( 19/15286) codegg agent::asset_refresh::tests::same_scope_requests_coalesce_to_one_publication`
  in **0.011 s**, at the same position in the run as the local timeout.

The local host has 14 CPUs against the profile's `test-threads = 4`, so the
`num-cpus` reservation and the slot count cannot both hold; the sweep was
re-run with `--test-threads 14` — the same slots-per-core ratio as CI (CI:
4 slots / 4 CPUs; local: 14 slots / 14 CPUs) — and the test timed out there
too, while passing alone under the same profile and the same slot count. It
is recorded, not repaired: the condition the profile's own comment names is
contention, the hosted run is green at the same position in the run, and the
branch does not touch the code the test exercises.

**And the sweep is not a clean-room feature-less run.** It is a feature-less
run *on a machine that has built the feature at least once* — which is how
§3.6's target passed here and failed on the hosted runner. The fix in §3.6
removes the dependency on that accident: the target is now skipped outright
in a feature-less sweep, exactly as its four siblings are.

## 5. Known limits and non-claims

- This record does **not** claim that `main` was green before the merge. The
  pre-existing failures it inherits are recorded, not repaired.
- The M013 LSP read-delivery proof runs in `verify.sh full`'s
  `--features server,plugins,lsp-test-support` pass, **not** in CI's
  feature-less `verify` job. That is the bargain the three sibling
  fake-LSP tests take, and it is now this test's bargain too (§3.3). The
  consequence is stated plainly: **CI does not execute the M013 read
  delivery's integration proof**; `verify.sh full` and the local sweep do.
- The Desktop E2E's `permission-list` timeout is recorded, not repaired, and
  is attributed to `main`'s PR #134 (§4.2). The repair belongs to the
  workstream that owns the temp-root redirect offer: the fixture's premise —
  that an out-of-workspace write under the phase home *deterministically
  pends* — is what PR #134 changed. This record does not repair it, and does
  not claim that `main` is green after PR #134; **no green Desktop E2E run
  exists after that commit.**
- This record does **not** claim that the earlier local sweeps were
  representative of the workspace. They ran the **library** unit tests and a
  handful of named integration targets; the `tests/` integration suite was not
  run in full on this branch, and that gap is what let §3.5 through. Worse,
  a sweep that *did* run a feature-dependent target feature-less (§3.6) still
  passed it, because a stale feature-enabled binary on disk satisfied the
  spawn. The whole-workspace sweep in §4 is the evidence that replaces them,
  and even it is not a clean-room feature-less run: it is a feature-less run
  **on a machine that has built the feature at least once**.
- **No mechanical guard for the missing-gate class is in this pull request.**
  §3.3 and §3.6 are the same class, fixed by hand twice. §3.6 describes what
  such a guard would have to decide — reachability through a `mod` tree, and
  acceptance of either gate idiom — and says plainly that the guard is not
  here.
- The whole-lib step's load-sensitive `interactive_terminal` failure is
  **not** claimed to be fixed by this merge, and is not claimed to be a new
  defect: the same class is proven in this workstream's earlier closure
  records, and the module passes **94 / 0** alone. One sweep of this tree
  failed one member, another failed five, and earlier sweeps of the
  pre-merge tree failed two or three; *which* members fail varies between
  runs.
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
