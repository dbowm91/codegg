# TUI Modularity and Render Invariants Milestone 010 — Closure Status

Status: conditionally closed

Source implementation plan:

- `plans/implementation/tui-modularity-and-render-invariants/010-slash-dispatch-tables.md`

Source subsystem roadmap:

- `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md#milestone-010--slash-dispatch-tables`

Repository baseline reviewed: `7642b9e38` (`feat-auto-20261109-f8abad0d`), plus
uncommitted working-tree work — 231 changed or untracked paths at the time of
review

Implementation commits or pull requests:

- **None.** This milestone exists only as uncommitted working-tree work on
  `feat-auto-20261109-f8abad0d`. No commit, branch or pull request carries it.

Labels: **measured** is what this record's author read out of the tree or ran;
**reported** is the implementing session's measurement, not repeated here.

## 1. Executive finding

The split is done, and it is done the way the plan's handoff notes demanded: as
a **delegation match**, not a table.

`App::dispatch_builtin_command` (`src/tui/app/mod.rs:5779`) is now **183 lines**
holding **161 `B::<Variant> =>` arms** and nothing else — measured, with **zero**
non-`B::` arms and **zero** `_ =>` wildcards. Each arm is one delegation into a
family module, and the dispatcher's `use` list names exactly **45** family
modules (measured; the guard's own header states the same figure, "Milestone 010
emptied the dispatcher body into 45 family modules"). The plan's baseline was
"**23 distinct `commands::*` modules … The remaining arms call `self.` inline —
**101 such calls**"; that count is now **0**. The argument normalisation survives
verbatim at the top of the function
(`let argument = argument.unwrap_or_default().trim();`), and every arm that needs
it receives the normalised `argument` — **91 of the 161** arms pass it, the rest
take none (measured by counting `argument` occurrences in the arm lines).

The repeated teardown is one helper: `close_command_mode`
(`src/tui/commands/mod.rs:84`) with **30 call sites** plus its definition
(measured). The plan's target was "the 30 teardown triples are one helper"
(criterion 4), and the count matches exactly.

Reachability is tested rather than asserted. `src/tui/commands/dispatch_tests.rs`
(300 lines) holds `every_builtin_action_reaches_a_handler` (`:120`), with
`empty_and_whitespace_arguments_dispatch_identically` (`:145`),
`an_empty_argument_refuses_before_the_effect_and_a_valid_one_performs_it`
(`:171`), `one_representative_command_per_family_reaches_its_stated_state`
(`:223`) and `a_refused_command_spawns_no_task` (`:289`), plus
`close_command_mode_clears_all_three_fields` (`:87`). The exhaustive variant list
is **one** list: `src/tui/command.rs`'s test module already had
`all_builtin_variants()`, and this milestone re-exported it (`#[cfg(test)]
pub(crate) use tests::all_builtin_variants;`, a 9-line diff) so the reachability
test reuses it instead of keeping a second copy of 161 variants. That is a small
change with real value: a second copy of the variant list would be a silent
coverage hole the day a variant is added to one list and not the other.

**The plan's guard claim is false, and it was false in the worst way: the guard
did not fail, it went silent.** This record carries that as a found-and-corrected
defect in the plan's own words (§1.1).

Status is conditional for three reasons: the repair to the argument-contract
guard is uncommitted work by the parent session and is itself the evidence that
the guard now bites; the plan's **exhaustiveness evidence artifact** (a quoted
compiler error from a temporarily added variant) was not produced, because this
record's author may not edit `src/` or build; and the corpus and `quick` results
are outstanding.

### 1.1 The correction: the plan's "passes unmodified" claim

The plan asserts, in three places, that the two guards over this surface need no
change. In §10 ("Migration and compatibility tests", lines 258-259):

> - `check_command_catalog_ownership.py` and
>   `check_command_argument_contract.py` pass unmodified.

and as acceptance criterion 6 (line 303):

> 6. The catalog and argument-contract guards pass unmodified.

and again in §3 (line 71), of both guards:

> must pass unmodified.

**The claim is half right, and the half that is wrong is the one that mattered.**

`scripts/check_command_catalog_ownership.py` did pass unmodified — measured, exit
0, "command catalog ownership: clean", and `git status` shows the file untouched.
The catalog genuinely was not touched: `src/tui/command.rs`'s diff is 9 lines and
touches no catalog entry, and `built_in_commands()` is unchanged (measured — the
diff contains no `CommandEntry`, no `name:` and no `=>` line).

