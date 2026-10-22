# M005 Measurement Record — Hosted Verification and Measurement Closure

Workstream: CI and test throughput recovery
Milestone plan: `plans/implementation/ci-test-throughput-recovery/005-hosted-verification-and-measurement-closure.md`
Preregistration: `plans/validation/ci-test-throughput-recovery/measurement-preregistration.md`

Record started: **2026-10-22T14:20Z**

This record is the result side of the preregistration. Every number below
names the command that produced it, the SHA it was produced on, and the
date. Numbers that were **not** obtained are listed in §7, with the
reason, rather than being omitted.

---

## 1. The two trees

| Side | SHA | Ref | Note |
|---|---|---|---|
| Before | `2c1953347896848c2f52c2dfbd9f04029d1c2cc2` | `ci-baseline/2c1953347` | The clean pre-change tree, already an ancestor of `origin/main`. |
| After | `3dfeb6e2f42faaff826991cec5d43c574554f66d` | `m005/candidate-3dfeb6e2f` | The change set plus the two defect repairs the hosted CI revealed. Preregistration §11.1 and §11.6 record both. |

---

## 2. The primary claim — the countable one

### 2.1 The local measurement, on both SHAs

Command (preregistration §3.1), run 2026-10-22 in a clean worktree per
side:

```bash
git worktree add /tmp/m005_before 2c1953347
git worktree add /tmp/m005_after  3dfeb6e2f
(cd <worktree> && cargo metadata --no-deps --format-version 1 | \
  python3 -c "import json,sys; m=json.load(sys.stdin); \
    print(len([t for p in m['packages'] if p['name']=='codegg' \
               for t in p['targets'] if 'test' in t['kind']]))")
```

| | before | after | change |
|---|---|---|---|
| `codegg` root test targets | **288** | **21** | −267 (**−92.7 %**) |
| Workspace test targets | **314** | **47** | −267 (**−85.0 %**) |

### 2.2 The same count from cargo's own build units

`cargo test --workspace --locked --no-run --timings` (preregistration §11
work package C) counts the units cargo actually built, which is a second,
independent route to the same number:

| | before | after |
|---|---|---|
| Root integration-test executables built | **275** | **17** |
| Linked test executables, whole workspace | **305** | **47** |
| Total compilation units | 988 | 730 |

The 275 and 17 are the counts **net of the feature-gated targets the CI
job cannot build** (13 and 4 respectively) — preregistration §3.3 gives
the arithmetic that reconciles cargo's units, `cargo metadata`'s targets
and nextest's binary count into one another.

---

## 3. The secondary claim — the per-family link cost (local)

### 3.1 The method

```bash
# A FRESH target dir: a warm tree reports the cost of checking, not of linking.
CARGO_TARGET_DIR="$(mktemp -d)" cargo test --workspace --locked --no-run --timings
# Then parse the emitted block — NOT `--timings=json`, which is unstable on cargo 1.89.
python3 - <<'PY'
import re
h = open('<target>/cargo-timings/cargo-timing.html', errors='replace').read()
units = json.loads(re.search(r'const UNIT_DATA = (\[.*?\]);', h, re.S).group(1))
PY
```

**The machine**, because unit-seconds are only comparable within a machine:

| | |
|---|---|
| platform | macOS, aarch64-apple-darwin |
| cores / RAM | 14 / 24 GiB |
| cargo / rustc | 1.89.0 / 1.89.0 |
| linker | Apple clang 21.0.0 (`ld`) — **mold is not installed locally**, while the hosted runner uses mold |
| load before the build | baseline `{5.88, 10.80, 15.31}`, candidate run 1 `{9.10, 9.93, 9.73}`, candidate run 2 `{9.71, 16.12, 16.47}` |

The candidate was measured **twice**, independently, so the measurement's
own noise is known rather than assumed.

### 3.2 The measurement

| bucket | before (1 run) | after run 1 | after run 2 | after median |
|---|---:|---:|---:|---:|
| **all compilation units** | **988** | 730 | 730 | — |
| **aggregate unit-seconds** | **4,300.8** | 2,610.5 | 2,627.3 | **2,618.9** |
| root integration-test targets | 275 units / 2,123.6 s | 17 units / 445.3 s | 17 units / 400.6 s | **17 units / 423.0 s** |
| root lib `--test` harness | 1 / 443.9 | 1 / 439.2 | 1 / 628.1 | — |
| root lib (dev-profile rlib) | 1 / 151.3 | 1 / 310.8 | 1 / 316.7 | — |
| other workspace test targets | 30 / 156.8 | 30 / 373.8 | 30 / 326.1 | — |
| binaries | 4 / 82.7 | 4 / 65.5 | 4 / 64.6 | — |
| dependency crates | 565 / 606.4 | 565 / 832.6 | 565 / 764.1 | — |
| build scripts | 56 / 11.9 | 56 / 20.9 | 56 / 17.3 | — |
| build-script **executions** | 56 / 724.1 | 56 / 122.5 | 56 / 109.8 | — |

### 3.3 The saving, against M004's projection

| | before | after (median) | saving |
|---|---:|---:|---:|
| **total workspace compile work** | 4,300.8 | 2,618.9 | **−39.1 %** |
| **the root package's integration-test link work** | 2,123.6 | 423.0 | **−80.1 %** |
| linked test executables | 305 | 47 | **−84.6 %** |

**M004's closure record states a "~41 % of workspace compile work" saving.
The measured same-session saving is 39.1 % of workspace compile work — the
projection reproduces, within 1.9 percentage points.** M004's figure was
explicitly a projection from the baseline's per-target median applied to
the new target count; this is the measurement.

### 3.4 What the measurement does **not** show

- **The aggregate carries a contention-mediated component.** The two sides'
  *unrelated* buckets disagree in **opposite** directions: the baseline's
  56 build-script **executions** cost 724.1 s against the candidate's
  ~115 s, while the candidate's 565 dependency crates cost ~800 s against
  the baseline's 606.4 s. These are the same units doing the same work.
  The difference is contention: the baseline's C-compiling build scripts
  ran while 288 link operations were saturating the machine. The aggregate
  is therefore reported, because it is the figure M004's projection is
  about, but its precision is limited to roughly ±15 %. The **bucket-level**
  figure is the reliable one, and it is the one that isolates what M004
  changed.
- **The §4.1 baseline table does not reproduce on this machine.** §4.1
  records the same root integration-test targets at 4,040.1 s and the same
  total at 9,155.6 s; the same work costs 2,123.6 s and 4,300.8 s here.
  §4.1 was taken at a different revision with a different command
  (`cargo build --workspace --all-targets --timings`) and under three
  concurrently-compiling worktrees. The projection's *inputs* are
  therefore not portable, which is itself worth recording: the
  same-session measurement is the one a comparison should be made against.
