# Measurement Preregistration — M005 Hosted Verification and Measurement Closure

Written: **2026-10-22T12:51:07Z**

Amended: **2026-10-22T13:50Z** — see §11. The amendment was made **before any
after-side run completed**, so the method was still fixed before the result
it describes existed.

Author: David Bowman (`dbowm91@proton.me`), M005 milestone implementation.

Milestone plan: `plans/implementation/ci-test-throughput-recovery/005-hosted-verification-and-measurement-closure.md`

Roadmap: `plans/subsystems/ci-test-throughput-recovery-roadmap.md` §7 (M005), §10 (D9), §11 (completion definition)

**This document is written before any candidate run is dispatched.** Every threshold, statistic, extraction command, and confounder below is fixed now. The GitHub run IDs and their `created_at` timestamps will be recorded in `plans/validation/ci-test-throughput-recovery/measurement-record.md`, and the commit that carries this document precedes them in the branch history. A threshold chosen after seeing a result is not a threshold; that is the whole point of this file.

---

## 1. The two SHAs, and why each was chosen

| Side | SHA | Remote ref dispatched on | Why this SHA |
|---|---|---|---|
| **Before** | `2c1953347896848c2f52c2dfbd9f04029d1c2cc2` | `ci-baseline/2c1953347` | The clean pre-change tree: the merge commit immediately preceding this workstream's single change commit. It is **already an ancestor of `origin/main`**, so it is already hosted and needs no new history to be reachable. It is the *same tree* that this workstream changed, so the delta it measures is the delta this workstream produced. |
| **After** | `8e3554ef4b93125cee685211872e77f254030d79` | `m005/candidate-8e3554ef4` | The change set this milestone measures, plus the one-commit defect repair recorded in §11. The repair exists because the candidate's own CI job could not complete on the hosted runner; it changes one guard script's self-test and nothing else. |

**The candidate moved.** It was `4f89b4b05` when this document was first
written. §11 records why it is now `8e3554ef4`, and the amendment was made
before any after-side run completed.

### 1.1 Why the before-run is `2c1953347` and not `239f06e7`

`239f06e7` is the baseline the roadmap's *research* was measured at, and it is **323 commits further back**. Using it would confound the wall-clock delta with 323 commits of unrelated change (the roadmap records the real root test-target count growing from ~203 at that revision to 282 at the research baseline — a growth of its own). `2c1953347` is the immediately-preceding tree, so the before/after delta is attributable to this workstream and to nothing else. The count that `239f06e7` supports is preserved in the roadmap's §4.1 baseline table, where it belongs.

### 1.2 Why the after-run is dispatched on a pinned ref rather than the branch tip

The milestone plan's work package B step 4 names `feat/auto-20261021-0bbb0778` as the after-run's ref. **That is refined here to a pinned ref, `m005/candidate-4f89b4b05`, pointing at exactly `4f89b4b05`.**

The reason is specific and it is not a preference: this milestone keeps committing to `feat/auto-20261021-0bbb0778` — the preregistration (this file), the measurement record, and the closure record all land there. Dispatching the after-runs on that branch would measure a *different tree on each dispatch*, so the three warm repeats would not be repeats of each other, and the "candidate" would not be the tree that was verified.

The pinned ref makes the candidate immutable for the duration of the measurement: every after-run is a run of exactly `4f89b4b05`. Both temporary refs are deleted after the runs (§8).

### 1.3 The dispatch commands, verbatim

```bash
# Before-side runs.
gh workflow run ci.yml --ref ci-baseline/2c1953347

# After-side runs.
gh workflow run ci.yml --ref m005/candidate-8e3554ef4
```

The workflow is `.github/workflows/ci.yml`, job `verify` (`CI / verify`), runner `ubuntu-latest`. Both SHAs carry a `workflow_dispatch:` trigger and the identical `concurrency: group: ci-${{ github.ref }}` with `cancel-in-progress: true`.

---

## 2. What is recorded from each run

For every run on both sides, the following is extracted and written to the measurement record with its source. Nothing else is used to support the claim.