`scripts/check_command_argument_contract.py` **could not** pass unmodified, and
more importantly it *appeared* to. Milestone 010 moved the 161 dispatcher arms
out of `App::dispatch_builtin_command` in `src/tui/app/mod.rs` into 45 family
modules under `src/tui/commands/**`, and the guard scanned only that one
function's body. Its real check therefore became **vacuous**: it reported clean
while scanning a 183-line delegation match, in which no arm reads
`command_palette.query` because no arm does anything at all. Only its
`--self-test` noticed, and it noticed by accident — the injection anchor it used
was `B::Help => {`, that anchor no longer existed in the dispatcher, and the
injection silently fell back to a synthetic fixture that names a **different**
arm. So the guard's non-vacuity test went on passing against a fixture that no
longer corresponded to the real surface. A scan that can silently lose its
subject is worse than no scan: it reports a clean bill of health about a file it
is no longer reading.

The parent session is repairing the guard to scan `src/tui/commands/**` plus the
dispatcher, to discriminate **writes** (`query = …`, `query.push…`) from reads so
a legitimate assignment is not reported as a violation, and to require the
scanned surface to be **non-empty** (a missing dispatcher function, a missing
family directory, and a directory below a module floor are each findings). That
repair is in the working tree as of this review: the guard's own docstring
records the episode under "The scanned surface, and why it is checked rather than
assumed" (`scripts/check_command_argument_contract.py:21-37`), the diff is
**401 insertions / 96 deletions**, the real run now reports "**58 dispatch
region(s)** read no palette query and `Dialog::is_open` covers every variant",
and `--self-test` reports **16 cases**, including "a read injected into a real
family module is rejected", "the family finding names its module and its
function", "writes to the palette's query are accepted", "a moved dispatcher
function is a gap, not a pass", "an absent family-module directory is a gap" and
"a family directory below the floor is a gap" (all measured, exit 0).

**What this means for the milestone.** The migration is not implicated in a
correctness defect: no arm in the scanned surface re-derives an argument from the
palette, and the guard's post-repair run confirms it across 58 regions. The defect
is in the **plan's claim** and in the **evidence discipline** around it — a
milestone that empties a guarded surface must re-run that guard's self-test and
confirm the guard's *subject* moved with it, not just that the guard exited 0.
The plan's acceptance criterion 6 should be amended to "the catalog guard passes
unmodified; the argument-contract guard passes **after being widened to the new
surface**, with its self-test proving the widened surface is what it scans". This
record does not edit the plan.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| `dispatch_builtin_command` is a delegation match under 250 lines | 183 lines measured (`src/tui/app/mod.rs:5779-5961`); every arm is one call | pass | Criterion 1 met with room. |
| The match remains exhaustive | 161 `B::<Variant>` arms, **0** `_ =>`, **0** non-`B::` arms (measured) | pass structurally | A new variant cannot be silently unhandled: there is no wildcard. |
| Exhaustiveness proved by a quoted compiler error for a variant without an arm | not produced | **not run** | The plan's own work package E calls this "the evidence the closure record carries". §10. |
| Every `BuiltinSlashAction` variant is reachable and tested | `every_builtin_action_reaches_a_handler` (`commands/dispatch_tests.rs:120`) over `command.rs::all_builtin_variants` (one list, re-exported `#[cfg(test)]`) | pass (test present) | Not run by this record; the test's existence and its single-source variant list are measured. |
| The 30 teardown triples are one helper | `close_command_mode` (`commands/mod.rs:84`) with **30 call sites** (measured: 31 `close_command_mode(` occurrences, one of which is the definition) | pass | Count matches the plan's baseline exactly. |
| The catalog is unchanged; no command added, removed or renamed | `check_command_catalog_ownership.py` exit 0, "clean", guard unmodified; `src/tui/command.rs` diff +9/−1 with no catalog entry touched | pass | |
| `check_command_argument_contract.py` passes **unmodified** | it is **modified**: +401/−96, and the modification is what makes the check real after the surface moved | **fail as written** | §1.1. |
| The widened argument-contract guard passes on the real tree | "command argument contract check passed — 58 dispatch region(s) read no palette query and `Dialog::is_open` covers every variant", exit 0 | pass | |
| The widened guard is not vacuous | `--self-test`: 16 cases, including an injected read in a real family module, a moved dispatcher function as a gap, a missing family directory as a gap and a below-floor directory as a gap | pass | The three "gap" cases are exactly the ones whose absence let the old guard go silent. |
| Argument handling is unchanged, including the `trim()` normalisation | `src/tui/app/mod.rs:5785`: `let argument = argument.unwrap_or_default().trim();`, above the match | pass | The normalised value is the dispatcher's; **91** arms pass it on, and none re-derives it from the palette. |
| An arm that performs a check before its effect still does | `an_empty_argument_refuses_before_the_effect_and_a_valid_one_performs_it`, `a_refused_command_spawns_no_task` | pass (tests present) | The plan's §10 security test. |
| Empty and whitespace-only arguments behave as before | `empty_and_whitespace_arguments_dispatch_identically` | pass (test present) | |
| The converted dispatcher stays synchronous; no new task spawn | no `async`/`spawn` in the delegation match; `a_refused_command_spawns_no_task` asserts the negative | pass | |
| The corpus is byte-identical | not measured for this milestone | **not measured** | No corpus number was available to this record. §10. |
| `check_frontend_action_set.py` clean | ran: "frontend-action set: clean", exit 0 | pass | |
| `scripts/check_tui_render_cost.py` still passes | ran: "the render path derives each result once", exit 0 | pass | |
| The comment ratchet: bank `app/mod.rs` down, list the new modules | `check_comment_budget.py` passes inside `scripts/verify.sh full` on this tree (2026-11-13), with the corpus banked at `max_comment_lines = 139245` | pass | Verified for this correction. |
| `scripts/verify.sh quick` green | not run at this pass; `scripts/verify.sh full` 2026-11-13 passed, and `run_full()` calls `run_quick()` | **pass, 2026-11-13** | §10. |

