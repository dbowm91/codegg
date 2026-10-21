# TUI Subagent Presentation and Name Allocation — Milestone P004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/tui-subagent-presentation-and-name-allocation/004-subagent-name-allocation-two-pass.md`

Source subsystem roadmap:

- `plans/subsystems/tui-subagent-presentation-and-name-allocation-corrective-addendum.md` §7 (P004)

Repository baseline reviewed: `31a412844fbbf36055b29245075121035eefc385`

Implementation commits or pull requests:

- Working tree only — not committed at the time of writing. The change is
  confined to `src/agent/subagent_names/registry.rs`,
  `scripts/generate_subagent_names.py`, `src/agent/subagent_names/generated.rs`
  (regenerated, never hand-edited), `assets/subagent-names/names.toml`,
  `architecture/tui.md`, `.opencode/skills/tui/SKILL.md`, and
  `scripts/check_tui_subagent_name_pool_isolation.py` (prose only — see
  §3.1), plus the plan records named above.

## 1. Executive finding

The name registry made the **numeric suffix the primary mechanism** instead of
the fallback it was meant to be. `SubagentNameRegistry::draw` walked the
46-entry pool from a random start and called `first_free(base, active)` on the
**first** base it reached. `first_free` answers `base` when the base is free
and `base-2`, `base-3`, … when it is not — so a single held base consumed
`-2` and the draw returned immediately. The other ~45 pool names were never
consulted, against a projection cap of `MAX_PROJECTION_SUBAGENTS = 16` live
subagents. A `Sally-2` in the sessions panel therefore meant "the one base
the walk happened to reach first was taken", not "the pool was exhausted".

The draw is now **two passes over the same pool in the same random order**:

1. **Pass 1 — a free bare name.** The first base name that is not held,
   returned verbatim and without a suffix.
2. **Pass 2 — the numeric ladder.** Only reached when *every* base name is
   held: the existing `first_free` walk, unchanged.

The last resort (`Agent-N`, when every base and every suffix up to
`MAX_SUFFIX` is held) is unchanged, as are the lease, the release, the event,
the draw site, and `pool_entry`'s suffix stripping. A free pool name is now
always preferred over a numeric variant of a taken one, which is what
capability K6 asked for; and because the suffix is now reached only in the
state it was written for, the sessions panel reads `Sally` where it used to
read `Sally-2`.

**The plan's own §10.3 test could not be written deterministically** — it
says so: *"this cannot be pinned without seeding the RNG"*. The draw was
therefore split into `draw` and `draw_from(active, start)`, a behaviour-neutral
parameterisation, so the selection rule can be exercised at **every** start
index instead of only the one the RNG happens to pick. The rule is now pinned
deterministically, and the non-vacuity proof is a deterministic failure
rather than a likely one. §4.2 holds the evidence.

## 2. Requirement-to-evidence matrix

