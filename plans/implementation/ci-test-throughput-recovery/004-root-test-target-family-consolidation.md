# CI and Test Throughput Recovery Milestone M004 — Consolidate Root Integration-Test Targets into Families

Status: implemented (2026-10-21); closure `plans/closure/ci-test-throughput-recovery/004-status.md`

Repository baseline: `239f06e7fc876eaa36156dbf4658bb56314c9673` (the tree the plan was authored against)

Implementation baseline: `2c1953347896848c2f52c2dfbd9f04029d1c2cc2` (the tree the work was applied to, after the 2026-10-21 rebase onto `origin/main`)

Source roadmap:

- `plans/subsystems/ci-test-throughput-recovery-roadmap.md#milestone-m004--consolidate-root-integration-test-targets-into-families`

Long-term requirements:

- `plans/000-long-term-specification.md` §4.7 — correctness before transparent magic;
- `plans/003-planning-process.md` §2.3–2.4.

Applicable ADRs: none required. This milestone changes the layout of the root package's integration-test targets. It does not alter product authority, protocol, storage, dependency selection, or runtime architecture.

Predecessor work (closed, historical, not reopened by this milestone):

- `plans/subsystems/ci-test-throughput-optimization-roadmap.md` M003 and M004 — the two consolidations this milestone extends;
- `plans/subsystems/ci-test-throughput-optimization-post-closure-corrective-addendum.md` C001–C002.

Primary class: infrastructure, with a polish obligation.

---

## 1. Objective

The root package links **materially fewer** test executables, while every test function that existed before consolidation still exists and still executes, in exactly one target.

## 2. Why this milestone is ready

**Hard dependencies:** none.

**Interface dependency: M003** (`plans/implementation/ci-test-throughput-recovery/003-change-scoped-verification-mode.md`). M003's ownership manifest names root test targets by name or pattern. M004 changes those names. The two must move together, or the manifest's references become stale and M003's scoped mode silently selects nothing.

**Why it is fourth:** it is the most invasive change in the roadmap and it is the one whose benefit is largest only after the cheaper structural wins have been measured. Running it earlier would confound every other measurement.

**Ordering rationale, stated because it was a real decision:** the reverse order (consolidate, then scope) would let M003's manifest be written against the consolidated topology directly, avoiding a rewrite. That was rejected because it places the owner's primary ask — a change-scoped per-commit loop — behind a large refactor. M003's manifest schema is therefore required to support target-name patterns and family concepts, so that M004 shrinks the manifest rather than rewriting it. That constraint is M003 work package A step 3 and it is not optional.

## 3. Current implementation evidence

### 3.1 The root package's target topology

`cargo metadata --no-deps` at the baseline reports **282** test targets for the root package and **308** across the workspace:

| Member | Test targets | Note |
|---|---|---|
| `codegg` (root) | **282** | 280 top-level `tests/*.rs` (269 auto-discovered and ungated, 11 also declared with a `required-features` gate) plus 2 consolidated directory targets |
| `codegg-core` | 13 | `crates/codegg-core/tests/` |
| `egglsp` | 10 (6 gated) | `crates/egglsp/tests/` |
| `codegg-client` | 2 | `crates/codegg-client/tests/` |
| `codegg-document` | 1 | `crates/codegg-document/tests/` |
| `codegg-config`, `codegg-protocol`, `codegg-providers`, `codegg-git`, `egggit`, `eggsentry`, `eggcontext` | 0 | all tests in the lib target |

### 3.2 The measured cost of the topology

From the baseline `cargo --timings` run:

- **271 root integration-test targets consumed 4,040.1 unit-secs — 44.1% of all workspace compile work.**
- Median root target: **13.9 s** of unit-time.
- The root `codegg` rlib is **855 MB**; each linked root test executable is **135–208 MB**; the root package's executables total **13.7 GB**.
- Root integration targets are individually cheap to *write* and individually expensive to *link*, because each one links the whole 855 MB library.

Consolidating 271 targets into a proposed ~15 families replaces 271 link operations with 15. On the measured unit-time model, that is a reduction from 4,040.1 to roughly 222 unit-secs — **~39% of the workspace's total compile work** — at the cost of coarser failure granularity.

### 3.3 The precedent this milestone extends

The predecessor already consolidated twice, and the pattern is in the tree:

- `tests/projection_replay/mod.rs` + eleven `*.inc` files, reached from a `[[test]]` declaration with `path = "tests/projection_replay/mod.rs"` — because Cargo does not auto-discover `tests/<dir>/mod.rs` as a test target;
- `tests/session_family/mod.rs` + five `*.inc` files, same mechanism.

Both were described in the predecessor's closure as reducing executable link steps. The predecessor's M003 consolidated eleven projection-replay binaries into one and M004 consolidated five session binaries into one.

**The predecessor's M003 commit message recorded that the workspace test count fell 11,781 → 11,721, i.e. 60 test functions disappeared with the 11 removed link steps.** That is the precedent's own caution: consolidation in this repository has historically removed test functions along with link steps. This milestone must establish that the removals were proven duplicates, and must not repeat the pattern silently.

### 3.4 The target-selection machinery the consolidation must keep working

The `ci` Nextest profile carries seven `threads-required = "num-cpus"` entries:

- six whole-binary exclusions — `eggwork_remote_execution_live`, `interactive_process_attach_resume`, `interactive_process_sessions`, `interactive_terminal_tui`, `scheduler_cancellation`, `causal_observe_replay`;
- one whole-test exclusion — `test(same_scope_requests_coalesce_to_one_publication)`.

These were added for **measured** reasons recorded in `.config/nextest.toml`: the interactive binaries drive real PTYs and subprocess trees with timing assertions; the live target owns exclusive fixture lifecycle (mTLS node plus Landlock); `scheduler_cancellation` has proven timing flakes; `causal_observe_replay` joined on 2026-10-16 after two hosted runs exceeded its frozen p95 gate while other binaries ran concurrently.

**Consolidation renames these binaries.** Each of the six must be re-pointed at its new family target, or — if a family would contain more than one of them — the family must be split so that the exclusivity granularity is preserved. The whole-test exclusion is unaffected by target naming, but the whole-binary ones are not.

### 3.5 A second consumer of the target names

The CI job excludes the live Eggwork binary from its second invocation with `-E 'not binary(eggwork_remote_execution_live)'`. That filter matches a **bare target name** and must be re-pointed too.

And M003's ownership manifest names targets. All three consumers — the Nextest profile, the CI `-E` filter, and the manifest — must move together with the target names.

## 4. Invariants that must not regress

- **I1 — No behavioural assertion is removed for timing.** Any test function that disappears must be a **proven duplicate**, and the closure record must name the surviving test that subsumes it. A consolidation that removes a test because it was in the same file as another is a violation of this invariant.
- **I2 — One bounded `CI / verify` job.** No new job, matrix, or lane.
- **I3 — The `ci` profile's 4-slot bound and its whole-binary exclusions remain in force.** Consolidation may re-point an exclusion; it may not delete one, and it may not widen a binary's exclusivity to a family that also contains unrelated binaries without re-validating.
- **I4 — Every test function that existed before consolidation still exists and executes exactly once.** The count must not decrease, except for proven duplicates named individually in the closure record.
- **I5 — Per-test process isolation is preserved.** The `ci` profile's per-test-process model is what makes the suite sound despite process-global environment mutation throughout the test code. Consolidation must not reintroduce in-process parallelism within a family.
- **I6 — The `eggwork_remote_execution_live` target keeps its `target_os = "linux"` gate and its exclusive fixture lifecycle.** The live Eggwork qualification is the only live mTLS/Landlock coverage in the suite.
- **I7 — No dependency, feature default, or product behaviour changes.**
- **I8 — The `required-features` gates are preserved.** A gated target must remain gated; consolidation must not make a `server`- or `lsp-test-support`-gated test runnable without its feature.

## 5. Scope

### In scope

- The root package's integration-test target layout: the `tests/` directory organisation and the root `Cargo.toml`'s `[[test]]` declarations.
- The `mod.rs` + `*.inc` family mechanism, extending the existing `tests/projection_replay/` and `tests/session_family/` precedent.
- The `ci` Nextest profile's whole-binary exclusion filters, re-pointed to the new target names.
- The CI job's `-E 'not binary(eggwork_remote_execution_live)'` filter, re-pointed.
- M003's ownership manifest, updated in the same change.
- Documentation of the new topology.

### Explicitly out of scope

