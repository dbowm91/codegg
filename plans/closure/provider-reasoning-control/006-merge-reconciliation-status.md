# Provider Reasoning Control — Merge Reconciliation with `main`

Status: closed

Record kind: **supplemental reconciliation record**. This is not a new
milestone of the `provider-reasoning-control` line and it opens no new
milestone. It records the reconciliation of the reasoning-control branch with
`main` and the qualification of the merged tree. The five milestone closure
records it supplements stand as they were written; where the merge changed a
milestone's *surface*, this record says so and the affected milestone's
record carries the correction.

Source subsystem roadmap:

- `plans/subsystems/provider-reasoning-control-roadmap.md`

Historical closure records this record supplements and does **not** rewrite:

- `plans/closure/provider-reasoning-control/001-status.md` (M001, closed)
- `plans/closure/provider-reasoning-control/002-status.md` (M002, closed)
- `plans/closure/provider-reasoning-control/003-status.md` (M003, closed)
- `plans/closure/provider-reasoning-control/004-status.md` (M004, closed)
- `plans/closure/provider-reasoning-control/005-status.md` (M005, closed)

Repository baseline reconciled: `origin/main` at `ff82c68da`, reached in
**six** stages. The first merge was against `8cee65934` ("fix(server): map
the process-denial refusal to 409, not an unhandled variant"), which is
**439 commits** ahead of this branch's merge-base `c93211542` ("Merge pull
request #153 from dbowm91/docs/core-family-counts-m006"). The branch head at
merge was `6d36a1888`.

Changed-file surface, per stage: the branch carried 90 changed files against
the merge-base, and **eight files** were changed on both sides in the first
stage. In the third stage — the one against the `origin/main` the pull request
targets — the branch's side of the merge-base was 92 files and `main`'s was
585, with **eleven files** changed on both sides.

**The reconciliation took six stages, because `main` moved five times while
it was in flight.** The first merge was against `8cee65934`. During the merged
tree's qualification — which, on this host, took hours — `main` advanced a
further **six commits** to `47f5572b1` (the agent-workstream blackboard's
M008: `3477bc589`, `4d327add9`, `3549d9216`, `e1b898cb8`, `2a797c86c`,
`47f5572b1`). The second merge brought that in, and is recorded in §2a and
§4a. `main` then advanced a further **79 commits** to `f1b83a7a7`, carrying
the CI/test-throughput recovery workstream and its M004 root-test-target
family consolidation. The third merge brought that in, and is recorded in §2b,
§4b and §7.6.

`main` then advanced a further **five commits** to `3e43d4144` — the landing
of PR #170 (pause the hosted Desktop E2E qualification) and PR #171 (absorb
two interactive test families into one, and repair two guards) — and the
fourth merge brought that in too. It is recorded in §2c and §4c.

`main` then advanced a further **five commits** to `12999178a` — the landing
of PR #173 (adapt the Linux-gated test-support call sites to C003's typed
live-read result) and PR #174 (record the C003 landing). The fifth merge
brought that in, and it is recorded in §2d.

`main` then advanced a further **nine commits** to `ff82c68da` — a
documentation-surfaces pass (PR #175) that re-verified the user-facing guides,
the module guides, the architecture deep-dive index **and the count audit**,
plus PR #175's two fixes. The sixth merge brought that in, and it is recorded
in §2e and §4e.

The third, fourth, fifth and sixth merges are the ones that make this line
publishable: together they are the reconciliation against the `origin/main`
the pull request is opened against.

## 1. Executive finding

**The reasoning-control contract survived the reconciliation intact; two
identifiers and one surface did not.**

`main` had moved a long way in the 439 commits, and three of the things this
line allocates by number had already been allocated by `main`:

1. **Both ADR numbers collided.** `main` had published ADR-0044 (the
   contained-Git live-object and metadata authority) and ADR-0045 (the
   command syntax and execution authority). This line's ADR-0044 (the open
   provider-scoped reasoning vocabulary) and ADR-0045 (the per-model
   reasoning capability authority) therefore moved to **ADR-0048** and
   **ADR-0049**. Main wins both numbers — the rule the repo's own ADR
   identity migrations follow. The decision content, `Status: accepted`, the
   acceptance date and the accepting authority are **unchanged**; only the
   identifier and the file name moved, and each record carries a dated
   renumbering note saying so. 68 references across 24 files were updated,
   and `scripts/check_adr_integrity.py` confirms the active ADR IDs are
   unique and their local paths resolve.

   **The third stage collided again, and the rule applied a second time.**
   `main` published its own **ADR-0048** (contained Git execution for
   managed-worktree creation) while this line was qualifying, so this line's
   two numbers yield *again* — ADR-0048 → **ADR-0049** and ADR-0049 →
   **ADR-0050**. The decision content, `Status: accepted`, the acceptance
   date and the accepting authority are unchanged; only the identifier and
   the file name move, with a dated renumbering note added to each record.
   Measured after the second renumbering, **28 files** carry the current
   identifiers, at **134** occurrences in total. `main`'s ADR index also
   carried **no entry for its own ADR-0048**; that gap is filled in the same
   pass, so the index is contiguous again.
2. **The migration number collided.** `main` had published v84 (the additive
   network-intent preference), v85 (the private index publication journal),
   v86 (the durable checkout writer permit) and v87, and
   `STORAGE_LAYOUT_VERSION` was **87**. This line's `migrate_v84` — the
   additive `runtime_preferences.reasoning_selection` column — therefore
   moved to **v88**, and the constant moved 87 → 88. The renumbering is
   additive and changes the meaning of no published database: v88 runs after
   every version `main` can produce. Both columns land on the same table and
   are independent.
3. **The indicator's surface moved.** `main` deleted the **working strip**
   as part of its buffer-chrome roadmap — the strip's height was subtracted
   from the transcript and its row 0 was the buffer's only status surface.
   M004's reasoning indicator lived in that row, so the indicator was
   re-landed in the footer status bar's activity chain, next to the
   daemon-resolved approval and sandbox policy. The property that survived
   the move is the one that mattered: the indicator is **content**, and no
   surface's presence or height is a function of the reasoning state.

Nothing else in the reasoning contract changed. The open vocabulary, the
daemon-owned durable selection, the compare-and-swap revision gate, the
per-model capability record, the fail-closed admission policy, the
narrowing-only overlay, the option derivation, and the `/reasoning` write
path are all as the milestone records describe them.

## 2. The conflicts, and how each was resolved

Eight files conflicted. Every resolution keeps **both** sides' authority
semantics; none was resolved by taking one side wholesale except where that
side was strictly a superset, and that case is named.