| # | Quantity | Source | Extraction |
|---|---|---|---|
| 1 | Run ID and URL | GitHub API | `gh run view <id> --json databaseId,url,headSha,headBranch,createdAt,conclusion,status` |
| 2 | The SHA the run actually executed | GitHub API | `headSha` from the same call — **asserted to equal the side's SHA**; a mismatch invalidates the run |
| 3 | Job total wall-clock | GitHub API | `gh api repos/{owner}/{repo}/actions/runs/<id>/jobs` → the `verify` job's `started_at` / `completed_at` |
| 4 | Per-step durations | GitHub API | the same `jobs` payload → each step's `started_at` / `completed_at` |
| 5 | **Build phase** (cargo) | job log | the `Finished \`test\` profile [unoptimized + debuginfo] target(s) in X` line in the `Workspace tests` step |
| 6 | **Execution phase** (nextest) | job log | the `Summary [ Xs] N tests run: ...` line in the `Workspace tests` step |
| 7 | **Linked test-executable count** | job log | the `Starting N tests across M binaries` line — **M is the count of linked test executables** |
| 8 | Test count and outcome | job log | the `Summary` line's `N tests run: N passed, N failed, N skipped` |
| 9 | Runner image | job log | the `Runner Image` line in the run's setup log |
| 10 | Cache state | run metadata + log | whether the `Swatinem/rust-cache@v2` step reported a cache **hit** or **miss** for the `target` workspace |
| 11 | Cache key | job log | the rust-cache step's reported key, which encodes the ref and the lockfile hash |
| 12 | Linker | job log | the `mold` presence line from `rui314/setup-mold@v1` |

The run's full log is downloaded (`gh run view <id> --log`) and kept, so every extracted number is re-derivable by a reader.

### 2.1 The phase split is recovered from the log, not from a new step

The CI job fuses build and test into a single cargo invocation:

```yaml
- name: Workspace tests
  run: cargo nextest run --workspace --locked --profile ci
```

There is **no separate "build the test targets" step**, so there is no step boundary to read the build phase off. The split is nevertheless exact, because cargo and nextest each print their own duration (rows 5 and 6 above). **No step is added to the job to obtain this split** — adding one would change the thing being measured, which invariant I3 forbids.

### 2.2 The `Workspace tests` step is the signal; the job total is context

The `Workspace tests` step is **byte-identical** between the two SHAs. The job total is **not** a clean comparison, because the after-side job carries **13 additional steps** that the before-side does not have:

```
Cargo profile alignment guard            Cargo profile alignment guard self-test
Scoped selection self-test               Test assertion quality guard
Test assertion quality guard self-test   Test duplication guard
Test duplication guard self-test         Test ownership guard
Test ownership guard self-test           Test target reference guard
Test target reference guard self-test    Tool Program parse-only guard
Tool Program parse-only guard self-test
```

These are the M003/M006/M007 guards and their `--self-test` invocations. They are fast Python scripts (seconds each, not minutes), and the before-side is not missing any step the after-side has (`comm` on the two step-name lists shows 0 steps only in the before-side). The **job total is therefore reported for context and is not the claim**; the claim is measured in the `Workspace tests` step, with the job total reported beside it.

The **job total is still affected by M001** — profile alignment means the `cargo build --bin codegg-sandbox-helper` step and the nextest invocation now share one compiled dependency graph instead of compiling the third-party graph twice — so the job total is reported, with the caveat above.

---

## 3. The primary claim — countable, not timed

**The primary claim is the number of linked root test executables.** It needs no wall-clock and no hosted run to establish, and it is the claim the roadmap's §11 first bullet asks for ("links and builds materially fewer test executables").

### 3.1 The measurement command, fixed now

```bash
git worktree add /tmp/m005_before 2c1953347
git worktree add /tmp/m005_after  4f89b4b05
(cd /tmp/m005_before && cargo metadata --no-deps --format-version 1 | \
  python3 -c "import json,sys; m=json.load(sys.stdin); \
    print(len([t for p in m['packages'] if p['name']=='codegg' \
               for t in p['targets'] if 'test' in t['kind']]))")
(cd /tmp/m005_after && cargo metadata --no-deps --format-version 1 | \
  python3 -c "import json,sys; m=json.load(sys.stdin); \
    print(len([t for p in m['packages'] if p['name']=='codegg' \
               for t in p['targets'] if 'test' in t['kind']]))")
git worktree remove /tmp/m005_before && git worktree remove /tmp/m005_after
```

### 3.2 The values, measured 2026-10-22T12:51Z with that exact command

| | Before (`2c1953347`) | After (`4f89b4b05`) |
|---|---|---|
| `codegg` root test targets | **288** | **21** |
| Workspace test targets | **314** | **47** |
| Root test target kinds | `test` only | `test` only |

**Reduction in linked root test executables: 267 of 288 = 92.7 %.**

### 3.3 The hosted cross-check