The plan's §13 acceptance criteria, in its order.

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| AC1 — with at least one free pool name, a new spawn receives a **bare** pool name, never a `-N` variant | `draw_from`'s pass 1 returns the first unheld base (`registry.rs:170`); `every_start_index_prefers_a_free_base_name_over_a_suffix` (`registry.rs:548`) holds every base but one and asserts the free one at **all 46** start indices; `a_free_base_is_never_suffixed_while_another_base_is_free` (`registry.rs:580`) holds half the pool and asserts no suffix at any start; `a_run_of_spawns_never_carries_a_numeric_suffix_while_a_name_is_free` (`registry.rs:635`) drives the public `assign` for 45 leases and asserts every name is bare | pass | all three of the new tests **fail** against the pre-change rule (§4.2) |
| AC2 — with every pool name held, the numeric ladder still applies and the drawn name does not collide | `the_suffix_ladder_applies_only_once_every_base_is_held` (`registry.rs:609`) asserts `-2` at every start index and no collision; the retained `draw_survives_a_pool_where_the_chosen_base_is_occupied` (`registry.rs:471`) and `a_fully_occupied_pool_walks_the_suffix_ladder` (`registry.rs:497`) | pass | this is the change's **negative control**: it passes on both sides, because the ladder must be preserved |
| AC3 — with every pool name and every suffix held, the `Agent-N` fallback still applies | the retained `exhausted_pool_falls_back_to_a_unique_name` (`registry.rs:762`) | pass | unchanged code path; the fallback line is byte-identical |
| AC4 — the name is still drawn **once at spawn**, travels inside the event, and replays identically | the draw site is unmoved: `src/agent/worker.rs:1011` still acquires the lease and `src/agent/worker.rs:1022` still publishes `AppEvent::SubagentStarted`; `draw` is still called only from `assign` (`registry.rs:89`) and `assign_without_lock` (`registry.rs:144`); the module doc's "Why the name is drawn here and not anywhere else" is untouched | pass | the policy change affects **new spawns only**; a recorded `Sally-2` stays `Sally-2` |
| AC5 — `pool_entry` still resolves a suffixed name to its pool entry | `pool_entry` (`registry.rs:317`) is unchanged; `pool_entry_strips_a_numeric_suffix_only` (`registry.rs:730`) and `a_suffixed_lease_resolves_to_the_same_entry_as_its_base` (`registry.rs:383`) both pass | pass | the suffix is rarer, so attribution is rarer — but never wrong |
| AC6 — the four policy statements describe the same policy, and `generate_subagent_names.py --check` exits 0 | the registry's module doc (`registry.rs:21`), the generator's emitted text (`scripts/generate_subagent_names.py:123`), the pool source's header (`assets/subagent-names/names.toml:9`), and `architecture/tui.md:752`; `python3 scripts/generate_subagent_names.py --check` → "All checks passed." | pass | a fifth statement — the isolation guard's rationale prose — was corrected too (§3.1) |
| AC7 — `git diff src/agent/subagent_names/generated.rs` shows **only** a doc-comment change | 5 insertions, 4 deletions, all inside the `SubagentName` doc comment; the 46 pool entries are byte-identical | pass | the file was written by `python3 scripts/generate_subagent_names.py`, never by hand |

## 3. Production implementation evidence

| File | Change | Anchor |
|---|---|---|
| `src/agent/subagent_names/registry.rs` | the module's "Collision policy" section now states the two-pass rule, and the "Determinism within a lease" section is scoped to the exhausted case | `:21`, `:35` |
| | `draw` is now a two-line entry point: it computes the random start and hands it to `draw_from` | `:156` |
| | `draw_from(active, start)` — the new selection rule: pass 1 returns the first **free bare** base; pass 2 walks the `-N` ladder over the same bases in the same order; the `Agent-N` last resort is unchanged | `:170` |
| | the empty-pool guard (`#[allow(clippy::const_is_empty)]`) moved with the rule into `draw_from`, so one place decides "the pool is empty" | `:178` |
| | `first_free`, `is_taken`, `random_index`, `pool_entry`, the lease, and `Drop` are **untouched** | `:220`, `:233`, `:243`, `:317` |
| | four new tests and one test helper | `:532`, `:548`, `:580`, `:609`, `:635` |
| `scripts/generate_subagent_names.py` | the doc text the generator emits now states the policy the code implements | `:123` |
| `src/agent/subagent_names/generated.rs` | **regenerated** by `python3 scripts/generate_subagent_names.py` — a doc-comment change and nothing else | `:5` |
| `assets/subagent-names/names.toml` | the header comment states the same policy as the generator's output | `:9` |
| `architecture/tui.md` | the naming paragraph: a free pool name first, a numeric variant only when every pool name is held | `:752` |
| `.opencode/skills/tui/SKILL.md` | new bullet: the name comes from the pool, and a free pool name always wins | `:1055` |
| `scripts/check_tui_subagent_name_pool_isolation.py` | the guard's rationale prose in two places corrected — the registry appends numeric variants only once every pool name is held | `:20`, `:193` |

### 3.1 Deviations from the plan, and why

1. **The draw was split into `draw` and `draw_from(active, start)`.** The plan
   did not ask for this, and it is the one structural change beyond the plan's
   letter. The plan's §10.3 said the occupied-first-base case *"cannot be
   pinned without seeding the RNG"* and asked for a probabilistic test over
   200 draws. A rule that is observable only through a random draw has
   regression evidence that is a **probability**, and a probability is not
   evidence: the test could pass against a broken implementation by luck, and
   its failure would be a matter of chance. The split makes the rule
   addressable at a chosen start index, which turns the plan's weakest
   requirement into a deterministic one — every start index is exercised, not
   one sample of them.
   The split is **behaviour-neutral**: `draw` computes `random_index` exactly
   as before and hands the value to `draw_from`, and `draw_from`'s body is the
   plan's own two-pass algorithm verbatim.