## 3. Production implementation evidence

- `src/tui/app/mod.rs:5779-5961` — the 183-line delegation match, with the doc
  comment that states the contract: "Every arm is **one delegation**: the handler
  lives in the family module under `crate::tui/commands/`, receives the
  normalised `argument`, and owns the whole body — including the command-mode
  teardown. The match is deliberately still a match, not a table: a new
  `BuiltinSlashAction` variant must fail to compile until it has an arm here,
  where a runtime lookup would silently do nothing."
- `src/tui/commands/mod.rs:83-84` — `close_command_mode`, the one teardown.
- `src/tui/commands/**` — 57 `.rs` files in total, of which **45** are the family
  modules the dispatcher delegates to; the remainder are `mod.rs`,
  `dispatch_tests.rs`, and submodules/helpers used by the families.
- `src/tui/commands/dispatch_tests.rs` — the reachability and semantic tests.
- `src/tui/command.rs` — one change: `all_builtin_variants` is re-exported for
  the dispatcher's reachability proof, so the exhaustive variant list has one
  definition.

Nothing in the plan's "explicitly out of scope" list was implemented:
`dispatch_tui_command` (`src/tui/runtime/command_dispatch.rs`) was not touched,
the catalog in `src/tui/command.rs` was not restructured, and no argument-parsing
semantics or authorization path changed (authorization stays daemon-side per
ADR-0052).

## 4. Verification executed

### Commands run

```bash
python3 scripts/check_command_catalog_ownership.py
python3 scripts/check_command_argument_contract.py
python3 scripts/check_command_argument_contract.py --self-test
python3 scripts/check_frontend_action_set.py
python3 scripts/check_tui_render_cost.py

# the dispatcher's measured shape: a throwaway python3 script brace-matches
# `fn dispatch_builtin_command`, then counts the body's lines, its `B::` arms,
# its wildcard arms, its non-`B::` arms and the arms that pass `argument`
ls src/tui/commands/*.rs | wc -l
grep -rn 'close_command_mode(' src/tui --include=*.rs | wc -l
git diff --stat scripts/check_command_argument_contract.py src/tui/command.rs
git status --porcelain scripts/check_command_catalog_ownership.py
```

### Results