- Changing any test's body, assertion, or helper semantics. Consolidation moves code; it does not rewrite it.
- Consolidating the **inline** unit tests. The root lib test harness is already a single linked unit (1,993.3 unit-secs for 10.9k inline test functions) and already links once.
- Consolidating the other workspace members' test targets. `codegg-core` (13), `egglsp` (10), `codegg-client` (2) and `codegg-document` (1) are small enough that the saving is minor, and the blast radius is not worth it.
- The 149 feature-gated test functions CI never executes (successor roadmap §10, D1).
- Changing the Nextest slot count (I3).
- Any change to a test's skip behaviour, its timeout, or its platform gate.

## 6. Required production changes

### Core/domain

None.

### Storage and migrations

None.

### Protocol and DTOs

None.

### Runtime and concurrency

None in production. The milestone changes the test-execution topology, and the `ci` profile's per-test-process isolation (I5) must be preserved.

### Frontend or operator surface

None.

### Security and authorization

None.

### Documentation and static guards

- `Cargo.toml` — the `[[test]]` declarations for the new families, each with a `required-features` gate where the family contains a gated target.
- `.config/nextest.toml` — the re-pointed whole-binary exclusion filters, with the existing rationale comments carried forward **per binary**, not per family, so the recorded flake history stays attached to the binary that flaked.
- `.github/workflows/ci.yml` — the re-pointed `-E` filter.
- `architecture/testing.md` — the new target topology, the before/after counts, and the measured per-family link cost.
- M003's ownership manifest — updated in the same change.
- A static guard is **required** to keep the three consumers and the target list in agreement: the `ci` profile's binary filters must name targets that exist, the CI job's `-E` filter must name a target that exists, and the manifest must name targets that exist. The repository's existing `scripts/check_*.py` idiom with a `--self-test` is the right shape.

## 7. Ordered work packages

### Work package A — Propose the family partition and get it decided

Intent: the partition is the whole design decision; it must be made explicitly, with a measured cost, before any file moves.

Required changes:

1. Propose a partition of the 280 root test files into families. A family is a set of test files that share a subject area and can share one executable. The target is **approximately 15 families** (roadmap §10, D4 — decided by the owner on 2026-10-21). Treat 15 as a **target, not a quota**: if a defensible partition needs 17 families because unrelated subjects must not share an executable, 17 is the better partition. Bending subjects to fit the count is the failure mode this milestone must avoid.
2. For the proposed partition, state:
   - the number of families and the resulting number of linked root executables;
   - the projected unit-time saving, using the measured baseline model (median 13.9 s per target) and stating the model;
   - the **failure-granularity cost**: which families mix unrelated subjects, and what a failure in such a family tells a developer;
   - how each of the six whole-binary exclusion binaries maps into the partition — which family, and whether that family contains other binaries that must not share its exclusivity.
3. Present the partition against the decided ~15-family target for review, with its measured cost. The owner's decision on the **count** is settled (D4); the decision on **which files land in which family** is the deliverable of this work package, and the implementing agent must not treat the count as permission to assign subjects arbitrarily.
4. Record the proposal in the milestone's own plan file as an amendment, or in a decision record attached to it, so the closure record can point at what was decided — including any departure from the ~15 target and its reason.

Acceptance evidence:

- The proposed partition with its counts and the measured projection, stated against the decided ~15-family target (roadmap §10, D4) with any departure and its reason recorded.
- The exclusion-binary mapping, with the families that would inherit a binary's exclusivity and the reasoning about whether that is safe.
- The partition presented for review with its failure-granularity cost.

### Work package B — Build the family mechanism

Intent: move files into families without changing what any test asserts.

Required changes:

1. Use the established mechanism: a `tests/<family>/mod.rs` that declares each member as a module, with the member files carrying a `.inc` extension, plus a `[[test]]` declaration with `path = "tests/<family>/mod.rs"` in the root `Cargo.toml`. Cargo does not auto-discover `tests/<dir>/mod.rs`, so the explicit declaration is required.
2. Choose the `.inc` extension over `.rs` deliberately: a `.rs` file inside `tests/<dir>/` is not auto-discovered as a target, but using `.rs` invites confusion about whether it is compiled as its own target. The existing precedent uses `.inc`.
3. Ensure each family's `mod.rs` gives each member module a **distinct test-name prefix** if the members' test function names could collide. Rust's test harness names tests by their fully-qualified path, so `family::member::test_name` is unique — but the implementing agent must verify no two members define the same function name in a way that collides within a module.
4. Handle the members' `mod common;` declarations. `tests/common/mod.rs` is shared by 44 top-level test files plus the two existing family roots. Moving a file into a family changes the relative path to `tests/common`, and the agent must confirm the resulting module paths still resolve — **this is the most likely mechanical failure in this milestone** and must be verified per family, not assumed.
5. Ensure the feature-gated targets keep their gates (I8). A family that contains both a gated and an ungated target cannot be gated as a whole; such a family must be split, or the gated member must keep its own target.