2. **A fifth policy statement was corrected: the isolation guard's prose.**
   The plan listed four places where the policy is stated, and the plan was
   right about the *code* — but `scripts/check_tui_subagent_name_pool_isolation.py`
   also states the policy in its module docstring and in its planted-violation
   message ("the registry appends numeric variants (`Sally-2`)"). Left alone,
   those two sentences would have described the old rule. The guard's
   **behaviour** is unchanged — it still forbids a TUI module from naming the
   pool — and only the wording moved. The plan's "no new static guard" is
   honoured: no guard was added, and `check_tui_subagent_name_pool_isolation.py
   --self-test` still reports 5 violations detected and 4 allowed cases not
   flagged.
3. **The empty-pool guard moved into `draw_from`.** The plan's §16 says to
   keep the `#[allow(clippy::const_is_empty)]` guard; it was kept, and it
   moved with the rule so that one place decides "the pool is empty" rather
   than two places deciding it for different functions.
4. **A fourth new test was added, and it is a negative control.** The plan
   asked for five tests (§10.1–§10.5); two of them (§10.4, §10.5) are
   *retained* existing tests, which §3.4 of the plan says survive the change
   unchanged. The plan's §10.2 is likewise the retained
   `draw_survives_a_pool_where_the_chosen_base_is_occupied`. The new suite is
   therefore four tests: three that assert the new rule (and must fail against
   the old one) and one that asserts the *preserved* rule (and must pass
   against both). The preserved-rule test is the one that makes the other
   three meaningful — without it, "the new tests pass" would not distinguish
   "the rule changed correctly" from "the test asserts nothing".
5. **The plan's §10.3 wording was kept in substance but made exact.** The plan
   asked for "a randomly chosen subset of bases"; the test holds a
   deterministic alternating half instead, because a deterministic subset is
   what makes the assertion hold at *every* start index rather than only at
   the sampled ones.

## 4. Verification executed

### 4.1 Commands run

```bash
# narrow tests first
cargo test -p codegg --lib agent::subagent_names

# the generator owns the pool file
python3 scripts/generate_subagent_names.py --check

# formatting and linting
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings

# static guards
python3 scripts/check_tui_subagent_name_pool_isolation.py
python3 scripts/check_tui_subagent_name_pool_isolation.py --self-test
python3 scripts/check_tui_transport_authority.py
python3 scripts/check_no_hardcoded_selection_colours.py
python3 scripts/check_first_party_skills.py

# the broader sweep
cargo test -p codegg --lib
scripts/verify.sh quick

# the pre-existing-failure comparison (§4.5): the same four tests and the
# same full sweep, run against the baseline `registry.rs`
git checkout 31a4128 -- src/agent/subagent_names/registry.rs
cargo test -p codegg --lib -- <the four failing test names> --test-threads=1
cargo test -p codegg --lib
```

### 4.2 Baseline-failing evidence (non-vacuity)

The proof reconstructed the **pre-P004 selection rule** and ran the new tests
against it. The reconstruction is the baseline file itself
(`git checkout 31a4128 -- src/agent/subagent_names/registry.rs`) with one
behaviour-neutral change: the baseline `draw` was refactored into
`draw` + `draw_from(active, start)` so the new tests could address the rule at
a chosen start index. The rule inside `draw_from` is the baseline's, verbatim
— one pass, `first_free` at the first base the walk reaches. The new tests
were appended to the baseline test module; the module doc, the generator, the
pool source, and the architecture doc are prose and carry no behaviour, so
they were left alone.

| Test | Against the pre-P004 rule |
|---|---|
| `every_start_index_prefers_a_free_base_name_over_a_suffix` | **FAILED** — `left: "Greg-2"`, `right: "Sally"` (start 0) |
| `a_free_base_is_never_suffixed_while_another_base_is_free` | **FAILED** — "start 0: Greg-2 is a variant while bare names are free" |
| `a_run_of_spawns_never_carries_a_numeric_suffix_while_a_name_is_free` | **FAILED** — "Cheryl-2 is a variant while a bare pool name was free" (and "Derek-2 …" on the other run) |
| `the_suffix_ladder_applies_only_once_every_base_is_held` | **passed** — the negative control: it asserts the behaviour the change must *preserve* |