The same quantity is independently countable from each hosted run's log, via
row 7 (`Starting N tests across M binaries`): M is the number of test
executables nextest ran.

**The expected values are 308 (before) and 50 (after), not 314 and 47.**
nextest's binary count is not `cargo metadata`'s test-target count, and the
difference is accounted for exactly:

| | before (`2c1953347`) | after (`8e3554ef4`) |
|---|---|---|
| `cargo metadata` test-kind targets | 314 | 47 |
| less feature-gated targets the job cannot build | −20 | −11 |
| buildable test-kind targets | 294 | 36 |
| plus one lib test harness per package | +12 | +12 |
| plus the root package's two bin test harnesses | +2 | +2 |
| **= nextest's expected binary count** | **308** | **50** |

Two things differ between the two columns and both are real. The 20 and 11
gated targets are the ones whose `required-features` the CI job does not
enable — the `server`, `lsp-test-support`, `lsp-real-server-tests`,
`linux-git-test-support` and `windows-sandbox-test-support` gates — and they
are **not built**, so they are in `cargo metadata`'s count but in no run's
count. And the lib and bin test harnesses are counted by nextest but **not**
by `cargo metadata`'s `kind == ["test"]` filter, because a package's lib
appears there under `kind == ["lib"]`.

The baseline's 308 was **observed** in the first hosted run, and the
arithmetic above reproduces it exactly, which is what makes the predicted 50
for the candidate credible rather than assumed. If the after-side's M
disagrees with 50, that disagreement is reported, not reconciled by adjusting
either number.

---

## 4. The secondary claim — hosted two-phase wall-clock

**This is noisy.** It is the secondary claim. The countable claim in §3 is the primary one, and it is the one that carries the milestone if the wall-clock is inconclusive.

### 4.1 What is compared

For each side, over the runs of that side:

- **Build phase** = the cargo `Finished ... in X` line (row 5).
- **Execution phase** = the nextest `Summary [ Xs]` line (row 6).
- **Step total** = the `Workspace tests` step's `completed_at − started_at` (row 4).
- **Job total** = the `verify` job's `completed_at − started_at` (row 3), reported for context only (§2.2).

### 4.2 The statistic

**The median, within each cache state, with the full set of raw values printed beside it. Never a best-of, never a minimum presented as the result.**

Medians are computed **within** a cache state and never across the mixed set, because a cold-cache run and a warm-cache run measure different things (§6.1). Where a state's sample is small, the raw values and the min/max are reported alongside the median, and the sample size is stated at every use.

### 4.3 Dispatch order

Runs are dispatched **serially**, not concurrently: the milestone plan's §8 requires it so that the two sides do not compete for the same runner capacity, and a contended pair is a worse measurement than a sequential pair. One run is in flight at a time.

**The order is interleaved (before, after, before, after, …), not all-before-then-all-after.** Eight runs span several hours, and the hosted fleet's load is not constant across that window. Interleaving pairs each before-run with an after-run in the same period, so a slow hour on the fleet lands on both sides rather than on one. Running all four before-runs and then all four after-runs would leave the after-side's numbers describing the fleet's state at a different time of day than the before-side's.

The interleaving is fixed here, before any run is dispatched.

---

## 5. The number of runs per side

**Four planned per side (eight total), against the plan's minimum of three.**

The distribution is fixed by the cache: `Swatinem/rust-cache@v2` keys its entry on `github.ref` and the lockfile hash, so the **first** run on each ref is **cold** (cache miss) and every subsequent run on the same ref is **warm** (cache hit). The two sides have different refs and therefore symmetric cache behaviour.

| Run | Side | Expected cache state |
|---|---|---|
| 1 | before | cold |
| 2 | after | cold |
| 3 | before | warm |
| 4 | after | warm |
| 5 | before | warm |
| 6 | after | warm |
| 7 | before | warm |
| 8 | after | warm |

This yields **n = 3 warm** per side for the median and its dispersion, and **n = 1 cold** per side.

**The cold-cache pair is reported as a single observation per side, not as a distribution.** A cold-cache run rebuilds the whole dependency graph from scratch, so it is the closest analogue to a fresh contributor's first CI run — it is genuinely interesting, and it is genuinely a sample of one. It is reported with that label. If the run budget is cut and only three runs per side are obtainable, the cold sample stays n = 1 and the warm sample becomes n = 2, and the median over n = 2 is reported as such, with the weakness stated rather than papered over.