Acceptance evidence:

- The family layout, with the `[[test]]` declarations.
- Per-family confirmation that every member's module paths resolve — a compile of each family.
- The gated/ungated split, with each gated target still gated.

### Work package C — Re-point the three target-name consumers

Intent: nothing that names a test target may be left pointing at a name that no longer exists.

Required changes:

1. `.config/nextest.toml` — re-point the six whole-binary exclusions. **The rationale comment stays attached to the binary that flaked**, rewritten if the binary now lives inside a family. If a family would contain two binaries that both previously held exclusivity separately, the family must be split so each keeps its own exclusivity — merging two exclusively-run binaries into one exclusive binary would serialise them against each other and change the suite's timing profile.
2. `.github/workflows/ci.yml` — re-point the `-E 'not binary(eggwork_remote_execution_live)'` filter.
3. M003's ownership manifest — update the group membership and any target-name patterns.
4. Confirm that `.config/nextest.toml`'s `ci`-profile scope note ("Workspace-wide only: subset runs … should use the default or timing profile") remains accurate after the change.

Acceptance evidence:

- All three consumers updated.
- A check that no filter in `.config/nextest.toml` and no `-E` filter in the CI job names a target that does not exist.

### Work package D — Prove the executed set is unchanged

Intent: establish the milestone's core correctness property.

Required changes:

1. Enumerate the test functions the root package's integration targets executed before consolidation, and after.
2. Compute the difference and account for every entry:
   - a test function present after but not before is **new** and must be justified (a name collision artifact, a module-visibility change, or an accidental duplication);
   - a test function present before but not after must be a **proven duplicate** with the surviving test named — or it is a violation of I1 and the milestone has failed.
3. Verify per-test process isolation is intact (I5): the family `mod.rs` must not introduce any in-test parallelism, and the `ci` profile's model is unchanged.

Acceptance evidence:

- The before/after enumerations and their difference, with a per-entry disposition.
- The executed test-function count before and after, showing no decrease except for individually named proven duplicates.
- The isolation confirmation.

### Work package E — Arm the consistency guard

Intent: the three target-name consumers and the target list cannot drift apart again.

Required changes:

1. Add a static guard that fails when:
   - a filter in `.config/nextest.toml` names a binary that does not exist as a test target;
   - the CI job's `-E` filter names a binary that does not exist;
   - M003's manifest names a target that does not exist, or a target exists that the manifest does not own;
   - the root package's test-target count differs from the count recorded in `architecture/testing.md` **without the recorded figure having been updated** — this is the drift that produced the successor roadmap's §4.4 finding, and arming it is the durable fix.
2. The guard must carry a `--self-test` that injects each violation class and proves rejection.
3. Wire the guard into `scripts/verify.sh quick` and the CI job.

Acceptance evidence:

- The guard passing on the changed tree.
- The `--self-test` output for every violation class.
- The guard listed in `scripts/verify.sh quick` and the CI job.

### Work package F — Measure and record

Intent: produce the hosted evidence and correct the documentation.

Required changes:

1. Run the hosted `CI / verify` job on the change; record total wall-clock and the test-step build/execution split, labelled hosted.
2. Record the before/after linked-executable count and the total size of the linked executables.
3. Record the measured per-family link cost.
4. Update `architecture/testing.md` and the M003 manifest.
5. Record what the change did not improve.

Acceptance evidence:

- Hosted run id, wall-clock, build/execution split, extraction method.
- Before/after executable counts and sizes; per-family link cost.
- Documentation updates.
- An explicit statement of what did not improve.

## 8. Failure, cancellation, restart, and contention semantics