- **The local link cost is not the hosted link cost.** mold is not
  installed locally, and mold is what the hosted runner links with.
- **The measurement is conservative for the claim.** The `pub mod` repair
  (preregistration §11.6) makes the family's items reachable, so dead code
  is no longer eliminated and the candidate's link work is, if anything,
  marginally **higher** than measured.
- **The noise is known**: the candidate side repeated within **±0.6 %** on
  the aggregate and **±5.5 %** on the 17-unit root bucket. The root bucket
  is noisier per unit because it has 17 units instead of 275.

### 3.5 The per-family link cost

The candidate's 47 linked test executables, by unit-seconds. These are the
units that the baseline built as 305 separate executables.

| linked executable | unit-secs | median unit-secs |
|---|---:|---:|
| `codegg-core` lib (test) | 82.0 | 82.01 |
| `codegg-protocol` lib (test) | 70.1 | 70.10 |
| `egglsp` lib (test) | 64.7 | 64.66 |
| `codegg` family `core_daemon` | 51.7 | 51.71 |
| `codegg` family `agent_loop` | 48.6 | 48.61 |
| `codegg` family `scheduler_jobs` | 47.1 | 47.11 |
| `codegg-providers` lib (test) | 46.7 | 46.67 |
| `codegg` family `tool_programs` | 43.0 | 43.03 |
| `codegg` family `tui_render` | 43.0 | 43.00 |
| `codegg` family `interactive_process_attach_resume` | 28.7 | 28.72 |
| `codegg` family `security_permission` | 28.3 | 28.34 |
| `codegg` family `session_family` | 28.3 | 28.29 |
| `codegg` family `config_provider` | 24.7 | 24.68 |
| `codegg` family `command_git` | 24.6 | 24.64 |
| `codegg-config` lib (test) | 20.1 | 20.05 |
| `codegg` family `ide_protocols` | 15.2 | 15.15 |
| `codegg` family `interactive_terminal_tui` | 13.5 | 13.46 |
| `codegg` family `causal_observe_replay` | 13.2 | 13.21 |
| `codegg` family `projection_replay` | 12.9 | 12.86 |
| `codegg` family `scheduler_cancellation` | 12.8 | 12.85 |
| `codegg` family `interactive_process_sessions` | 9.4 | 9.36 |
| `egggit` lib (test) | 8.2 | 8.21 |
| `codegg-git` lib (test) | 6.5 | 6.55 |
| `codegg-core` `user_prompt_queue_delivery` | 5.7 | 5.68 |
| `codegg-core` `project_catalog` | 5.6 | 5.57 |
| `codegg-core` `user_prompt_queue` | 5.3 | 5.33 |
| `codegg-core` `context` | 5.0 | 5.04 |
| `codegg-core` `human_shell_promotion` | 4.8 | 4.77 |
| `codegg-client` lib (test) | 4.7 | 4.70 |
| `codegg-core` `work_plan_projection_arbiter` | 4.7 | 4.73 |
| `codegg-core` `user_prompt_queue_mutation` | 4.5 | 4.53 |
| `codegg-core` `work_plan_foundation` | 4.3 | 4.32 |
| `codegg-core` `cached_models_composite_key` | 4.1 | 4.14 |
| `eggsentry` lib (test) | 4.0 | 4.00 |
| `codegg-client` `session_projection_driver` | 3.9 | 3.93 |
| `codegg-core` `continuation_checkpoint` | 3.5 | 3.55 |
| `codegg-core` `session_title_slug` | 3.4 | 3.43 |
| `codegg-client` `gui_client` | 3.0 | 2.96 |
| `codegg-core` `decision_runtime_fixtures` | 2.2 | 2.22 |
| `codegg-core` `identity` | 1.8 | 1.76 |
| `egglsp` `lsp_cache_benchmark` | 1.8 | 1.82 |
| `codegg-document` `transactions` | 1.0 | 0.98 |
| `eggcontext` lib (test) | 0.9 | 0.93 |
| `egglsp` `phase6_regression` | 0.6 | 0.60 |
| `codegg` family `eggwork_remote_execution_live` | 0.3 | 0.27 |
| `codegg-document` lib (test) | 0.3 | 0.33 |
| `egglsp` `empty_diagnostics_readiness` | 0.2 | 0.18 |

A family's cost is dominated by the size of the code it links, not by the
number of test files it holds: `core_daemon` at 51.7 s carries the daemon
and its server wiring, while `command_git` at 24.6 s carries the Git
policy surface. The baseline's median was **13.9 s per *file*** across 271
separate executables; the candidate's is **24.7 s per *family*** across 17
— a family costs more than a file because it links more code into one
binary, and 17 of them replace 275.

---

## 4. The hosted measurement

### 4.0 Invariant I3 — what did and did not change in the measured job

The two sides are different trees, so the CI job definition **did** change
between them. That is not a violation of I3 — I3 forbids changing the job
*to make the measurement* — but it has to be stated exactly, because the
job total (T5) is the one quantity it touches.

Verified by diffing the two SHAs directly:

| | before (`2c1953347`) | after (`3dfeb6e2f`) | comparable? |
|---|---|---|---|
| The `Workspace tests` step, byte for byte | `cargo nextest run --workspace --locked --profile ci` | identical | **yes** |
| The `ci` nextest profile's settings | `test-threads = 4`, `slow-timeout = { period = "60s", terminate-after = 2 }`, six whole-binary exclusions | identical | **yes** |
| `Cargo.lock` | — | byte-identical between the sides | **yes** |
| Steps in the `verify` job | 44 | 57 | **no** — the job total only |

The **13 added steps are all guard and self-test steps**: cargo-profile
alignment (×2), test-ownership (×2), scoped selection (×1), test-target
references (×2), test-assertion quality (×2), test-duplication (×2), and
Tool Program parse-only (×2). They run before the test step and touch
neither the build phase nor the execution phase.