**Cache state is read from the run log, not assumed.** If the rust-cache step's reported behaviour disagrees with the expectation above, the run is labelled with the state that was actually observed.

### 5.1 The cold-cache expectation was wrong — recorded, and it does not change the comparison

The **first** before-side run (`39782552982`) restored a cache and was
therefore **warm**, not cold as §5 predicted. The reason is rust-cache's
**shared key**: the cache is saved under a key that encodes the lockfile but
not the ref, and restored under a matching prefix, so a cache saved by *any*
branch with the same lockfile is restored by any other.

`Cargo.lock` is **byte-identical** between the two sides (verified with
`git diff 2c1953347 8e3554ef4 -- Cargo.lock`), so both sides restore the
same shared cache and **both sides are warm**. That is symmetric, which is
what the comparison needs, and it is also the measurement that isolates what
M004 changed: with the dependency graph already compiled, the build phase is
almost entirely the root package's **link** work — 288 executables on the
before side against 21 on the after side.

The consequence for the plan is concrete: **a cold-cache sample is not
obtainable without deleting a cache entry that other branches share**, which
is an owner action this milestone does not take unilaterally. The
preregistered warm-cache comparison in §4.2 and threshold T2 are therefore
the ones that apply, and the cold-cache rows of §5 are reported as *not
obtained*, with this reason.

---

## 6. Confounders, and how each is handled

### 6.1 Cache state — the dominant confounder

A cold-cache run and a warm-cache run measure different things. A warm run skips compiling the entire third-party dependency graph; a cold run pays for it. Quoting a wall-clock without its cache state is not a measurement.

**Handled by:** labelling every run (row 10), never pooling across states (§4.2), and reporting the cold and warm comparisons separately.

### 6.2 Runner image

`runs-on: ubuntu-latest` is a moving target — the label resolves to whatever image GitHub currently offers, and it can change between runs. A different image is a different machine.

**Handled by:** recording the image per run (row 9) and reporting it beside every number. If the image differs between the two sides' runs, that is stated as a confounder on the wall-clock comparison; it does not affect the countable claim.

### 6.3 The `mold` linker

Both sides use `rui314/setup-mold@v1` — a drop-in GNU-ld replacement that materially changes link time. The link cost is the mechanism M004 targets, so the linker must be the same on both sides.

**Handled by:** recording mold's presence per run (row 12). If a run lacks mold, that run's link timing is not comparable and is reported separately, not folded into the median.

### 6.4 The `ci` nextest profile's 4-slot bound and run-alone binaries

`.config/nextest.toml`'s `ci` profile runs 4 tests concurrently (`test-threads = 4`), with `slow-timeout = { period = "60s", terminate-after = 2 }`, and six heavy binaries (`eggwork_remote_execution_live`, `interactive_process_attach_resume`, `interactive_process_sessions`, `interactive_terminal_tui`, `scheduler_cancellation`, `causal_observe_replay`) running **alone** — exclusive, not excluded. The profile is byte-identical on both SHAs.

**Handled by:** the execution phase is compared under an identical profile, and the run-alone binaries are the same set on both sides. The profile's 4-slot bound is **not** raised for this measurement (the roadmap's §8 forbids it without a re-validated green run).

### 6.5 The live-Eggwork detection step — the two sides run the identical suite

The `Workspace tests` step is conditional on `live_required`, computed by `scripts/detect-live-eggwork-changes.sh`. That script **fails open** for any event that is not `pull_request`, and `workflow_dispatch` is not `pull_request`. Therefore `live_required = true` on **both** sides: both runs prebuild the live-Eggwork fixtures and execute the **complete** suite, with no `-E` filter.

**Handled by:** this is a fact, not a risk — both sides execute the same test set. It is recorded here because if it were ever false on one side and true on the other, the test counts would not be comparable and invariant I1 would be untestable.

### 6.6 The 13 additional guard steps on the after side

Recorded in §2.2. They cost seconds, not minutes, and no step is missing from the before side.

**Handled by:** the claim is measured in the `Workspace tests` step, whose definition is identical on both sides.

### 6.7 Machine contention on the hosted runner

The hosted runner is shared infrastructure. GitHub-hosted runners are separate VMs, so the two sides do not share a machine, but dispatching many runs at once loads the fleet.

**Handled by:** serial dispatch (§4.3).

### 6.8 Local machine contention (for the work-package C local measurement)

`agent::asset_refresh::tests::same_scope_requests_coalesce_to_one_publication` times out at 300 s when other cargo builds share the machine and passes in **0.031 s** alone. This is a documented artefact of the M004 and M006 closure records.