| Command | Result |
|---|---|
| `check_command_catalog_ownership.py` | **pass** — "command catalog ownership: clean", exit 0 |
| `check_command_argument_contract.py` | **pass** — "58 dispatch region(s) read no palette query and `Dialog::is_open` covers every variant", exit 0 (with the repair in the tree) |
| `check_command_argument_contract.py --self-test` | **pass** — "self-test passed: 16 case(s)", exit 0 |
| `check_frontend_action_set.py` | **pass** — "frontend-action set: clean", exit 0 |
| `check_tui_render_cost.py` | **pass** — "the render path derives each result once", exit 0 |
| Dispatcher shape | 183 lines, 161 `B::` arms, 0 wildcard, 0 non-`B::` arms (measured) |
| `src/tui/commands/*.rs` | 57 files; 45 modules in the dispatcher's `use` list (measured) |
| `close_command_mode(` | 31 occurrences = 1 definition + **30** call sites (measured) |
| `command.rs` diff | +9/−1, test-only re-export of `all_builtin_variants` (measured) |
| argument-contract guard diff | **+401 / −96** (measured) |

### Not run

| Command | Why |
|---|---|
| `cargo test --lib -- tui::app::commands` and `-- tui::commands` | Compiling is expensive and three sessions were compiling concurrently, by instruction. The tests' presence is measured; their result is not. |
| Adding a temporary `BuiltinSlashAction` variant and quoting the compiler error (plan work package E) | Requires a `src/` edit and a build, both outside this record's ownership. This is the one piece of plan-mandated closure evidence that is **missing**, and §10 records it as such rather than describing the wildcard-free match as a substitute. |
| `cargo fmt --check`, clippy, `scripts/verify.sh quick` | Not run by this session. **All three ran inside `scripts/verify.sh full` on this tree at 2026-11-13 and passed** — 17,377 workspace tests, 8,441 feature-pass tests, 0 failures; fmt clean; clippy `-D warnings` clean. |

**Tree state caveat.** The argument-contract guard's repair was landing in the
same working tree while this record was written (three other sessions were
active). The results above are a property of the tree at that moment, and the
guard file is uncommitted, so a later run may differ for reasons that are not this
milestone.

## 5. Invariant review

- **The match stays exhaustive.** Holds, and holds by construction rather than by
  convention: the match has no wildcard. A new variant fails to compile until it
  has an arm. The plan's *evidence artifact* for this is missing (§10), but the
  mechanism is measurably present.
- **The catalog is unchanged.** Holds — the guard passes unmodified and the diff
  in `command.rs` touches no entry.
- **Argument handling is unchanged.** Holds — the `trim()` normalisation is the
  dispatcher's first statement and every family receives the result.
- **Every action remains reachable and is covered by a test.** The test exists and
  iterates the single variant list. Not run here.
- **The corpus is byte-identical.** **Not measured** by this record.
- **I1–I6 stay green.** The render ratchet is unaffected (61 / 2495 / 0 measured);
  the corpus is not re-run.
- **The comment ratchet holds, with `app/mod.rs` banked down and the family
  modules listed.** **Verified 2026-11-13**: `check_comment_budget.py` passes
  inside `scripts/verify.sh full`, with the corpus banked at 139,245 comment
  lines. The direction this record described — 3,101 lines leaving `app/mod.rs`
  — is what the green guard now measures.

## 6. Failure and recovery review

- **No async boundary moved.** The dispatcher is synchronous before and after; the
  family functions perform the same sequence an arm performed. The plan's §8
  hazard — "an arm that enqueues a `TuiCommand` must still enqueue it at the same
  point relative to any state it mutates first" — is preserved by construction
  (the arm body moved wholesale) and asserted by
  `one_representative_command_per_family_reaches_its_stated_state`, which checks
  resulting state rather than "a command was enqueued".
- **No cancellation, restart or persistence.** Nothing to review.
- **Failure visibility.** An unhandled variant is a **compile error**, which is
  the strongest failure mode available here and the reason the plan refused a
  `HashMap`. A reordered side effect is a failed state assertion in the family
  tests.
- **The guard-silence failure is the one that actually happened**, and it is the
  instructive one: a static check whose subject moved reports success. The repair
  makes the surface's presence a finding in its own right, which is the general
  fix for the class.

## 7. Migration and compatibility review

No schema, protocol, configuration or dependency surface. No `TuiCommand` variant
was added or changed, and no command's behaviour changed. The migration is
internal and complete in one milestone: no arm was left behind a flag or a
fallback path — the dispatcher contains no inline `self.` body at all (measured),
which is the strongest available evidence that nothing was half-moved.