Totals: `cargo test -p codegg --lib agent::subagent_names` **21 passed, 3
failed** against the pre-change rule; **24 passed, 0 failed** against the
final tree.

The three failures are the shape a non-vacuity proof must have: each names a
concrete suffixed name (`Greg-2`, `Cheryl-2`, `Derek-2`) where the new rule
returns a bare pool name. The fourth test passing on **both** sides is the
control — it shows the suite distinguishes the changed rule from the preserved
one, rather than failing indiscriminately.

The proof script restores the tree on every exit path (`trap restore EXIT
INT TERM`), and the restore was confirmed by the diff of the final tree and by
the re-run of the suite above.

### 4.3 Results

| Command | Result |
|---|---|
| `cargo test -p codegg --lib agent::subagent_names` | 24 passed, 0 failed |
| `cargo test -p codegg --lib` | 7 216 passed, 4 failed, 4 ignored (run 1); 7 214 passed, 6 failed, 4 ignored (run 2) — see §4.5 |
| `cargo fmt --all -- --check` | clean (one `cargo fmt --all` pass was applied first, to the new test helper) |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean, no warnings |
| `python3 scripts/generate_subagent_names.py --check` | pass — "All checks passed." |
| `python3 scripts/check_tui_subagent_name_pool_isolation.py` | pass |
| `python3 scripts/check_tui_subagent_name_pool_isolation.py --self-test` | pass — 5 violations detected, 4 allowed cases not flagged |
| `python3 scripts/check_tui_transport_authority.py` | pass |
| `python3 scripts/check_no_hardcoded_selection_colours.py` | pass |
| `python3 scripts/check_first_party_skills.py` | pass |
| `scripts/verify.sh quick` | pass — "Quick verification passed." |

### 4.4 Not run

- `scripts/verify.sh full` — the nextest workspace sweep and the
  `--features server,plugins,lsp-test-support` pass were **not** run. The
  change is confined to an in-memory selection rule over an active-lease set
  and touches no protocol, storage, server, scheduler, or execution path.
- The TUI was **not launched**. No claim of a runtime observation is made
  anywhere in this record. The change has no TUI surface: the TUI renders
  whatever name it is given, and the name it is given is now a bare pool name.
- `cargo test -p codegg` in full (the whole crate, including its integration
  binaries) was **not** run; the root crate's library suite was, and it covers
  the changed module and every consumer of the registry's public API.

### 4.5 The wider sweep's failures — pre-existing, and none of them this change's

The root crate's library suite reported failures in every run. They are
**pre-existing and load-induced**, and the comparison is the evidence:

| Run | Tree | Result |
|---|---|---|
| 1 | final | 7 216 passed, **4 failed**, 4 ignored |
| 2 | final | 7 214 passed, **6 failed**, 4 ignored |
| 3 | **baseline** — `registry.rs` reverted to `31a4128` | 7 213 passed, **3 failed**, 4 ignored |

The count **varies run to run on identical code** (4, then 6), which is the
signature of timing sensitivity rather than a regression — and the **baseline
fails the same way**. There are two distinct causes.

1. **`core::daemon_refresh::tests::session_projection_rebuilds_owner_scoped_live_jobs`
   and `core::daemon_refresh::tests::session_projection_snapshot_includes_in_process_live_child_instances`.**
   Each registers a workspace at the hardcoded root `/tmp`, and
   `canonicalize_workspace_root` (`crates/codegg-core/src/workspace.rs:626`)
   rejects a root that is itself a symlink. On macOS `/tmp` **is** a symlink to
   `/private/tmp`, so both fail with `InvalidRootPath("/tmp")`. They fail
   identically with the baseline `registry.rs` and with the final one, and
   they fail single-threaded in isolation — they are **platform-specific, not
   load-sensitive**. The same tests pass where `/tmp` is a real directory.
2. **`src/tui/commands/interactive_terminal.rs`'s badge and PTY tests.**
   A **load-sensitive** class already recorded in the P002 and P003 closure
   records. The file's own doc comment at
   `src/tui/commands/interactive_terminal.rs:3033` records that a single
   25 ms drain "is not enough". Under the 7 224-test parallel sweep they
   fail intermittently — two in one run, four in the next, one in the baseline
   run — and they **pass in isolation, single-threaded, on both the baseline
   and the final tree**. The four-test comparison, run single-threaded
   against each tree, gave the same result on both sides: the two
   `interactive_terminal` tests passed, and the two `daemon_refresh` tests
   failed with the same `InvalidRootPath("/tmp")` panic.