**Handled by:** a local timed-out run under contention is an artefact and is re-run alone, never reported as a failure.

---

## 7. Acceptance thresholds — fixed before the runs

### T1 — Primary claim (countable) — **must** pass

> The candidate's linked root test executables must be **≤ 43**, i.e. at least an **85 % reduction** from the baseline's 288.

Measured locally: **21** (a 92.7 % reduction). The hosted cross-check (§3.3) must agree.

**A failure here is a stop condition, not a number to report** (plan §14): it would mean the consolidation did not happen, which is a defect to investigate.

### T2 — Secondary claim (build phase) — the projection's test

> The **median warm-cache build phase** of the `Workspace tests` step on the candidate must be **≤ 60 %** of the baseline's median warm-cache build phase — i.e. a **≥ 40 % reduction**.

M004's closure record states a **~41 %** workspace-compile saving, projected from the baseline's 13.9 s/target median applied to the new target count. **40 %** is the threshold, chosen to test that projection without being a restatement of it.

**A failure here is reportable if the reason can be established** (invariant I5: "a measurement that is not reproduced is reported as not reproduced, with the reason"). **A failure with no established reason is a stop condition** (plan §14).

### T3 — Secondary claim (execution phase) — the no-regression check

> The **median warm-cache execution phase** on the candidate must be **≥ 90 %** of the baseline's.

The workstream consolidated test *files* into families; it did not remove test functions. The execution phase should therefore be roughly unchanged, or slightly better (fewer process spawns). A materially shorter execution phase would mean **tests stopped running**, which invariant I1 forbids — so this threshold is also a coverage check.

### T4 — Invariant I1 — the coverage check

> **Corrected at the amendment (§11.2).** The original formulation — "the
> after-run's test count is at least the before-run's, and the after-run
> executes a superset of the before-run's test functions" — is
> **unsatisfiable** for this workstream, and was wrong in this
> preregistration rather than in the product. M007's recorded purpose is to
> remove **85 duplicate test functions** and to stop the shared-helper
> fan-out (**40 executions of 5 assertions** → 5). A duplicate is the same
> assertion asserted twice; removing one copy removes a *function* without
> removing *coverage*. A superset-of-functions rule would forbid M007's
> entire deliverable, and the before-side's 15,841 executed tests are
> therefore expected to fall to fewer, not to grow.
>
> The corrected rule is the roadmap's §11 third bullet, restated as a
> checkable predicate, and it is **stronger** where it matters — every
> removed function is tied to a survivor that the after-run actually
> executes:
>
> - **I1a — no unexplained loss.** Every test function the before-run
>   executed that is **not** a recorded removal in
>   `docs/test-duplication.toml` must still be executed by the after-run.
> - **I1b — every removal is covered.** For every test function the
>   before-run executed that **is** a recorded removal, the removal's
>   recorded **survivor** must be executed by the after-run.
> - **I1c — no mass loss.** The number of before-only functions must not
>   exceed the number of recorded removals (85).
>
> The comparison is made on the **test function path** nextest reports, not
> on the binary id, because M004 moved functions between binaries: the
> binary id changed and the function did not. The decision table's 85
> removals and their survivors are read from
> `docs/test-duplication.toml`, the table M007 shipped.

### T5 — Job total — **no threshold**

Reported for context (§2.2). No threshold is set, because the after-side job carries 13 additional steps.

### T6 — The cold-cache pair — **no threshold**

Reported as a single observation per side (§5). No threshold is set on n = 1.

---

## 8. Hygiene

- The temporary refs `ci-baseline/2c1953347` and `m005/candidate-4f89b4b05` are **deleted from the remote after their runs complete**. Leaving them would leave a stale copy of the pre-change tree on the remote that a later `workflow_dispatch` could silently measure (§8 of the milestone plan).
- A run dispatched on the **before** side is never re-dispatched until it has been examined. A failing after-run is a **regression** and is investigated as one (plan §8) — it is not discarded and re-dispatched until it passes.
- A run cancelled by the job's own `concurrency` group contributes no signal and is recorded as cancelled.
- Every number in the measurement record carries the command that produced it, the SHA, and the date (invariant I4).

---

## 9. What this preregistration does not do

- It does not decide roadmap §10 **D1** — the 149 feature-gated test functions CI never executes. That remains an open owner coverage decision.
- It does not decide the **rebase**; that is deferred past the measurement (roadmap **D9**) and is carried into the closure record as an open item.
- It does not change the CI job's definition, the test set, or the feature set. Invariants I1 and I3 forbid it.
- It does not reopen the predecessor roadmap's closed milestones.

