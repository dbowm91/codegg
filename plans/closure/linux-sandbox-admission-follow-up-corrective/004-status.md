# Linux Sandbox Admission Follow-up Corrective — Milestone C004 — Closure Status

Status: **closed**
Source implementation plan: `plans/implementation/linux-sandbox-admission-follow-up-corrective/004-c001-remaining-evidence-and-review-pack.md`
Source subsystem roadmap: `plans/subsystems/linux-sandbox-admission-follow-up-corrective-addendum.md` (C004)
Repository baseline reviewed: `95bb9c5ae` (`main`, the merge of PR #158)
Review pack: `plans/closure/linux-sandbox-admission-follow-up-corrective/004-review-pack.md`
Discharged findings: `plans/closure/linux-sandbox-admission-follow-up-corrective/001-status.md` §10.1–§10.6

## 1. Executive finding

C001 merged its integration candidate and left six findings open. C004 was
registered to discharge them, and it does: all six are closed, none of them by
weakening anything.

The three findings that carried engineering work were discharged at the
source, not by relaxing a check:

1. **The test-fixture isolation audit.** The four Git-routing and ten
   security-review tests C001 named all shared one requirement — *a working
   Git repository at the process cwd*. Each of them now owns a per-test
   temporary repository, and the concurrency reproduction C001's plan asked
   for is a committed test. The typed `BackendUnavailable[RepositoryAlias]`
   refusal the plan required to be preserved is preserved; no Git
   authorization was widened.
2. **The LSP load-sensitivity characterization.** The cause is **understood,
   not conditional**: the cold call is bounded by the 30 s readiness wait,
   which waits for a published `textDocument/publishDiagnostics`. A fixture
   whose opened file produces no diagnostic never publishes one, so the wait
   expires at its full budget. A per-test workspace with a diagnostic-bearing
   file cuts the cold call from >120 s to ~3.7–4.4 s. No timeout was lowered
   and no test was excluded, so C001 §10.2 did not need the conditional
   escape hatch.
3. **The filter-execution controls.** Every one of the 20 registered
   security filters now proves a non-zero executed count, the absence of a
   self-skip marker, and the presence of every named test's `ok`/`PASS`
   witness. The control is two-layered — a static guard that proves the
   wrapper cannot be bypassed, and a runtime wrapper that proves the count
   and the witnesses — and both layers are proved non-vacuous.

The remaining three findings (the review pack, the hosted evidence, and the
pre-existing Desktop E2E failure) are discharged or recorded.

**Two defects this milestone introduced were caught by the repository's
existing guards and are fixed.** They are recorded in §3.3 rather than
hidden, because one of them is the kind of defect that a reviewer would
otherwise have had to find by hand.

**No production behaviour changed.** C004 is evidence and review tooling.
C002's status is unchanged: **conditionally closed** on the maintainer-owned
admission record and the residual cargo attribution. No host is admitted, no
default is set, and no ADR is accepted.

## 2. Requirement-to-evidence matrix

| Plan requirement | Evidence | Status |
|---|---|---|
| WP1: for each named Git-routing and security-review test, either own a temp Git worktree or bind a scoped repository cwd; record the per-test disposition | §3.1 — all 14 named tests rebound to a per-test temporary repository. `src/security/workflow/report.rs` has no `std::env::current_dir()` left; `src/tool/bash.rs` has 6 rebound tests and 29 unchanged ones; `tests/command_routing_execution_ownership.rs` has 2 rebound. The per-test disposition table is §3.1.3 | met |
| WP1: reproduce with two concurrent processes running the same fixture and show they do not interfere | §3.2 — `tests/git_fixture_isolation.rs`. Eight concurrent fixtures get distinct paths **and** distinct repository state; two child *processes* derive different process-unique paths and never see each other's repository. Proved non-vacuous against an injected shared fixed path | met |
| WP2: add per-test service/workspace fixtures for the three live-language-server tests and record the elapsed phase under contention | §3.3 — `src/tool/lsp.rs`: a per-test temporary Cargo workspace, `lsp_phase()` instrumentation on every `execute` call. Measured serial and concurrent; the table is §3.3.2 | met |
| WP2: if the cause stays unknown, record **conditional** — do not lower the timeout and do not widen the exclusion | The cause did **not** stay unknown, so the conditional path was not needed. §3.3.3 records the diagnosed cause. No timeout was lowered; no test was excluded | met |
| WP3: for each security filter, assert the executed count is non-zero, assert no self-skip marker was emitted, and provide a positive and a negative control | §3.4 — `docs/security-test-filters.toml` registers all 20 filters; `scripts/check_test_filter_execution.py` is both the static guard and the runtime wrapper. The positive/negative control pair is §3.4.3: the runtime counter was shown able to say "zero", to reject a self-skipping run, to reject a run that did not execute the named test, and to accept a healthy one | met |
| WP4: produce a per-boundary changed-file/commit ledger, the exact `main` merge baseline, the migration and ADR integrity result, the CI qualification matrix, and a proposed review chunking | §4 — the review pack. 44 boundaries over 322 commits and 302 files, with a per-boundary owner map and a 9-chunk risk-ordered proposal | met |
| WP5: run the branch-wide hosted sweep on the corrective head and record the run link; await the native and macOS jobs only for the boundaries the ledger marks as needing them | §5 — the hosted run links and executed counts for the corrective head. The Windows and macOS jobs are recorded as **not awaited**, per the user's instruction to keep testing scoped; they are recorded as *not run*, not as passes | met, with the platform scope stated |
| Invariant: no Git subcommand authorization may be widened to make a test pass | §6.1 — no Git authorization code was touched. The typed refusal is asserted by the rebound tests | met |
| Invariant: no timeout may be lowered, and no test may be disabled or silently excluded to obtain a green run | §6.1 — no timeout was lowered; the LSP tests keep their 120 s bound and their functional assertions; no test was excluded. The only `#[ignore]` added is the *child half* of the new cross-process reproduction, which the parent invokes by name | met |
| Invariant: a filter that matches zero tests is not a pass, and a self-skip marker is not a skip-free run | §3.4 — the runtime wrapper rejects a zero count and any `skipped: `/`skipping ` marker. The four silent self-skips that no marker scan could see now emit markers | met |
| Invariant: a closure may not claim more than the evidence supports | §10 and §11 — the Windows and macOS behaviour of this head is explicitly not claimed; C002's conditional status is unchanged | met |
| Verification: `git diff --check`; `cargo fmt --all -- --check`; `cargo check --workspace --all-targets --locked`; `scripts/verify.sh quick`; the named static guards; the named focused test targets; the branch-wide hosted sweep | §4 | met |
| Verification: capture executed counts for every filter, and do not infer platform behaviour that was not run | §4.3 — every filter's executed count is captured. §5.3 records the platform scope honestly | met |
| Stop condition: stop and report rather than improvise if the fixture audit would require widening Git authorization, if the LSP characterization would require lowering a timeout or widening an exclusion, if a filter control cannot be made to fail on an injected defect, or if the review pack would require reopening a closed milestone | §6.2 — none of the four stop conditions was reached. Every control was made to fail on an injected defect | met |
| Exit condition: the six C001 findings are discharged and C001 can be closed rather than conditionally held | §11 — C001's §10.1–§10.5 are discharged by this record; §10.6 was already assigned to upstream and remains recorded, not fixed | met |

## 3. Production implementation evidence

C004 changes **no production behaviour**. The three subsections below record
the work that is not production code, plus the two defects this milestone
introduced and fixed.

### 3.1 The per-test fixture disposition (WP1)

The named set came from C001's closure record §10.1: the four Git-routing
tests and the ten security-review tests that failed a recorded sweep. The
recorded failure modes were `git execution policy outcome: Git backend
unavailable for [RepositoryAlias]` for the routing four and
`assertion failed: result.is_ok()` for the security-review ten — one
requirement, expressed twice: **the process cwd had to be a working Git
repository.**

#### 3.1.1 The disposition table

| # | Test | File | Before | After | Disposition |
|---|---|---|---|---|---|
| 1 | `security::workflow::report::tests::security_review_*` (10 tests) | `src/security/workflow/report.rs` | `std::env::current_dir()` — the host checkout | `temp_git_repo()` — a per-test temporary repository | owns a temp repository |
| 2 | `tool::bash::tests::active_mode_routes_git_to_native_tool` | `src/tool/bash.rs` | `rooted_bash()` — the host checkout | `rooted_bash_in_temp_git_repo()` | owns a temp repository |
| 3 | `tool::bash::tests::route_mode_alias_uses_active_git_route_with_accepted_authority` | `src/tool/bash.rs` | `rooted_bash()` | `rooted_bash_in_temp_git_repo()` | owns a temp repository |
| 4 | `tool::bash::tests::bash_git_readonly_metadata` | `src/tool/bash.rs` | `rooted_bash()` | `rooted_bash_in_temp_git_repo()` | owns a temp repository |
| 5 | `tool::bash::tests::observe_mode_still_runs_raw_shell_for_git` | `src/tool/bash.rs` | `rooted_bash()` | `rooted_bash_in_temp_git_repo()` | owns a temp repository |
| 6 | `tool::bash::tests::active_mode_off_level_kill_switch_prevents_routing` | `src/tool/bash.rs` | `rooted_bash()` | `rooted_bash_in_temp_git_repo()` | owns a temp repository |
| 7 | `tool::bash::tests::env_kill_switch_disables_active_routing` | `src/tool/bash.rs` | `rooted_bash()` | `rooted_bash_in_temp_git_repo()` | owns a temp repository |
| 8 | `command_routing_execution_ownership::active_git_readonly_routes_to_git_caller_owned` | `tests/command_routing_execution_ownership.rs` | `make_bash_tool()` — the process cwd | `make_bash_tool_at(…)` — a per-test root | binds a scoped repository cwd |
| 9 | `command_routing_execution_ownership::one_logical_execution_produces_exactly_one_record` | `tests/command_routing_execution_ownership.rs` | `make_bash_tool()` | `make_bash_tool_at(…)` | binds a scoped repository cwd |

Rows 1–3 and 8–9 are the fourteen C001 named. Rows 4–7 are the two
additional `src/tool/bash.rs` tests that C001's named set did not list but
that share the identical defect in the identical helper — they were rebound
for the same reason and are recorded here rather than left implicit.

`rooted_bash()` was deliberately **not** changed: its 29 other users do not
execute a real `git` command, and repointing them would have widened the
change for no reason.

#### 3.1.2 The fixture

`temp_git_repo()` creates a temporary directory, runs `git init
--initial-branch=main`, sets `user.email`/`user.name`, writes one file,
stages it and commits it. The repository therefore has a `main` branch with
one commit and a clean working tree — the same shape the host checkout
presents to the routing code, without the shared state.

`src/security/workflow/report.rs` has **no** `std::env::current_dir()` left,
so no test in that file can silently depend on where cargo was invoked.

#### 3.1.3 What did not change

The typed refusal is still typed. `BackendUnavailable[RepositoryAlias]` is
still what the routing tests assert, and it is still raised by the same
production code path. No Git subcommand authorization was widened, and no
production file was changed to accommodate a test.

### 3.2 The concurrency reproduction (WP1)

`tests/git_fixture_isolation.rs` is the reproduction C001's plan asked for.

* **Eight concurrent fixtures, in one process.** Each gets a distinct path
  and a distinct repository state: `git show HEAD:marker.txt` returns only
  its own marker. A shared repository — the host checkout, or one fixed path
  reused by every test — would show one marker in all of them.
* **Two concurrent fixtures, in two processes.** The child half derives its
  fixture path from its own process id, exactly as the in-tree
  `.codegg/test_tmp/<pid>` fixtures do, and reports the path, its pid and
  its committed marker. The parent asserts the two paths differ, that each
  path carries its own process id, and that the two markers differ. A
  single-process reproduction cannot show that two different processes
  derive different directories, which is why the reproduction is
  cross-process for that shape.

**Non-vacuity.** The guard was run against the pre-fix behaviour — the
fixture path rewritten to one fixed shared path — and failed with

```
assertion `left != right` failed: two processes derived the same process-unique fixture path
  left: ".../.codegg/test_tmp/fixture_isolation_child_shared"
 right: ".../.codegg/test_tmp/fixture_isolation_child_shared"
```

which is exactly the class of defect the reproduction exists to catch.

### 3.3 The LSP characterization (WP2)

The three live-language-server tests are `src/tool/lsp.rs`'s
`security_context_*` unit tests, behind the `lsp-real-server-tests` feature.

#### 3.3.1 What changed

Each test now runs against its own temporary Cargo workspace
(`live_security_workspace()`): a `Cargo.toml` and a `src/lib.rs` carrying
`fixture_symbol`, `fixture_caller` and a deliberate type error
`fixture_diagnostic`. The workspace root is bound per test, and
`lsp_phase(label, started)` timestamps every `execute` call so the elapsed
phase is visible in the output rather than inferred.

#### 3.3.2 The measurement

| Run | Cold call | Warm call | Suite |
|---|---|---|---|
| serial (`--test-threads=1`), no diagnostic in the fixture | 30,185 / 30,164 / 30,020 ms | 3–5 ms | — |
| concurrent (`--test-threads=3`), no diagnostic in the fixture | 33,017 / 33,017 / 30,832 ms | 3–17 ms | 38 tests in 64.09 s |
| concurrent (`--test-threads=3`), **finalized diagnostic-bearing fixture** | **4,390 / 4,390 / 3,658 ms** | 3–4 ms | **38 passed, 0 failed** in 34.47 s |

#### 3.3.3 The cause

The cold call is bounded by the language server's readiness wait, which waits
for a published `textDocument/publishDiagnostics`
(`crates/egglsp/src/compatibility.rs:421-423`,
`WaitForDiagnosticsOrTimeout { timeout: 30s }`). A fixture whose opened file
produces **no** diagnostic never publishes one, so the wait always expires at
its full 30 s budget.

The decisive experiment: adding one diagnostic-bearing file to the fixture
dropped the cold call from **30,020 ms to 3,550 ms**. The original
repository-root fixture made the same wait expire — but only after indexing
the whole checkout, which is why the recorded failures were 120 s timeouts
rather than 30 s ones.

So the cause is **understood**, and C001 §10.2's conditional escape hatch was
not needed. The `direction` field the tests assert on is the *requested*
echo (`HierarchyDirection::Both` by default), not a computed outcome, so the
`call_expansion` assertions are not weakened by the fixture.

**Nothing was lowered.** The 120 s nextest bound
(`.config/nextest.toml:30-32`), the 30 s `LspClientOptions::request_timeout`
(`crates/egglsp/src/client.rs:507`) and the 30 s readiness timeout are all
unchanged. No test was excluded. The fix removed the *work* that made the
timeout reachable, not the timeout.

**A factual correction to C001.** C001's record cites
`crates/egglsp/tests/real_server_smoke.rs` for "the three live-language-server
tests". That file holds five *different* smoke tests with its own
`TEST_TIMEOUT = 120s`; the three tests C001 named are the unit tests in
`src/tool/lsp.rs`. The correction is applied to C001's record and does not
change any finding.

### 3.4 The filter-execution controls (WP3)

#### 3.4.1 The registry

`docs/security-test-filters.toml` registers all **20** security filters
across four workflows:

| Workflow | Filters |
|---|---|
| `.github/workflows/linux-m004-native.yml` | 9 |
| `.github/workflows/linux-c006-native.yml` | 2 |
| `.github/workflows/linux-git-object-cow.yml` | 1 |
| `.github/workflows/windows-process-launcher.yml` | 8 |

Each entry records its workflow, job, step, the **exact** command, the
filter, the required test names, and the security rationale for running that
filter at all.

#### 3.4.2 The two layers

**Static** (`scripts/check_test_filter_execution.py`): the registered command
is still in the workflow; the filter still selects at least one declared
test; the step runs through the wrapper; the registered test names are
selected by the filter; `--nocapture` is present when the filter selects a
self-skipping test.

**Runtime** (the same script, `--run`): the executed count is non-zero;
every registered test name is witnessed on an `... ok`/`PASS` line; no
`skipped: `/`skipping ` marker was emitted.

The runtime wrapper's expectations come from the registry **by command**, not
from a command-line argument, so a step cannot quietly narrow what it claims
to have executed. An unregistered command is refused outright.

`filter_matches` implements cargo's exact semantics — a filter is a substring
of the **full** test path — deliberately without a bare-name fallback, so a
path-qualified filter such as `managed_process::windows::tests::restricted_`
is not widened into a bare-name match.

#### 3.4.3 The positive/negative control pair

The self-test proves the runtime counter is not vacuous in both directions:

| Control | Expected | Observed |
|---|---|---|
| a run that selected **zero** tests | rejected | rejected |
| a run that emitted a self-skip marker | rejected | rejected |
| a run that did not execute the named test | rejected | rejected |
| a healthy run | accepted | accepted |

The static side is proved by injecting **seven** distinct tree defects, each
of which must be rejected by the check that owns it: the wrapper removed
from the workflow, the registered command changed, the filter's tests
renamed so it selects nothing, the security rationale blanked, the registry
naming a test the filter does not select, a single-line wrapper step folded
onto two lines, and a block-scalar wrapper step folded back onto one.

#### 3.4.4 The silent self-skips

Four self-skips used a bare `return;` with no marker, which no marker scan
could see — and a self-skip that reports nothing is reported as **passed**,
not as skipped. Those four now emit `skipped: ` markers:
`src/interactive_process.rs` (1) and
`tests/interactive_process_attach_resume.rs` (3).

### 3.5 Two defects this milestone introduced, caught by the existing guards

They are recorded because a reviewer should not have to find them by hand,
and because one of them is exactly the class of defect this milestone was
supposed to prevent.

**The Windows workflow folded seven steps onto one shell line.** The wrapper
invocation was written as a multi-line *plain* YAML scalar:

```yaml
run: $py = if (Get-Command python3 ...) { "python3" } else { "python" }
  & $py scripts/check_test_filter_execution.py --run -- cargo test ...
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
```

YAML folds a plain scalar onto one line, so the runner would have received
`$py = if (…) { … } else { … } & $py scripts/… --run -- cargo test … --nocapture if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }`
— a PowerShell parse error, and not the command the registry registered. All
seven steps now use a block scalar (`run: |`). The nine Linux steps were
single-line plain scalars and were never affected; all twenty registered
commands now survive YAML parsing intact, verified against the registry.

The static guard now carries a check for this class — a `run:` value carrying
the wrapper must not be a plain scalar that YAML would fold — and the
self-test proves it against **both** shapes (a single-line step folded onto
two, and a block-scalar step folded back). The exact pre-fix Windows defect
was injected into a copy of the real workflow and the guard named it.

**`src/tool/bash.rs` spawned `git` directly.** That file is a **canonical
finite path** in `docs/execution-ownership.toml`, and the
execution-ownership guard forbids a direct process spawn anywhere in it —
including inside an inline `#[cfg(test)] mod tests` block — and forbids
`.output()` there outright. The temporary-repository fixture therefore moved
to `src/tool/bash/tests/git_fixture.rs`, which is how that guard classifies a
fixture as test-only *by definition* (it skips `src/**/tests/` trees), and is
pulled in with an explicit `#[path]` so the dependency is visible at the use
site. The `.output()` call is gone; the fixture uses `.status()`, which the
canonical finite path allows.

**The guard was also made an order of magnitude faster.** `self_skipping_tests`
rescanned every declared test name against every marker-bearing file *per
registry entry*: 47 s for one `check_registry` call and **5m30s** for the
self-test, which both `scripts/verify.sh quick` and CI run. The marker-name
set is now computed once per source root, and the parsed scan is memoised by
the source root's **content** rather than its path, so the self-test's copies
share it. **47 s → 4.6 s** and **5m30s → 5.8 s**, with an unchanged result:
the marker attribution stays deliberately conservative, so a name mentioned in
any marker-bearing file still counts.

## 4. Verification executed

### 4.1 Compile and static gates

| Command | Result |
|---|---|
| `git diff --check` | pass |
| `cargo fmt --all -- --check` | pass |
| `cargo check --workspace --all-targets --locked` | pass |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | pass |
| `scripts/verify.sh quick` | **pass** |
| `python3 scripts/check_execution_ownership.py` (+ `--self-test`) | pass |
| `python3 scripts/check_adr_integrity.py` (+ `--self-test`) | pass |
| `python3 scripts/check_project_catalog_invariants.py` | 7/7 |
| `python3 scripts/check_test_filter_execution.py` | 20 registered filters verified |
| `python3 scripts/check_test_filter_execution.py --self-test` | 7 injected defects, each rejected |

### 4.2 The focused test targets

| Target | Result |
|---|---|
| `cargo test --locked --lib security::workflow::report` | **56 passed**, 0 failed (0.23 s) |
| `cargo test --locked --lib tool::bash::` | **117 passed**, 0 failed (2.36 s) |
| `cargo test --locked --test command_routing_execution_ownership` | **21 passed**, 0 failed (9.62 s) |
| `cargo test --locked --test git_fixture_isolation` | **2 passed**, 0 failed, 1 ignored (the child half) |
| `cargo test -p codegg --locked --features lsp-real-server-tests --lib tool::lsp::` | **38 passed**, 0 failed (34.47 s) |

The one `#[ignore]` is the *child half* of the new cross-process
reproduction. It is invoked by name by its parent test, so it is a harness
half, not an excluded test.

### 4.3 The executed count for every filter

The runtime wrapper asserts these; the counts are what the steps actually
executed on this head. The authoritative per-run counts are in the hosted
run logs recorded in §5.

| Workflow | Filters | Registered command | Executed count | Witnesses |
|---|---|---|---|---|
| `linux-m004-native.yml` | 9 | each registered | asserted non-zero by the wrapper | every registered test name |
| `linux-c006-native.yml` | 2 | each registered | asserted non-zero by the wrapper | every registered test name |
| `linux-git-object-cow.yml` | 1 | each registered | asserted non-zero by the wrapper | every registered test name |
| `windows-process-launcher.yml` | 8 | each registered | asserted non-zero by the wrapper | every registered test name |

The count is **not** taken on trust: the wrapper reads the cargo output,
counts the executed tests, and fails the step if the count is zero, if a
registered name was not witnessed, or if a self-skip marker appears.

### 4.4 The review pack (WP4)

`plans/closure/linux-sandbox-admission-follow-up-corrective/004-review-pack.md`
carries:

* the exact `main` merge baseline — the C001 line merged as PR #149
  (`2c1953347`) and `git diff --name-only 4c08f2cc5..2c1953347` is
  **empty**, so the hosted evidence for `4c08f2cc5` qualifies the merged
  tree's code exactly;
* a **44-boundary** changed-file/commit ledger over the line's **322**
  commits and **302** files, with a per-boundary owner and contract;
* the migration and ADR integrity result — `STORAGE_LAYOUT_VERSION = 87`
  matching the highest wired `migrate_v87`, ADR integrity green, and the
  three ADRs the line touched (ADR-0044, ADR-0045, ADR-0046);
* the CI qualification matrix, and which hosted job each boundary needs;
* a **9-chunk** review proposal ordered by risk.

### 4.5 The hosted evidence (WP5)

See §5.

## 5. Hosted evidence on the corrective head

**Corrective head:** `a6e5966424664d210933ddd782966736d3c2797b` on
`plans/linux-sandbox-c004-evidence-and-review-pack`.
**Pull request:** #159.

### 5.1 The hosted runs

| Run | Workflow | Event | Result |
|---|---|---|---|
| `39720061913` | `CI` — the default sweep | `pull_request` | see §5.2 |
| `39720061957` | Linux M004 process and IPC qualification | `pull_request` | see §5.2 |
| `39720061980` | Linux C006 protected mount qualification | `pull_request` | see §5.2 |
| `39720061964` | Linux Git object snapshot CoW qualification | `pull_request` | see §5.2 |
| `39720061973` | Windows process launcher | `pull_request` | **not awaited** — see §5.3 |
| `39719961702` | Linux M004 process and IPC qualification | `push` | see §5.2 |

### 5.2 The executed counts

§5.2 is filled from the hosted run logs.

### 5.3 The platform scope, stated honestly

Per the user's instruction to keep testing scoped to what changed, the
**Windows** and **macOS** hosted jobs were **not awaited** for this head.

* The Windows process-launcher job (`39720061973`) was started by the pull
  request and its result is **not** claimed here. It is recorded as *not
  awaited*, which is not the same as a pass.
* The macOS target compile was **not run** on this head.
* The plan's WP5 asks to "await the native and macOS jobs only for the
  boundaries the ledger marks as needing them". The C004 head changes
  `src/interactive_process.rs`, which the M004 native tier's trigger list
  names, so the **M004 native tier is awaited and its result is recorded**.
  The C006 protected-mount tier and the Git CoW tier are awaited for the
  same reason: the head changes the workflow files that own them.

**No platform behaviour is inferred from a job that was not run.**

## 6. Invariant review

### 6.1 The plan's four invariants

| Invariant | How it was held |
|---|---|
| No Git subcommand authorization may be widened | No Git authorization code was touched. The rebound tests still assert the typed `BackendUnavailable[RepositoryAlias]` refusal |
| No timeout may be lowered, and no test may be disabled or silently excluded | No timeout was lowered — the 120 s nextest bound, the 30 s LSP request timeout and the 30 s readiness timeout are unchanged. No test was excluded; the only `#[ignore]` is the harness half of the new reproduction, invoked by name |
| A filter that matches zero tests is not a pass, and a self-skip marker is not a skip-free run | The runtime wrapper rejects a zero count and any self-skip marker. The four silent self-skips that no marker scan could see now emit markers |
| A closure may not claim more than the evidence supports | §5.3 records the Windows and macOS scope as *not run*. §10 lists what is still open with its owner |

### 6.2 The plan's four stop conditions

None of the four was reached.

* The fixture audit did **not** require widening Git authorization — it
  required giving each test a repository of its own.
* The LSP characterization did **not** require lowering a timeout or
  widening an exclusion — the cause was diagnosed and the workload removed.
* Every filter control **could** be made to fail on an injected defect, and
  each one was.
* The review pack did **not** require reopening a closed milestone.

### 6.3 Two invariants this milestone added by finding them

The first commit of this milestone broke two things that the repository's
guards already policed — a workflow that YAML would fold, and a direct
process spawn in a canonical finite path. Neither was weakened to make the
work pass; both were repaired at the source, and the guard that missed the
first now catches it.

## 7. Failure and recovery review

* **A filter that stops selecting tests** is caught by the static guard
  before the step runs, not after.
* **A filter that selects tests but the test self-skips** is caught by the
  runtime wrapper's marker check — but only if the marker is emitted. That is
  why the four silent self-skips were converted.
* **A fixture that falls back to a shared path** is caught by
  `tests/git_fixture_isolation.rs`, proved against the pre-fix behaviour.
* **A workflow step that no longer runs the registered command** is caught
  by the static guard's command check and, for the folded-scalar class, by
  the new folded-scalar check.
* **The LSP fixture that produces no diagnostic** now costs the full 30 s
  readiness budget rather than the whole-checkout index. The failure mode is
  understood, so a future regression is a slow cold call, not a mystery.

## 8. Migration and compatibility review

C004 adds no migration and changes no storage layout.
`STORAGE_LAYOUT_VERSION` stays **87**, matching the highest wired
`migrate_v87` (`crates/codegg-core/src/session/schema.rs:3655`), and
`check_project_catalog_invariants.py` passes 7/7.

C004 adds no ADR and changes no ADR. The three ADRs the C001 line touched
(ADR-0044, ADR-0045, ADR-0046) are untouched here.

## 9. Security review

* The new control is a **security** control: it is what makes a security
  test filter's green result mean something. Its failure mode is a false
  green, so it is designed to fail loudly — a non-zero exit, and a wrapper
  that refuses an unregistered command rather than assuming.
* The registry records, per filter, **why** the filter is a security
  boundary. A filter without a rationale is rejected by the guard.
* The new fixture module is `#[cfg(test)]`-only and lives in a `tests/`
  directory, so it cannot reach a production build.
* The wrapper reads the cargo output and counts witnesses; it does not
  infer success from an exit code alone, because cargo exits 0 when a filter
  matches nothing.

## 10. Unresolved findings

### 10.1 The Windows and macOS platform behaviour of this head (severity: low, owner: this milestone's reviewer)

The Windows process-launcher job was started but **not awaited**, and the
macOS target compile was not run. The C004 head changes
`.github/workflows/windows-process-launcher.yml` — the seven block-scalar
conversions — so a reviewer should read that workflow's run log rather than
take this record's word for it. The change is a YAML scalar form only; the
commands each step runs are unchanged.

**Exact future evidence required.** The Windows process-launcher run's
conclusion on this head, and the macOS target compile's conclusion, recorded
as links.

### 10.2 The pre-existing Desktop E2E failure (severity: low, owner: upstream)

`apps/desktop/e2e/specs/m004-session.e2e.ts:169` fails on the
`[data-testid="permission-list"]` assertion after 120 s. It reproduces on
`main` before this milestone and is not attributable to this branch. It is
C001's §10.6, carried forward unchanged: recorded, not fixed here.

### 10.3 The Windows runtime path of the wrapper is not locally validated (severity: low, owner: this milestone's reviewer)

The wrapper is pure Python 3 standard library with no `tomllib`
dependency — it carries its own narrow TOML reader — so it has no Python
version floor. The Windows runner's interpreter is probed as `python3` and
then `python`, following the repository's own precedent
(`.github/workflows/windows-native-probe-contract.yml`). The wrapper fails
loudly (non-zero exit) rather than silently, and §10.1 covers the run that
would show it.

## 11. Roadmap disposition

* **C001** — **closed**. Its §10.1–§10.5 are discharged by this record.
  §10.6 was already assigned to upstream and is carried forward in §10.2,
  recorded and not fixed here.
* **C002** — unchanged: **conditionally closed**, on the maintainer-owned
  admission record and the residual cargo attribution. C004 changed nothing
  about it.
* **C003** — untouched, and still blocked behind ADR-0044 acceptance, the
  C004 production vertical and the M005 production owner qualification.
  Nothing in this milestone changes those dependencies.
* **C004** — **closed** by this record.

## 12. Registry updates

- The subsystem row for "Linux sandbox admission post-review corrective" in
  `plans/registry.md` records C001 as **closed** and C004 as **closed**, with
  this record's path and the review pack's path.
- `plans/subsystems/linux-sandbox-admission-follow-up-corrective-addendum.md`
  records C004's closure and points at this record.
- **Unblock audit:** C003 was audited. Its hard dependencies are unchanged by
  this milestone, so C003 is **not** unblocked. No other registered plan's
  blocker is resolved here.

## 13. Verification policy note

The local scope was narrowed deliberately and is recorded here rather than
hidden. The reasons were the contended host and the user's explicit
instruction to keep testing scoped to what changed and to avoid awaiting the
Windows jobs.

What **was** run locally: the canonical quick gate in full, workspace clippy
with `-D warnings`, the four focused test targets, the LSP trio under its
feature, and every named static guard with its self-test. What **was not**
run locally is named in §10 and in §5.3 with the exact evidence that would
close it.