- **A family that mixes exclusive binaries changes the suite's timing profile.** Two binaries that previously ran exclusively against the whole runner would now share one exclusive slot with each other. If the family's other members are not also exclusive-safe, the family must be split. This is the milestone's main contention hazard.
- **A family whose members share a module namespace can collide.** Two members defining the same function name inside one module is a compile error, not a runtime failure — which is safe, but it must be found by compiling every family, not by compiling the workspace once.
- **A family's members share a process only if the harness is changed.** Under the `ci` profile each test still runs in its own process, so members of a family remain isolated from each other at runtime. The milestone must not introduce in-process parallelism (I5).
- **A failure in a family reports the family's target name**, not the individual file's. The developer must be able to map a family name back to its member files; the family `mod.rs` and the documentation must make that mapping obvious.
- **Cancellation and restart semantics are unchanged.** Nextest owns them and the `ci` profile is untouched apart from re-pointed filters.

## 9. Compatibility and migration

- **Developer-local compatibility.** A developer who ran a single test target directly (`cargo nextest run --test <old-name>`) must be able to find the new name. The family naming must be discoverable, and the documentation must carry the old-to-new mapping.
- **CI compatibility.** The CI job's `-E` filter is re-pointed in the same change. The hosted job's executed test set must be unchanged.
- **Migration.** The old target names disappear. Because they are developer-facing command-line identifiers, the documentation must record the mapping, and the closure record must list every removed target name and its successor.
- **M003's manifest migration.** The manifest's group membership changes. M003's schema was chosen in work package A step 3 to support this as a membership update rather than a schema rewrite; if it turns out to require a rewrite, that is a finding to record in M003's closure as well.

## 10. Required tests

### Focused unit tests

None new for the consolidation itself. The mechanism is mechanical; its correctness is established by the enumeration comparison in work package D.

### Integration tests

- Every family compiles and its members' tests execute: proven by running each new family target.
- The per-family module-path resolution check from work package B step 4.

### Restart and recovery tests

None.

### Contention and cancellation tests

- The re-pointed whole-binary exclusions are demonstrated to still hold their binary exclusively: the Nextest listing under the `ci` profile shows each of the six binaries running with `threads-required = "num-cpus"`.
- The `ci` profile's 4-slot bound is unchanged.
- The live Eggwork target's Linux gate and exclusive fixture lifecycle are intact, and the CI `-E` filter still excludes it from the second invocation.

### Security and negative tests