Rollback is reverting the 45 family modules and restoring the inline arms in
`dispatch_builtin_command`. There is no durable state and no user setting, and
the `command.rs` change is a test-only re-export, so it reverts with them.

## 8. Security review

Authorization is unchanged and remains daemon-side (ADR-0052); this milestone
moves where a frontend arm body lives, not what a command is or who authorizes
it. One ordering property is security-relevant and is asserted: an arm that
performs a check before its effect still performs the check before the effect
(`an_empty_argument_refuses_before_the_effect_and_a_valid_one_performs_it`).
The argument contract itself — a handler receives the extracted argument and
never the palette's whole typed line — is the subject of the widened guard, whose
post-repair run covers 58 dispatch regions and whose self-test injects a real
violation into a real family module.

## 9. Documentation and operations

- `scripts/check_command_argument_contract.py:21-37` — the scanned surface and
  why it is asserted rather than assumed, including the record of this
  milestone's silent-vacuity episode. This is the operational documentation that
  matters most, because it is what stops the same silence recurring.
- `architecture/tui.md` — the plan asks for the delegation rule ("a new command
  adds one arm and one family function, and the family owns the body"). **Present**
  at `architecture/tui.md:3272-3301`, "The slash-command argument contract":
  "since M010 lives in one of the dispatcher's family modules under
  `src/tui/commands/`: the dispatcher routes to it and passes the extracted
  argument". The guard's own scope statement follows in the same section.
- `docs/comment-budget.toml` — the plan asks to bank the `app/mod.rs` reduction
  and list the new family modules. **Verified**: `check_comment_budget.py`
  passes with the corpus banked at 139,245 lines.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| medium | The plan's claim that `check_command_argument_contract.py` "passes unmodified" (acceptance criterion 6; §10 lines 258-259; §3 line 71) is false. The guard scanned only `dispatch_builtin_command`, the milestone emptied that body, and the check went **vacuous** while still exiting 0; only `--self-test` noticed, via an injection anchor (`B::Help => {`) that had moved and silently fell back to a synthetic fixture naming a different arm. | A static guard reported a clean bill of health about a surface it was no longer reading. The failure mode is general: any milestone that moves a guarded surface can produce it, and a green `quick` would have propagated the false claim to the merge. | Amend the plan's criterion 6 to require the widened surface **and** its self-test, and land the parent's repair (already in the working tree: scans `src/tui/commands/**` + the dispatcher, 58 regions, 16 self-test cases). Record the episode in `architecture/` so the next split re-runs the self-test. |
| medium | The plan's required exhaustiveness evidence — "a temporary variant added locally, the compiler error quoted, and the variant removed" (work package E) — was not produced. | Criterion 2 has no quoted artifact. The mechanism is measurably present (0 wildcards, 161 explicit arms), which is strong but is not what the plan asked the record to carry. | Either produce the artifact in a session that may edit `src/`, or amend the plan to accept "no wildcard arm + every arm names a variant" as the evidence. Do not describe one as the other. |
| low | The corpus byte-identity result is not recorded for this milestone. | Criterion 5 has no evidence; the change is routing only, so the risk is low, but "low risk" is not a measurement. | Record the `tui_render` corpus result on the merged tree alongside `quick`. |
| low | The comment-budget bank (`app/mod.rs` down, 45 modules listed) was not inspected. | The plan's comment invariant was unverified in a milestone that moved ~3,000 lines between files. | **Verified 2026-11-13**: `check_comment_budget.py` passes inside `scripts/verify.sh full`, corpus banked at 139,245 lines. |
| low | `architecture/tui.md`'s delegation rule was not checked. | A plan-mandated documentation item may have been absent, and the rule is the thing a future contributor needs. | **Verified present** at `architecture/tui.md:3272-3301` ("The slash-command argument contract"): the dispatcher routes to a family module and passes the extracted argument; a handler that read the query would be re-deriving what it was given. |
| low | `scripts/verify.sh quick` was not run; the tree is uncommitted and the guard repair is uncommitted. | Criterion 8 had no evidence, and the central finding's fix had not itself been through the lane that will run it. | **Discharged 2026-11-13**: `scripts/verify.sh full` passed — which runs `quick`'s guards *and* the rest, so the widened guard and its `--self-test` went through the lane. The tree is still uncommitted. |

| medium | **Found after this record was written, and fixed: this milestone's split left `mod tests` *before* the `dispatch_*` functions in 15 family modules**, so `clippy::items_after_test_module` fired 18 times under `-D warnings`. `scripts/verify.sh quick` cannot see it — `quick` runs the guards and a workspace `check`, not clippy — so the tree was green on `quick` and red on `full`, which is exactly the asymmetry the repository's own verification doctrine warns about. | `scripts/verify.sh full` could not pass at all until it was fixed; a milestone conditionally closed on `quick` alone would have shipped a red `full`. | **Fixed in the same worktree**: every test-gated module in those 15 files now sits at the end of its file, located with `scripts/rust_test_scan.py` (`cfg_test_module_spans`, which is Rust-aware and spans the governing attributes) rather than by brace counting. `cargo fmt --all --check` is clean. Re-run clippy to confirm; `full` is still outstanding as a whole. |

## 11. Roadmap disposition

**Milestone conditionally closed with named operational evidence outstanding**,
and with acceptance criterion 6 amended rather than marked met.

The conditional element: the green-`quick` half is **met** — `scripts/verify.sh
full` passed on this tree at 2026-11-13, and it runs `quick`'s guards, the format
check and clippy — and what remains is the corpus result recorded on the merged
tree, the argument-contract guard's repair committed with its self-test as the
evidence, and the exhaustiveness artifact produced or the criterion reworded. The
migration itself is sound, and this record found no correctness defect in the 161
converted arms.

The finding the roadmap should carry forward as a **process** item, not just a
milestone item: a milestone that empties or moves a guarded surface must re-run
that guard's `--self-test` and confirm the injected defect still lands on the real
surface. This is the same class as the `--self-test`-proves-non-vacuity rule the
repository already applies elsewhere; what this milestone shows is that a
self-test can go vacuous *quietly* when its anchor moves.

## 12. Registry updates

- `plans/registry.md` line 29 (subsystem row): M010's status among "M005–M010 in
  progress or pending" needs its conditional status named.
- `plans/registry.md` line 688 (the M010 milestone row): status "**Blocked —
  hard dependency on M001**" → conditionally closed, with this record's path and
  the correction in §1.1.
- `plans/subsystems/tui-modularity-and-render-invariants-roadmap.md` §12 row
  `010`: `blocked` → `conditionally closed`, closure record
  `plans/closure/tui-modularity-and-render-invariants/010-status.md`, blockers
  "green `quick` on the merged tree; the exhaustiveness artifact; the
  argument-contract guard repair landed with its self-test".
- `plans/implementation/tui-modularity-and-render-invariants/010-slash-dispatch-tables.md`
  header: `Status: blocked` → `Status: conditionally closed`, and acceptance
  criterion 6 amended per §1.1. **This record's author made only the header edit**
  — the criterion text is left for the parent, as instructed.

## Correction log (2026-11-13)

- **§2 and §4, the `quick`/`fmt`/clippy rows.** Were "not run". Met by: `scripts/verify.sh full` on this tree, 2026-11-13 — 17,377 workspace tests, 8,441 feature-pass tests, 0 failures; `cargo fmt --all --check` clean; clippy `-D warnings` clean. `run_full()` calls `run_quick()`, so the guards — including the widened argument-contract guard and its 16-case self-test — went through the lane.
- **§2, §5 and §10, the comment-budget rows.** Were "not checked" / "not verified here". Met by: `check_comment_budget.py` green inside the same `full`, corpus banked at 139,245 lines.
- **§9 and §10, the `architecture/tui.md` delegation rule.** Was "**Not verified in this pass**" / "was not checked". Verified present at `architecture/tui.md:3272-3301`, "The slash-command argument contract", including the sentence that the handler "since M010 lives in one of the dispatcher's family modules under `src/tui/commands/`".
- **§10, the `quick` row.** Discharged by the 2026-11-13 `full`; the commit is still outstanding.
- **§11, the disposition.** The green-`quick` element is recorded as met; the remaining conditions are the corpus result on the merged tree, the guard repair committed with its self-test, and the exhaustiveness artifact (or the criterion reworded).

**No promotion.** The status stays `conditionally closed`: the exhaustiveness artifact the plan's work package E requires was not produced, the corpus result is still to be recorded on the merged tree, the guard repair is uncommitted, and the tree is uncommitted. No condition was deleted without naming what met it.