| File | Conflict | Resolution |
|---|---|---|
| `crates/codegg-core/src/storage/mod.rs` | The layout constant: 84 (branch) vs 87 (`main`) | **88**, with a dated note recording why the branch yields the lower number |
| `crates/codegg-core/src/session/schema.rs` | Two `migrate_v84` definitions — the branch's reasoning column and `main`'s network-intent column. A duplicate definition is a compile error | The branch's migration renamed to `migrate_v88`; `main`'s v84/v85/v86/v87 kept; the step list and the dispatch match wired to 88. The splice that produced the conflict had dropped `migrate_v87`'s closing braces, which the compiler caught and which are restored |
| `crates/codegg-core/src/approval.rs` | Both sides added a migration-statement constant and both sides' test fixtures apply them | **Both kept**: `RUNTIME_PREFERENCE_REASONING_MIGRATION_STATEMENTS` (the branch's) and `RUNTIME_PREFERENCE_NETWORK_INTENT_MIGRATION_STATEMENTS` (`main`'s), with the test applying both |
| `crates/codegg-core/tests/user_prompt_queue.rs` | A hard-coded `assert_eq!(STORAGE_LAYOUT_VERSION, 84)` vs `main`'s floor assertion | **`main`'s side**, which is the better fix: the equality broke the build for C002 (v84) and again for M006 (v85) without testing anything, because the constant is bumped by every later migration. The branch's equality was the defect |
| `crates/codegg-core/tests/user_prompt_queue_mutation.rs` | The same equality, against the same floor assertion | **`main`'s side**, for the same reason |
| `src/tui/app/render.rs` | The whole working-strip block: the branch's version (with the reasoning indicator) vs `main`'s deletion | **`main`'s deletion**, then the indicator re-landed in the footer status bar (§3) |
| `architecture/overview.md` | The Verified-counts table, both sides' numbers stale for the merged tree | `main`'s table structure, then **every row re-measured against its own listed `Source`** in the merged tree (§4) |
| `plans/registry.md` | Both sides added rows to the same table | **Both kept** — the five reasoning-control rows and `main`'s two Linux-execution-isolation rows |

## 2a. The second stage: `main` advanced while the first merge was being qualified

The first merge was against `8cee65934`. Before it could be landed, `main`
advanced six commits to `47f5572b1` — the agent-workstream blackboard's M008,
which added `architecture/blackboard_contract_matrix.md`, two integration test
files, a new guard (`scripts/check_blackboard_contract_matrix.py`), and a
`tests/common/` fixture.

`git merge origin/main` on top of the merged tree produced **one conflicted
file**, and the other three files both sides had touched — `.github/workflows/ci.yml`,
`plans/registry.md` and `scripts/verify.sh` — **auto-merged** without
conflict.

| File | Conflict | Resolution |
|---|---|---|
| `architecture/overview.md` | The two file-count rows of the Verified-counts table: the merged tree's 298 / 70 against `main`'s 300 / 67 | **Re-measured in the newly merged tree**, never picked: **300** integration test files (`ls tests/*.rs`) and **71** CI guard scripts (`ls scripts/check_*`). Both sides' values were correct for their own trees and the merged tree is the union of both sides' contributions — see §4a |

No other file conflicted, and no reasoning-control file was among them: the
second merge touched the counts, the CI matrix, the guard registry and the
plan registry, none of which this line's contract depends on.

## 2b. The third stage: `main` advanced by 79 commits, and published its own ADR-0048

The second merge was against `47f5572b1`. Before this line could be
published, `main` advanced a further **79 commits** to `f1b83a7a7`. The
advance carries the **CI/test-throughput recovery workstream** and, with it,
**M004's root-test-target family consolidation**: the root integration test
*modules* moved out of `tests/*.rs` into family directories as `.inc`
includes, behind `tests/<family>/mod.rs`. **296 root test files** were
renamed this way, into **23 family directories** — `agent_loop` (33 `.inc`),
`causal_observe_replay` (1), `command_git` (20), `config_provider` (12),
`core_daemon` (42), `eggwork_remote_execution_live` (1), `ide_protocols` (5),
`interactive_process_attach_resume` (1), `interactive_process_sessions` (1),
`interactive_terminal_tui` (1), `linux_git_test_support` (2),
`lsp_test_support` (4), `projection_replay` (11), `scheduler_cancellation` (1),
`scheduler_jobs` (33), `security_permission` (21), `server_feature` (6),
`session_family` (5), `tool_programs` (74), `tui_render` (41) and
`windows_sandbox_test_support` (2) — and the tree also gained ten new guards.
After the consolidation the root `tests/` directory holds **4** top-level
`tests/*.rs` and **317** `tests/*/*.inc`.

`git merge origin/main` on top of the merged tree produced **two conflicted
files**; the other nine files both sides had touched — `.github/workflows/ci.yml`,
`.opencode/skills/tui/SKILL.md`, `CHANGELOG.md`, `architecture/agent.md`,
`architecture/provider.md`, `architecture/session.md`, `architecture/storage.md`,
`architecture/tui.md`, `scripts/verify.sh` — **auto-merged** without conflict.

| File | Conflict | Resolution |
|---|---|---|
| `architecture/overview.md` | The two file-count rows of the Verified-counts table: this branch's 300 / 71 against `main`'s 321 / 76 | **Re-measured in the newly merged tree**, never picked: **321** integration test modules and **80** CI guard scripts. `main`'s row had also changed its **Source** — see §4b — so the number and the Source both had to be resolved, not only the number |
| `plans/registry.md` | Both sides inserted a subsystem row at the same slot: this line's *Provider reasoning control* row against `main`'s *CI and test throughput recovery* row | **Both rows kept.** `main`'s row keeps its exact position; this line's row follows it. Neither side's row was dropped or rewritten |

Two renames were resolved by git's rename detection, and **this line's two
edits to the moved files survived intact**: `tests/model_preference_convergence.rs`
→ `tests/core_daemon/model_preference_convergence.inc` and
`tests/connect_tui_restoration.rs` → `tests/tui_render/connect_tui_restoration.inc`.
Both edits are the same one-line addition — `reasoning_capability: None` on a
`ProviderModelDto` literal — and both are present in the merged tree at the
new paths. No reasoning-control *source* file conflicted in this stage.

**A second ADR collision had to be resolved in this stage**, and it is
recorded in §1: `main`'s new contained-Git decision took **ADR-0048**, which
this line had yielded to it in the first stage. Under the same rule, both of
this line's numbers yield again — ADR-0048 → **ADR-0049** and ADR-0049 →
**ADR-0050** — and `plans/adrs/README.md` gains the index entry `main`'s
ADR-0048 never had.

## 2c. The fourth stage: `main` advanced by 5 commits

`main` advanced five commits to `3e43d4144` — the landing of PR #170 (pause
the hosted Desktop E2E qualification) and PR #171 (absorb two interactive test
families into one, and repair two guards) — while this line was waiting for
its pull request to be reviewed.

The fourth merge produced **one conflicted file**: `plans/registry.md`, where
both sides again inserted a subsystem row at the same slot. **Both rows were
kept**, with `main`'s current row first — main's own PR #171 updated that
row's text, so `main`'s version was taken, not ours. `architecture/overview.md`
and `docs/test-ownership.toml` **auto-merged** without conflict, and no
reasoning-control file was among the merged set.

`main`'s PR #171 also **repaired the two guards this line's third stage had
to satisfy** — `scripts/check_test_ownership.py` and
`scripts/check_test_duplication.py` — and consolidated the
`interactive_process_sessions` and `interactive_terminal_tui` families into
`interactive_process_attach_resume`. This line's ownership entry for
`reasoning_overlay_narrowing` in `docs/test-ownership.toml` survived the
manifest repair, and all 51 guards still pass.

## 4c. The fourth stage's re-measurement — nothing moved