- The consistency guard's `--self-test` for every violation class.
- The guard run against a deliberately broken configuration — for example a filter naming a nonexistent target — proving the repository-side rejection independently of the self-test.
- A family whose members collide on a function name must fail to **compile**, not to pass silently. This is demonstrated by the compile check, and must not be worked around by renaming a test function (which would violate I1's "no behavioural assertion removed" spirit by changing identity).

### Migration and compatibility tests

- The before/after test-function enumeration and its difference, with a per-entry disposition.
- The old-to-new target-name mapping recorded and complete — every removed target name has a successor.

## 11. Required verification commands

```bash
# the consistency guard, including its self-test
python3 scripts/check_test_target_consistency.py
python3 scripts/check_test_target_consistency.py --self-test

# enumerate the targets that exist
cargo metadata --no-deps --format-version 1

# run each family target (repeat per family)
cargo nextest run -p codegg --locked --profile default --test <family>

# confirm the exclusions still hold their binary exclusively
cargo nextest list --workspace --locked --profile ci

# formatting and linting
cargo fmt --check
cargo clippy --workspace --all-targets --locked -- -D warnings

# routine guard sweep
scripts/verify.sh quick

# broader suite
cargo nextest run --workspace --locked --profile ci

# hosted evidence
# .github/workflows/ci.yml — CI / verify, on the change
```

## 12. Documentation updates

- `architecture/testing.md` — the new target topology, the before/after linked-executable counts, the old-to-new target-name mapping, and the measured per-family link cost. The recorded counts must be measured, not estimated.
- `.config/nextest.toml` — the re-pointed filters, with the flake rationale kept attached to the binary that flaked.
- `docs/test-ownership.toml` (M003's manifest) — updated group membership.
- `AGENTS.md` — if it references any root test target by name.
- `plans/subsystems/ci-test-throughput-recovery-roadmap.md` — M004's status row, and the D4 decision recorded in the decision-point table.
- `plans/registry.md` — M004's status transitions.

## 13. Acceptance criteria

1. The root package links materially fewer test executables than the baseline's 271 root integration targets, with the before/after count measured and stated. The partition lands at approximately the decided ~15 families, or the closure record states why a different count is the better partition.
2. Every test function that existed before consolidation still exists and executes exactly once — the count does not decrease except for individually named proven duplicates, each with its surviving test named in the closure record.
3. All six `ci`-profile whole-binary exclusions remain in force, re-pointed at their binaries, with the `ci` profile's 4-slot bound unchanged and the profile otherwise byte-identical apart from the filter targets.
4. The CI job's `-E` filter and M003's manifest are updated in the same change; no consumer names a target that does not exist.
5. A static guard with a passing `--self-test` is wired into `scripts/verify.sh quick` and the CI job, and it rejects an injected violation of each class — including a target count that has drifted from the recorded figure without the record being updated.
6. Every feature-gated target remains gated; no gated test became runnable without its feature.
7. The live Eggwork target keeps its Linux gate and its exclusive fixture lifecycle.
8. No test body, assertion, or helper semantics changed; no dependency, feature default, or product behaviour changed.
9. `scripts/verify.sh quick` and `verify.sh full` both pass, and `cargo clippy --workspace --all-targets --locked -- -D warnings` passes.
10. `architecture/testing.md` records the hosted measurement, the before/after counts, the old-to-new name mapping, and what did not improve.

## 14. Stop conditions

The implementing agent must stop and report rather than improvise when:

- the family partition would merge two binaries that each previously held whole-runner exclusivity, and the family cannot be split to preserve both exclusivities — this would change the suite's timing profile and could reintroduce the flake class the exclusions exist to prevent;
- a proposed family would make a `required-features`-gated target runnable without its feature (I8);
- the partition would reduce the executed test-function count beyond individually proven duplicates (I1, I4) — the predecessor's M003 consolidation removed 60 test functions, and this milestone must not repeat that silently;
- the consolidation cannot be done without changing a test's body or assertion;
- the only way to reach the ~15-family target is to assign unrelated subjects to the same family purely to hit the count — the count is a target, and a defensible 17-family partition is the better outcome;
- the work would require changing the `ci` profile's slot count or deleting an exclusion (I3);
- scope would expand into consolidating the inline unit tests, the other members' test targets, or the D1 coverage decision.

## 15. Closure evidence required

The closure record `plans/closure/ci-test-throughput-recovery/004-status.md` must contain:

1. the implemented partition, the recorded decision it was taken against (roadmap §10 D4 — ~15 families, decided by the owner on 2026-10-21), and any departure from that target with its reason;
2. the before/after linked-executable counts, measured, and the total size of the linked executables;
3. the measured per-family link cost;
4. the before/after test-function enumerations and their difference, with a per-entry disposition and the surviving test named for every proven duplicate;
5. the exclusion re-pointing, with the Nextest listing showing each of the six binaries still holding `threads-required = "num-cpus"`;
6. the CI `-E` filter's re-pointing, and confirmation that the live Eggwork target is still excluded from the second invocation and still Linux-gated;
7. the consistency guard's name, its `--self-test` output for every violation class, and its output against a deliberately broken configuration;
8. the old-to-new target-name mapping, complete;
9. the hosted run id, wall-clock, and test-step build/execution split, labelled hosted, with the extraction method;
10. a comparison against the successor roadmap §4.1 baseline and against the predecessor's recorded run, stating explicitly that the predecessor's figure was taken against a different target count;
11. a confirmation that no test body, assertion, or feature gate changed, and that the `ci` profile is unchanged apart from filter targets;
12. the documentation diffs for `architecture/testing.md`, `.config/nextest.toml`, the M003 manifest, and `AGENTS.md`;
13. the registry updates performed, including the dependency audit against the registry's blocked-work section.

## 16. Handoff notes

- **This milestone moves 280 files.** Use `git mv` so history follows the move, and keep the work in reviewable batches per family — a single 280-file commit is unreviewable and makes a mistake expensive to find.
- **The module-path hazard is the likely failure.** `tests/common/mod.rs` is shared by 44 top-level files plus both existing family roots. Moving a file into a family changes its relative path to `tests/common`. Compile each family individually rather than compiling the workspace once at the end.
- **Do not delete a test to make a family fit.** The predecessor's M003 consolidation removed 60 test functions with 11 link steps; if the same pressure appears here, the answer is a different partition, not a deleted test.
- **Keep the flake rationale attached to the binary, not to the family.** `.config/nextest.toml` records *why* each binary is exclusive. Rewriting that comment to be about a family loses the evidence and makes a future re-merge look harmless.
- **Resource constraint.** The baseline root test executables total 13.7 GB. The consolidated set is smaller, but each family executable will be larger than an individual file's binary, because it links the same 855 MB library and more of it.
- **Toolchain.** Rust 1.89+, edition 2021, stable. `cargo-nextest` 0.9.140 at the baseline. `cargo --timings=json` is nightly-only — use plain `--timings`.
- **Preserved user changes.** The worktree may carry unrelated local modifications; do not stash, reset, or discard them.

## 17. Implementation record — the partition as built

Status: **implemented** (2026-10-21); closure `plans/closure/ci-test-throughput-recovery/004-status.md`.

This section is the amendment that work package A §4 required: the partition that was actually built, the decision it was taken against, and the departure from the ~15-family target with its reason.

### 17.1 The partition — 19 families, and why not 15

Roadmap §10 D4 was decided by the owner on 2026-10-21 at "approximately 15 families", with the explicit qualifier that the count is a **target, not a quota**. The implemented partition is **19 families**:

| # | Family | Members | Kind |
|---|---|---|---|
| 1 | `tool_programs` | 72 | subject |
| 2 | `tui_render` | 41 | subject |
| 3 | `core_daemon` | 38 | subject |
| 4 | `scheduler_jobs` | 32 | subject |
| 5 | `agent_loop` | 31 | subject |
| 6 | `security_permission` | 19 | subject |
| 7 | `command_git` | 16 | subject |
| 8 | `config_provider` | 12 | subject |
| 9 | `ide_protocols` | 5 | subject |
| 10 | `lsp_test_support` | 4 | feature-gated (`lsp-test-support`) |
| 11 | `server_feature` | 5 | feature-gated (`server`) |
| 12 | `windows_sandbox_test_support` | 2 | feature-gated (`windows-sandbox-test-support`) |
| 13 | `linux_git_test_support` | 2 | feature-gated (`linux-git-test-support`) |
| 14 | `eggwork_remote_execution_live` | 1 | `ci`-profile exclusion singleton |
| 15 | `interactive_process_attach_resume` | 1 | `ci`-profile exclusion singleton |
| 16 | `interactive_process_sessions` | 1 | `ci`-profile exclusion singleton |
| 17 | `interactive_terminal_tui` | 1 | `ci`-profile exclusion singleton |
| 18 | `scheduler_cancellation` | 1 | `ci`-profile exclusion singleton |
| 19 | `causal_observe_replay` | 1 | `ci`-profile exclusion singleton |

**Ten of the nineteen are structurally forced**, not chosen:

- the **six exclusion singletons** each hold `threads-required = "num-cpus"` in `.config/nextest.toml`'s `ci` profile because the binary they named flaked under contention. A family is a single executable, so two previously-exclusive binaries in one family would serialise against each other and change the suite's timing profile — work package C §1 forbids it. Each therefore stays its own executable.
- the **four feature-gated families** exist because work package B §5 requires each gate to keep its own target: a family mixing a gated and an ungated target cannot be gated as a whole. The baseline's thirteen gated targets collapse to one family per gate.

The remaining **nine are coherent subjects**. Reaching 15 would have required merging four unrelated subjects into one executable — exactly the milestone's own stop condition, "unrelated subjects share a family purely to hit the count". The count moved instead of the subjects.

The two pre-existing families, `projection_replay` (11 members) and `session_family` (5 members), are **not** part of this milestone's 19; they were built by the predecessor's M003/M004 and are untouched.

### 17.2 The exclusion-binary mapping

Every one of the six `ci`-profile exclusion binaries keeps its **exact target name**:

| Excluded binary | Lands in | Inherits other binaries' exclusivity? |
|---|---|---|
| `eggwork_remote_execution_live` | itself (singleton) | no |
| `interactive_process_attach_resume` | itself (singleton) | no |
| `interactive_process_sessions` | itself (singleton) | no |
| `interactive_terminal_tui` | itself (singleton) | no |
| `scheduler_cancellation` | itself (singleton) | no |
| `causal_observe_replay` | itself (singleton) | no |

Because the names are preserved, `.config/nextest.toml`'s `ci`-profile whole-binary filter and `.github/workflows/ci.yml`'s `-E 'not binary(eggwork_remote_execution_live)'` are **byte-identical** to the baseline, and the flake rationale stays attached to the binary that flaked rather than to a family. No exclusion collision exists: no family holds two previously-exclusive binaries.

### 17.3 A fourth target-name consumer — material deviation

Work package C named **three** consumers of target names: the Nextest profile's binary filters, the CI job's `-E` filter, and M003's ownership manifest. The implementation found a **fourth**: `src/test_runner/resolve.rs`'s `resolve_file` maps a test file to the target that runs it. Consolidation invalidates that mapping for every moved file.

This required a **production change**, not a documentation change: a new `declared_family_target` helper resolves a family member to its family target through the root manifest's `[[test]]` declarations, with a unit test covering it. The change is recorded here rather than absorbed silently, per the planning process's material-deviation rule.

### 17.4 Measured outcome

| | Before | After |
|---|---|---|
| Root integration test targets (`cargo metadata`) | 286 | **21** |
| Workspace test-kind targets (`cargo metadata`) | 314 | **47** |
| Root integration test modules | 286 top-level `tests/*.rs` | **301** `tests/*/*.inc` |
| Declared `[[test]]` entries | 15 | **21** |
| `tests/test_util.rs` | present, dead (0 test functions, 0 references) | removed |

The dead `tests/test_util.rs` was removed with `mavis-trash` after confirming it contained no test functions and was referenced nowhere.

### 17.5 Coverage preserved and coverage dropped

**Preserved.** Every test function survives: 301 root integration-test modules moved without a single test body, assertion, or feature gate being edited. The `ci` profile's slot bound and its seven `threads-required = "num-cpus"` reservations are unchanged. The four feature gates are unchanged. All four platform-specific qualification workflows and `scripts/test-eggsearch-pair.sh` still name live targets.

**Dropped.** Two things, and only two:

1. **Per-target failure granularity.** A failure now names a family; the offending member must be read out of the failure line. The Nextest test id is `family::member::test_name`, so the information is present — it is not free.
2. **The ability to select a single former target by name.** `cargo nextest run --test <old_name>` no longer resolves. The 14 `--test <target>` references in the workflows were re-pointed at their families; this *broadens* each workflow's selection rather than narrowing it, which is deliberate — a narrow filter that matched nothing would make a qualification step silently vacuous, and that is the worse failure.

### 17.6 What is evidence and what is still not

**Now measured.** The full workspace suite was run end to end with
`--no-fail-fast`: **15,580 tests run, 15,572 passed, 7 failed, 1 timed out,
8 skipped.** Every failure was accounted for against the baseline commit
`2c1953347`, which was checked out into a separate worktree and re-run for the
failing tests: six of the seven fail **identically at the baseline** (macOS
`/tmp` is a symlink and the workspace registry rejects a symlinked root; the
`OfflineRequired` network intent is Linux-only; the scratch-cache contract
expects Linux's `$HOME/.cache` where macOS uses `$HOME/Library/Caches`; and
the standalone-shell delivery test fails at the baseline too), and the
seventh — `tool_program_m012_process_recovery::c30_all_m012_test_files_exist`,
a **structural** test asserting that seven M012 test files exist at their
pre-consolidation paths — was a **real regression this milestone introduced**.
It is fixed, and the fix is recorded in the closure record's §3.1.

That regression is worth stating plainly, because it is the failure mode this
milestone's own verification missed: the test file was byte-identical to its
original, so a file-by-file diff could not see it. What changed was the
**repository layout the test asserted**. A consolidation changes the layout, and
a structural test that names the layout is a structural test the consolidation
invalidates.

**Still not measured.** The ~41% workspace-compile saving quoted in the closure
record is a **projection** from the baseline's 13.9 s/target median applied to
the 288 → 21 target reduction. It is **not a measurement**. The per-family link
cost, the before/after linked-executable total size, and the hosted run are
outstanding; the milestone's closure recommendation is **conditionally closed**
accordingly.
