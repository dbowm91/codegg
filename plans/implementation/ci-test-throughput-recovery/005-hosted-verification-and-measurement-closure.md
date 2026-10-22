# CI and Test Throughput Recovery Milestone M005 — Hosted Verification and Measurement Closure

Status: ready for handoff

Repository baseline: `4f89b4b05455f308076074e637e09affb97cbf93` — the committed change set this milestone measures ("the candidate")

Measurement baseline: `2c1953347896848c2f52c2dfbd9f04029d1c2cc2` — the pre-change tree, already pushed ("the before")

Source roadmap:

- `plans/subsystems/ci-test-throughput-recovery-roadmap.md#milestone-m005--hosted-verification-and-measurement-closure

Long-term requirements:

- `plans/000-long-term-specification.md` §4.7 — correctness before transparent magic;
- `plans/003-planning-process.md` §2.3–2.4.

Applicable ADRs: none required. This milestone changes **no product source, no test, no build configuration, and no CI job definition**. It produces measurements and records.

Predecessor work (closed, historical, not reopened by this milestone):

- `plans/subsystems/ci-test-throughput-optimization-roadmap.md` M001–M005 and corrective C001–C002 — the predecessor consolidations and the corrected verification counts;
- `plans/subsystems/ci-test-throughput-recovery-roadmap.md` M001–M004, M006, M007 — the change set this milestone measures.

Primary class: infrastructure, with a polish obligation.

---

## 1. Objective

Turn the workstream's benefit from a **projection** into a **measurement**, and close the workstream.

Two claims are outstanding and both are currently unsupported:

1. **The link-cost claim.** M004's closure record states a **~41 %** workspace-compile saving. It is explicitly *a projection* from the baseline's 13.9 s/target median, not a measurement, and the roadmap's completion definition asks for the linked-executable count and hosted wall-clock evidence.
2. **The scoped-cost claim.** M003's closure record states that its dependency coupling is *argued rather than measured* for four of its five representative cases, and the roadmap's §7 makes the demonstration an exit condition of M003.

This milestone discharges both, produces the hosted evidence the completion definition asks for, and writes the workstream's closure record.

## 2. Why this milestone is ready

**Hard dependencies: none.** M001–M004, M006 and M007 are implemented, locally verified, and committed at `4f89b4b05`.

**The owner decision that gated this milestone has been partially discharged.** The registry's M005 row blocked on "the M001–M004 change set is **uncommitted** (397 paths; `git log origin/main..HEAD` is empty), so the hosted 'after' run cannot exist". The change set is now committed:

```
4f89b4b05  feat(ci): the CI/test-throughput recovery workstream — M001–M004, M006, M007
468 files changed, 15817 insertions(+), 2014 deletions(-)
```

The remaining owner actions are named in §3.5 and are **mechanical**, not design decisions: the branch must be pushed, and the before-run must be dispatched.

**Why a whole milestone for this.** The workstream's completion definition (§11 of the roadmap) has six bullets. Four are discharged by the implementation. Two are not: the hosted evidence and the measured link cost. A milestone whose entire deliverable is a measurement still needs a preregistration, a method, a record, and a closure disposition — the same discipline as any other milestone, and the repository has a strong precedent for exactly this shape (a measurement preregistration written *before* the candidate, and a closure record enumerating what the evidence must contain).

## 3. Current implementation evidence

### 3.1 The countable claim already holds, locally

The primary claim is not a wall-clock; it is a **count**, and it is countable on both SHAs with no hosted run at all:

| | Before (`2c1953347`) | After (`4f89b4b05`) |
|---|---|---|
| Root package test targets | **288** | **21** |
| Root integration-test executables | 271 declared + 13 auto-discovered | 21 declared, 0 auto-discovered |
| Workspace test targets | **314** | **47** |
| Root test binaries' total size | 13.7 GB | (measured in work package C) |

The 288 → 21 figure is `cargo metadata --no-deps` on each SHA, and it is the number the roadmap's §11 first bullet asks for ("links and builds materially fewer test executables"). A 92.6 % reduction in linked root test executables is not a projection.

### 3.2 What the hosted job can and cannot measure

This is the constraint that shapes every work package, and it is worth stating precisely because it is easy to assume the hosted job reports more than it does.

The CI job's test step is:

```yaml
- name: Workspace tests
  run: cargo nextest run --workspace --locked --profile ci