---

## 10. Records this preregistration produces

| File | Content |
|---|---|
| `plans/validation/ci-test-throughput-recovery/measurement-record.md` | the results, each with its source, plus an explicit **"not reproduced"** section |
| `plans/closure/ci-test-throughput-recovery/005-status.md` | the workstream's closure, with a recommendation |

---

## 11. Amendment — 2026-10-22T13:50Z

**This amendment was made before any after-side run completed.** The
after-side's first run failed at a guard step and never reached the test
suite, so no after-side result existed when the method was amended. The
before-side's first run had completed, and everything below is derived from
it or from the repository, not from an after-side outcome.

### 11.1 The candidate SHA moved: `4f89b4b05` → `8e3554ef4`

The after-side's first run, `39786287698`, **failed** at the **Scoped
selection self-test** step, before it reached the test suite:

```
FAIL: the real selection failed: could not compute a merge base against
'origin/main': git merge-base origin/main HEAD failed: fatal: Not a valid
object name origin/main.
```

`actions/checkout@v4` fetches depth 1, so the runner's clone has no
`origin/main`. The self-test called `select(DEFAULT_BASE, ...)`, and
`select()` computes `git merge-base <base> HEAD` before replacing the
computed diff with the explicit file list it was given. The failure was a
property of the **checkout**, not of the selection algorithm — but it meant
the committed CI job could never go green on the hosted runner, so the
candidate tree could not be measured at all.

The repair is commit `8e3554ef4`, one file, `scripts/scoped_selection.py`:
the self-test now anchors its merge base on `origin/main` when that ref
exists and on `HEAD` when it does not, and it additionally proves the
changed-file computation against a temporary git repository it creates, so
the path that needs a merge base has coverage in a shallow checkout too.

Proven in a depth-1 clone with no `origin/main`: the unfixed script fails
with exactly the hosted error, the fixed script passes. `scripts/verify.sh
quick` passes on the repaired tree (59 steps, exit 0).

**What the repair does not touch:** no product source, no test, no build
configuration, no CI job definition, and no measured quantity. The linked
test-executable count, the build phase and the execution phase are all
unaffected by a Python guard script's exit status.

The pinned ref was moved with it: `m005/candidate-4f89b4b05` was **deleted**
and `m005/candidate-8e3554ef4` created. Leaving the old ref would have left
a stale copy of a tree whose CI job is red, which is the hazard §8 of the
milestone plan warns about.

### 11.2 The I1 threshold was wrong and is corrected

Corrected in **T4** above. The original formulation is unsatisfiable for
this workstream because M007's recorded purpose is to remove duplicate test
functions and duplicate executions; a superset-of-functions rule forbids
that deliverable. The corrected rule is the roadmap's §11 third bullet
restated as a predicate — no unexplained loss, every removal covered by a
recorded survivor, no mass loss — and it is strictly stronger where it
matters, because it ties each removed function to an assertion the
after-run actually executes.

**This is a correction to the preregistration, not a relaxation of a
threshold to fit a result.** No after-side result existed when it was made.

### 11.3 The hosted cross-check's expected values were wrong

Corrected in **§3.3**: **308** (before) and **50** (after), not 314 and 47.
nextest's binary count excludes the feature-gated targets the job cannot
build and includes the lib and bin test harnesses that `cargo metadata`'s
`kind == ["test"]` filter does not. The arithmetic reproduces the
before-side's **observed** 308 exactly.

### 11.4 The cold-cache expectation was wrong

Recorded in **§5.1**. Both sides are warm, because rust-cache's shared key
restores across refs with the same lockfile and `Cargo.lock` is identical
on both sides. The preregistered warm-cache comparison applies; a
cold-cache sample would require deleting a cache entry other branches share,
which this milestone does not do unilaterally.

### 11.5 The before-side's first run, observed

Recorded here so the amendment is anchored to a fact rather than to a
hypothesis:

| | run `39782552982` |
|---|---|
| conclusion | success |
| head SHA | `2c1953347` |
| runner image | `ubuntu-24.04` |
| cache state | **warm (hit)** |
| nextest binaries | **308** |
| tests run | 15,841 (all passed, 8 skipped) |
| build phase | 615.0 s (`Finished \`test\` profile ... in 10m 15s`) |
| execution phase | 436.888 s |
