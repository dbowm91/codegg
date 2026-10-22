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

Repository baseline reconciled: `origin/main` at `8cee65934` ("fix(server):
map the process-denial refusal to 409, not an unhandled variant"), which is
**439 commits** ahead of this branch's merge-base `c93211542` ("Merge pull
request #153 from dbowm91/docs/core-family-counts-m006"). The branch head at
merge was `6d36a1888`.

Changed-file surface: the branch carried 90 changed files against the
merge-base, and **eight files were changed on both sides**.

**The reconciliation took two stages, because `main` moved while it was in
flight.** The first merge was against `8cee65934`. During the merged tree's
qualification — which, on this host, took hours — `main` advanced a further
**six commits** to `47f5572b1` (the agent-workstream blackboard's M008:
`3477bc589`, `4d327add9`, `3549d9216`, `e1b898cb8`, `2a797c86c`,
`47f5572b1`). The second merge brought that in, and is recorded in §2a and
§4a.

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
- record the ADR renumbering (ADR-0044 → **ADR-0048**, ADR-0045 →
  **ADR-0049**) and the migration renumbering (v84 → **v88**, layout
  87 → 88) so a later reader does not read the numbers as a collision;
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