`.config/nextest.toml` also gained a **new `[profile.subset]` profile**
(M002's bounded feature-enabled pass). The CI job does **not** use it —
`--profile subset` appears nowhere in `.github/workflows/ci.yml`; the job
uses `--profile ci` on both sides — so it cannot affect either measured
phase.

**The build phase and the execution phase are therefore measured under
identical commands, an identical nextest profile, and an identical
lockfile.** The job total is reported for context only, as T5 specifies.

### 4.1 Every run, with its extracted quantities

| label | run id | head SHA | conclusion | cache | image | binaries | tests | build phase | execution phase | `Workspace tests` step | job total |
|---|---|---|---|---|---|---:|---:|---:|---:|---:|---:|
| `before-1` | [39782552982](https://github.com/dbowm91/codegg/actions/runs/39782552982) | `2c1953347` | success | warm (hit) | ubuntu-24.04 | 308 | 15841 | 615.000 s | 436.888 s | 1060.000 s | 1495.000 s |
| `before-2` | [39786543784](https://github.com/dbowm91/codegg/actions/runs/39786543784) | `2c1953347` | success | warm (hit) | ubuntu-24.04 | 308 | 15841 | 744.000 s | 527.888 s | 1282.000 s | 1775.000 s |
| `before-3` | [39792559544](https://github.com/dbowm91/codegg/actions/runs/39792559544) | `2c1953347` | success | warm (hit) | ubuntu-24.04 | 308 | 15841 | 720.000 s | 505.452 s | 1232.000 s | 1705.000 s |
| `before-4` | [39797262082](https://github.com/dbowm91/codegg/actions/runs/39797262082) | `2c1953347` | success | warm (hit) | ubuntu-24.04 | 308 | 15841 | 646.000 s | 470.185 s | 1124.000 s | 1542.000 s |
| `after-1` | [39801183836](https://github.com/dbowm91/codegg/actions/runs/39801183836) | `3dfeb6e2f` | success | warm (hit) | ubuntu-24.04 | 50 | 15532 | 308.000 s | 360.489 s | 672.000 s | 1125.000 s |
| `after-2` | [39804196133](https://github.com/dbowm91/codegg/actions/runs/39804196133) | `3dfeb6e2f` | failure | warm (hit) | ubuntu-24.04 | 50 | None | 337.000 s | — | 718.000 s | 1194.000 s |
| `after-3` | [39807350392](https://github.com/dbowm91/codegg/actions/runs/39807350392) | `3dfeb6e2f` | failure | warm (hit) | ubuntu-24.04 | 50 | None | 400.000 s | — | 851.000 s | 1404.000 s |
| `after-4` | [39811032039](https://github.com/dbowm91/codegg/actions/runs/39811032039) | `3dfeb6e2f` | success | warm (hit) | ubuntu-24.04 | 50 | 15532 | 433.000 s | 527.838 s | 964.000 s | 1594.000 s |
| `after-topup-1` | [39816301661](https://github.com/dbowm91/codegg/actions/runs/39816301661) | `3dfeb6e2f` | success | warm (hit) | ubuntu-24.04 | 50 | 15532 | 359.000 s | 377.692 s | 740.000 s | 1215.000 s |

Runs whose conclusion is not `success` are listed above but **contribute no measurement**: they are pooled into no median and no dispersion. `after-2` (39804196133, failure) — see the record's §4.5, `after-3` (39807350392, failure) — see the record's §4.5.

### 4.2 The statistic — the median within the cache state, with the raw values

**before-side** (n = 4, all warm-cache):

- build phase raw: [615.0, 646.0, 720.0, 744.0] → **median 683.000 s**
- execution phase raw: [436.888, 470.185, 505.452, 527.888] → **median 487.818 s**

**after-side** (n = 3, all warm-cache):

- build phase raw: [308.0, 359.0, 433.0] → **median 359.000 s**
- execution phase raw: [360.489, 377.692, 527.838] → **median 377.692 s**

The after side reaches n = 3 successful runs because **two of the first
three after-side runs failed** on the documented pre-existing flake
(§4.5). The plan asks for at least three runs per side; the two
failures are not samples, so additional successful runs were dispatched
rather than the failures being counted. The after-side's third run,
`after-topup-1`, was dispatched for exactly that reason, and the
dispatches are listed in §4.1 with their run ids.

**The after side's dispersion is wider than the before side's, and it
matters.** Its execution phase spans **360.489 s to 527.838 s** — a
167 s spread — against the before side's 436.888 s to 527.888 s. The
after side's `after-4` is the slowest run on either side of the
execution phase, slower than three of the four before-side runs. This
is the dispersion that makes the wall-clock unusable as a precise
effect size (§4.4).

### 4.3 The comparison, against the preregistered thresholds

| | before median | after median | ratio | threshold | verdict |
|---|---:|---:|---:|---|---|
| build phase (T2) | 683.000 s | 359.000 s | **52.6 %** | ≤ 60 % | **PASS** |
| execution phase (T3) | 487.818 s | 377.692 s | **77.4 %** | ≥ 90 % | **FAIL** |

### 4.4 T2 and T3 — the two thresholds, and what the wall-clock can and cannot carry

**T2 — the build phase — passes, and it passes by a wide margin.** The
after side's median warm-cache build phase is well inside the ≤ 60 %
threshold (the table above).

**T3 — the execution phase — does not hold as written, and the reason is
established.** T3 says the after side's median execution phase must be
**≥ 90 %** of the before side's, on the reasoning that "the workstream
consolidated test *files* into families; it did not remove test functions",
so the execution phase "should therefore be roughly unchanged, or slightly
better (fewer process spawns)".

Two things make the measured execution phase shorter, and they are of very
different kinds.

**(a) 313 fewer test executions ran.** This is real, intended, and exactly
reconciled — see §6.6: 85 to M007's recorded duplicate removals, 3 to
M006's recorded vacuous-body removals, 225 to the shared-helper fan-out,
**0 elsewhere**.

**(b) The remaining tests ran faster on the after side — but not by a
well-determined factor.** This is *not* a property of the change, and the
measurement proves it. One test is the instrument:

| run | `response_budget_trims_trailing_nodes_with_a_reason` |
|---|---:|
| before-1 | 61.1 s |
| before-2 | 84.3 s |
| before-3 | 82.6 s |
| before-4 | 81.8 s |
| **after-1** | **38.7 s** |
| **after-4** | **82.1 s** |
| **after-topup-1** | **43.9 s** |

`src/core/daemon_documents.rs` is **byte-identical** between the two
sides, and so is its production dependency graph, the `test` profile, and
`Cargo.lock`. The test does the same work in the same process on both
sides. It is therefore a **host-speed measurement**, not a code
measurement: GitHub's hosted runners are shared, uncontrolled 4-vCPU VMs,
and the before side's own four runs span 61.1–84.3 s on this one test
(1.38×).

**The after side's own three runs are the decisive part of this table.**
They span **38.7–82.1 s — a 2.13× spread on byte-identical code** — and
the after side's slowest run is indistinguishable from the before side's
slowest. The after side's *internal* dispersion therefore **exceeds the
whole before-vs-after difference the wall-clock reports** (the execution
phase's ratio is 1.29×). The measurement's error bar, taken on identical
code, is larger than the effect.

This also explains the after side's execution-phase dispersion directly:
`after-4` is the slowest execution phase on either side (527.838 s)
because its host ran this one test as slowly as the before side's hosts
did, while `after-1` is among the fastest (360.489 s) because its host
ran it at 38.7 s.

The effect is uniform across the suite — the same factor appears on tests
of 5 s and on tests of 84 s — and the before side's own per-run dispersion
on the aggregate is **83.3 / 106.0 / 100.3 / 91.7 ms per execution**
(1.27× spread on an identical tree).

**What follows for the milestone.**

1. The **countable** claim carries the milestone, exactly as the
   preregistration said it would: the linked root test-executable count
   fell from **288 to 21**, and nextest's hosted binary count fell from
   **308 to 50**.
2. The hosted **wall-clock is reported with its dispersion and its cache
   state and is never presented as a precise effect size**. A ratio
   computed across two sets of runs on two different hosts is not a
   measurement of the change; the ~2× host-speed difference is the same
   order as the effect being measured, so the wall-clock cannot separate
   the two.
3. **T3's purpose is discharged by I1, not by T3.** T3 was a proxy for
   "did tests stop running?" — it says so in its own text. That question
   is answered directly and decisively by the I1 check (§6): **zero**
   unexplained losses, every removal recorded with its survivor, and every
   one of the 313 lost executions attributed to a recorded change. The
   execution phase fell for a reason that has nothing to do with coverage.
4. This is a **reportable deviation with an established reason** (invariant
   I5), not a stop condition (plan §14): the reason *is* established, and
   it is a property of the measurement environment, not of the product.

**What would make the wall-clock usable.** A before/after pair run on the
*same* host — the only way to remove the host-speed term — or a runner
class with a fixed, documented CPU allocation. Neither is available from a
`workflow_dispatch` on GitHub-hosted runners, and this milestone does not
change the CI job's runner definition to get one (invariant I3). The
before-side's four runs show what a single tree's wall-clock looks like
across four hosts: 436.888 / 527.888 / 505.452 / 470.185 s for the
execution phase — a **1.21× spread on the same code**, which is the
honest error bar on any hosted wall-clock from this fleet.

### 4.5 The two failing after-side runs — one documented pre-existing flake

`after-2` (run `39804196133`) and `after-3` (run `39807350392`) **failed**,
on **the same single test**:

```
thread 'goal::checkpoint::tests::test_read_checkpoint_tail_returns_latest_updates'
panicked at crates/codegg-core/src/goal/checkpoint.rs:323:9:
assertion failed: tail.contains("Phase 3 newest")
```

The plan's §14 requires a failing after-run to be *investigated as a
regression* rather than re-dispatched until it passes, so here is the
investigation.

**The failure.** One test, in the `codegg-core` lib harness: the
checkpoint journal's tail read did not contain the phase the test had just
appended.

**It is a pre-existing CI-only flake, and the repository already records
it.** Three closure records from earlier workstreams name **this test,
this file, this line and this assertion**:

- `plans/closure/managed-agent-scratch-native-qualification-corrective/001-status.md`
  — *"failed at 11720/14813 on `codegg-core
  goal::checkpoint::tests::test_read_checkpoint_tail_returns_latest_updates`
  (`crates/codegg-core/src/goal/checkpoint.rs:323`, `assertion failed:
  tail.contains("Phase 3 newest")`) … The same test passes 3/3 locally on
  the development host, so it is a **CI-only flake under runner load**. It
  did not recur in the qualifying run … The flake is recorded here rather
  than hidden"* — and, in the same record's risk table: *"**Do not weaken
  the flaky test to make a lane green**; report it in its own line if it
  recurs."*
- `plans/closure/ci-test-throughput-optimization-post-closure-corrective/001-status.md`
  — *"Superseded v1 `36772073029` (13m25s failure on `codegg-core
  goal::checkpoint::test_read_checkpoint_tail_returns_latest_updates`,
  unrelated to C001 change; passed in all other runs) **is not evidence**."*
- `plans/closure/provider-connect-qualification-wire-dispatch-post-closure-corrective/001-status.md`
  — names the same test.

**Why it cannot be a regression from this workstream.**

1. The test's file, `crates/codegg-core/src/goal/checkpoint.rs`, is
   **byte-identical** between the two sides (`git diff 2c1953347 3dfeb6e2f`
   is empty for it), and so is the production code it exercises.
2. The test **passed** in all four before-side runs and in `after-1`.
3. **Reproduced locally, fresh, for this milestone:** the test was run
   **40 times** on the development host on 2026-10-22 and
   **passed 40/40** ("1 passed; 0 failed; 883 filtered out" every time),
   on a machine that was simultaneously under load from the coverage
   measurement's instrumented build. That is the repository's own
   recorded characterisation — *"The same test passes 3/3 locally on the
   development host, so it is a CI-only flake under runner load"* —
   reproduced at 40/40.
4. The repository's own precedent for this exact failure is the
   disposition used here: **record it in its own line, do not count it as
   evidence either way, and do not weaken the test.**

**An observation that is recorded rather than smoothed over.** The hit
rate is **not** symmetric between the sides: the flake fired in **0 of 4**
before-side runs and in **2 of 3** after-side runs at the time of
writing. The mechanism is **undiagnosed** — it is undiagnosed in the
three prior records as well — and the repository attributes it to CI-only
flake behaviour under runner load. The after-side's runs were dispatched
in a later window of the hosted fleet's day than the before side's, and
the preregistration itself warns that "the hosted fleet's load is not
constant across that window"; but the after-side's runs were *faster* per
test (§4.4), so a heavier window does **not** explain the asymmetry, and
this record does not claim that it does. **The asymmetry is stated here as
an open observation, not explained away.** What can be said is that the
test's code, the code it exercises, the profile and the lockfile are
identical on both sides, and the failure is a documented pre-existing
flake.

**Disposition.** `after-2` and `after-3` contribute **no measurement** —
they are pooled into no median and into no dispersion — and they are
**not** discarded: both runs, their logs, their failures and their cause
are recorded in the failure ledger. Additional after-side runs were
dispatched so that the after-side's **successful** sample reaches the
plan's minimum of three; the successful runs are `after-1` and the
top-up runs, and the median is reported over those only.

The flaky test is **not** touched: this milestone changes no test, and the
repository's recorded guidance for this flake is explicitly *"do not
weaken the flaky test"*.

### 4.6 The two candidate defects, in brief

The after-side's **first two** runs also failed, and both were real
defects in the change set rather than flakes — they are described in full
in the closure record §9 and in the preregistration's §11.1 and §11.6:

| run | failed at | cause | repair |
|---|---|---|---|
| `39786287698` | the scoped-selection self-test | the runner's depth-1 clone has no `origin/main` | `8e3554ef4` |
| `39791127092` | Workspace Clippy | M004's private family modules made the members' `pub` items unreachable, so the dead-code lint reported latent dead code | `3dfeb6e2f` |

Both repairs are recorded in the preregistration §11, which was amended
**before any after-side result existed**.

---

## 5. The scoped-cost comparison (M003's four remaining cases)

M003 left four representative cases **argued, not proven**. This section
measures them.

### 5.1 The method

For each case, the changed file's line coverage is measured under three
scopes:

- **A** — the changed member's own tests;
- **B** — the **scoped selection's** targets, as `scripts/scoped_selection.py`
  computes them;
- **C** — the workspace-wide run, measured once and shared by all four cases.

The selection is not restated here; it is **re-derived from M003's own
tool** on the candidate tree, which is the only thing that makes the
comparison a measurement rather than a restatement:

```bash
python3 scripts/scoped_selection.py --base 2c1953347 --files <changed-file> --format json
```

The coverage itself is `cargo llvm-cov --json --ignore-run-fail`, reduced
on the way out to the per-file line summary (the raw llvm-cov export is
~400 MB per scope and carries every segment; only the line summary is
needed).

**Three concessions. The first two apply identically to every scope, so
the comparison between scopes is unaffected; the third is a fact about
the measurement, not a limit on it:**

1. **The measurement is local**, not hosted. The CI job emits no coverage
   data, so it cannot be produced hosted at all.
2. **Failing tests are tolerated, not skipped.** This was the one real
   methodological hazard in the measurement, and it is resolved by the
   tool rather than by weakening the suite. `cargo llvm-cov` aborts at its
   *report* step when the test command exits non-zero: the tests run, the
   `.profraw` files are written, and then **no JSON is emitted** — so a
   single failing test destroys the entire scope. The first revision of
   this measurement worked around that by **skipping** six tests, which
   is strictly worse than running them: a skipped test contributes **no**
   coverage, while a failing test contributes the coverage of every line
   it executed before its assertion. `cargo-llvm-cov` 0.8.7 provides
   `--ignore-run-fail` — *"Run all tests regardless of failure and
   generate report. If tests failed but report generation succeeded, exit
   with a status of 0"* — so **nothing is skipped**. Every test in every
   scope runs, and the scopes' coverage figures include the lines a
   failing test still executed.

   The switch was **validated before it was used**: the single
   macOS-only failure `session_projection_rebuilds_owner_scoped_live_jobs`
   was run under `--ignore-run-fail`; the test failed (exit status 101)
   and a valid 102 KB JSON coverage report was still produced.

   The recorded scope's failing tests are **ten**, and they are listed
   per scope rather than predicted in advance (the per-scope lists are
   `/tmp/m005_cov/<scope>.failed.txt`, reproduced in §5.3):

   - **three macOS-only failures** — they fail on macOS and pass in the
     Linux CI job, and they are recorded in `architecture/testing.md`
     ("Known macOS-local failures") as proved pre-existing at the
     baseline: `session_projection_rebuilds_owner_scoped_live_jobs`,
     `session_projection_snapshot_includes_in_process_live_child_instances`,
     `offline_required_scheduler_carried_intent_refuses_disabled_sandbox_before_spawn`;
   - **seven environment-sensitive failures** — two in the root lib's
     `tui::commands::interactive_terminal::tests` module, which assert on
     a live PTY surface; two in
     `async_background_c006_tool_to_idle_ci`; two in
     `scratch_cache_root_contract`; and one in `scratch_retention_sweep`.
     All seven assert on real scheduler, terminal, cache-root or
     filesystem behaviour, and all are sensitive to this machine's load.

   **An earlier hypothesis about the wall-clock budget tests was wrong
   and is corrected here.** A first pass of the driver (with the skips)
   failed on `replay_p95_within_budget`, and the natural reading was that
   instrumentation inflates a 5 ms p95 past its own budget so the test
   is *unpassable under a profiler*. The recorded scope **falsifies that
   reading**: `replay_p95_within_budget` and the root lib's
   `m002_frontier_eval_p95_within_budget` both **passed** in the scope
   that was kept. Their earlier failure was contention on this machine,
   not the profiler. The distinction matters because the earlier reading
   would have meant *excluding* them, and they are now simply part of the
   run.

   None of the ten touches the four files under measurement.
3. **The absolute figures are floors for the lines a failing test never
   reached.** A test that fails *at* its assertion has still executed the
   lines before it, so the effect is small; but it is not zero, and the
   figures are labelled as covering the scope's tests *as they ran*, not
   as a suite that passed.

### 5.2 The four cases

#### Case 1 — `src/tui/app/mod.rs` (20,345 lines), root-only member

| | |
|---|---|
| **Selection** | root lib + `tui_render`. Not widened: the file has no dependents outside the root package. |
| **Behavioural dependency** | the TUI's own state machine — `App`'s chat send/render round-trip, the change-review approval trajectory, key handling, project tabs. The canonical assertion is `tests/tui_render/collaboration_m002_chat_tui.inc::send_reply_render_round_trip`, which builds a real `codegg::tui::app::App` through `App::new_for_testing`, wires a fake core client, sends a chat message, and asserts both the emitted `TuiCommand::ChatMessageSent` and the rendered draft. **30 of `tui_render`'s 41 members construct an `App`.** |
| **In the selection?** | **Yes.** `tui_render` is the selected target, and the assertion is one of its members. |
| **Now measured** | the scoped selection's coverage of the changed file, against the member's own tests and against the workspace-wide run. |
| **Still argued** | that `tui_render` is the *right* member to carry the TUI's behaviour — the coverage numbers show what the selection covers, not that the grouping is the best one. |

#### Case 2 — `crates/codegg-core/src/session/schema.rs` (3,940 lines), non-root member

| | |
|---|---|
| **Selection** | `codegg-core` lib + `cached_models_composite_key` / `continuation_checkpoint` / `session_title_slug`; **plus the root package's full surface** — 21 targets with the features `linux-git-test-support,lsp-test-support,server,windows-sandbox-test-support` — because the root package depends on `codegg-core` and "the coupling runs through its own source and public API" (`widened: true`). |
| **Behavioural dependency** | three schema contracts, each with its own named target:<br>• `cached_models` (schema.rs:732) — the real identity of a discovered model is `(id, provider)`, not `id` alone. `cached_models_composite_key` asserts exactly that; the table originally declared `id TEXT PRIMARY KEY`, so a batch insert aborted and the cache was left permanently empty, making every discovery refresh re-hit the network.<br>• `continuation_checkpoint` (schema.rs:2563) and its three indexes — the `continuation_checkpoint` target asserts the table and indexes exist after migration and that the layout version tracks the canonical version.<br>• the session title/slug columns — `session_title_slug` asserts a title update recomputes the slug and a non-title update leaves it alone. |
| **In the selection?** | **Yes — all three**, and each maps to exactly one schema object. The root's widened surface is the safety net for whatever the member's own three targets do not reach. |
| **Now measured** | both halves of the widened selection separately: the `codegg-core` member's three targets, and the root's full 21-target surface. The gap between them is the *dependency-widening* margin, and it is now a number rather than an argument. |
| **Still argued** | that the root's widened surface is *necessary* rather than merely safe. A number showing how much of the changed file the three narrow targets cover does not prove the other 21 are required; it bounds the cost of the widening. |

#### Case 3 — `src/scheduler/admission.rs` (827 lines), root-only member, three groups

| | |
|---|---|
| **Selection** | root lib + `interactive_process_sessions` + `scheduler_cancellation` + `scheduler_jobs`. |
| **Behavioural dependency** | admission control's **decisions** and its **permit accounting**:<br>• `scheduler_jobs/async_background_c005_idle_resume.inc::prod_resume_admits_exactly_one_owner_turn_through_fanout` — a resume admits exactly one owner turn, and `fifty_way_wake_race_admits_exactly_one_turn` — a 50-way wake race admits exactly one.<br>• `scheduler_cancellation/scheduler_cancellation.inc::cancel_running_job_terminates_process_and_releases_permit` — cancellation terminates the process **and releases the permit** (this is `AdmissionController::release`).<br>• `interactive_process_sessions/interactive_process_sessions.inc::permits_release_on_exit_and_terminate` — permits release on exit and on terminate. |
| **In the selection?** | **Yes — all three**, one per selected group. |
| **Now measured** | the coverage of the changed file by the three selected groups, and the part the root lib's own inline tests contribute. |
| **Still argued** | that admission control's *accounting* invariants (the budget, the queue caps, the fair-queue interaction) are fully asserted rather than incidentally exercised. |

#### Case 4 — `src/lsp/mod.rs` (170 lines), root-only member, feature-gated target

| | |
|---|---|
| **Selection** | root lib + `lsp_test_support`, which is gated behind `required-features = ["lsp-test-support"]`. |
| **Behavioural dependency** | this file is a **compatibility shim**. Everything but two items is a `pub use egglsp::*` re-export. The only logic is `config_lsp_to_egglsp` — a serde round-trip that logs loudly and falls back to a default rather than silently returning one — and the `Lsp` aggregator struct. The assertion whose outcome a change here would move is the file's **own inline unit test**, `src/lsp/mod.rs::tests::lsp_config_conversion_round_trips_default`, which asserts the conversion round-trips the default config. |
| **In the selection?** | **Yes — via the root lib**, which every selection includes. |
| **The finding** | the *named* target `lsp_test_support` does **not** cover the shim's behaviour at all: it exercises `egglsp` itself (`egglsp::LspConfig`, `LspService`, the fake LSP server). And it is **feature-gated**, so the CI job — which does not enable `lsp-test-support` — never builds it. The behavioural cover for `src/lsp/mod.rs` in CI is the root lib's inline unit test **alone**. |
| **Now measured** | that the selection is *not vacuous* for this file — the root lib covers it — and that the feature-gated target contributes nothing to the shim. |
| **Still argued** | whether the selection should name a gated target at all for a file whose behaviour is carried by the root lib. This measurement shows the gate is inert for this file; it does not decide roadmap §10 **D1**, the feature-gated coverage decision, which stays open. |

### 5.3 The coverage comparison

**The measured table.** Line coverage of the changed file, per scope.
Scope A is the changed member's own tests, scope B is the scoped
selection's targets, scope C is the workspace-wide run (measured once and
shared by all four cases). The "failing tests" column is the number of
tests that failed *in that scope* under instrumentation; nothing is
skipped, so a failing test still contributes the coverage of the lines it
executed (§5.1).

| case | changed file | scope | lines covered / total | % | failing tests |
|---|---|---|---:|---:|---:|
| 1 | `src/tui/app/mod.rs` | A — the member's own tests (root package, every target) | 7474 / 14075 | 53.10 | 10 |
| 1 | `src/tui/app/mod.rs` | B — the scoped selection (root lib + `tui_render`) | 7426 / 14075 | 52.76 | 4 |
| 1 | `src/tui/app/mod.rs` | C — the workspace-wide run | 7474 / 14075 | 53.10 | 9 |
| 2 | `crates/codegg-core/src/session/schema.rs` | A — the member's own tests (`codegg-core`) | 2763 / 2937 | 94.08 | 0 |
| 2 | `crates/codegg-core/src/session/schema.rs` | B — the scoped selection's `codegg-core` half | 2763 / 2937 | 94.08 | 0 |
| 2 | `crates/codegg-core/src/session/schema.rs` | B — the scoped selection's root half (21 targets, widened) | _not attributable_ — see below | — | 11 |
| 2 | `crates/codegg-core/src/session/schema.rs` | C — the workspace-wide run | 2765 / 2937 | 94.14 | 9 |
| 3 | `src/scheduler/admission.rs` | A — the member's own tests (root package, every target) | 362 / 458 | 79.04 | 10 |
| 3 | `src/scheduler/admission.rs` | B — the scoped selection (root lib + 3 groups) | 362 / 458 | 79.04 | 10 |
| 3 | `src/scheduler/admission.rs` | C — the workspace-wide run | 362 / 458 | 79.04 | 9 |
| 4 | `src/lsp/mod.rs` | A — the member's own tests (root package, every target) | 13 / 72 | 18.06 | 10 |
| 4 | `src/lsp/mod.rs` | B — the scoped selection (root lib + the gated `lsp_test_support`) | 13 / 72 | 18.06 | 6 |
| 4 | `src/lsp/mod.rs` | C — the workspace-wide run | 13 / 72 | 18.06 | 9 |

**Case 2's root half is "not attributable", and that is a property of the
prescribed command, not a coverage fact.** The milestone plan's WP-D
prescribes `cargo llvm-cov -p codegg --lib --test <each selected target>`
for the root package's side. `cargo llvm-cov -p <pkg>` instruments **only
that package's** sources, and the scope's report bears it out: the root
half's report contains **650 files, all of them the root package's**, and
**zero** from `codegg-core` — while the workspace reference's report
contains 896 files, 128 of them from `codegg-core`. The root package's
tests *do* execute `codegg-core`'s schema code, but llvm-cov does not
instrument that crate in this scope, so its lines are not attributed.

**The widening's margin is nevertheless bounded exactly, and the bound is
decisive.** The root half's 21 targets are a **subset** of the workspace's
test set, so the root half's contribution to this file's coverage cannot
exceed the workspace's. The workspace covers **2,765** lines against the
narrow selection's **2,763** — so the *entire* workspace buys **2 lines
out of 2,937 (0.07 %)** over the three narrow targets, and the root half
buys **at most those 2 lines**. Measured this way, the dependency
widening's margin for this file is a rounding error, and the three named
targets are already sufficient in practice.

**The per-scope failing tests, and what they mean.** The three macOS-only
failures appear in **every** scope that runs the root package, and in
none of the two `codegg-core` scopes (which is consistent with their
being root-package tests). The rest are load-sensitive and scope-local:
`interactive_terminal::tests` (up to three per scope), `scratch_cache_root_contract`
(two), `async_background_c006_tool_to_idle_ci` (one or two),
`scratch_retention_sweep` (one), `managed_process_descendants` (four, in
the root half only), plus three singletons. **None of them touches the
four files under measurement**, and none is excluded from the coverage
figures — every one of them ran, and its executed lines are counted.

**Per case, what the comparison now shows.**

| case | what is now measured | what remains argued |
|---|---|---|
| 1 | the scoped selection covers **7,426** of the file's lines against the **7,474** the member's own full test surface covers — **99.4 %** of it — and against the workspace's **7,474**. | that `tui_render` is the *right* member to carry the TUI's behaviour. The numbers say the selection loses 48 lines; they do not say the grouping is the best one. |
| 2 | the three named targets cover **2,763 of 2,937** lines (**94.08 %**) — **identical** to the whole `codegg-core` crate's own tests — and the whole workspace adds **2 lines**. The widening's margin is **≤ 2 lines**. | whether the root's 21-target widening is *necessary* rather than merely safe. The measurement says its margin is at most 0.07 % of this file; deciding whether that margin is worth the cost is a policy question this milestone does not make. |
| 3 | the scoped selection covers **362 of 458** lines (**79.04 %**) — **identical** to the member's own full test surface and to the workspace-wide run. The selection is exactly sufficient here. | that admission control's *accounting* invariants (the budget, the queue caps, the fair-queue interaction) are fully asserted rather than incidentally exercised. Coverage cannot distinguish the two. |
| 4 | the selection covers **13 of 72** lines (**18.06 %**) — **identical** to the member's own full test surface and to the workspace-wide run. The feature-gated `lsp_test_support` target contributes **nothing**: adding it changes the figure by **zero lines**. | whether the selection should name a gated target at all for a file whose behaviour is carried by the root lib. This measurement shows the gate is inert for this file; it does not decide roadmap §10 **D1**, which stays open. |

**The shape of the result.** For three of the four cases the scoped
selection delivers **exactly** what the whole workspace delivers for the
changed file; for the fourth it delivers 99.4 % of it. That is the
negative property M003 names — *a change that touches a subsystem with a
known covering test selects that test* — measured rather than argued,
across four files in three crates.

### 5.4 What this section does and does not discharge

M003's §6.1 was explicit that at line level the member's own tests already
covered a superset, and that the real finding was the **behavioural
dependency**. This section keeps that shape: the line numbers are reported,
but each case's conclusion is stated in terms of the assertion whose
outcome would move and whether that assertion is in the selection — which
is the property M003's guard actually promises.

The **negative** property M003 names is what cases 1, 3 and 4 demonstrate
directly: a change to a subsystem with a known covering test **does** select
that test. Case 2 demonstrates it for a non-root member and measures the
dependency widening's margin rather than merely asserting it.

This section **does not** decide roadmap §10 **D1** — the 149 feature-gated
test functions CI never executes. Case 4 is the sharpest instance of why
that decision is still open, and it is left open.

---

## 6. The coverage check (invariant I1)

### 6.1 The predicate, and the two corrections it needed

The predicate is the preregistration's §11.2 correction, restated here
because the **plan's own acceptance criterion §13.5 still says** "the
after-run executes a superset of the before-run's test functions", and the
supersession is recorded rather than quietly worked around:

- **I1a — no unexplained loss.** Every test function the before-run
  executed that is **not** a recorded removal in `docs/test-duplication.toml`
  must still be executed by the after-run.
- **I1b — every removal is covered.** For every function the before-run
  executed that **is** a recorded removal, the removal's recorded
  **survivor** must be executed by the after-run.
- **I1c — no mass loss.** The number of before-only functions must not
  exceed the number of recorded removals (85).

The original superset-of-functions formulation was **unsatisfiable for
this workstream**: M007's recorded purpose is to remove 85 *duplicate*
test functions and to stop the shared-helper fan-out, and removing a
duplicate removes a *function* without removing *coverage*. The correction
was made before any after-side result existed — both candidate runs had
*failed* when it was made — and the corrected predicate is strictly
stronger where it matters, because it ties each removed function to a
survivor the after-run must actually execute.

### 6.2 The comparison basis — a second correction the measurement required

The first implementation compared the two runs' test **paths** for
equality. That is wrong for this workstream, and it would have produced a
meaningless result rather than a false alarm.

M004's consolidation changes the test id **by construction** — the
family's own `mod.rs` says so: *"The only visible change is the test id,
which becomes `<family>::<member>::<test>`"*. A member that was
`tests/ssrf.rs` (its own executable, path `tests::test_x`) becomes
`tests/security_permission/ssrf.inc` (path `ssrf::tests::test_x`). A path
equality comparison therefore reports **every consolidated function as a
loss** — thousands of false positives that say nothing about coverage.

The identity that survives consolidation is the function's **name** — the
last `::` segment — which the consolidation preserves exactly, because the
test body and the test name are copied verbatim into the member. The
check therefore compares **counts per function name**: membership alone
would hide a dropped copy of a duplicated name, and a count comparison
does not. The path-level detail is reported alongside, so a relocation is
visible as a relocation and never as a silent loss.

The check's `--self-test` proves it is not vacuous: it must pass on
identical sets, fail when an unrecorded function disappears, fail when a
removal's survivor is also gone, pass when a function is merely relocated
under a new family prefix, and detect a dropped duplicate.

### 6.3 The inputs

| | before | after |
|---|---|---|
| Executed test **paths** (nextest result lines) | 15,612 | 15,531 |
| Distinct executed test **function names** | 15,403 | 15,329 |
| Executed test **instances** (result lines) | 15,841 | 15,532 |
| Recorded removals in `docs/test-duplication.toml` | 85 | 85 |
| Recorded removals the before-run actually executed | **85 of 85** | — |
| Distinct survivors those removals name | 76 | — |

All **85** recorded removals were executed by the before-run, so the
decision table and the before-side's executed set agree completely: there
is no recorded removal that the before-run never ran, and therefore no
recorded removal that could be a *silent* loss.

One survivor, `test_mcp_ipv4_multicast`, was **not** executed by the
before-run: the integration copy it names did not exist at the baseline
(it was `tests/ssrf.rs::test_ipv4_multicast` then, and it still exists in
the candidate as `tests/security_permission/ssrf.inc::test_ipv4_multicast`).
That is not a defect — I1b asks whether the after-run executes the
survivor, not whether the before-run did.

### 6.4 The result

**Verdict: I1 holds — with one record-completeness finding, stated below.**

**The check was run against every successful after-side run, and all
three agree exactly.** `before-2` is the reference — the four before-side
runs execute the same set, so which one is the reference does not change
the answer — and `after-1`, `after-4` and `after-topup-1` each produce
the same numbers: 15,329 distinct names after against 15,403 before,
15,532 executions against 15,841, zero unexplained losses, zero removals
without a surviving assertion, 78 ≤ 88 on the mass-loss bound, and an
exact 313-execution reconciliation. The two runs that failed are not
samples and contribute nothing to this check.

The check is reported **twice**: against the removal record the
preregistration named (M007's decision table), and against the complete
record. The difference between the two is the finding.

| | against M007's table alone | against the complete record |
|---|---:|---:|
| Recorded removals read | 85 | 88 |
| Names that vanished entirely | **78** | **78** |
| **I1a** — unexplained losses | **3** | **0** |
| **I1b** — removals with no surviving assertion | **0** | **0** |
| **I1c** — mass loss (78 ≤ bound) | **pass** (85) | **pass** (88) |
| Verdict | **fail** on I1a | **pass** |

### 6.5 The three functions the first reading left unexplained

Read against M007's decision table alone, three function names are
unexplained losses. They are **M006's three vacuous-body removals** in
`crates/codegg-core/src/tool_program/guards.rs`, and M006's closure record
already documents each one:

| function | why it could not fail | its survivor |
|---|---|---|
| `parse_only_marker_compiles` | "called a no-op macro and asserted nothing" | `scripts/check_tool_program_parse_only.py` |
| `cpython_guard_function_exists` | "called a documentation-only function" | the same guard |
| `no_cpython_import_in_module` | "had a body of nothing but comments" | the same guard |

The survivor is a **static guard**, not a test function: it rejects a
`pyo3` dependency in any workspace manifest (manifest *and* lockfile), a
`pyo3` import in the Tool Program module, a process spawn in the module,
and a Python FFI link — four concrete classes the three tests could not
check at all. It runs in `scripts/verify.sh quick` (and therefore in the
CI `verify` job) and it has a `--self-test` that injects each class.

The public API the three tests sat next to — `ParseOnlyPipeline`,
`ToolProgramCompilerIsParseOnly`, `assert_parse_only!`,
`cpython_execution_is_forbidden` — is **unchanged**; only the
`#[cfg(test)] mod tests` block that held them was deleted.

**This is a finding about the *record*, not about the product.** The
roadmap's §11 third bullet asks that every removal be "recorded as a proven
duplicate naming its survivor". These three were not duplicates and they
are not in the duplicate table; they are recorded in **M006's closure
record**. The property the bullet wants — *every removal is recorded, and
names its survivor* — holds. But it is currently spread over **two**
records, and a consumer that reads only `docs/test-duplication.toml` sees
three unexplained losses. That gap is recorded as an open item in the
closure record, with the recommendation that a single machine-readable
removal record should own both kinds.

### 6.6 The execution-level reconciliation — every lost execution attributed

The name-level predicate is the invariant. The **execution-level**
reconciliation is the stricter accounting, and it closes exactly.

| | executions |
|---|---:|
| before-side executions | 15,841 |
| after-side executions | 15,532 |
| **lost** | **313** |
| attributed to the **85** recorded duplicate removals (M007) | 85 |
| attributed to **M006's three** vacuous-body removals | 3 |
| attributed to the **shared-helper fan-out** (M007 §6) | 225 |
| **attributed elsewhere** | **0** |

The reconciliation is exact: **313 = 85 + 3 + 225**, with nothing left
over.

**The shared-helper fan-out, measured.** The same five assertions —
`assert_no_credentials_passes_when_clean`,
`assert_no_credentials_fails_on_leak`, `sentinel_unique_per_call`,
`collect_bytes_finds_file_contents`, `sentinel_format_starts_with_prefix` —
ran **46 times each, in 46 distinct root test binaries** on the
before side, and **once each, in one binary** (`codegg::command_git`) on
the after side: **230 executions → 5**.

**A figure to correct, not to hide.** M007's closure record says the
helper "was compiled into all eight and ran **forty** times". That figure
is correct **for the tree M007 measured on** — the already-consolidated
M004 tree, where eight families bound `tests/common/` — but the
before-side of *this* comparison is the **pre-consolidation** baseline, in
which **47** top-level test files declared `mod common;` (46 of them
compiled into the CI job; the 47th was `tests/test_util.rs`, a helper with
no test functions that M004 deleted). The measured baseline figure is
therefore **230 executions, not 40**, and the discharge is larger than
M007's record states. The record's substance — five assertions, one
target, 46 binaries down to one — is confirmed; only the baseline scale is
corrected.

---

## 7. Not obtained, and why

| Not obtained | Why |
|---|---|
| A **cold-cache** run on either side | rust-cache's shared key restores across refs that share a lockfile, and `Cargo.lock` is byte-identical between the two sides, so every run is warm. A cold run would require deleting a cache entry other branches share, which this milestone does not do unilaterally. Preregistration §5.1. |
| A **same-machine baseline** re-measurement under §4.1's exact command | The baseline's 305 linked test executables are 13.7 GB of binaries plus their intermediates; the first attempt to build them ran the disk to 9 GiB free and was stopped to avoid filling the user's disk. The same-session comparison in §3 was obtained instead. |
| A **per-target link cost from the hosted job** | The CI job emits no `--timings` data, so this number cannot be produced hosted at all. It is a local measurement, by construction. |
| A **before/after pair measured on the same host** | This is what would remove the ~2× host-speed term from the wall-clock comparison (§4.4). GitHub's hosted runners are shared, uncontrolled 4-vCPU VMs, and `workflow_dispatch` cannot pin a host, so the term cannot be removed. The only way to obtain it is to change the CI job's runner definition, which invariant I3 forbids and which would change the job being measured. |
| A **diagnosis of the checkpoint-tail flake's mechanism** | The failure is a documented pre-existing CI-only flake (§4.5) and the repository's recorded guidance for it is not to weaken the test. Diagnosing it further would mean changing a test, which this milestone does not do. |