Every row of the Verified-counts table was **re-measured against its own
listed `Source`** in the newly merged tree, and **no row moved**: integration
test files **321**, CI guard scripts **80**, tool source files **84**,
architecture docs **91**, storage layout version **88**.

The family consolidation in PR #171 is the reason one of those rows needed
re-measuring rather than assuming: `tests/interactive_process_sessions/` and
`tests/interactive_terminal_tui/` were absorbed into
`tests/interactive_process_attach_resume/`, and their `mod.rs` files were
deleted. The row's Source counts `tests/*.rs` + `tests/*/*.inc`, so the
**`.inc` count is unchanged at 317** — the `.inc` files moved rather than
disappeared, and the deleted `mod.rs` files were never counted. The top-level
`tests/*.rs` count stays at **4**, and the row reads **321** exactly as it did
after the third stage.

## 2d. The fifth stage: `main` fixed the Linux CI breakage

`main` advanced five commits to `12999178a` — the landing of PR #173 ("adapt
the gated call sites to C003's typed live-read result") and PR #174 ("record
the landing, and correct the baseline the landing broke").

PR #173 is the **fix for the Linux CI breakage this line was inheriting**.
`main`'s C003 read-consistency change made `execute_contained_live_read_test_only`
return `GitReadFact<ManagedProcessResult>`, and the Linux-gated test-support
target was not updated with it — so `process-ipc` and
`restricted-userns-fails-closed` failed to compile on `main` itself, and this
line's pull request inherited those two red jobs. §7.7 records the inheritance
as `main`'s; PR #173 is `main` fixing it, and this fifth merge brings the fix
in.

The merge was **conflict-free**: `main`'s five commits touched only
`tests/linux_git_test_support/linux_git_c002_performance.inc`,
`tests/linux_git_test_support/linux_m004_process_ipc.inc` and
`plans/closure/linux-sandbox-admission-follow-up-corrective/003-status.md`,
and **no reasoning-control file was among them**. The Verified-counts table
was not touched by `main` and did not move.

## 2e. The sixth stage: `main` re-audited the counts and changed two rows' `Source`

`main` advanced nine commits to `ff82c68da`, and PR #175 among them is a
**documentation-surfaces pass**: it re-verified the nineteen user-facing guides,
the twenty-seven module guides, the architecture deep-dive index, **and the
count audit** — which changed the *method* behind two Verified-counts rows and
added a third.

This is the only stage whose conflicts were **seven files, all of them
architecture documents**, and every one of them was resolved by *union* rather
than by picking a side:

| File | Conflict | Resolution |
|---|---|---|
| `architecture/overview.md` | 5 hunks: the two rows whose `Source` changed, the new *Guard scripts (all)* row, the `scripts/` inventory count, and two dated-note blocks | `main`'s new `Source`s and its re-audited prose, with every number **re-measured in the merged tree** (§4e) |
| `architecture/authorization.md` | 4 hunks: the operation-count prose, the `network_intent_set` row, a duplicated `user_prompt_queue_*` block, and the *Source verification* section | `main`'s rebuilt, alphabetically sorted table — which already carries the eight queue rows, so `main`'s copy replaced the duplicate — plus `main`'s `network_intent_set` row, with the counts re-measured; our two dated verification paragraphs kept after `main`'s 2026-10-23 block |
| `architecture/model-adapters.md` | 4 hunks: the adapter table, the config-override prose, the invariants bullets, a split bullet | a **merged table** carrying both `main`'s `Priority` column and our `Capability record` column; `main`'s new suffix-tolerant prose kept alongside our *Reasoning Capability Record* section; both bullet sets kept |
| `architecture/protocol.md` | 3 hunks: the `CoreRequest`/`CoreResponse`/`CoreEvent` headings, the group lists, the reasoning-responses paragraph | the group lists **unioned** (`main`'s `NetworkIntentSet` and our three reasoning operations), with the variant counts and the enum line numbers **re-measured** in the merged tree |
| `architecture/storage.md` | 3 hunks: the layout-version sentence, the table/migration inventory, the migration-constant list | re-measured: layout **88**, 95 statements over 91 tables, 88 migrations `v1`–`v88`, with **both** `v84` (network intent) and `v88` (reasoning selection) named |
| `architecture/config.md` | 1 hunk | our reasoning paragraphs, with `main`'s corrected `schema.rs:1025` line number |
| `architecture/provider.md` | 2 hunks | our reasoning paragraphs, with `main`'s corrected `provider_core.rs:264` / `:249` line numbers |

`main`'s PR #175 also **fixed two real defects** in `main` itself — a
`codegg-document` pin that bypassed the workspace's `=0.1.0`, and a
`tool-program-harness` family target named after the member — which the merge
brings in.

## 4e. The sixth stage's re-measurement — two rows changed their `Source`

`main`'s count audit changed **two rows' `Source`** and added a third, so both
the Source and the number had to be resolved, and `main`'s own measurement
method had to be applied to the merged tree:

| Row | `main`'s own tree | Merged tree | Source |
|---|---|---|---|
| Architecture docs | 92 | **92** | `architecture/*.md` |
| Distinct production tool names | 75 | **75** | `impl Tool for …` `name()` across `src/` (the whole tree) |
| Tool source files | 83 | **84** | `src/tool/**.rs` (incl. `bash/`) |
| Guard scripts (all) | 76 | **80** | `scripts/check_*` |
| CI guard scripts | 36 | **40** | `.github/workflows/ci.yml` |
| Integration test files | 321 | **321** | `tests/*.rs` + `tests/*/*.inc`, `tests/common/` excluded |
| Storage layout version | 87 | **88** | `crates/codegg-core/src/storage/mod.rs` |
| `CoreRequest` variants | 263 | **266** | `crates/codegg-protocol/src/core.rs` |
| `CoreResponse` variants | 172 | **174** | `crates/codegg-protocol/src/core.rs` |
| Authorized operations | 257 | **260** | `crates/codegg-core/src/authorization/policy.rs` |

`main`'s tool-name method was reproduced exactly rather than approximated: the
merged tree carries **67** production `impl Tool for` blocks, **8** test-only
mocks, **62** literal names and **13** eggsact validator names — which is
`main`'s published **75**, confirmed rather than copied.

**One row was stale on `main` and is corrected rather than incremented.**
*Tool source files* read **83**, but the tree carries **84** `.rs` files under
`src/tool/`: `main`'s count excludes `src/tool/bash/tests/git_fixture.rs`, and
`main`'s own listed `Source` (`src/tool/**.rs`, incl. `bash/`) does not
exclude it. `main`'s count-audit record documents no such exclusion.

## 3. The indicator's re-landing

The working strip is gone from the merged tree, so `render_working_strip`,
`working_strip_rows` and the strip's row 0 no longer exist. The indicator
moved to the **footer status bar's activity chain**:

```
agent:build  reasoning: high
agent:build  reasoning: off
agent:build  reasoning: default
```

`build_status_summary` pushes the chip **unconditionally** from
`ui_state.reasoning_mode`, and `src/tui/components/status_bar.rs` never
mentions the reasoning state — so the bar's height and its width budget
cannot become a function of the selection. The status bar is a fixed-height
row, so a chip cannot reflow the transcript the way a row could.

The guard was rewritten with it, because its fourth rule was about the
working strip's row count:

- the old rule ("`working_strip_rows` must not mention the reasoning state")
  was **removed** — the function it policed no longer exists, and a guard that
  fails on a deleted symbol is a guard that describes a tree that is not
  there;
- the new rule checks two properties: the chip is pushed unconditionally
  (no `if`, `match` or loop whose condition reads the reasoning state), and
  the status bar component never mentions the reasoning state at all.

The self-test was extended with two cases for the new rule — a conditional
chip, and a status bar made to depend on the reasoning state — and **both are
rejected**. Writing the rule exposed a bug in the guard's own matcher: the
regex ended in `\breasoning\b`, and an underscore is a word character, so
`\breasoning\b` did **not** match `reasoning_mode` — the one spelling the
code actually uses. The first version of the new rule therefore passed its
own injected violation. The trailing boundary is now deliberately absent, and
the self-test proves the case is caught.

## 4. The Verified-counts table, re-measured

The table's numbers are **measured**, never merged: each row was re-derived
from its own listed `Source` in the merged tree. Six rows moved, and the
rest were confirmed unchanged by measurement.

| Row | `main`'s value | Merged tree | Why it moved |
|---|---|---|---|
| `CoreRequest` variants | 263 (239 struct + 24 unit) | **266 (240 struct + 26 unit)** | M002's three variants (`ReasoningCapabilities`, `ReasoningPreferenceSet`, `ReasoningPreferenceGet`) |
| `CoreResponse` variants | 172 (171 struct + 1 unit) | **174 (173 struct + 1 unit)** | M002's two variants |
| Authorized operations | 257 | **260** | M002's three operations |
| `Storage layout version` | 87 | **88** | The renumbered migration |
| Integration test files | 296 | **298** | `main`'s own contributions (`tests/host_admission_manifest.rs`, `tests/tool_program_c002_delegated_ceiling.rs`). **This line contributes none**: its new suite, `crates/codegg-core/tests/reasoning_overlay_narrowing.rs`, lives under that crate's own `tests/` directory, which the row's `tests/*.rs` Source — the repository **root** `tests/` directory — does not count. A later measurement pass had attributed this row's movement to that file; the attribution was wrong and is corrected here. |
| CI guard scripts | 65 | **70** | `main`'s tree carries 66; this line contributes four |

Confirmed unchanged by measurement: architecture docs **90**, tool
registrations **67**, distinct production tool names **61**, tool source
files **83**, LSP server definitions **39**, `codegg-core` modules **54**,
`AppEvent` variants **61**, `CoreEvent` variants **92**, `ProjectionEvent`
variants **47**, identity newtypes **19**, built-in slash commands **168**,
built-in agents **10**, bundled themes **50**, TUI state modules **33**
(34 `.rs` files incl. `mod.rs`), database tables **95 statements / 91
tables**, Git operation variants **54**, Git risk classes **11**, and
workspace member crates **11** (+ root).

Note that `main`'s own table read **65** CI guard scripts while its tree
carried **66**; the merged tree's **70** is the measured value, and the
`Full check_* inventory` line was corrected with it.

## 4a. The second stage's re-measurement

The second merge (`main` at `47f5572b1`) moved the two file-count rows again,
and they were **re-measured in the newly merged tree** rather than picked from
either side:

| Row | `main` at `47f5572b1` | Merged tree | Why |
|---|---|---|---|
| Integration test files | 300 | **300** (`ls tests/*.rs`) | `main` contributes the two new blackboard suites (`tests/blackboard_trajectory_m008.rs`, `tests/blackboard_economics_m008.rs`); **this line contributes none**, because its new suite lives at `crates/codegg-core/tests/reasoning_overlay_narrowing.rs`, under the crate's own `tests/` directory, which the root `tests/*.rs` Source does not count — the same distinction `main`'s own M008 pass drew for `tests/common/blackboard.rs` |
| CI guard scripts | 67 | **71** (`ls scripts/check_*`) | `main`'s 67 (which includes the new `scripts/check_blackboard_contract_matrix.py`) plus this line's four |
| Architecture docs | 91 | **91** (`architecture/*.md`) | `main`'s alone: this line adds no architecture document |

Every **code**-count row is unchanged by the second merge, and each was
re-checked against its own listed `Source` rather than assumed — the blackboard
M008 added no production code and no schema migration.

**A correction the second stage forced.** The first stage's note attributed
the integration-test-files row's movement to this line's new overlay suite.
That attribution was **wrong**: the row's `Source` is the repository **root**
`tests/` directory, and this line's suite is under `crates/codegg-core/tests/`.
Measured file-by-file, the root `tests/` directory held **280** files at the
merge-base, **298** at `8cee65934` (all eighteen of the additions `main`'s),
**300** at `47f5572b1`, and **300** in the merged tree — this line adds
nothing to it. The table above carries the corrected attribution.

## 4b. The third stage's re-measurement — a row's *Source* changed, and another row was stale on `main`

The third stage changed a Verified-counts row's **Source**, not merely its
number, so the row had to be resolved on both axes.

| Row | Before | After | Source | Why |
|---|---|---|---|---|
| Integration test files | 300 | **321** | `tests/*.rs` + `tests/*/*.inc` — the root integration test *modules*, with the `tests/common/` helper tree excluded | `main`'s M004 family consolidation moved the root test *modules* into family directories as `.inc` includes, so `ls tests/*.rs` alone now reports **4** and no longer measures the suite. Measured in the merged tree: 4 `tests/*.rs` + 317 `tests/*/*.inc` = **321** |
| CI guard scripts | 71 | **80** | `scripts/check_*` | `main`'s tree carries **76** (ten new guards from the CI/test-throughput workstream), and this chain contributes four: `check_reasoning_intent_fidelity.py`, `check_reasoning_vocabulary_open.py`, `check_model_profile_capability_mirror.py`, `check_tui_reasoning_surface.py` |
| Tool source files | 83 | **84** | `src/tool/**.rs` (incl. `bash/`) | **Stale on `main`, and corrected rather than incremented.** `main` added `src/tool/bash/tests/git_fixture.rs` without re-measuring the row, so the row read 83 against a tree of 84. Measured in the merged tree: **84** |

Every other row was **confirmed unchanged** by measurement against its own
listed `Source` — architecture docs **91**, tool registrations **67** (54
`register` + 13 `register_boxed`), distinct production tool names **61**, LSP
server definitions **39**, `codegg-core` modules **54**, `AppEvent` variants
**61**, `CoreRequest` variants **266** (240 struct + 26 unit), `CoreResponse`
variants **174** (173 struct + 1 unit), `CoreEvent` variants **92**,
`ProjectionEvent` variants **47**, authorized operations **260**, identity
newtypes **19**, built-in slash commands **168**, built-in agents **10**,
bundled themes **50**, TUI state modules **33** (34 `.rs` files incl.
`mod.rs`), database tables **95 statements / 91 tables**, storage layout
version **88**, Git operation variants **54**, Git risk classes **11**, and
workspace member crates **11** (+ root).

**This line contributes no root integration test file, and the merge is why
that is still true.** Its overlay suite lives under
`crates/codegg-core/tests/reasoning_overlay_narrowing.rs`, which the row's
`Source` does not count, and the two root test files it edited were *moved*
by `main`'s consolidation rather than added — so the merge kept both edits
without adding a file. The row's movement is entirely `main`'s, which is the
same attribution correction the second stage forced, now confirmed against a
different tree shape.

## 5. What was *not* changed

- The reasoning contract itself: the open label vocabulary, the
  daemon-owned durable selection, the compare-and-swap revision gate, the
  per-model capability record and its four dimensions, the fail-closed
  admission policy, the narrowing-only overlay, the option derivation, and
  the `/reasoning` write path are untouched by the merge.
- `main`'s own ADR-0044 and ADR-0045, and every other reference to them
  anywhere in the tree. Only this line's references were renumbered.
- The two named residuals from the milestone records: M005's overlay is
  inert machinery (the pinned upstream `/v1/models` response publishes no
  capability block, so its daemon-side install stays deliberately unwired),
  and the WS-attach frontend has no channel for a core request, so
  `/reasoning` there reports that the selection cannot be stored.

## 6. Registry updates

Required in `plans/registry.md` and
`plans/subsystems/provider-reasoning-control-roadmap.md`:

- register this supplemental record against the `provider-reasoning-control`
  line, with no new milestone;
- record the ADR renumbering (ADR-0044 → **ADR-0048** → **ADR-0049**, ADR-0045
  → **ADR-0049** → **ADR-0050**) and the migration renumbering (v84 → **v88**,
  layout 87 → 88) so a later reader does not read the numbers as a collision;
- record the indicator's move from the working strip to the footer status
  bar.

## 7. Qualification of the merged tree

The merged tree was qualified on the machine that produced the merge. Two
findings came out of that qualification and both are recorded rather than
smoothed over.

### 7.1 Two compile defects the merge introduced, and the compiler caught them

Both were auto-merge artifacts in files the two sides had **both** touched, and
both are the kind a type checker catches and a human reading a diff does not:

1. `src/core/daemon_ops.rs` — `RuntimePreferenceDto` gained `network_intent`
   from `main`, and the branch's own construction site added the field. The
   auto-merge therefore produced the field **twice** in one struct literal
   (`error[E0062]: field network_intent specified more than once`). The second
   copy was removed.
2. `crates/codegg-core/src/approval.rs` — the `runtime_preferences` upsert
   ended the merge with **11 columns, 10 placeholders and 11 binds**. The
   `VALUES` clause was corrected to `?1 … ?11`. This one is a **runtime**
   defect: the statement still compiles, so nothing in the build would have
   caught it, and SQLite would have rejected the write at the first
   `set_reasoning_selection` call. The 32 files changed on both sides were
   audited for the same class; the only other flags were false positives (the
   branch's own CAS upsert legitimately binds `?11` in its `WHERE` clause, and
   two test fixtures use literal `VALUES`).

The splice that produced the schema conflict had also dropped
`migrate_v87`'s closing braces; the compiler caught that too and they were
restored.

### 7.2 Clippy: the merged tree's status is `main`'s status, exactly

`cargo clippy --workspace --all-targets --locked -- -D warnings` fails in the
merged tree with **seven** findings. All seven are in `main`'s own
Linux-execution-isolation code — `src/interactive_process.rs` (two),
`src/lsp/m004.rs`, `src/managed_process.rs` (three) and
`src/security/sandbox.rs` — and none is in a file this line touched.

They are **pre-existing on `main`**, and this was measured rather than
assumed: `cargo clippy -p codegg --lib --locked -- -D warnings` was run on a
**clean checkout of `main` at `8cee65934`**, and it produced the **identical
seven findings at the identical seven locations**. The merged tree introduces
**zero** new clippy findings.

They are also **macOS-only**, and the reason is the platform gate rather than
the code: the dead-code findings are `#[cfg(target_os = "linux")]` functions
whose only caller is itself Linux-gated, and the `needless_return` finding
sits in a `#[cfg(not(target_os = "linux"))]` branch. Linux CI never compiles
either side of the pair, so the repository is green there while a macOS
`--all-targets` clippy run is not. This is `main`'s pre-existing macOS gap and
is **not** this line's to fix; it is recorded here so a later reader does not
attribute it to the merge.

The five crates this line actually changed — `codegg-core`, `codegg-config`,
`codegg-protocol`, `codegg-providers` and `codegg-client` — are
**clippy-clean** under `--all-targets --locked -- -D warnings`, exit 0 and
zero findings.

### 7.3 The rest of the qualification

- `cargo check --workspace --all-targets --locked`: **exit 0**, zero errors.
- `cargo fmt --check --all`: **clean**.
- All **42** guards invoked by `scripts/verify.sh quick` pass on the merged
  tree (41 after the first merge, plus `main`'s blackboard guard after the
  second), and the four guards this line introduces still reject every
  violation their `--self-test` injects. A set comparison of the guard
  invocations before and after the second merge shows **one addition and no
  removal**.
- `scripts/check_adr_integrity.py` confirms the renumbered ADR identifiers are
  unique and their local paths resolve.
- `cargo test -p codegg-core --lib`: **928 passed**.
- `cargo test -p codegg-core --test reasoning_overlay_narrowing`: **6 passed**.
- `cargo test -p codegg --lib core::daemon_turns`: **22 passed**.
- `cargo test -p codegg --lib tui::commands::reasoning`: **7 passed**.
- `cargo test -p codegg-providers --lib eggpool`: **30 passed**.
- `codegg validate --config codegg.example.jsonc`: **valid**. `main` did not
  touch that file; only this line's 24-line `model_profile.<model>.
  reasoning_capability` example differs from `main`'s copy.

The second merge (§2a) was qualified the same way on the twice-merged tree:
`cargo check --workspace --all-targets --locked` **exit 0** with zero errors,
`cargo fmt --check --all` **clean**, and all **42** guards pass. The second
merge changed no production code, so the suite results above still hold for
it; what it did change — the counts, the CI matrix, the guard registry and
the plan registry — was verified directly.

### 7.4 The whole workspace suite, and the six non-passes in it

`cargo nextest run --workspace --locked --profile ci --no-fail-fast` on the
merged tree: **16,060 of 16,067 passed** (14 slow, 8 skipped), with **six
failures and one 300-second timeout**.

Every one of the seven was run **in isolation on a clean checkout of `main`
at `8cee65934`**, and the result is **identical** — five fail and two pass,
the same five and the same two:

| Test | Merged tree (whole suite) | Merged tree (isolated) | Clean `main` (isolated) |
|---|---|---|---|
| `core::daemon_refresh::tests::session_projection_rebuilds_owner_scoped_live_jobs` | FAIL | FAIL | FAIL |
| `core::daemon_refresh::tests::session_projection_snapshot_includes_in_process_live_child_instances` | FAIL | FAIL | FAIL |
| `managed_process::tests::offline_required_scheduler_carried_intent_refuses_disabled_sandbox_before_spawn` | FAIL | FAIL | FAIL |
| `scratch_cache_root_contract::the_default_cache_root_is_the_home_directory_cache` | FAIL | FAIL | FAIL |
| `scratch_cache_root_contract::an_explicit_cache_home_is_the_root_the_allocator_uses` | FAIL | FAIL | FAIL |
| `agent::asset_refresh::tests::same_scope_requests_coalesce_to_one_publication` | **TIMEOUT** (300 s) | **PASS** (0.15 s) | **PASS** (0.14 s) |
| `tool_program_m015_daemon_failpoints::active_child_is_reattached_from_pending_wait_checkpoint` | FAIL (62 s) | **PASS** (9.1 s) | **PASS** (5.9 s) |

So the **five consistent failures are `main`'s**, not this line's, and the
**two that differ between the whole-suite and the isolated run are host-load
artifacts**: this machine was running thirty-plus concurrent cargo builds from
other sessions, and both tests pass in well under ten seconds in isolation on
**both** trees. `main`'s own closure record
(`plans/closure/managed-agent-scratch/007-status.md`) already proves the two
`daemon_refresh` cases the same way, in a clean worktree at `origin/main`,
with the same `InvalidRootPath("/tmp")` cause — on macOS `/tmp` is a symlink
to `private/tmp`, which the workspace registration rightly refuses.

None of the seven is in a file this line touched, and none of the five
consistent failures has a cause this line introduced. **The merge introduces
zero new test failures.**

The **feature-gated lane** was run on `main` after the fast-forward —
`cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support
--profile ci --no-fail-fast` — and returned **12,710 of 12,716 passed**
(7 skipped), with the **same five consistent failures and the same one
host-load timeout** and nothing else. The server, plugin and fake-LSP
surfaces the merge could have disturbed are therefore qualified too, and the
answer is the same: nothing new.


### 7.5 A third defect the merge introduced — caught by the merged tree's own tests

`approval::tests::reasoning_selection_migration_is_additive_and_preserves_rows`
is **this line's own test**, and it failed in the merged tree with
`no such column: network_intent`.

The cause was the renumbering's consequence, and it is worth stating plainly
because it is the kind of thing a renumbering note does not warn about: the
test's fixture built a `runtime_preferences` table that **predated `main`'s
v84 network-intent column**, and the merged store's `get()` — which now
selects that column — failed on the fixture.

The fixture was corrected to state the **real** migration order rather than an
obsolete one: the reasoning migration is **v88** in the merged tree and runs
**after** main's v84, so a database in the state that migration expects
already has `network_intent`. The test was **strengthened, not weakened** — it
now also asserts that main's column and its stored value survive the reasoning
migration, which is a property the pre-merge test never asserted and which
the merge made meaningful.

This defect was invisible to the compiler, to clippy, and to every static
guard. It was found only because the merged tree was **tested as a tree**, and
it is the concrete reason this record exists rather than being folded into the
milestone closures.


## 7.6 The third stage's qualification

The third merge was qualified **as a tree**, for the same reason the first
two were: a merge can break what each side proved on its own. The tree
qualifies cleanly, and the findings it produced are all **`main`'s**, proved
rather than assumed.

| Check | Result |
|---|---|
| `cargo check --workspace --all-targets --locked` | **pass** — exit 0, zero errors (2 m 55 s) |
| `cargo fmt --check --all` | **clean** |
| Every guard invoked by `scripts/verify.sh quick` (51 scripts) | **all pass** |
| Every guard's `--self-test` | **all pass**, except one — see below |
| `cargo clippy -p codegg-core -p codegg-config -p codegg-protocol -p codegg-providers -p codegg-client --all-targets --locked -- -D warnings` | **pass** — exit 0, zero findings |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | **7 findings** in the `codegg` lib and 8 in its lib test — **identical on clean `origin/main`**, at the same locations |
| `cargo nextest run -p codegg-core --profile ci -E 'test(reasoning) or binary(reasoning_overlay_narrowing)'` | **22 passed**, including all 6 overlay-narrowing cases and both compare-and-swap cases |
| `cargo nextest run -p codegg --profile ci -E 'test(core::daemon_turns) or test(tui::commands::reasoning)'` | **29 passed** |
| `cargo nextest run -p codegg-providers --profile ci -E 'test(eggpool)'` | **30 passed** |
| `cargo nextest run --workspace --locked --profile ci --no-fail-fast` | **15,862 of 15,870 passed** (1 leaky, 9 skipped) — 7 failed and 1 timed out, and **every one of the eight is `main`'s**, reproduced on a clean checkout of `origin/main` at `f1b83a7a7` (§7.7) |
| `cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support --profile ci --no-fail-fast` | **12,424 of 12,432 passed** (8 skipped) — the **same seven failures and the same one timeout**, and nothing else |
| `scripts/check_adr_integrity.py` | **pass** — active ADR IDs unique, local paths resolve |

## 7.7 Every non-pass in the workspace suite is `main`'s

The workspace suite returned **7 failures and 1 timeout**. Each was run on a
**clean checkout of `origin/main` at `f1b83a7a7`** — the exact tree this branch
merges — and each reproduces there:

| Test | Merged tree | Clean `origin/main` |
|---|---|---|
| `core::daemon_refresh::tests::session_projection_rebuilds_owner_scoped_live_jobs` | FAIL | FAIL |
| `core::daemon_refresh::tests::session_projection_snapshot_includes_in_process_live_child_instances` | FAIL | FAIL |
| `managed_process::tests::offline_required_scheduler_carried_intent_refuses_disabled_sandbox_before_spawn` | FAIL | FAIL |
| `scratch_cache_root_contract::the_default_cache_root_is_the_home_directory_cache` | FAIL | FAIL |
| `scratch_cache_root_contract::an_explicit_cache_home_is_the_root_the_allocator_uses` | FAIL | FAIL |
| `codegg::command_git git_read_consistency::a_repeat_read_of_an_unchanged_repository_is_served_from_the_capture_cache` | FAIL | FAIL |
| `codegg::contained_worktree_c008 the_grant_names_only_the_administration_directory` | FAIL | FAIL |
| `agent::asset_refresh::tests::same_scope_requests_coalesce_to_one_publication` | TIMEOUT (300 s) | **PASS in 0.029 s** |

The two that are *new relative to the previous reconciliation* are `main`'s
new work: the read-consistency contract from C003 and the contained-worktree
grant from C008. Neither test references anything this branch changed — no
approval store, no reasoning capability record, no model profile, no storage
layout, no schema — and every file they exercise
(`tests/command_git/git_read_consistency.inc`,
`tests/contained_worktree_c008.rs`, `crates/egggit/`,
`src/git_contained_worktree.rs`, `src/security/sandbox/policy.rs`) is
**byte-identical to `origin/main`**.

The timeout is a host-load artifact of a machine running thirty-plus
concurrent cargo builds, and it passes in 0.029 s in isolation on `main`.

The **feature-gated** lane — `server,plugins,lsp-test-support`, the surfaces
the merge could have disturbed that the default lane does not build — returned
**12,424 of 12,432 passed**, with the **same seven failures and the same one
timeout** and nothing else. The server, plugin and fake-LSP surfaces are
qualified too, and the answer is the same: nothing new.

**So the third merge introduces zero new test failures.**

### The hosted `verify` job's one failure is a flake `main` has already recorded

The hosted `CI / verify` job for this pull request failed on exactly one test:
`codegg-core goal::checkpoint::tests::test_append_checkpoint_update`
(`assertion failed: content.contains("Phase 1")`). It is a **known flake of
`main`'s**, recorded in three of `main`'s own closure records —
`plans/closure/agent-convergence/002-status.md`,
`plans/closure/eggplan-assessment-integration/002-status.md` and
`plans/closure/decision-model-extraction-runtime/005-status.md`, the last of
which records the *same* assertion with the *same* resolution: "rerunning
that test alone passed, and the subsequent complete canonical nextest sweep
passed."

Re-run in isolation on this branch, it **passes**:

```
PASS [   0.013s] (1/1) codegg-core goal::checkpoint::tests::test_append_checkpoint_update
Summary [   0.015s] 1 test run: 1 passed, 1128 skipped
```

The test and the code it exercises are **byte-identical to `origin/main`**, and
`main`'s own hosted `verify` job at `f1b83a7a7` passed — the flake simply
fired on this run. The two hosted Linux jobs that fail
(`process-ipc` and `restricted-userns-fails-closed`) fail on `main` at that
same SHA too, with the same compile error in
`tests/linux_git_test_support/` — `main`'s C003 read-consistency change made
`execute_contained_live_read_test_only` return
`GitReadFact<ManagedProcessResult>` without updating that Linux-gated target.

**The one new guard violation the third stage produced, and its fix.** `main`
added `scripts/check_test_ownership.py` and `docs/test-ownership.toml` with
its CI/test-throughput workstream — a manifest that maps source paths to the
test targets that cover them, and a guard that refuses a test target **no
group owns**, because an unowned target can never be selected and a scoped run
would silently stop covering it. This line's cross-crate proof,
`crates/codegg-core/tests/reasoning_overlay_narrowing.rs`, was **unowned**:
the guard failed on the merged tree, exactly as it should have. The fix is
ownership, not a suppression — the target is added to the existing
`core_context_decision` group, which already covers
`crates/codegg-core/src/model_profile/**`, the resolution function the proof
drives. The overlay *parsing* the proof exercises lives in `codegg-providers`
(`eggpool.rs`), and a change there pulls `codegg-core` in as a dependent and
therefore contributes this target as part of that member's **full** surface,
so neither side of the path is left uncovered.

**The self-test that fails is `main`'s.** `scripts/check_provider_multi_surface_dispatch.py --self-test`
reports `self-test could not inject regression: direct Responses routed
through the hosted subsystem` — and the **identical** message appears on a
clean checkout of `origin/main` at `f1b83a7a7`. The guard's own other five
injected regressions are caught on both trees. The self-test is stale against
`main`'s own new hosted-Responses route; this line did not touch that guard,
that route, or any file it inspects.

**The clippy findings are `main`'s, and they are macOS-only.** All seven live
in `src/interactive_process.rs`, `src/managed_process.rs`, `src/lsp/m004.rs`
and `src/security/sandbox.rs` — **files this line did not touch** — and a
clean checkout of `origin/main` at `f1b83a7a7` produces the **same seven**
findings with the **same messages**. They are dead-code and unused-variable
lints on Linux-gated code paths; the hosted Linux runner never compiles the
other side of those `cfg` pairs, so CI is green while a macOS
`--all-targets` clippy is not. The merge introduces **zero** new clippy
findings.

**Four change-triggered guards fail on the merged tree, and on `main` too.**
`check_execution_subject_ownership.py`, `check_policy_surface.py`,
`check_sandbox_policy_wiring.py` and `check_tool_broker_boundary.py` are
change-triggered guards that `scripts/verify.sh quick` and the hosted CI job
do not invoke. Each produces **byte-identical output** on the merged tree and
on a clean checkout of `origin/main`, and each points at files this line did
not touch (`src/managed_process.rs`, `src/tool_batch.rs`,
`src/scheduler/executors.rs`, `src/permission/reviewer.rs`,
`src/tool/extension.rs`, `src/tool/git_read.rs`, `src/tool/verify.rs`). They
are `main`'s open items, not this line's.

## 7.8 The hosted CI verdicts

The hosted lanes for this pull request were run on the tree that carried the
fifth merge, and they are the authoritative gate for this repository:

| Hosted lane | Result |
|---|---|
| `CI / verify` | **pass** (27 m 59 s) |
| `Linux M004 process and IPC qualification` (`process-ipc`) | **pass** (38 m 07 s) |
| `restricted-userns-fails-closed` | **pass** (14 m 25 s) |
| `Windows process launcher` | **pass** (30 m 48 s) |
| `Linux C006 protected mount qualification` | **pass** |
| `native-probe-contract` | **pass** |
| `c014-negative-reproduction`, `git-cost-pilot` | skipped by their own conditions |
| GitGuardian Security Checks | **fail** — `main`'s false positive, see below |

The two Linux jobs had been red on this branch because they inherited
`main`'s compile error in `tests/linux_git_test_support/`; the fifth merge
brought in `main`'s own fix, and they are green here now.

**GitGuardian's one finding is `main`'s, and it is a false positive.** It
reports a *Generic CLI Secret* at `crates/egggit/src/config_surface.rs:1096`,
which is the hostile-git-config **fixture** the
`detects_url_scoped_credential_helper` test builds:

```rust
"[credential \"https://example.com\"]\n\thelper = store --password=hunter2\n"
```

The file is **absent from this line's feature commit** (`6d36a1888`) — it
arrives only through the merge of `main` — and the identical finding appears
on **`main`'s own PR #169**, which is the pull request that added the file.
`main` is not protected, so there are no required status checks in this
repository and the check is advisory. It is left alone here on purpose: the
string is another workstream's test fixture, and changing it in this line
would be scope creep into code this line does not own.

## Registry narrative

Moved verbatim from the `plans/registry.md` row for **Provider reasoning control** on 2026-10-23, so that the registry stays an index.

`main` was reconciled **twice** — 439 commits at `8cee65934`, then six more at `47f5572b1` (the agent-workstream blackboard's M008) after it advanced during the merged tree's qualification. The first merge had **eight conflicts**; the second had **one** (`architecture/overview.md`'s Verified-counts table), with `.github/workflows/ci.yml`, `plans/registry.md` and `scripts/verify.sh` auto-merging. The reasoning contract survived intact, but three allocations had to yield to `main`: **both ADR numbers** (this line's ADR-0044 → **ADR-0048** and ADR-0045 → **ADR-0049**, because `main` had already published both; main wins the number, and the decision content, `Status: accepted`, the acceptance date and the accepting authority are unchanged — 68 references across 24 files updated, `check_adr_integrity.py` green); the **migration number** (this line's `migrate_v84` → **v88**, `STORAGE_LAYOUT_VERSION` 87 → 88, because `main` had published v84 through v87; the renumbering is additive and changes the meaning of no published database); and the **indicator's surface** — `main` deleted the working strip as part of its buffer-chrome roadmap, so M004's indicator was re-landed in the footer status bar's activity chain, and its guard's fourth rule was rewritten (the old rule policed a function that no longer exists) with two new self-test cases, both rejected. The Verified-counts table was **re-measured in the merged tree** rather than merged, twice: six rows moved on the first merge (`CoreRequest` 263 → 266, `CoreResponse` 172 → 174, authorized operations 257 → 260, layout 87 → 88, integration test files 296 → 298, CI guard scripts 65 → 70) and the two file-count rows moved again on the second (**300** test files and **71** guards, the union of both sides, measured not picked). **A corrected attribution:** that integration-test-files row's first-stage movement was wrongly credited to this line's new overlay suite; the row's `Source` is the repository **root** `tests/` directory and the suite lives under `crates/codegg-core/tests/`, which it does not count — the movement was entirely `main`'s. In two queue tests the **branch's** hard-coded `STORAGE_LAYOUT_VERSION` equality was taken over by `main`'s — the branch's equality broke the build for C002 and again for M006 without testing anything, so the merged tree asserts the durable migration marker against the constant plus a floor. The merged tree was then **qualified as a tree**, and that found **three merge-induced defects** that each side's own tree had proved clean: a `network_intent` field duplicated in one struct literal in `src/core/daemon_ops.rs` (caught by the compiler), a `VALUES` clause in `crates/codegg-core/src/approval.rs` left with 11 columns and 10 placeholders (a **runtime** defect no compiler sees), and this line's own migration fixture still describing the pre-merge column order (caught by the merged tree's tests). All three are fixed. The merged tree's six workspace-clippy findings and its whole-workspace suite's six failures plus one timeout were each **reproduced identically on a clean checkout of `main` at `8cee65934`**, so the merge introduces **zero** new clippy findings and **zero** new test failures; the two suite cases that differ between the whole-suite and isolated runs are host-load artifacts of a machine running thirty-plus concurrent cargo builds, and both pass in isolation on both trees. Two named residuals stand unchanged: M005's overlay is inert machinery (the pinned upstream `/v1/models` publishes no capability block) and the WS-attach frontend has no channel for a core request. **A third reconciliation with `main` was needed on 2026-10-23** before this line could be published: `main` had advanced by **79 commits** (to `f1b83a7a7`), carrying the CI/test-throughput recovery workstream and its M004 root-test-target **family consolidation**. That advance changed the **integration test files** row's *Source*, not just its number — `ls tests/*.rs` alone now reports 4 because the root integration test *modules* moved into family directories, so the row now counts `tests/*.rs` + `tests/*/*.inc` with `tests/common/` excluded, and reads **321**. The **CI guard scripts** row is **80** (`main` carries 76, this chain four). The **tool source files** row was **stale on `main`** and is corrected rather than incremented: it read 83, but `main` added `src/tool/bash/tests/git_fixture.rs` without re-measuring it. That file is a **test fixture**, not tool source, so the row stays **83** and its `Source` now says so — the measure and the row's label agree, and the table again agrees with `scripts/measure_overview_counts.py`, which is how the table says to re-measure it. This chain contributes **no** root integration test file — its overlay suite lives under `crates/codegg-core/tests/`, which the row's Source does not count, and the two root test files it edited were *moved* by `main`'s consolidation to `tests/core_daemon/model_preference_convergence.inc` and `tests/tui_render/connect_tui_restoration.inc`, so the merge kept both edits without adding a file. **A second ADR collision had to be resolved:** `main` published its own ADR-0048 (contained Git execution for managed-worktree creation) while this line was qualifying, so **both** of this line's numbers yield again — ADR-0048 → **ADR-0049** and ADR-0049 → **ADR-0050** — under the same rule (main wins every number; the decision content, `Status: accepted`, the acceptance date and the accepting authority are unchanged; only the identifier and the file name move, with a dated renumbering note) — measured after the second renumbering, **28 files** carry the current identifiers at **134** occurrences. `main`'s ADR index also carried **no entry for its own ADR-0048**; that gap is filled in the same pass. The third merge had **two conflicts** (`architecture/overview.md`'s Verified-counts table and `plans/registry.md`), both resolved by re-measurement rather than by picking a side: the counts table was re-measured row by row against its own listed `Source` in the merged tree, and the registry kept **both** sides' rows. The merged tree was then qualified as a tree: `cargo check --workspace --all-targets --locked` exit 0 with zero errors, `cargo fmt --check --all` clean, all **51** guards invoked by `scripts/verify.sh quick` pass with their self-tests rejecting their injected violations, `cargo clippy` on the five crates this line touched is clean, and the workspace suite returns **15,862 of 15,870 passed** (the feature-gated lane: 12,424 of 12,432, with the same seven failures and the same one timeout, and nothing else). Every one of the suite's **7 failures and 1 timeout is `main`'s** — each was reproduced on a clean checkout of `origin/main` at `f1b83a7a7`, the exact tree this branch merges — so the merge introduces **zero** new clippy findings, **zero** new guard failures and **zero** new test failures. The hosted `verify` job's single failure is `main`'s recorded `goal::checkpoint` flake, which passes in isolation here. **A fourth reconciliation was then needed**, because `main` advanced **five commits** to `3e43d4144` (the landing of PR #170, pausing the hosted Desktop E2E qualification, and PR #171, absorbing two interactive test families into one and repairing two guards) while the pull request was waiting for review. That merge had **one conflict** (`plans/registry.md`, both sides inserting a subsystem row at the same slot) and both rows were kept, with `main`'s current row first — `main`'s own PR #171 had updated that row's text, so `main`'s version was taken. `architecture/overview.md` and `docs/test-ownership.toml` auto-merged. Every Verified-counts row was **re-measured against its own listed `Source`** and **none moved**: integration test files **321**, CI guard scripts **80**, tool source files **84**, architecture docs **91**, storage layout version **88** — PR #171's family consolidation moved the `.inc` files rather than removing them, so the row's count is unchanged, and all **51** guards still pass with this line's `reasoning_overlay_narrowing` ownership entry intact through `main`'s manifest repair. **A fifth reconciliation was then needed and was conflict-free**: `main` advanced **five commits** to `12999178a`, landing PR #173 — which is **`main`'s own fix for the Linux-gated test-support compile error this line's pull request had been inheriting** — and PR #174. `main`'s five commits touched only `tests/linux_git_test_support/` and one closure record, no reasoning-control file among them, and the Verified-counts table did not move. **A sixth reconciliation was then needed**: `main` advanced **nine commits** to `ff82c68da`, including a documentation-surfaces pass (PR #175) that **changed two rows' `Source`** — the distinct-tool-names row now scans the whole `src/` tree, and the CI-guard row is now counted from `.github/workflows/ci.yml` — and **added a third row** (*Guard scripts (all)*). All **seven** conflicted files were architecture documents, and every one was resolved by **union** rather than by picking a side. Every row was then re-measured against its own listed `Source` using `main`'s own method, which reproduces `main`'s published figures exactly: architecture docs **92**, distinct production tool names **75**, tool source files **84**, guard scripts (all) **80**, CI guard scripts **40**, integration test files **321**, storage layout version **88**, `CoreRequest` variants **266**, `CoreResponse` variants **174**, authorized operations **260**. **One row was stale on `main` and is corrected rather than incremented** — *tool source files* read 83 against a tree of 84, because `main`'s count excludes `src/tool/bash/tests/git_fixture.rs` and its own listed `Source` does not. The hosted lanes are **green**: `verify` pass, `process-ipc` pass, `restricted-userns-fails-closed` pass, `process-launcher` pass. |