No file in either group is touched by this milestone, and none of them uses
the name registry. This is recorded here rather than left to be discovered as
a surprise.

### 4.6 Wider sweep

`scripts/verify.sh quick` was run on the final tree state and passed
("Quick verification passed."). It covers the agent schema, the
core/client/desktop boundaries, the sandbox, execution-ownership, TUI
authority, http-route-disposition, audit-coverage, queue-delivery-callsites,
queue-write-transaction-shape, scheduler-bypass, config-merge-coverage, the
no-hardcoded-models guard, the provider wire/catalog/resilience guards, the
OpenAI endpoint guard, Eggwork target routing, both generated-source checks
(`generate_builtin_agents.py --check` and `generate_subagent_names.py
--check`), and a workspace `cargo check`.

## 5. Invariant review

- **I7 — the name is drawn once, at spawn.** The draw site did not move.
  `src/agent/worker.rs:1011` still acquires the `SubagentNameLease` and
  `src/agent/worker.rs:1022` still publishes `AppEvent::SubagentStarted`;
  `draw` is still called only from `assign` and `assign_without_lock` inside
  the registry. No other module draws a name.
- **Replay determinism.** The draw depends on exactly two things: the
  random start index and the currently held set. The random start was already
  a source of non-determinism *before* this change — the policy decides only
  *which* name that randomness lands on, and the drawn name still travels
  inside the event. A replayed stream reproduces the recorded names because
  the reducer never re-draws. The change cannot make a replay disagree with a
  durable `agent_run` row.
- **No name is ever handed out twice while held.** `is_taken` is still the
  single collision check and is still the only thing both passes consult.
  Pass 1 asks it about bare names; pass 2 asks it about suffixed names
  through `first_free`, which asks the same question.
- **The lease is still released on every terminal path** — completion,
  failure, cancellation, panic — by construction through `Drop`
  (`registry.rs:306`). Nothing in the change touches the lease, and the
  retained `lease_releases_even_when_the_body_unwinds` and
  `lease_releases_on_drop` tests still pass.
- **The empty-pool and exhausted-pool fallbacks still hold.** The empty pool
  returns bare `Agent`; the exhausted pool returns `Agent-N`. The
  `#[allow(clippy::const_is_empty)]` guard is kept.
- **`pool_entry` still resolves a suffixed name to its pool entry.** The
  function is untouched. The change makes suffixed names *rarer*, so it
  cannot orphan a descriptor; and where a suffix does occur, the descriptor
  still belongs to the base entry.
- **K6 — a new subagent gets a new name.** The suffix is now the fallback.

## 6. Failure and recovery review

No state, async, or cancellation behaviour changes. The registry's mutex is
held for exactly as long as it was before: the draw is a synchronous function
over an in-memory map, and the added pass costs at most 46 additional
`is_taken` calls, each O(active). With `MAX_PROJECTION_SUBAGENTS = 16` that
is negligible, and the draw happens once per spawn.

- **Duplicate delivery and idempotency.** Unchanged: `assign` is idempotent
  for an already-active `task_id` and returns the name it already holds.
- **Cancellation races.** Unchanged: the lease's `Drop` releases the name on
  every terminal path, so a cancelled spawn returns its name to the pool.
- **Restart and recovery.** Unchanged: the registry holds nothing across a
  restart. On restart the pool is fully free, and the new policy simply
  hands out bare names from the start.
- **Partial persistence failure.** Not applicable — the registry persists
  nothing.
- **Stale generation or lease.** Unchanged: the registry is active-scoped,
  and a released name may be re-drawn to a different subagent. That is the
  documented behaviour and the reason the TUI must never resolve a name
  against the pool.
- **Contention and resource release.** Unchanged: concurrent leases still
  receive distinct names, and the retained
  `lease_names_are_distinct_across_concurrent_holders` proves it. Under the
  new policy the names are distinct **bare** names rather than distinct
  variants of one base, which is strictly stronger.

## 7. Migration and compatibility review

No schema, protocol, or configuration change. No migration: names are not
durable state.

- **Backward compatibility.** A session recorded before this change keeps its
  `Sally-2`, because the name is drawn once and travels inside the event.
  `pool_entry` resolves it to the Sally entry exactly as before, so the
  descriptor, the persona, and the row's appearance are unchanged.