```

**The build and the run are fused in one cargo invocation.** The job has no separate "build the test targets" step, so there is no step boundary to read the build phase off. The split is still recoverable from the log, because cargo and nextest each print their own duration:

| Line in the job log | What it measures |
|---|---|
| `Finished \`test\` profile [unoptimized + debuginfo] target(s) in 5m 37s` | the **build** phase — cargo's compile time for that invocation |
| `Summary [ 776.090s] 15466 tests run: ...` | the **test execution** phase — nextest's own wall-clock |
| The job's `total_duration` | the sum, plus the guard/fmt/clippy steps |

So the hosted measurement is a **two-phase wall-clock** plus the job total, and the phase split is available without changing the job. The guards, `cargo fmt --check` and `cargo clippy` are separate steps and have their own timings, from the Actions API (`gh api repos/{owner}/{repo}/actions/runs/{id}/jobs` returns each step's `started_at` / `completed_at`).

**What the hosted job cannot report: the per-target link cost.** The job never emits cargo's `--timings` data, so the number M004's closure record owes cannot be produced hosted at all. It is a local measurement, and work package C produces it.

### 3.3 The per-family link cost has never been measured

M004's closure record states the saving as "~41 % of workspace compile work", attributed to a **13.9 s/target median** from the baseline's unit-time table. That median is the §4.1 baseline's, not a measurement of the consolidated families. The roadmap's §11 asks for "hosted wall-clock evidence and the measurement method stated", and M004's own exit condition asks for "the measured per-family link cost". Neither exists.

The method is known and was measured during the plan's research: `cargo test --workspace --locked --no-run --timings`, then parse the emitted `const UNIT_DATA = [...]` block. The `--timings=json` form is **unstable on cargo 1.89** and must not be used.

### 3.4 M003's scoped cost has never been discharged

M003's closure record §6.1 is explicit: the coverage-instrumented comparison was run for **one** representative case (the leaf crate `eggsentry`), and the other four rest on the manifest's mapping argument. The four that remain argued-not-proven are the ones whose shape differs from the measured case:

| Changed file | Selected member(s) | Selected targets | Why it is a different case |
|---|---|---|---|
| `src/tui/app/mod.rs` | `codegg` | `tui_render` | The root package has **no dependents**, so nothing is pulled in — the selection is *precise* and must be shown to be sufficient, not merely wide |
| `crates/codegg-core/src/session/schema.rs` | `codegg-core`, `codegg` | `codegg-core`: 3; `codegg`: 21 | A **non-root** member with several groups, plus a transitive pull-in |
| `src/scheduler/admission.rs` | `codegg` | 3 groups | One root member covered by **three** separate groups |
| `src/lsp/mod.rs` | `codegg` | `lsp_test_support` | The selected target is **feature-gated**; the comparison must account for the gate |

M003's own note says the remaining cases are argued-not-proven because "instrumenting and relinking all 47 test targets costs exactly what this workstream exists to reduce". **After M004, that cost is gone**: the workspace has 47 test targets instead of 314, so the instrumented comparison is now affordable. This milestone is the first point at which the measurement can be made for the price the workstream was built to pay.

### 3.5 The rebase is a measurement decision, not only a merge decision

The roadmap lists the rebase as an M005 dependency, and it is real: `origin/main` is 111 commits ahead, and its advance adds **twelve** new top-level `tests/*.rs` files, which — because the root package sets no `autotests = false` — would auto-discover as twelve new root test targets and take the count from 21 to **33**. The twelve are:

```
build_quota_admission.rs                checkout_reservation_c004.rs
checkout_reservation_c004_http.rs       host_admission_manifest.rs
macos_native_shared_checkout_containment.rs
process_denial_c005.rs                  session_launch_service.rs
shared_checkout_c006.rs                tool_program_c002_delegated_ceiling.rs
writer_disposition_guards.rs            writer_fence_c003.rs
writer_permit_service.rs
```

plus one `tests/common/` helper (`tests/common/checkout_reservation.rs`) and a modified `tests/common/mod.rs`. This reproduces the roadmap's claim exactly.

There are two coherent measurement designs, and they are not equivalent:

| | **(a) Measure the verified tree** | **(b) Rebase first, then measure** |
|---|---|---|
| Before SHA | `2c1953347` | the new `origin/main` tip |
| After SHA | `4f89b4b05` | the rebased branch tip |
| The hosted delta measures | exactly this workstream | exactly this workstream |
| The "after" tree is | the tree that passed the local suite (15,466 tests) | a **different** tree that must be re-verified locally |
| The rebase's 12 new files | not yet absorbed | absorbed into families, changing the topology the measurement captures |
| The branch's end state | needs a later rebase | mergeable |

**This plan recommends (a), and records the deviation from the roadmap's dependency list.** The reason is specific: the hosted "after" run must measure *the tree that was verified*, and `4f89b4b05` is that tree — it ran 15,466 tests locally with only the six pre-existing platform failures. Rebasing produces a different tree, which would need its own full local suite run and its own verification before it could be measured, and the absorption of the twelve new files would change the target count that the measurement is *about*. Measuring (a) and then rebasing keeps the measurement a property of the verified change set, and turns the rebase into an ordinary, separately-verified merge activity rather than something entangled with the workstream's closing evidence.

**The deviation is recorded, not hidden**: the roadmap's M005 dependency 3 (rebase) is **deferred past the measurement** and becomes a separate change with its own local verification. §14 (stop conditions) and §15 (closure evidence) carry it forward explicitly.

### 3.6 The remaining owner actions

| Action | State | Who |
|---|---|---|
| Commit the change set | **done** — `4f89b4b05` | — |
| Push the branch | **outstanding** | owner |
| Choose the before-run baseline | **recommended** `2c1953347`; see §6.2 | owner |
| Rebase | deferred past the measurement — see §3.5 | owner, after closure |

`origin/main` is `95bb9c5ae4c43b0fe5fbd200918ae1debc759463` as of this plan's authoring. The before-run's baseline must be re-read at execution time, because `main` moves.

### 3.7 What this milestone is not

- It is not a throughput milestone. It changes no code and no configuration; it produces evidence.
- It does not reopen any predecessor milestone, and it does not re-litigate any recorded disposition.
- It does not decide the feature-gated coverage question (roadmap §10, D1). That is an owner coverage decision and is explicitly deferred.
- It does not reduce the number of tests the hosted job executes in order to reach a number.

## 4. Invariants that must not regress

| ID | Invariant |
|---|---|
| I1 | The hosted "after" run executes **every test function** the "before" run executed, plus whatever the change set added. No milestone in this workstream may reduce the coverage the hosted CI job executes in order to reach a number. |
| I2 | The measurement method is **preregistered before the candidate run is dispatched**. A method chosen after seeing the result is not evidence. |
| I3 | The hosted job's **definition is not changed** by this milestone. Adding a build step to separate the phases would change the thing being measured; the split is recoverable from the log instead (§3.2). |
| I4 | Every reported number names its **source**: the command that produced it, the SHA it was produced on, and the date. A number without a source is not evidence. |
| I5 | A measurement that is **not** reproduced is reported as not reproduced, with the reason. The workstream's precedent is a predecessor's baseline that was wrong by ~2.4× and was corrected in the open rather than quietly replaced. |
| I6 | The local verification of the candidate is not re-done by this milestone except where a work package requires it, and the local suite's known platform failures are reported as the same six the M004 closure record proved pre-existing. |

## 5. Scope

### In scope

- the hosted before-run and after-run of the `CI` workflow, on the two preregistered SHAs;
- the per-family link cost, measured locally from cargo's unit-time data;
- the four remaining coverage-instrumented comparisons M003 left argued-not-proven;
- the republication of `architecture/testing.md` with the measured numbers;
- the workstream's closure record, and the registry and roadmap updates that close it.

### Explicitly out of scope

- **Any change to product source, tests, build configuration, or the CI job definition.** I3 forbids it and this milestone has no reason for it.
- **The rebase.** Deferred past the measurement per §3.5; it is a merge activity with its own verification.
- **The feature-gated coverage decision** (roadmap §10, D1).
- **Any further consolidation.** The workstream's own completion definition permits a negative optimisation experiment to close if it is reverted and documented; this milestone does not open one.
- **Re-measuring the §4.1 baseline.** The baseline's unit-time table was measured at `2c1953347` and is the reference the delta is computed against. Re-measuring it on a different machine would make the two numbers incomparable.

## 6. Required production changes

**None.** The only new files this milestone adds are two documents under `plans/`:

### 6.1 The measurement preregistration

`plans/validation/ci-test-throughput-recovery/measurement-preregistration.md` — written **before** the candidate run is dispatched, and it must contain:

1. the two SHAs, and the reason each was chosen;
2. the workflow and the ref each run is dispatched on, and the exact `gh` commands;
3. what is recorded from each run: the job total, the per-step timings from the Actions API, the cargo `Finished ... in X` line, the nextest `Summary [ Xs]` line, and the test count;
4. the **primary** claim — the linked root-test-executable count, measured with `cargo metadata --no-deps` on both SHAs — and the **secondary** claim — the hosted two-phase wall-clock;
5. the number of runs per side (at least **three**, so a dispersion can be reported) and the statistic reported (the **median**, with the full set of raw values, never a best-of);
6. the acceptance threshold, stated as a number **before** the runs;
7. the confounders to be recorded and how they are handled: runner image, cache state (the job uses `Swatinem/rust-cache@v2`, so a cold-cache run and a warm-cache run are **different measurements** and must be labelled), the `mold` linker, and the `ci` profile's four-slot bound;
8. the explicit statement that the wall-clock is **noisy** and that the countable claim is the primary one.

A preregistration that cannot be checked against the runs it describes is not a preregistration.

### 6.2 The measurement record

`plans/validation/ci-test-throughput-recovery/measurement-record.md` — the results, with each number's source, and an explicit "not reproduced" section.

### 6.3 The closure record

`plans/closure/ci-test-throughput-recovery/005-status.md` — the workstream's closure, with a recommendation.

## 7. Ordered work packages

### Work package A — Preregister the measurement

Intent: fix the method before any result exists.

1. Read `origin/main`'s tip and record the baseline SHA chosen, with the reason (§6.2 item 1).
2. Write the preregistration document (§6.1) in full.
3. Record the **primary** claim's numbers on both SHAs with `cargo metadata --no-deps`:
   `git worktree add` each SHA, run the command, record the target counts, remove the worktree.
4. Have the preregistration reviewed before proceeding. **A preregistration that is reviewed after the candidate run is not a preregistration.**

Acceptance evidence:

- the preregistration document exists, is dated, and names both SHAs;
- it states the threshold, the run count, the statistic, and the confounders;
- the primary claim's counts are recorded on both SHAs.

### Work package B — Produce the hosted before-run and after-run

Intent: the hosted evidence itself.

1. **Push the branch.** `git push origin feat/auto-20261021-0bbb0778`. This is the owner action; the milestone cannot proceed without it, and without it the hosted "after" run cannot exist.
2. **Create the before-run's ref.** `workflow_dispatch` takes a **branch or tag**, not an arbitrary SHA, so the baseline gets a temporary branch: `git branch ci-baseline/2c1953347 2c1953347 && git push origin ci-baseline/2c1953347`. The branch is deleted after the run.
3. **Dispatch the before-run**: `gh workflow run ci.yml --ref ci-baseline/2c1953347`.
4. **Dispatch the after-run**: `gh workflow run ci.yml --ref feat/auto-20261021-0bbb0778`.
5. **Run each side at least three times**, and label each run's **cache state**. The job uses `Swatinem/rust-cache@v2`; a warm-cache run and a cold-cache run measure different things, and a wall-clock quoted without its cache state is not a measurement. The preregistration says which state the comparison uses.
6. **Collect**: for each run, the job's `total_duration`, the per-step timings from the Actions API, the cargo `Finished` line, the nextest `Summary` line, and the test count.
7. **Verify I1**: the after-run's test count is at least the before-run's, and the set of test functions the after-run executes is a **superset** of the before-run's. The `CI` job's `paths-ignore` does not apply to `workflow_dispatch`, so both runs execute the full suite — this must be checked rather than assumed.

Acceptance evidence:

- the run URLs and run IDs for every run, with cache state;
- the extracted numbers per §6.1 item 3, per run;
- the I1 verification.

### Work package C — Measure the per-family link cost

Intent: discharge M004's outstanding measurement.

1. On the candidate, in a **fresh** `CARGO_TARGET_DIR` (a warm tree would report the cost of *checking* rather than of linking), run:
   `cargo test --workspace --locked --no-run --timings`
2. Parse the emitted `const UNIT_DATA = [...]` with `re.S` — **not** `--timings=json`, which is unstable on cargo 1.89.
3. Report, per family: the number of linked executables, the aggregate unit-seconds, and the median unit-seconds. The baseline's counterpart is the §4.1 table: **271** root integration-test targets, **4,040.1** unit-secs, **13.9 s** median.
4. Report the **actual** saving as a percentage, and compare it with the **~41 % projection** in M004's closure record. If it does not reproduce, §I5 applies: report that, with the reason. The projection was computed from the baseline's median applied to the new count; the measurement is the real thing.
5. Record the machine's shape with the numbers (cores, RAM, cargo version, linker), because unit-seconds are only comparable within a machine.

Acceptance evidence:

- the parsed unit-time data for the candidate, with the parse command;
- the per-family table;
- the measured saving, stated against the projection.

### Work package D — Discharge M003's scoped cost

Intent: turn four argued-not-proven cases into measured ones.

1. For each of the four cases in §3.4, run the coverage-instrumented comparison with `cargo llvm-cov`:
   - the **changed member's own tests**: `cargo llvm-cov -p <member>`;
   - the **root package's tests**: `cargo llvm-cov -p codegg --lib --test <each selected target>`.

   `cargo llvm-cov -p <pkg>` reports only that package's files, so the root's side must name each selected target explicitly. M003's measured case is the template.
2. Record, per case: the lines of the changed member's code covered by the changed member's own tests, by the **scoped selection's** targets, and by the **workspace-wide** run; and the **behavioural** dependency — the assertion whose outcome a change to the member would move, and whether that assertion lives in the scoped selection.
3. The property to demonstrate is the **negative** one M003 names: *a change that touches a subsystem with a known covering test must select that test*. For the root-only cases the selection is narrow by design, so the comparison must show the selected targets cover the changed code — and, where they do not, that the **dependency widening** (the root package's full surface) covers it.
4. State, per case, what the measurement does **not** show. M003's §6.1 is the model: it was explicit that at line level the member's own tests already covered a superset, and that the real finding was the behavioural dependency. A comparison that reports only a line-coverage number, without the behavioural dependency, is the wrong measurement.

Acceptance evidence:

- the four comparisons, with their numbers;
- per case, the itemised statement of what is now measured and what remains argued;
- an amended M003 closure record, or a cross-reference from the M005 closure record to the amended §6.1. M003's record is **not** rewritten in place — it is a closed record — so the amendment is recorded in M005's record with a pointer to M003's §6.1.

### Work package E — Republish `architecture/testing.md`

Intent: the recorded counts must be the measured ones.

1. Replace every count in `architecture/testing.md` with the measured value and its source.
2. Add the hosted two-phase wall-clock, with its dispersion and its cache state, and the countable link-cost claim, and the per-family link cost.
3. Re-verify the "Verified counts" table in `architecture/overview.md` against its own listed `Source` for every number this milestone changes.
4. Keep the table's single-source-of-truth property: `architecture/overview.md` owns the counts, `architecture/testing.md` owns the method and the totals, and neither copies a number from the other without naming where it came from.

Acceptance evidence:

- the diff of `architecture/testing.md` and `architecture/overview.md`, with each changed number's source.

### Work package F — Close the workstream

Intent: the closure is a disposition, not a summary.

1. Write `plans/closure/ci-test-throughput-recovery/005-status.md` against §15's evidence list.
2. Walk the roadmap's **§11 completion definition**, bullet by bullet, and mark each one **discharged** or **explicitly waived with a recorded reason**. The definition permits a waiver; it does not permit silence.
3. Update the roadmap's §12 status table and the registry's M001–M007 rows from *conditionally closed* to their final dispositions, and the workstream row to **closed**.
4. Record the **deferred rebase** (§3.5) as an open item with its own owner and its reason, so the next reader does not think the branch is mergeable.
5. Leave the roadmap's **D1** (the feature-gated coverage decision) open and state that it is open — it is an owner coverage decision and closing the workstream does not close it.

Acceptance evidence:

- the closure record;
- the §11 bullet-by-bullet table;
- the registry and roadmap diffs.

## 8. Failure, cancellation, restart, and contention semantics

- **A dispatched run that fails** is a measurement of a failing tree, not of the throughput. If the after-run fails, that is a **regression** and it is investigated as one — the run is not discarded and re-dispatched until it passes.
- **A superseded run** is cancelled by the job's own `concurrency` group (`ci-${{ github.ref }}`) when a newer run starts on the same ref. A cancelled run contributes no signal and is recorded as cancelled.
- **The two refs do not share a concurrency group** (the group is keyed on the ref), so the before-run and the after-run do not cancel each other. A run dispatched while the other is in flight is still measured, but the plan should dispatch them **serially** so the two sides do not compete for the same runner capacity — a contended pair is a worse measurement than a sequential pair, and the runner is shared.
- **The baseline branch** `ci-baseline/2c1953347` is created for the run and **deleted afterwards**. Leaving it on the remote would leave a second, stale copy of the pre-change tree that a later `workflow_dispatch` could silently measure.
- **A local re-run of the suite** (work packages C and D) is subject to the same contention artefact the M004 and M006 closure records document: `agent::asset_refresh::tests::same_scope_requests_coalesce_to_one_publication` times out at 300 s when other cargo builds share the machine, and passes in **0.031 s** alone. A timed-out run under contention is a measurement artefact and must be re-run alone, not reported as a failure.

## 9. Compatibility and migration

None. This milestone changes no artefact that a consumer depends on. The only observable changes are the recorded numbers in `architecture/`, the two new documents under `plans/validation/`, and the closure record.

## 10. Required tests

This milestone adds no test. Its "tests" are measurements, and they must be **repeatable**: a reader must be able to re-run the measurement and get the same method.

| Measurement | Command | Repeatability requirement |
|---|---|---|
| Root test-target count | `cargo metadata --no-deps --format-version 1` on each SHA | exact, deterministic |
| Per-family link cost | `cargo test --workspace --locked --no-run --timings` in a fresh `CARGO_TARGET_DIR` | same machine, same cargo, same linker |
| Scoped coverage comparison | `cargo llvm-cov -p <member>` and `cargo llvm-cov -p codegg --lib --test <targets>` | same toolchain |
| Hosted wall-clock | the dispatched `CI` runs | statistical, with the dispersion reported |

A measurement that cannot be re-run by a reader is a claim, not a measurement, and the workstream's precedent (the predecessor's ~2.4×-wrong baseline) is the cost of accepting claims.

## 11. Required verification commands

```bash
# The candidate tree must still be the tree that was verified.
git log -1 --format='%H %s'
git status --short                       # empty

python3 scripts/rust_test_scan.py --validate
python3 scripts/check_test_assertion_quality.py
python3 scripts/check_test_duplication.py
python3 scripts/check_tool_program_parse_only.py
bash scripts/verify.sh quick

# The primary claim, on both SHAs, in a clean worktree each.
git worktree add /tmp/before 2c1953347
git worktree add /tmp/after  4f89b4b05
(cd /tmp/before && cargo metadata --no-deps --format-version 1 | \
  python3 -c "import json,sys; m=json.load(sys.stdin); \
    print(len([t for p in m['packages'] if p['name']=='codegg' \
               for t in p['targets'] if 'test' in t['kind']]))")
(cd /tmp/after && cargo metadata --no-deps --format-version 1 | \
  python3 -c "import json,sys; m=json.load(sys.stdin); \
    print(len([t for p in m['packages'] if p['name']=='codegg' \
               for t in p['targets'] if 'test' in t['kind']]))")
git worktree remove /tmp/before && git worktree remove /tmp/after

# The per-family link cost, on the candidate, from a cold target directory.
CARGO_TARGET_DIR="$(mktemp -d)" cargo test --workspace --locked --no-run --timings
```

## 12. Documentation updates

- `architecture/testing.md` — the measured counts, the hosted two-phase wall-clock with its dispersion and cache state, the per-family link cost, and the scoped-cost results. The scanner's validation counts stay the source for the function totals.
- `architecture/overview.md` — the "Verified counts" table, re-pinned for every number this milestone changes, each verified against its own listed `Source`.
- `plans/subsystems/ci-test-throughput-recovery-roadmap.md` — §12's status table; the M005 section's "reserved, not opened" heading; and **§10's decision log**, which gains the measurement-ordering decision (D9) and the deferred-rebase deviation.
- `plans/registry.md` — the M001–M007 rows' final dispositions, the M005 row, and the workstream row.
- `plans/closure/ci-test-throughput-recovery/001-status.md` … `004-status.md` — the *conditionally closed* recommendations become final, each with a pointer to the M005 closure record's hosted evidence. The records are **amended, not rewritten**: the conditional recommendation was the correct disposition at the time, and the amendment says what discharged it.
- `AGENTS.md` — no change expected; if the measured numbers appear there, they are re-pinned from `architecture/overview.md`.

## 13. Acceptance criteria

1. The measurement preregistration exists, is dated, and was written **before** the candidate run was dispatched, and it names both SHAs, the threshold, the run count, the statistic, and the confounders.
2. The hosted before-run and after-run both exist, with at least three runs per side, each labelled with its cache state, and every run's URL and extracted numbers recorded.
3. The **primary** claim — the linked root-test-executable count — is recorded on both SHAs and its reduction is stated as a number.
4. The **secondary** claim — the hosted two-phase wall-clock — is recorded with its dispersion, its cache state, and its phase split, and is never quoted as a single best-of number.
5. I1 holds: the after-run executes a **superset** of the before-run's test functions, and that is checked rather than assumed.
6. The per-family link cost is measured, and the measured saving is compared with M004's **~41 %** projection, with the comparison reported either way.
7. M003's four remaining representative cases have a coverage-instrumented comparison, and each carries an itemised statement of what is now measured and what remains argued.
8. `architecture/testing.md` and `architecture/overview.md` carry the measured numbers with their sources; no published number is a projection.
9. The roadmap's **§11** completion definition is walked bullet by bullet, each bullet discharged or explicitly waived with a recorded reason.
10. The deferred rebase is recorded as an open item with its owner and its reason; the branch is not described as mergeable.
11. D1 (the feature-gated coverage decision) is recorded as **still open**.
12. The closure record carries a recommendation: closed, conditionally closed, corrective pass required, or blocked.

## 14. Stop conditions

Stop and report, rather than proceeding, if:

- the hosted after-run **fails** — that is a regression, and the milestone stops to investigate it rather than re-dispatching;
- the hosted runs cannot be produced at all (the push is refused, the workflow is disabled, or the runner capacity is unavailable) — the milestone is **blocked** on an owner action, and its closure record says so;
- the primary claim does not reproduce — i.e. the linked root-test-executable count is not materially lower on the candidate than on the baseline. That would mean the consolidation did not happen, and it is a defect to investigate, not a number to report;
- the wall-clock does not reproduce the projection, **and** the reason cannot be established. A number that does not reproduce is reportable (§I5); a number that does not reproduce *and cannot be explained* is a stop;
- the measurement would require changing the CI job's definition, the test set, or the feature set. I1 and I3 forbid it, and "the measurement needs a different job" is a finding, not a licence;
- the rebase, if attempted, cannot be completed without changing a test's assertions or dropping a test. The rebase is deferred past the measurement precisely so that it cannot contaminate the evidence; if it is attempted anyway and it would cost an assertion, that is a stop.

## 15. Closure evidence required

The closure record `plans/closure/ci-test-throughput-recovery/005-status.md` must contain:

1. the preregistration, or a link to it, and the evidence that it predates the candidate runs;
2. both SHAs, the reason each was chosen, and the run URLs and run IDs for every run;
3. the primary claim's numbers on both SHAs and the command that produced them;
4. the hosted two-phase wall-clock per run, with the cache state, the runner image, and the full set of raw values beside the median;
5. the I1 verification, stated as a check and its result;
6. the per-family link-cost table, the parse command, the machine's shape, and the measured saving against the projection;
7. the four coverage-instrumented comparisons, with per case what is now measured and what remains argued;
8. the republicated `architecture/testing.md` and `architecture/overview.md` diffs;
9. the roadmap's §11 completion definition, bullet by bullet, each discharged or explicitly waived with a reason;
10. the deferred rebase, recorded as an open item with its owner and its reason;
11. D1, recorded as still open;
12. the recommendation: closed, conditionally closed, corrective pass required, or blocked.

## 16. Handoff notes

- **The first action is the owner's**: push `feat/auto-20261021-0bbb0778`. Nothing in this milestone can start without it, and until it is pushed the hosted "after" run cannot exist. The local work is already committed at `4f89b4b05`.
- **The preregistration is the second action, and it is the one that must not be skipped.** The hosted runs are the expensive, noisy part; the preregistration is what makes them evidence rather than numbers. Writing it after the runs would make the threshold a function of the result.
- **The baseline branch is temporary.** `ci-baseline/2c1953347` exists only to give `workflow_dispatch` a ref at the baseline SHA, and is deleted after the before-run.
- **The rebase is deliberately not in this milestone.** It is deferred past the measurement, for the reason in §3.5, and it is recorded as an open item rather than quietly dropped. It is a real piece of work: twelve new top-level test files and one `tests/common/` helper have to be absorbed into families, and the result takes the root target count from 21 to 33 and back down to 21.
- **This milestone does not advance the throughput objective.** It measures the workstream's benefit. The workstream's own claim is that the benefit is real; this milestone's job is to say how real, with a number and a source, or to say that the number did not reproduce.
- **The two local measurements have known, documented hazards**: the contention-sensitive timeout (work package C's suite run must not be reported as a failure on that basis), and cargo 1.89's unstable `--timings=json` (the plain `--timings` form and the `re.S` parse are the only supported path).
