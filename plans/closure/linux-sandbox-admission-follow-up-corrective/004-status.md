# Linux Sandbox Admission Follow-up Corrective — Milestone C004 — Closure Status

Status: **closed**
Source implementation plan: `plans/implementation/linux-sandbox-admission-follow-up-corrective/004-c001-remaining-evidence-and-review-pack.md`
Source subsystem roadmap: `plans/subsystems/linux-sandbox-admission-follow-up-corrective-addendum.md` (C004)
Repository baseline reviewed: `95bb9c5ae` (`main`, the merge of PR #158)
Review pack: `plans/closure/linux-sandbox-admission-follow-up-corrective/004-review-pack.md`
Discharged findings: `plans/closure/linux-sandbox-admission-follow-up-corrective/001-status.md` §10.1–§10.6

## 1. Executive finding

C001 merged its integration candidate and left six findings open. C004 was
registered to discharge them, and it does: five of the six are closed and the
sixth is recorded and carried forward, none of them by weakening anything.

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

**Four defects this milestone introduced were caught by the repository's
existing guards, and all four are fixed.** They are recorded in §3.5 rather
than hidden.

**Two further findings are pre-existing defects that this milestone's own work
exposed.** §3.6 is a false green in the M004 tier that qualified 9 of the 11
regressions it named. §3.7 is a cancel/spawn race in one of those
regressions, which §3.6's own gate fix is what made run in that job for the
first time. Both are fixed, and both are recorded with the evidence that
proves the diagnosis rather than a plausible story about it.

**Three of the six findings were found by the hosted sweep** — two in §3.5
and the fifth in §3.6 — which is the sweep doing its job.

**And the control found a false green that was already there.** The M004
native tier's "Protected mount and Git regressions" step was reporting a
green run having qualified **9** of the **11** tests its filter selected: two
of them were behind a native-qualification gate the job does not enable, and
one of them skipped *silently*. That is C001 §10.3's failure mode, still
live, in a tier nobody had re-read. §3.6 records it, and a static check now
catches the class before a hosted run can.

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
| WP5: run the branch-wide hosted sweep on the corrective head and record the run link; await the native and macOS jobs only for the boundaries the ledger marks as needing them | §5 — the hosted run links and executed counts for the corrective head. The `CI` sweep, the C006 and Git CoW native tiers and the Windows job all ran and passed. The M004 native tier ran and **failed once on a pre-existing cancel/spawn race that this milestone's own gate fix exposed**; §3.7 records the diagnosis, the deterministic reproduction and the fix. The **macOS** target compile was **not run**, per the user's instruction to keep testing scoped, and is recorded as *not run*, not as a pass | met, with the platform scope stated |
| Invariant: no Git subcommand authorization may be widened to make a test pass | §6.1 — no Git authorization code was touched. The typed refusal is asserted by the rebound tests | met |
| Invariant: no timeout may be lowered, and no test may be disabled or silently excluded to obtain a green run | §6.1 — no timeout was lowered; the LSP tests keep their 120 s bound and their functional assertions; no test was excluded. The only `#[ignore]` added is the *child half* of the new cross-process reproduction, which the parent invokes by name | met |
| Invariant: a filter that matches zero tests is not a pass, and a self-skip marker is not a skip-free run | §3.4 — the runtime wrapper rejects a zero count and any `skipped: `/`skipping ` marker. **Five** silent self-skips that no marker scan could see now emit markers, and a whole-tree sweep confirms none is left. §3.6 — the static guard additionally requires every gate behind a self-skipping selected test to be enabled by the step's job | met |
| Invariant: a closure may not claim more than the evidence supports | §10 and §11 — the macOS behaviour of this head is explicitly not claimed; the Windows result is recorded as observed; C002's conditional status is unchanged | met |
| Verification: `git diff --check`; `cargo fmt --all -- --check`; `cargo check --workspace --all-targets --locked`; `scripts/verify.sh quick`; the named static guards; the named focused test targets; the branch-wide hosted sweep | §4 | met |
| Verification: capture executed counts for every filter, and do not infer platform behaviour that was not run | §4.3 and §5.2 — every filter's executed count is captured, read from the run logs. §5.3 records the platform scope honestly | met |
| Stop condition: stop and report rather than improvise if the fixture audit would require widening Git authorization, if the LSP characterization would require lowering a timeout or widening an exclusion, if a filter control cannot be made to fail on an injected defect, or if the review pack would require reopening a closed milestone | §6.2 — none of the four stop conditions was reached. Every control was made to fail on an injected defect | met |
| Exit condition: the six C001 findings are discharged and C001 can be closed rather than conditionally held | §11 — C001's §10.1–§10.5 are discharged by this record; §10.6 was already assigned to upstream and remains recorded, not fixed | met |

## 3. Production implementation evidence

C004 changes **no production behaviour**. The three subsections below record
the work that is not production code, plus the five defects this milestone
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

The static side is proved by injecting **ten** distinct tree defects, each
of which must be rejected by the check that owns it: the wrapper removed
from the workflow, the registered command changed, the filter's tests
renamed so it selects nothing, the security rationale blanked, the registry
naming a test the filter does not select, a single-line wrapper step folded
onto two lines, a block-scalar wrapper step folded back onto one, a
registered command whose whitespace does not survive the shell-to-argv path,
a job that stops enabling a native-qualification gate its step needs, and a
registry that stops declaring one.

The runtime side adds two more cases: a verdict that interleaved onto the
following line must be recognised as a verdict, and a named test that was
never reported must still be reported as missing.

#### 3.4.4 The silent self-skips

Four self-skips used a bare `return;` with no marker, which no marker scan
could see — and a self-skip that reports nothing is reported as **passed**,
not as skipped. Those four now emit `skipped: ` markers:
`src/interactive_process.rs` (1) and
`tests/interactive_process_attach_resume.rs` (3).

### 3.5 Four defects this milestone introduced, caught by the existing guards

They are recorded because a reviewer should not have to find them by hand,
and because two of them are exactly the class of defect this milestone was
supposed to prevent.

All four are defects **this milestone introduced** — they are in the code this
milestone wrote. The two further findings in §3.6 and §3.7 are of a different
kind: they are **pre-existing** defects that this milestone's own controls
*exposed*, and they are counted separately for exactly that reason.

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

**The wrapper stripped the `--` that separates cargo's arguments from the
test binary's.** This one was caught by the **hosted** run `39720061964`
(Linux Git object snapshot CoW qualification), which failed with

```
filter-execution control failed: the command is not a registered security
filter, so nothing can be asserted about what it executed
```

The wrapper removed *every* `--` from the argv it received. The shell passes
the command as `-- <tokens>`, and `argparse`'s REMAINDER keeps that
separator — so exactly one **leading** `--` has to go. But a cargo command
also carries an **interior** `--`, the one that separates cargo's own
arguments from the test binary's arguments. Stripping that one changed the
command, so the registry lookup — which is by the joined command — missed
every one of the 20 entries, and the wrapper refused the step. The same
strip also removed the separator cargo needs to forward `--nocapture`.

The fix removes exactly one leading `--`. A new static check computes the
key the wrapper would look up — the shell's tokens behind a `--`, with one
leading separator removed — and requires it to equal the registered command.
The check is not vacuous: reinstating the pre-fix normalisation makes it
report **all 20** entries, and the self-test adds an injected defect (a
registered command whose whitespace does not survive the shell-to-argv path)
that the guard rejects.

**The wrapper could not see a libtest verdict that interleaved onto the next
line.** This one was caught by the hosted run `39720934488` (Linux C006
protected mount qualification), which failed with

```
filter-execution control failed: the run did not report
trusted_git_status_neutralizes_included_repository_configuration as executed
test result: ok. 20 passed; 0 failed; 0 ignored; 0 measured; 91 filtered out
```

All 20 tests ran and passed. libtest prints `test <name> ... <verdict>`, but
a test whose *process-level* stderr interleaves puts the verdict on a
following line — `git init` writes to the process stderr, which libtest does
not capture:

```
test process::tests::trusted_git_status_neutralizes_included_repository_configuration ... Initialized empty Git repository in /tmp/.tmp9VQoEv/repo/.git/
ok
```

The witness pattern required the verdict on the same line, so a test that
really ran was reported as not executed. The same step would have failed on
the M004 native tier, which runs the same filter.

The witness extraction now also recognises a verdict that landed on a
following line. **The self-test case for it is non-vacuous**, and the first
attempt at it was not: the missing-witness check is skipped when the run
reported on no test at all, so a *lone* interleaved test passed the control
even with the pre-fix extraction. The fixture now carries a cleanly-reported
test beside the interleaved one; with the pre-fix extraction reinstated the
control reports `the run did not report some_test as executed`, and with the
fix it reports nothing.

**The guard was also made an order of magnitude faster.** `self_skipping_tests`
rescanned every declared test name against every marker-bearing file *per
registry entry*: 47 s for one `check_registry` call and **5m30s** for the
self-test, which both `scripts/verify.sh quick` and CI run. The marker-name
set is now computed once per source root, and the parsed scan is memoised by
the source root's **content** rather than its path, so the self-test's copies
share it. **47 s → 4.6 s** and **5m30s → 5.8 s**, with an unchanged result:
the marker attribution stays deliberately conservative, so a name mentioned in
any marker-bearing file still counts.

### 3.6 The M004 native job was qualifying 9 of its 11 protected-mount regressions

This one was caught by the hosted M004 run `39722365986`, and it is the
fifth of this milestone's findings, because the milestone's own control
exposed a **pre-existing** false green rather than one of its own making.
§3.7 is the sixth, and is pre-existing for the same reason.

The M004 `process-ipc` job's "Protected mount and Git regressions" step runs
the `protected_mount_` filter, which selects **11** tests. Two of them — the
real managed-process handoff and the PTY first hop — are behind the
`CODEGG_RUN_C006_BWRAP_NATIVE` gate, and the M004 job does not enable it: it
enables `CODEGG_RUN_C009_BWRAP_NATIVE`, `CODEGG_RUN_M004_BWRAP_NATIVE`,
`CODEGG_RUN_M004_CARGO_NATIVE`, `CODEGG_RUN_M004_PTY_NATIVE` and
`CODEGG_RUN_M005_OFFLINE_NATIVE`. So those two tests skipped there, and the
step reported `11 passed` while qualifying **9**.

One of the two skipped **silently** — `src/interactive_process.rs`'s
`protected_mount_pty_first_hop_preserves_terminal_and_workspace_write` had a
bare `return;` with no marker — so nothing reported it. The other emitted a
marker, and the runtime control caught it:

```
filter-execution control failed: the run emitted 1 self-skip marker(s); a
self-skip is not a skip-free run: skipped: set CODEGG_RUN_C006_BWRAP_NATIVE=1
for native Bubblewrap qualification
```

**Three things were fixed.**

1. The silent skip now emits a marker. A whole-tree sweep — every
   environment-guarded `return` in `src/`, `tests/` and `crates/` — found
   this one remaining site; the other four Class C silent skips were already
   converted in this milestone.
2. The M004 job now enables `CODEGG_RUN_C006_BWRAP_NATIVE`. The job already
   installs Bubblewrap and enables user namespaces — **the same setup the
   C006 job has** — so the gate's preconditions hold, and the step now
   qualifies all 11 of the regressions it names instead of 9. The extra cost
   is the two tests' own runtime, which the C006 job already runs in 0.68 s
   alongside the other nine.
3. A **static** control now catches this class before a hosted run can. The
   registry records the native-qualification gates each step's job enables,
   and the guard requires that every gate behind a self-skipping selected
   test is one of them — and that every gate the registry declares is
   actually set by that job. The gate is read from the **test's own
   function body**, not from its file, because a file can hold tests behind
   different gates.

The static check is scoped to the step's **job**: a gate set on a different
job does not enable anything for this one, and a workflow-level `env:` does
apply to every job.

**Non-vacuity.** Reinstating the pre-fix state — the C006 gate absent from
the M004 job — makes the guard report exactly this entry:

```
.github/workflows/linux-m004-native.yml::process-ipc::Protected mount and Git
regressions::cargo test --locked --lib protected_mount_ -- --nocapture: the
registry declares the gate CODEGG_RUN_C006_BWRAP_NATIVE, but the workflow does
not set it, so the step could not run what it claims to qualify
```

The self-test injects this defect two ways — removing the gate from the job,
and removing it from the registry — and each is rejected by the check that
owns it.

**What this finding is worth.** Before this milestone, a native qualification
step could report a green run having qualified only part of what its filter
selected, and nothing in the repository could see it. That is precisely the
failure C001 §10.3 named, and it was still live in the M004 tier when this
milestone started.

### 3.7 A pre-existing cancel/spawn race, exposed by this milestone's gate fix

This is the second pre-existing defect this milestone's own work exposed, and
like §3.6 it is a **false green** class finding: the test the M004 job now
runs could fail for a reason that has nothing to do with the behaviour it
claims to prove.

**How it surfaced.** The hosted M004 run `39753981468` on head `5b7ab4887`
failed with:

```
thread 'managed_process::tests::protected_mount_view_real_managed_process_handoff'
panicked at src/managed_process.rs:4312:14:
cancelled child is reaped: CancelledBeforeSpawn
test result: FAILED. 10 passed; 1 failed; 0 ignored; 0 measured; 7355 filtered out
```

The other ten `protected_mount_` regressions passed, and the step failed —
which is the filter-execution control working: it refused to report a green
run over a failed test.

**Why C004 surfaced it.** §3.6's fix added `CODEGG_RUN_C006_BWRAP_NATIVE: '1'`
to the M004 job. That gate is what had been keeping
`protected_mount_view_real_managed_process_handoff` from running in this job
at all — before §3.6's fix it skipped there. So the milestone that made the
M004 tier qualify **all eleven** of its regressions also made this one run for
the first time, and its race became visible.

**The race.** The test's cancellation scenario spawned the managed child and
then slept a fixed **200 ms** before cancelling:

```rust
let task = tokio::spawn(ManagedProcessService::run(cancel_request));
tokio::time::sleep(Duration::from_millis(200)).await;
cancellation.cancel();
```

A wall-clock deadline is not a synchronisation point. The launch's sandbox
setup and spawn are **not bounded**, and this test runs concurrently with the
other `protected_mount_` regressions — eleven multi-threaded runtimes on a
four-CPU runner. A cancellation that lands *inside* the launch sequence makes
the launch fail with a launch error instead of reaping a cancelled child, so
the test fails at `.expect("cancelled child is reaped")` **without ever
exercising the cancellation-cleanup path it exists to prove**. Depending on
where the cancel landed, the launch reported `CancelledBeforeSpawn` (before
the launch's own cancellation check) or a sandbox failure (after it).

**The fix removes the deadline rather than widening it.** The child script now
creates a `cancel-started` marker before it sleeps, and the test waits for that
marker before cancelling:

```rust
wait_for_marker(&cancel_started_marker, Duration::from_secs(60)).await;
cancellation.cancel();
```

The precondition is now **observed** rather than assumed. No timeout was
lowered, no assertion was removed, and the production behaviour is untouched:
`ManagedProcessError::CancelledBeforeSpawn` still means exactly what it
means, and the whole change is inside the `#[cfg(test)]` module.

It also **strengthens** what the assertions mean. The `cancel-started` marker
proves the child really was running and could write to the workspace when the
cancellation landed, so the reaped result proves the cancellation stopped a
**live** child rather than merely that nothing was started. The old
`assert!(!cancel_marker.exists())` could pass vacuously if the workspace had
been unwritable; it can no longer.

**The diagnosis was proved, not assumed.**

* **The failure mode is reproducible deterministically.** With the pre-fix
  code and the sleep shortened from 200 ms to 0 ms — so the cancellation
  provably lands before the spawn — the test fails at **the same panic site**,
  `src/managed_process.rs:4312:14: cancelled child is reaped: …`. The error
  variant differs only in how far into the launch the cancellation landed.
  That is the hosted failure, reproduced on purpose.
* **The new wait is load-bearing.** With the `cancel-started` marker removed
  from the child's script, the fixed test fails with
  `timed out waiting for the managed child to start: …/cancel-started was never
  created`. The fixed test cannot pass unless the child actually ran.
* **The fix holds.** With the fix in place the whole filter passes locally
  under the native gate — 11 passed, 0 failed, in 0.82 s, faster than the
  pre-fix 0.98–1.02 s because the cancellation now lands as soon as the child
  starts rather than after a fixed wait.

The race could not be reproduced by looping the hosted filter condition
locally — 15 runs at full CPU and 12 more pinned to two CPUs all passed — so
the deterministic reproduction above, rather than a local flake hunt, is the
evidence for the diagnosis.

**Scope.** This is the one source file outside the milestone's original
boundary, and it is touched **only inside its test module**. No production
behaviour changed. The review pack's chunk 9 is updated to include it.

**The fix is validated on the runner that failed it.** The hosted M004 run
`39763779281` on the fix head `99f15e180` reports the `protected_mount_`
filter at **11 executed, 11 witnessed, no self-skip marker** — the same
filter that reported `10 passed; 1 failed` before the fix. Every other
filter count on that head is identical to the earlier green heads, and the
CI sweep passed 16,061 tests.

### 3.8 What the wrapper refuses on a host without native qualification

Two of the registered filters — `protected_mount_` and the M004 native
filters — are **native qualification** filters: their tests self-skip unless
the host is natively qualified. On this workstation the wrapper **rejects**
those steps, and that is the control working as intended: a native
qualification step that skipped is exactly the false green the control exists
to prevent.

The qualified runners set the gates — `CODEGG_RUN_C009_BWRAP_NATIVE=1`,
`CODEGG_RUN_M004_PTY_NATIVE=1`, `CODEGG_RUN_M004_RESTRICTED_USERNS=1` — so
the tests run there and no marker is emitted. With
`CODEGG_RUN_M004_RESTRICTED_USERNS=1` the restricted-userns PTY step passes
locally: `1 test(s) executed, 1 registered test(s) witnessed, no self-skip
marker emitted`.

This is recorded rather than smoothed over: the local rejection is a
property of this host, not a defect, and the hosted native tier is the
evidence that the qualification actually happened.

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
| `python3 scripts/check_test_filter_execution.py --self-test` | 10 injected defects, each rejected |

**The gate was re-run on the merged tree, not carried over.** `main` advanced
under this branch (§5.0) and brought Rust source changes with it, so the
quick gate was executed again on the tree that `259794bb9` produces, not on
the pre-merge tree the gate first passed on. It reports **Quick verification
passed** there: every guard green, `cargo check --workspace --all-targets
--locked` clean, and the filter-execution control verifying all 20 registered
filters with its self-test rejecting all 10 injected defects.

### 4.2 The focused test targets

| Target | Result |
|---|---|
| `cargo test --locked --lib security::workflow::report` | **56 passed**, 0 failed (0.23 s) |
| `cargo test --locked --lib tool::bash::` | **117 passed**, 0 failed (2.36 s) |
| `cargo test --locked --test command_routing_execution_ownership` | **21 passed**, 0 failed (9.62 s) |
| `cargo test --locked --test git_fixture_isolation` | **2 passed**, 0 failed, 1 ignored (the child half) |
| `cargo test -p codegg --locked --features lsp-real-server-tests --lib tool::lsp::` | **38 passed**, 0 failed (34.47 s) |
| `CODEGG_RUN_C006_BWRAP_NATIVE=1 cargo test --locked --lib protected_mount_ -- --nocapture` | **11 passed**, 0 failed (0.82 s) — the native qualification filter, under the gate §3.6 enabled |

The last row runs the **native** qualification filter on a host with
Bubblewrap and unprivileged user namespaces available, which is what §3.7's
fix needed to be verified against. Its two companion runs are the
deterministic reproduction and the non-vacuity proof recorded in §3.7: the
pre-fix code with a 0 ms wait fails at the same panic site, and the fixed
code with the child's start marker removed fails with
`timed out waiting for the managed child to start`.

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

**Code-bearing head:** `99f15e180d2b84dc04df506bb3bb6a89efe90bb6` — the
merge commit `259794bb9` (this branch merged with `main`@`47f5572b1`) plus
§3.7's fix.
**Branch:** `plans/linux-sandbox-c004-evidence-and-review-pack`.
**Pull request:** #159.

**How the attribution terminates.** The sweep below is measured on the
**code-bearing** head. Everything above it on this branch is a
**documentation-only** commit: it touches this record and the sibling review
pack, and moves no source, no test, no workflow and no guard. So the sweep
qualifies the branch tip's code file for file, and no further attribution
commit is needed when a documentation-only commit lands on top. This is the
same property the C001 line's baseline claims, and it is verified by
`git diff --name-only` rather than asserted.

Concretely: the head `d85c402a9` is `259794bb9` plus one documentation-only
commit, and its own sweep is green on all five workflows (`39748205888`,
`39748205822`, `39748205948`, `39748205831`, `39748205884`). The head
`5b7ab4887` is that plus two more documentation-only commits; four of its
five workflows were green and the M004 native job failed on §3.7's race,
which its re-run then passed — a pre-fix tree, so the race is intermittent
and load-dependent rather than deterministic on that runner. The head
`99f15e180` adds §3.7's fix and is green on all six workflows, including the
M004 native tier that failed before the fix.

### 5.0 The base moved under this branch

`main` advanced from `95bb9c5ae` (the C002 merge, PR #158) to `47f5572b1`
while this milestone was in flight, bringing the M008 blackboard work and a
TUI change set with it. A `pull_request` run builds the **merge** commit —
this branch's head merged into the current `main` — so the sweep on a head
does not measure that head alone; it measures the head *plus whatever `main`
has become*.

That is why the CI count moves from **16,031** to **16,061** between the runs
below. The 30 extra tests are M008's, not this milestone's: 18 in
`blackboard_trajectory_m008`, 9 in `blackboard_economics_m008`, and 3
`m008_peer_observation_contract` tests in `agent_loop_harness`. Diffing the
two runs' per-binary test sets confirms it exactly — the `codegg` binary's own
7,362 test names are identical in both, and the delta is precisely those 30.

The branch therefore merges `main` explicitly, and the merge is **textually
clean**. `main` and this branch both touched `scripts/verify.sh` and
`.github/workflows/ci.yml`, so the merge is *verified* rather than assumed:
after it, this milestone's filter-execution control and `main`'s new
blackboard contract-matrix guard are both still wired into both files, and
both guards and both self-tests pass on the merged tree.

The merge commit `259794bb9` has tree `d4ae08f6`, which is **byte-identical**
to the tree of GitHub's merge commit `22630735a` — the exact tree the green
CI run `39740206655` (attempt 2) executed. The hosted evidence qualifies the
merged tree's code exactly, file for file.

### 5.1 The hosted runs

The sweep was run on successive heads of the branch, because the sweep itself
found three of the five defects in §3.5 and §3.6 — two of them in §3.5 and the
fifth in §3.6. The other two were caught locally, before a hosted run ever
executed them. The intermediate heads are recorded rather than hidden: they are
the evidence that the sweep is doing work.

| Run | Head | Workflow | Result |
|---|---|---|---|
| `39720061964` | `a6e596642` | Linux Git object snapshot CoW qualification | **failure** — the wrapper refused the step; §3.5 defect 3 |
| `39720934488` | `8aebb0b38` | Linux C006 protected mount qualification | **failure** — the wrapper could not see a witness; §3.5 defect 4 |
| `39722365986` | `e3685b568` | Linux M004 process and IPC qualification | **failure** — the wrapper caught a self-skip in the protected-mount step; §3.6 |
| `39731707246` | `18bffb339` | `CI` — the default sweep | **success** — 16,031 tests run, 16,031 passed, 9 skipped |
| `39731707315` | `18bffb339` | Linux M004 process and IPC qualification | **success** — `restricted-userns-fails-closed` **success**, `process-ipc` **success**, `git-cost-pilot` skipped |
| `39731707200` | `18bffb339` | Linux C006 protected mount qualification | **success** |
| `39731707258` | `18bffb339` | Linux Git object snapshot CoW qualification | **success** |
| `39731707127` | `18bffb339` | Windows process launcher | **success** |
| `39735856172` | `fa8dbf413` | `CI` — the default sweep | **success** — 16,031 tests run, 16,031 passed, 9 skipped |
| `39735856365` | `fa8dbf413` | Linux M004 process and IPC qualification | **success** — `restricted-userns-fails-closed` **success**, `process-ipc` **success**, `git-cost-pilot` skipped |
| `39735856175` | `fa8dbf413` | Linux C006 protected mount qualification | **success** |
| `39735856194` | `fa8dbf413` | Linux Git object snapshot CoW qualification | **success** |
| `39735856171` | `fa8dbf413` | Windows process launcher | **success** |
| `39740206655` | `f82c1f980` | `CI` — the default sweep | **failure** — the hosted runner was terminated mid-sweep; §5.1.1 |
| `39740206800` | `f82c1f980` | Linux M004 process and IPC qualification | **success** |
| `39740206612` | `f82c1f980` | Linux C006 protected mount qualification | **success** |
| `39740206639` | `f82c1f980` | Linux Git object snapshot CoW qualification | **success** |
| `39740206635` | `f82c1f980` | Windows process launcher | **success** |
| `39740206655` (attempt 2) | `f82c1f980` | `CI` — the default sweep | **success** — 16,061 tests run, 16,061 passed, 9 skipped |
| `39748205888` | `d85c402a9` | `CI` — the default sweep | **success** — 16,061 tests run, 16,061 passed, 9 skipped |
| `39748205822` | `d85c402a9` | Linux M004 process and IPC qualification | **success** — `restricted-userns-fails-closed` **success**, `process-ipc` **success**, `git-cost-pilot` skipped |
| `39748205948` | `d85c402a9` | Linux C006 protected mount qualification | **success** |
| `39748205831` | `d85c402a9` | Linux Git object snapshot CoW qualification | **success** |
| `39748205884` | `d85c402a9` | Windows process launcher | **success** |
| `39753981468` | `5b7ab4887` | Linux M004 process and IPC qualification | **failure** — the `protected_mount_` step failed on a cancel/spawn race in one of its own regressions; §3.7 |
| `39753981468` (attempt 2) | `5b7ab4887` | Linux M004 process and IPC qualification | **success** — the same tree, re-run; the race is intermittent and load-dependent; §3.7 |
| `39763779271` | `99f15e180` | `CI` — the default sweep | **success** — 16,061 tests run, 16,061 passed, 9 skipped |
| `39763779281` | `99f15e180` | Linux M004 process and IPC qualification | **success** — the §3.7 fix, qualified on the runner that failed it |
| `39763779119` | `99f15e180` | Linux C006 protected mount qualification | **success** |
| `39763779280` | `99f15e180` | Linux Git object snapshot CoW qualification | **success** |
| `39763779264` | `99f15e180` | Windows process launcher | **success** |
| `39763779109` | `99f15e180` | Shared checkout macOS qualification | **success** — `main`'s M005 shared-checkout lane, pulled in by the §5.0 merge; see §5.3 |

**All five milestone workflows are green on the closing head `99f15e180`**, and
so is the sixth, `main`'s macOS shared-checkout lane. The failures above are
the earlier heads, and each one found a defect that is now fixed and guarded —
they are recorded here rather than hidden, because they are the evidence that
the sweep is doing work. Two of them are of a different kind and are recorded
separately: the CI failure on `f82c1f980` found no defect at all (§5.1.1), and
the M004 failure on `5b7ab4887` found a **pre-existing** race that this
milestone's own gate fix had exposed (§3.7) — not a defect this milestone
introduced.

### 5.1.1 The CI failure on `f82c1f980` was a runner termination, not a test result

This is recorded because a reader of the PR will see a red CI check, and
because "the sweep found a defect" is a claim that must not be made about
every red run.

The run's own log ends with:

```
##[error]The runner has received a shutdown signal. This can happen when the
runner service is stopped, or a manually started runner is canceled.
##[error]Process completed with exit code 143.
```

Exit code 143 is `SIGTERM` from the runner service, not a test exit. The
evidence that no test failed:

* **No `FAIL`, no panic, no assertion failure anywhere in the log.** The
  16,061-test sweep never reported a result; the runner was killed mid-run.
* All **50 preceding steps succeeded** — every guard, `Formatting`, and
  `Workspace Clippy`.
* The sweep step began at 07:31:43 and the runner was killed at 07:42:45,
  about 11 minutes in. The previous green CI run's test phase took 539 s, so
  the run died as the sweep was finishing.
* The four native workflows on the same commit all succeeded.
* The commit differed from the last green CI head by **two markdown files**.

The remedy was to re-run the failed job on the same head
(`gh run rerun --failed`, attempt 2), which passed 16,061 tests. That re-run
is the result for this head, and it is what the merge commit's tree identity
above is measured against.

The four native workflows, the CoW tier, and the Windows job were **not**
re-run: they had already succeeded on that commit, and re-running them would
have measured nothing new.

The macOS target compile was **not run** on this head; see §5.3.

### 5.2 The executed counts

Every count below is the wrapper's own line, read from the run log — not an
estimate, and not a count the guard assumed.

| Run | Workflow | Step | Executed | Witnesses | Self-skip marker |
|---|---|---|---|---|---|
| `39731707246` | `CI` | `Workspace tests` | **16,031 tests run, 16,031 passed**, 9 skipped (1 slow) | — | none |
| `39731707315` | M004 | 7 single-test filter steps | **1 each** | **1 each** | none |
| `39731707315` | M004 | Protected mount and Git regressions — `protected_mount_` | **11 executed** | **11** | none |
| `39731707315` | M004 | Protected mount and Git regressions — `trusted_git` | **20 executed** | **17** | none |
| `39731707200` | C006 | ManagedProcessService protected-mount qualification | **11 executed** | **11** | none |
| `39731707200` | C006 | Trusted Git repository-config qualification | **20 executed** | **17** | none |
| `39731707258` | Git CoW | Prove CoW snapshot isolation from source mutation | **1 executed** | **1** | none |
| `39731707127` | Windows | 6 dispatchable filter steps | **1, 3, 1, 6, 5, 6 executed** | 1, 3, 1, 6, 5, 6 | none |
| `39735856172` | `CI` | `Workspace tests` | **16,031 tests run, 16,031 passed**, 9 skipped (1 slow) | — | none |
| `39735856365` | M004 | 7 single-test filter steps | **1 each** | **1 each** | none |
| `39735856365` | M004 | Protected mount and Git regressions — `protected_mount_` | **11 executed** | **11** | none |
| `39735856365` | M004 | Protected mount and Git regressions — `trusted_git` | **20 executed** | **17** | none |
| `39735856175` | C006 | ManagedProcessService protected-mount qualification | **11 executed** | **11** | none |
| `39735856175` | C006 | Trusted Git repository-config qualification | **20 executed** | **17** | none |
| `39735856194` | Git CoW | Prove CoW snapshot isolation from source mutation | **1 executed** | **1** | none |
| `39735856171` | Windows | 6 dispatchable filter steps | **1, 3, 1, 6, 5, 6 executed** | 1, 3, 1, 6, 5, 6 | none |
| `39740206655` (attempt 2) | `CI` | `Workspace tests` | **16,061 tests run, 16,061 passed**, 9 skipped | — | none |
| `39740206800` | M004 | 7 single-test filter steps | **1 each** | **1 each** | none |
| `39740206800` | M004 | Protected mount and Git regressions — `protected_mount_` | **11 executed** | **11** | none |
| `39740206800` | M004 | Protected mount and Git regressions — `trusted_git` | **20 executed** | **17** | none |
| `39740206612` | C006 | ManagedProcessService protected-mount qualification | **11 executed** | **11** | none |
| `39740206612` | C006 | Trusted Git repository-config qualification | **20 executed** | **17** | none |
| `39740206639` | Git CoW | Prove CoW snapshot isolation from source mutation | **1 executed** | **1** | none |
| `39740206635` | Windows | 6 dispatchable filter steps | **1, 3, 1, 6, 5, 6 executed** | 1, 3, 1, 6, 5, 6 | none |
| `39748205888` | `CI` | `Workspace tests` | **16,061 tests run, 16,061 passed**, 9 skipped | — | none |
| `39748205822` | M004 | 7 single-test filter steps | **1 each** | **1 each** | none |
| `39748205822` | M004 | Protected mount and Git regressions — `protected_mount_` | **11 executed** | **11** | none |
| `39748205822` | M004 | Protected mount and Git regressions — `trusted_git` | **20 executed** | **17** | none |
| `39748205948` | C006 | ManagedProcessService protected-mount qualification | **11 executed** | **11** | none |
| `39748205948` | C006 | Trusted Git repository-config qualification | **20 executed** | **17** | none |
| `39748205831` | Git CoW | Prove CoW snapshot isolation from source mutation | **1 executed** | **1** | none |
| `39748205884` | Windows | 6 dispatchable filter steps | **1, 3, 1, 6, 5, 6 executed** | 1, 3, 1, 6, 5, 6 | none |
| `39753981468` (attempt 2) | M004 | 4 single-test filter steps | **1 each** | **1 each** | none |
| `39753981468` (attempt 2) | M004 | Protected mount and Git regressions — `protected_mount_` | **11 executed** | **11** | none |
| `39753981468` (attempt 2) | M004 | Protected mount and Git regressions — `trusted_git` | **20 executed** | **17** | none |
| `39763779271` | `CI` | `Workspace tests` | **16,061 tests run, 16,061 passed**, 9 skipped | — | none |
| `39763779281` | M004 | 7 single-test filter steps | **1 each** | **1 each** | none |
| `39763779281` | M004 | Protected mount and Git regressions — `protected_mount_` | **11 executed** | **11** | none |
| `39763779281` | M004 | Protected mount and Git regressions — `trusted_git` | **20 executed** | **17** | none |
| `39763779119` | C006 | ManagedProcessService protected-mount qualification | **11 executed** | **11** | none |
| `39763779119` | C006 | Trusted Git repository-config qualification | **20 executed** | **17** | none |
| `39763779280` | Git CoW | Prove CoW snapshot isolation from source mutation | **1 executed** | **1** | none |
| `39763779264` | Windows | 6 dispatchable filter steps | **1, 3, 1, 6, 5, 6 executed** | 1, 3, 1, 6, 5, 6 | none |

**Every security-filter count is identical on all four green heads**: the same
executed count, the same witness set, and the same absence of self-skip markers
on every job. That is the reproducibility claim made concrete, and it holds
exactly where it is claimed — on the filter steps, which are this milestone's
own surface.

The **CI** count is the one that moves, from 16,031 to 16,061, and §5.0
explains why: the base moved, not the code. The 30 added tests are M008's.
The count is not a property of this milestone and is not claimed as one.

The M004 protected-mount step's **11 executed, 11 witnessed, no marker** is
the fix in §3.6 working: before it, the step reported `11 passed` while
qualifying **9**, because two of the eleven sat behind a gate the job did not
enable and one of them skipped silently.

The Windows job's **8** registered filters run in **6** steps on a
`pull_request`: the WRITE_RESTRICTED feasibility probe and the C044 token-flag
differential are gated on `workflow_dispatch` with
`inputs.run_bounded_successor_probes`, so they did not run here. That is the
step's own `if:` condition, not a skip the wrapper tolerated.

The C006 job's `trusted_git` step reports **17** witnesses against **20**
executed: the filter selects 20 tests and the registry names the 17 it
requires. The registry's list is the *required* set, not the exhaustive one.

### 5.3 The platform scope, stated honestly

* The **Windows** process-launcher job was run and **passed** on the
  corrective head (`39748205884`, and again on the three preceding heads
  `39740206635`, `39735856171` and `39731707127`). The final result is
  recorded from the run rather than inferred.
* The **macOS** target compile was **not run** on this head. It is recorded
  as *not run*, not as a pass. The workflow that runs it
  (`command-authorization-macos-compile.yml`, one-shot) is triggered by
  `workflow_dispatch` or a push to one of two *named* branches —
  `planning/linux-sandbox-next-admission-and-adr-cleanup` and
  `planning/linux-execution-isolation` — and this branch is neither, so it
  cannot run here without a manual dispatch. The **macOS shared-checkout
  qualification** (`39763779109`) is a *different* workflow from `main`'s
  M005 line, it did run on this head, and it passed; it qualifies the shared
  checkout profile, not this milestone's boundary.
* The plan's WP5 asks to "await the native and macOS jobs only for the
  boundaries the ledger marks as needing them". The C004 head changes
  `src/interactive_process.rs`, which the M004 native tier's trigger list
  names, so the **M004 native tier is awaited and its result is recorded** —
  including the failure that found §3.6 and the failure that found §3.7. The
  C006 protected-mount tier and the Git CoW tier are awaited for the same
  reason: the head changes the workflow files that own them.

**No platform behaviour is inferred from a job that was not run.**

## 6. Invariant review

### 6.1 The plan's four invariants

| Invariant | How it was held |
|---|---|
| No Git subcommand authorization may be widened | No Git authorization code was touched. The rebound tests still assert the typed `BackendUnavailable[RepositoryAlias]` refusal |
| No timeout may be lowered, and no test may be disabled or silently excluded | No timeout was lowered — the 120 s nextest bound, the 30 s LSP request timeout and the 30 s readiness timeout are unchanged. No test was excluded; the only `#[ignore]` is the harness half of the new reproduction, invoked by name. §3.7 replaces a fixed 200 ms *sleep* with a bounded 60 s *wait on an observed precondition* — that is not a lowered timeout but the removal of a timing assumption, and it removes no assertion |
| A filter that matches zero tests is not a pass, and a self-skip marker is not a skip-free run | The runtime wrapper rejects a zero count and any self-skip marker. **Five** silent self-skips that no marker scan could see now emit markers, and a whole-tree sweep confirms none is left. The static guard additionally requires every gate behind a self-skipping selected test to be enabled by the step's job — which is what the M004 tier was missing |
| A closure may not claim more than the evidence supports | §5.3 records the macOS scope as *not run* and the Windows result as it was observed. §10 lists what is still open with its owner. §3.7 records the hosted M004 failure that this milestone's own gate fix exposed, and the evidence for the diagnosis rather than a plausible story about it |

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
  why the five silent self-skips were converted, and why a whole-tree
  sweep now confirms that none is left.
* **A fixture that falls back to a shared path** is caught by
  `tests/git_fixture_isolation.rs`, proved against the pre-fix behaviour.
* **A workflow step that no longer runs the registered command** is caught
  by the static guard's command check and, for the folded-scalar class, by
  the new folded-scalar check.
* **The LSP fixture that produces no diagnostic** now costs the full 30 s
  readiness budget rather than the whole-checkout index. The failure mode is
  understood, so a future regression is a slow cold call, not a mystery.
* **A cancellation issued on a wall-clock deadline** now waits for an
  observed precondition instead. §3.7's fix replaces the assumption with a
  marker the child itself creates, so the cancellation path is reached
  deterministically rather than whenever the launch happens to be fast
  enough.

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

### 10.1 The macOS target compile of this head (severity: low, owner: this milestone's reviewer)

The **Windows** process-launcher job was run and **passed** on the corrective
head (`39748205884`, and again on `39740206635`, `39735856171` and
`39731707127`), so the seven block-scalar conversions and the interior
`--` fix are both proven on the platform that needed them.

The **macOS** target compile was **not run** on this head. It is recorded as
*not run*, not as a pass. The C004 head does not change any macOS-visible
source — it changes `src/interactive_process.rs`, `src/tool/bash.rs`,
`src/tool/lsp.rs`, `src/security/workflow/report.rs` and the workflows — and
the macOS compile's trigger list does not name any of them, so no macOS
qualification was required by this milestone's boundary ledger.

**Exact future evidence required.** The macOS target compile's conclusion on
this head, recorded as a link, if a reviewer wants the platform's compile
result on this tree.

### 10.2 The pre-existing Desktop E2E failure (severity: low, owner: upstream)

`apps/desktop/e2e/specs/m004-session.e2e.ts:169` fails on the
`[data-testid="permission-list"]` assertion after 120 s. It reproduces on
`main` before this milestone and is not attributable to this branch. It is
C001's §10.6, carried forward unchanged: recorded, not fixed here.

### 10.3 The Windows runtime path of the wrapper was not locally validated (severity: low, owner: this milestone's reviewer)

The wrapper is pure Python 3 standard library with no `tomllib`
dependency — it carries its own narrow TOML reader — so it has no Python
version floor. The Windows runner's interpreter is probed as `python3` and
then `python`, following the repository's own precedent
(`.github/workflows/windows-native-probe-contract.yml`). The wrapper fails
loudly (non-zero exit) rather than silently.

This was **not** validated locally — this workstation is Linux — but it was
validated on the platform: the Windows process-launcher job passed on the
corrective head with all six dispatchable filter steps reporting their
executed counts and witnesses, so the probe and the block-scalar form both
work on the Windows runner. What remains unverified on Windows is the two
`workflow_dispatch`-only filters, which do not run on a pull request.

## 11. Roadmap disposition

* **C001** — **closed**. Its §10.1–§10.5 are discharged by this record.
  §10.6 was already assigned to upstream and is carried forward in §10.2,
  recorded and not fixed here.
* **C002** — unchanged: **conditionally closed**, on the maintainer-owned
  admission record and the residual cargo attribution. C004 changed nothing
  about it.
* **C003** — untouched, and still blocked behind ADR-0044 acceptance, the
  C004 production vertical *in the M005 writer-containment line* and the M005
  production owner qualification. Nothing in this milestone changes those
  dependencies: C004 in *this* line is the evidence and review-pack
  milestone, and it is not the C004 those dependencies name.
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
Windows and macOS jobs. The Windows job was awaited in the end, because the
head changed the Windows workflow itself and its result was needed to
qualify the change; the macOS compile was not.

What **was** run locally: the canonical quick gate in full, workspace clippy
with `-D warnings`, the four focused test targets, the LSP trio under its
feature, every named static guard with its self-test, and the
Linux-capable registered filters run through the wrapper itself — including
the two that need the `CODEGG_RUN_M004_RESTRICTED_USERNS` gate. What **was
not** run locally is named in §10 and in §5.3 with the exact evidence that
would close it, and the hosted sweep in §5 is what closed it.