- **Forward compatibility.** A name drawn under the new policy is an ordinary
  pool name with no suffix. Nothing downstream distinguishes it: the
  projection copies `display_name` verbatim, and the TUI renders it.
- **Rollback.** Reverting the selection rule is a one-file change and needs
  no data repair — the registry holds no durable state and the drawn names
  are ordinary strings either way.

## 8. Security review

No new input, no new parsing, no new external call, no new authority, no new
privilege. The change is a **narrowing** of what the registry can hand out:
the suffix ladder is reached in strictly fewer states than before, and the
`Agent-N` last resort — which already existed for the exhausted case — is
still reachable. No secret, credential, prompt, or raw log is introduced; a
name is a curated pool string from a 46-entry authored list, and the pool's
contents did not change.

The registry's mutex scope is unchanged, and the added pass does not extend
it: both passes run inside the same `assign` call, under the same guard.

## 9. Documentation and operations

- `src/agent/subagent_names/registry.rs` — the module's "Collision policy"
  section states the two-pass rule; the "Determinism within a lease" section
  is scoped to the exhausted case.
- `assets/subagent-names/names.toml` — the header comment matches the
  generator's output.
- `scripts/generate_subagent_names.py` — the emitted doc text matches the
  code, and `src/agent/subagent_names/generated.rs` was regenerated with
  `python3 scripts/generate_subagent_names.py`. The file was never hand-edited.
- `architecture/tui.md` — the naming paragraph states the same policy.
- `.opencode/skills/tui/SKILL.md` — one new bullet, so the skill and the
  architecture doc move together as `AGENTS.md` requires.
- `scripts/check_tui_subagent_name_pool_isolation.py` — the rationale prose
  in two places corrected; the guard's behaviour and its `--self-test` are
  unchanged.
- **No new static guard.** The behaviour is covered by unit tests, and
  `generate_subagent_names.py --check` proves the generated file is in sync
  with its source.
- No user-facing operational change: no new key, no new command, no new
  configuration, no new diagnostic. The only visible difference is that a
  subagent row reads `Sally` where it used to read `Sally-2`.

## 10. Unresolved findings

- None **in this milestone**. The plan's seven acceptance criteria all pass,
  the new tests are shown to fail against the pre-change implementation, and
  the policy is stated identically wherever it is stated.
- **Low — the plan's §10.3 probabilistic framing was superseded.** The plan
  asked for a test over many random draws because the rule could not be
  pinned without seeding the RNG. The draw was parameterised instead, and the
  rule is now pinned at every start index. This is an improvement over the
  plan, not a gap in it, and it is recorded in §3.1 so the next reader does
  not mistake the extra structure for scope creep.
- **Low — pre-existing failures outside this milestone's blast radius.** The
  two `core::daemon_refresh` tests and the
  `src/tui/commands/interactive_terminal.rs` badge/PTY tests described in
  §4.5 fail in the wider library sweep. They fail identically at the
  baseline, they are caused by a macOS `/tmp` symlink and by single-drain
  timing respectively, and **none of them was fixed here** — they are not in
  this milestone's blast radius, and fixing them would be an unrelated
  change. They are recorded rather than left to be discovered as a surprise.

## 11. Roadmap disposition

**P004 is closed.** P001, P002, and P003 stay closed. **All four milestones
in the corrective addendum are now closed**, and the addendum's §11
completion definition is met: each milestone has a closure record under
`plans/closure/tui-subagent-presentation-and-name-allocation/` mapping its
acceptance criteria to exact evidence, including an honest statement of what
was not run.

The addendum's status table and `plans/registry.md` are updated in §12.

## 12. Registry updates

- `plans/registry.md` — the P004 row moves to **closed**, with this closure
  path and the evidence for the seven acceptance criteria; the addendum's own
  row records that P001–P004 are all closed.
- `plans/subsystems/tui-subagent-presentation-and-name-allocation-corrective-addendum.md`
  — the §12 status table gives P004 its closure path; §4.8 is marked as a
  **baseline record** so its citations stay pinned to `31a4128`; §13 records
  that all four milestones are closed.
- `plans/implementation/tui-subagent-presentation-and-name-allocation/004-subagent-name-allocation-two-pass.md`
  — status → `closed`, with an `Outcome:` paragraph, and §3 marked as a
  baseline record.
