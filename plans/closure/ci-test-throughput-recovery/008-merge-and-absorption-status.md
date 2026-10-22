# CI and Test Throughput Recovery — Merge and Absorption Status

Status: closed

Record kind: **supplemental reconciliation record**. This is not a new
milestone of the recovery line and it opens no new milestone. It records the
reconciliation of the recovery branch with `origin/main` and the absorption of
the integration tests `main` carries into the branch's family topology.

Source subsystem roadmap:

- `plans/subsystems/ci-test-throughput-recovery-roadmap.md`

Historical closure records this record supplements and does **not** rewrite:

- `plans/closure/ci-test-throughput-recovery/001-status.md` (M001, closed)
- `plans/closure/ci-test-throughput-recovery/002-status.md` (M002, closed)
- `plans/closure/ci-test-throughput-recovery/003-status.md` (M003, closed)
- `plans/closure/ci-test-throughput-recovery/004-status.md` (M004, closed)
- `plans/closure/ci-test-throughput-recovery/005-status.md` (M005, closed)
- `plans/closure/ci-test-throughput-recovery/006-status.md` (M006, closed)
- `plans/closure/ci-test-throughput-recovery/007-status.md` (M007, closed)

Repository baselines reviewed: `f4019a410` (the fifth merge), `7d3c6ed09`
(the sixth, §2.7) and `726f3c645` (the seventh, §2.10)

Implementation commits or pull requests:

- `54715a41e` — merge(main): reconcile the CI/test-throughput recovery line
  with origin/main
- `<this record's commit>` — feat(ci): absorb main's integration tests into
  the recovery families

## 1. Executive finding

The branch is **merged with `origin/main` and absorbed**, and the merged tree
compiles, passes the repository's guard tier, and lands with no loss of
coverage. What the merge had to reconcile was not two *values* but two
*shapes*: `origin/main` never received M004's family consolidation and still
carries its root integration tests as top-level `tests/*.rs` files, so the
merge landed **sixteen** new top-level cargo targets that no family owned and
that the ownership manifest could not map.

Thirteen of the sixteen were absorbed into the family that already owned their
subject; three stayed top-level because a hosted qualification lane names
them. At that merge the root package's test-target count fell from **37** to
**24** and the workspace's from **65** to **52** (both re-measured by the
sixth reconciliation, §2.7). No test function was removed, no assertion
was weakened, and every guard the repository runs is green on the merged tree.

`main` then advanced a second time (§2.7) and the line was reconciled with it
again: one more top-level file arrived, and it **stays** top-level — not
because a lane names it, but because it shares a process-global event bus with
the agent-loop harness suites, which drain that bus and assert on what they
find. The two test-target counts move to 25 and 53, and the merge produced a
tree that **did not compile** — a `#[path]` attribute inside a file that had
moved directory. No conflict reported it; `cargo check --workspace
--all-targets` on the merged tree did.

`main` advanced a third time (§2.10) to `726f3c645`, and the line was
reconciled with it a **seventh** time. That advance is documentation only, so
the merge carried no code and no coverage: it changed four records, and both
sides' edits to the two files they both touched were verified to survive. The
seventh merge did surface one thing worth stating plainly, and it is a
*non*-event: a test fixture's implementation changed during the **sixth**
merge, the merge did it automatically, and reading the two implementations
side by side shows the shared helper performs the same operations the inline
copy did.

## 2. What the merge changed

### 2.1 The four textual conflicts

| File | Resolution |
|---|---|
| `AGENTS.md` | The quick-tier guard list is the **union** of both sides: `cargo-profile-alignment`, `test-ownership`, `test-target-references` and `writer-disposition`. |
| `scripts/verify.sh` | The branch split the guard tier into `run_guards` and `run_quick`; `main` added the test-filter-execution guard inside the old flat tier. The resolved file keeps the split and places `main`'s guard inside `run_guards`, beside the others it belongs with. |
| `.github/workflows/linux-m004-native.yml` | `main`'s C008 step and path triggers are kept. The branch's rename of the M004 process/IPC target (`linux_m004_process_ipc` → `linux_git_test_support`) wins over `main`'s reference to the old name, because the old name no longer names a cargo target. `main`'s `check_test_filter_execution.py --run` wrapper is kept on the renamed steps. |
| `architecture/overview.md` | Both sides' reconciliation narratives are kept: each records a dated pass over its own tree, and neither is invalidated by the other. The Verified-counts table is re-measured on the merged tree. |

### 2.2 The absorption

Thirteen files moved; the mapping and the three that stayed are in
`architecture/testing.md` § "The families after the fifth merge (2026-10-22)".

### 2.3 Repairs the merge exposed

These are not cosmetic. Each was a **real defect** that the merge surfaced,
and each is recorded here rather than hidden.

| Defect | Where | Repair |
|---|---|---|
| A hosted workflow step named a cargo target that does not exist: `--test linux_m004_process_ipc` (renamed to `linux_git_test_support` by M004) | `.github/workflows/linux-m004-native.yml` | Repointed at the family, keeping `main`'s `check_test_filter_execution.py --run` wrapper. |
| A hosted workflow step named a cargo target that does not exist: `--test sandbox_containment` (absorbed into the `security_permission` family by M004) | `.github/workflows/shared-checkout-macos-qualification.yml` | Repointed at the family with the member name as the filter, so the lane's scope stays narrow. |
| The test-filter-execution guard could not see a family target: it resolved `--test NAME` to `tests/NAME.rs` and globbed only `*.rs`, so a family (a directory of `.inc` members) reported "selects zero declared tests" | `scripts/check_test_filter_execution.py` | The guard now resolves a family target to its directory, scans `*.inc` members as well as `*.rs`, and strips a `.inc` suffix from a module path. Its `--self-test` now proves the family resolution is not vacuous — it asserts that a family member's test is selected, and that a non-matching filter in the same family is still rejected. |
| The Windows test-support release guard named two `[[test]]` targets that M004 had consolidated into the `windows_sandbox_test_support` family, so it failed on every run | `scripts/check_windows_test_support_release.py` | The guard now checks the family target's `required-features` and scans the family's `.inc` members for the feature gate. It is not in `verify.sh` or CI, so it was failing silently. |
| A member's re-exec used `--exact <bare test name>`, which matches the *full* test path — so it selected nothing and the parent read the empty child report as a failure | `tests/command_git/git_fixture_isolation.inc` | The child is now named by its qualified path, `git_fixture_isolation::child_prints_its_own_process_unique_fixture_path`. |
| A member called `crate::scheduler_admission_service(…)`, which resolved through the standalone file's own crate-root import | `tests/security_permission/process_denial_c005.inc` | The member names the helper directly. |
| A member reached the shared helper with `use common::…`, which only resolves where `tests/common/` is a sibling directory | the five members that bound `mod common;` | Rewritten to `use super::common::…`; the `security_permission` family gained the `#[path = "../common/mod.rs"]` binding it needed. |
| An `#[ignore]`d test wrote inside the repository tree with no opt-in, which is what M006's assertion-quality guard exists to prevent | `tests/command_git/git_fixture_isolation.inc` | The child now carries the repository-writer opt-in (`CODEGG_REGENERATE_ASSETS=1`) and the parent sets it, so a routine `cargo test -- --ignored` writes nothing. |
| A skip marker told a reader to set the very variable whose presence made the test **skip** | `tests/interactive_process_attach_resume/interactive_process_attach_resume.inc` | The message now describes the condition that actually holds. The condition itself is **not** changed: it is deliberate — commit `f4f41a766` made the two constrained-PTY halves mutually exclusive, so the qualified half is covered by `m004_daemon_constrained_pty_uses_principal_profile_and_cancels_descendant`. |
| Eight test bodies that `main`'s CI never saw reported *passed* having skipped, because `main` does not carry M006's assertion-quality guard | `docs/test-assertion-quality.toml` | Triaged into the manifest: seven `[[skip-exceptions]]` entries, each naming the hosted lane that satisfies it, plus the repository-writer entry above. |

The eight assertion-quality failures were **pre-existing at the merge commit**:
the pre-merge branch head `644d6208a` reports **zero** of them, and the merge
commit `54715a41e` reports **eight**. `main`'s CI does not run this guard,
because the guard is the branch's, so `main`'s new tests were never triaged
against it.

### 2.4 Two more defects the hosted runs exposed

The hosted runs are the only place these two appear: neither is reachable
from the local gate, and neither is reachable from `main`.

**The branch had stripped the test functions out of ten files it does not own.**
Commit `4f89b4b05` — the workstream's own 468-file commit — removed **every**
`#[test]` / `#[tokio::test]` attribute from ten files while leaving their
bodies in place, so the files no longer parse:

| File | Test functions stripped |
|---|---:|
| `apps/desktop/src-tauri/src/lib.rs` | 19 |
| `apps/desktop/src-tauri/src/route.rs` | 24 |
| `apps/desktop/src-tauri/src/present.rs` | 12 |
| `apps/desktop/src-tauri/src/projection.rs` | 11 |
| `apps/desktop/src-tauri/src/lifecycle.rs` | 8 |
| `examples/plugins/sdk-rust/tests/sdk_smoke.rs` | 12 |
| `research/windows-native-sandbox/mxc-c035-harness/src/bin/c036.rs` | 6 |
| `research/windows-native-sandbox/two-hop-native-diagnostic/src/main.rs` | 4 |
| `research/windows-native-sandbox/mxc-c035-harness/src/bin/c037.rs` | 1 |
| `research/windows-native-sandbox/mxc-c035-harness/src/main.rs` | 1 |

**98 test functions** were affected. The root workspace was not touched —
`cargo check --workspace --all-targets` compiles every one of those files and
passes — because the ten files are all **outside** it: the desktop crate and
the research crates are excluded workspaces, and the plugin SDK example is a
separate crate. Nothing in the repository's local gate compiles them, which
is why the corruption survived three local verification runs and a hosted
measurement.

The hosted `Desktop E2E` run is what found it, at
`tests/security_permission/../../apps/desktop/src-tauri/src/lib.rs`: `error:
unexpected closing delimiter: }` in the desktop crate's own lib test. The
repair is to restore the ten files from the merge base `2c1953347`, which is
**byte-identical to `origin/main`** for all ten — `main` never changed them.
The 98 test functions are back, and the ten files now differ from `main` by
nothing.

**The `security_permission` family did not compile on Windows.** M004 moved
`sandbox_containment.rs` into the `security_permission` family. On `main` that
file was its own cargo target, which the Windows lane never built; as a
family member it is built by `windows-process-launcher.yml`, and it uses
Unix-only APIs — `Command::pre_exec`, `libc::fcntl`, `libc::dup2`,
`std::os::unix::fs::symlink` — so the hosted Windows lane failed with eight
compile errors before it could run a single test.

The repair is `#![cfg(unix)]` on the member, which is the pattern the same
family already uses for its Linux-only members
(`linux_bubblewrap_first_hop.inc`, `linux_native_scratch_containment.inc`,
`sandbox_landlock.inc`). **No coverage is lost**: on Unix the member is
compiled exactly as before — the family still carries 325 tests on macOS,
16 of them in `sandbox_containment::` — and on Windows the member is excluded,
which is what happened before the families existed.

### 2.5 The Windows lane, and the `--exact` defect behind two red lanes

The hosted Windows native probe contract lane is the one lane that never went
green on this branch. It is worth being precise about **when** it went red,
because the answer is not "the merge broke it".

**The lane was already red before the merge, and `main`'s lane is green.**
`origin/main`'s lane at `f4019a410` is run `39838658024`, `success`: it builds
`--test windows_native_probe_contract` (1 test) and
`--test windows_restricted_managed_process` (6 tests), and its sibling
`windows-process-launcher.yml` run `39838658197` is `success` with
`--test windows_process_launcher` → `8 passed`. Every one of the branch's six
runs of that workflow failed, the earliest being run `39781219609` at
`559eee514` — a **pre-merge** commit — which already failed with the eight
compile errors §2.4 describes.

The cause is the pre-merge workstream commit `4f89b4b05` (M004), not the merge.
That commit repointed both Windows workflows at the whole `security_permission`
family and moved eight standalone targets into it. `git diff 54715a41e^1
54715a41e -- .github/workflows/windows-native-probe-contract.yml` is **empty**:
the merge changed nothing in this lane. What the merge *did* do is add
`#![cfg(unix)]` to `sandbox_containment.inc`, which converted the lane from a
**compile** failure into a **five-assertion** failure — and made the underlying
defects visible for the first time.

Five assertions failed. They are three defects.

**One: a self-re-exec test named its test by a path the family move had
invalidated.** `tests/security_permission/windows_process_launcher.inc` builds
its fixture child with `--exact windows_process_fixture`. When that file was the
standalone target `tests/windows_process_launcher.rs`, the bare name *was* the
full path. As a family member the path is
`windows_process_launcher::windows_process_fixture`, and a bare name matches
**nothing**: cargo prints `0 passed; … 32 filtered out` and exits **0**. The
child therefore did no work and exited successfully, and the parent read an
empty success as evidence. The hosted log shows exactly that child output. Three
of the five failures are this one defect.

The same defect was **actively failing the Linux M004 lane** in
`tests/linux_git_test_support/linux_m004_process_ipc.inc`, where a recovery
worker is re-execed with
`--exact test_only_contained_git_add_stages_index_and_objects_in_private_gitdir`.
The bare name matched nothing, so the worker exited 0 having done no work and
never created the interrupted publisher's `index.lock`. The parent then failed
at `linux_m004_process_ipc.inc:1977` with *"displaced authorized gitdir lost
the interrupted publisher lock"* — run `39850835891`, `18 passed; 1 failed`. The
consequence there is the quieter and worse one: the assertion that reads the
child's exit status as evidence would have accepted an empty success, and the
interrupted-publication **recovery scenario was never exercised at all**. A
third site, `tests/command_git/git_fixture_isolation.inc`, had already been
repaired in this line for the same reason.

The repair is to name each test by its full module path. A new guard,
`scripts/check_test_self_reexec_paths.py`, now rejects a `--exact` argument
that is not the full module path of a test function defined in that same file —
so the next absorption cannot silently gut a test this way again.

**Two: `sandbox_policy_wiring`'s assertion could not be reached.** The test
`unsupported_host_rejects_constrained_command_before_execution` builds
`BashTool::new().with_sandbox_profile(…)` and expects the constrained request to
be refused with "…filesystem containment is unavailable…". It never got there.
`BashTool::new()` carries **no workspace root**, so the managed shell refuses
the call with `MissingWorkspaceRoot` *before* it consults the host capability —
a different refusal from the one the test names. The test therefore asserted
nothing on any host: on macOS the `platform_sandbox_capability()` guard returns
early (macOS has the Seatbelt backend), and on Windows — the only host with no
backend — the tool failed earlier, for the unrelated reason above.

The repair is to give the tool the workspace root it needs, so the refusal
under test is actually the one that happens. The assertion now also reports the
actual error text when it fails, so a future divergence says what it produced.

**Three: C014's Git step aborted the child and discarded every host-denial
probe.** `c014_projection_tool_smoke_and_unprojected_host_denials` writes a
PowerShell child that (1) proves the projection is writable, (2) runs `git init`
in it, and (3) then probes **seven** host paths for denial. The Git step threw
on failure, and under the AppContainer it does fail: Git for Windows opens the
null device (`/dev/null` → `\Device\Null`) and the AppContainer child cannot
open it, so `git init` dies with `fatal: could not open '/dev/null' for
reading and writing: Permission denied`. The throw aborted the child, so steps
(3) — the seven denial probes — **never ran**. The test was failing at step (2)
and silently carrying no coverage of step (3).

The repair makes the Git step report its outcome instead of aborting the child,
and asserts that a Git failure is the *named* null-device limitation — any other
Git failure still fails the test. The seven denial probes now execute, which is
coverage the test was not actually delivering before.

### 2.6 The null-device limitation is a real product gap, not a test artefact

The `NUL` denial is a genuine limitation of the current AppContainer
projection, and this record does **not** claim it away. The C019 probe's own
report shows it: the `null_device_access` observation is `state: denied`,
`expected: allowed`, and the same report shows `git_init`, `git_status`,
`python_version` and `cargo_version` all `state: failed` where `expected:
allowed`. The probe *reports* those states; it does not enforce its `expected`
field for them, which is why the C019 test passes with them failing.

Granting `\Device\Null` to an AppContainer child means changing a device
object's DACL, which is a Windows security-hardening change to the backend
itself — not a test repair, and not something this merge should make silently.
It is carried forward as an open item in §7.

### 2.7 The sixth reconciliation, and a defect no conflict reports

`origin/main` advanced again while this record was being written, to
`7d3c6ed09` (PR #166, the Linux trusted-Git C008 contained worktree), so the
line was reconciled with it a **sixth** time. The merge itself was clean —
`git merge-tree --write-tree origin/main HEAD` produced a tree with no
conflicts — and `main`'s advance added **one** top-level root integration-test
file, `tests/composed_two_agent_race_m011.rs`, and no family member.

The file **stays top-level**, and the reason is a shared global rather than a
lane. The composed two-agent race drives two live agents against one checkout,
and the agent-loop harness suites **drain the process-global event bus** and
assert that nothing was left pending on it — `agent_loop_harness::
temp_root_write_following_the_offer_needs_no_human` counts
`AppEvent::PermissionPending` on a global broadcast receiver. Absorbing the
file into `agent_loop` was tried first and **rejected on evidence**: the
harness then failed with *"no attempt may reach the human when the model
follows the offer"*, `left: 1, right: 0`, while the file passed in its own
binary (`3 passed`). **A test that observes a process-global cannot share a
binary with the tests that observe it too.** The file keeps its own target,
the way `main` shipped it, and gets its own ownership-manifest group
(`root_composed_two_agent_race_m011`).

The root package's test-target count is therefore **25** (21 families + 4)
and the workspace's **53**; the root integration-test *module* count rises
318 → **319** (4 top-level + 315 members).

**The merge produced a tree that did not compile, and no merge tool said so.**
`main`'s edit to the agent-loop harness included a shared helper with
`#[path = "common/scripted_provider.rs"]`. A `#[path]` attribute is relative
to **the file that carries it**, and the harness had moved into
`tests/agent_loop/` on this branch — so the path resolved to
`tests/agent_loop/common/scripted_provider.rs`, which does not exist. The two
sides touched different paths, git paired them by rename, and the result was a
silent break rather than a conflict.

The repair reaches the shared tree the way every other member of the family
does. The family's root already binds it, so the private include was both
broken and redundant — it compiled a second copy of a file the binary already
had — and the harness now uses `use super::common::scripted_provider::…`.

**The generalisable finding.** A move that changes a file's directory can
break a path *inside* it that no other file mentions, and neither the merge
nor the text of the diff points at it. The audit that caught it was
`cargo check --workspace --all-targets` on the merged tree, not the merge.

### 2.8 A family member that observes a process-global

The merged tree failed one `agent_loop` test under a plain `cargo test`:
`agent_loop_harness::temp_root_write_following_the_offer_needs_no_human`,
*"no attempt may reach the human when the model follows the offer"*, `left: 1,
right: 0`.

**The attribution is measured, not inferred.** At the branch tip before
`main`'s advance (`f8a9b6496`) the family passes under plain `cargo test` —
**530 passed, 0 failed, 1 ignored**. At the merged tip it fails —
**529 passed, 1 failed, 1 ignored** — with the same total, so no test moved.
The failing test subscribes to the **process-global** event bus
(`GlobalEventBus::subscribe()`) and drains it counting **every**
`AppEvent::PermissionPending`, with no session filter. M004's consolidation
put that harness into the `agent_loop` family alongside thirty other members,
so it now shares a binary — and a process — with tests that publish their own
permission events. The assertion became a cross-test race, and `main`'s
advance was what shifted the timing enough to make it fire.

The repair scopes the drain to the test's own session, which is what the
assertion means: *no attempt **for this agent** may reach the human*. The
file's own `EventCollector` helper already filters that way, so the fix
follows the idiom already in the file rather than inventing one.

**This is invisible to the hosted CI, and that is worth saying plainly.** The
hosted `CI` job runs `cargo nextest run --workspace --locked --profile ci`,
and the `ci` profile runs **one process per test** — a process-global cannot
be shared between two of them. Only a plain `cargo test`, which runs a
binary's tests as threads in one process, sees the interference. The same is
true of the `agent_loop` family's other global-bus assertions, which is a
standing property of the family rather than of this merge.

### 2.9 A private binding of the shared helper tree, found only by hosted clippy

The hosted `CI` job's **"Workspace Clippy"** step failed on the merged tree:
`could not compile 'codegg' (test "composed_two_agent_race_m011") due to 5
previous errors`, every one of them `never used` in
`tests/common/secret_scan.rs`.

**The cause is a difference between the two sides' lint posture.** This
branch removed the blanket `#[allow(dead_code)]` that `tests/common/
secret_scan.rs` used to carry, on purpose — the file says so in its own
header — and made every binder of the shared tree declare it **public**,
because a private binding makes the tree's `pub` items unreachable from the
binding crate root and rustc reports each of them as dead code.
`origin/main` still carries the blanket suppression, and `main`'s own clippy
step passes at `7d3c6ed09`. But `main`'s PR #166 added two standalone targets
that bind the tree **privately** — `mod common;` in
`tests/contained_worktree_c008.rs` and in `tests/composed_two_agent_race_m011.rs`
— which is harmless on `main` and a hosted clippy failure here.

The mechanism was **proved in isolation**, not inferred: a two-file rustc
reproduction with `mod common;` reports
`error: constant 'SENTINEL_PREFIX' is never used` and exits 1 under
`-D warnings`, and the same tree with `pub mod common;` exits 0.

The repair binds both standalone targets publicly, exactly as the nine
families already do, and `tests/common/mod.rs`'s header now says so.
`scripts/check_test_common_binding.py` closes the class: every binder of the
root package's shared helper tree must bind it publicly. It is in
`verify.sh quick` and in CI, and its `--self-test` rejects a private binding
in each kind of binder while accepting the unmodified copy.

**Why the local gate could not see it.** This tree's macOS build fails
earlier, on eight pre-existing findings in `origin/main`'s own files, so the
dead-code analysis of the test targets never runs locally — the hosted Linux
runner is the only place this class is visible. That is the same shape as
§2.4's Windows compile break: a platform-specific gate is the authority on
that platform.

### 2.10 The seventh reconciliation, and a fixture the merge changed for free

`origin/main` advanced a seventh time, to `726f3c645` (PR #167, the Linux
trusted-Git C008 contained worktree). **The advance is documentation only** —
four files, all of them records: `architecture/blackboard_contract_matrix.md`,
`plans/closure/agent-workstream-blackboard/011-status.md`, `plans/registry.md`
and `plans/subsystems/agent-workstream-blackboard-roadmap.md`. The merge was
clean, and `git merge` reported exactly those four files auto-merged. No code
and no coverage came across, so the counts did not move: the root package's
test-target count stays **25** and the workspace's **53**.

Both sides' edits survived, and that was **measured** rather than assumed —
a documentation merge is exactly where one side's row can be quietly
overwritten, and the two sides edited the same file.

| File | Side that edited it | How the merge was checked |
|---|---|---|
| `plans/registry.md` | the branch — the `CI and test throughput recovery` row | the merged row is byte-identical (md5) to the branch's pre-merge row |
| `plans/registry.md` | `main` — the `Agent workstream blackboard, peer visibility and workspace placement` row | the merged row is byte-identical (md5) to `origin/main`'s row |
| `architecture/blackboard_contract_matrix.md` | both | the merged file differs from **both** parents, which is what a file both sides edited looks like |

**One merge-visible change to a test fixture, and it dropped no coverage.**
`tests/contained_worktree_c008.rs`'s `FakeMode::LinkedWorktree` arm changed
during the **sixth** merge: the inline copy of the linked-worktree creation
was replaced by the shared helper's `common::linked_worktree::LinkedWorktreeCreator`.

The sequence is reconstructable from the three-way merge. At the merge base
(`f4019a410`) the arm carried an inline copy of three Git operations — create
the branch, `worktree add --detach`, then `symbolic-ref HEAD refs/heads/<branch>`.
The branch did not touch the file after that base. `main`'s PR #166
**refactored** the inline copy into the shared helper. Only one side changed
the region, so git took `main`'s change automatically —
`git show --remerge-diff fd69d19d2 -- tests/contained_worktree_c008.rs` is
empty, so no manual resolution is recorded and none was needed.

The change is not a silent weakening, and the check that settles it is reading
the two implementations side by side. `tests/common/linked_worktree.rs` runs
the **same three operations in the same order**, and its own header states why
that order matters: *"the checkout is created **detached** and the branch is
attached afterwards with `git symbolic-ref`"*. Two fields differ —
`backend` (`"fixture-linked-worktree"` against `"test-double"`) and
`network_enforced` (`false` against `true`) — and **nothing in the suite
asserts on either**: the C008 suite asserts on `outcome.administration_dir`
alone, and `network_enforced` is not read by `codegg-core`'s worktree service.
The helper's own header draws the boundary it is entitled to draw: *"it is
not a containment proof … the containment contract is C008's, proved by its
own suite."*

So the merge replaced a duplicated implementation with the shared one, and no
coverage left the tree with it.

## 3. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Merge `origin/main` without losing a branch change | Merge commit `54715a41e`; four conflicts resolved as recorded in §2.1 | pass |
| Every root integration test is owned by a family or a named standalone target | `scripts/check_test_ownership.py` — "the test-ownership manifest matches the tree and the workspace" | pass |
| The ownership guard is not vacuous | `scripts/check_test_ownership.py --self-test` — 13 injected violations rejected, "unmodified tree still passes" | pass |
| The merged tree compiles, every target | `cargo check --workspace --all-targets` — 0 errors | pass |
| The merged tree introduces no new clippy finding | `cargo clippy --workspace --all-targets` — the 8 errors are in files **byte-identical** to `origin/main` and are macOS-only artifacts of `#[cfg(not(target_os = "linux"))]` code; `main`'s own Linux CI runs "Workspace Clippy" to success | pass (no new finding; the 8 are pre-existing on `main` and Linux-invisible) |
| The test-filter-execution guard sees family targets | `scripts/check_test_filter_execution.py` and `--self-test` | pass |
| The Windows test-support guard names live targets | `scripts/check_windows_test_support_release.py` and `--self-test` | pass |
| No test body reports pass after skipping | `scripts/check_test_assertion_quality.py` and `--self-test` | pass |
| No duplicated test body | `scripts/check_test_duplication.py` and `--self-test` | pass |
| The 98 test functions the workstream commit stripped are restored | the ten files are byte-identical to `origin/main`; the SDK example compiles (`cargo check --tests` in `examples/plugins/sdk-rust`); the desktop and research files parse (`rustfmt --check`) | pass |
| The `security_permission` family compiles on Windows | `sandbox_containment.inc` carries `#![cfg(unix)]`; on Unix the family still carries 325 tests, 16 of them in `sandbox_containment::` | pass locally; the hosted Windows lane is the authority |
| A self-re-exec test names its test by a path that exists | `scripts/check_test_self_reexec_paths.py` and `--self-test`; the three known sites carry their full module path | pass |
| The Linux M004 lane's interrupted-publication recovery is exercised | the worker is named `linux_m004_process_ipc::test_only_contained_git_add_stages_index_and_objects_in_private_gitdir`, so it actually creates the interrupted `index.lock` the parent asserts on; run `39850835891` failed at `linux_m004_process_ipc.inc:1977` with the bare name | pass locally; the hosted Linux M004 lane is the authority |
| `sandbox_policy_wiring`'s refusal assertion is reachable | the tool is given the workspace root it needs, so the refusal under test is the capability refusal and not `MissingWorkspaceRoot`; the assertion reports the actual error text | pass locally; the hosted Windows lane is the authority |
| C014's host-denial probes run | the Git step reports its outcome instead of aborting the child; a Git failure must be the named null-device limitation | pass locally; the hosted Windows lane is the authority |
| The recorded counts are true on the merged tree | `architecture/overview.md`'s Verified-counts table, re-measured against each row's own `Source` | pass |
| The sixth reconciliation's merged tree compiles | `cargo check --workspace --all-targets` — 0 errors. The merge was clean (`git merge-tree --write-tree origin/main HEAD`), and the break it carried — a `#[path]` attribute relative to a file that had moved directory — was found by the compile, not by the merge | pass |
| The sixth reconciliation's new file keeps its own target | `tests/composed_two_agent_race_m011.rs` stays top-level: it drives two live agents on the process-global event bus, and the agent-loop harness suites drain that bus and assert that nothing was left pending on it. Absorbing it into `agent_loop` was tried and **rejected on evidence** — the harness failed with "no attempt may reach the human when the model follows the offer" (`left: 1, right: 0`) while the file passed alone (`3 passed`). It gets its own `root_composed_two_agent_race_m011` ownership group | pass |
| The `agent_loop` family passes under a plain `cargo test` | at `f8a9b6496` (before `main`'s advance): **530 passed, 0 failed, 1 ignored**; at the merged tip: **529 passed, 1 failed** — the harness drained the process-global bus with no session filter, and M004's consolidation had put it in a binary with thirty other members. The drain is now scoped to the test's own session, the idiom the file's own `EventCollector` already uses | pass (repeated runs); the hosted `CI` job cannot see this class, because the `ci` nextest profile runs one process per test |
| The shared helper tree is bound publicly everywhere | `scripts/check_test_common_binding.py` and `--self-test`; the two standalone targets `main`'s PR #166 added now bind it `pub`, as the nine families already do. The mechanism was proved in isolation with rustc: a private binding reports `error: constant 'SENTINEL_PREFIX' is never used` under `-D warnings`, a public one exits 0 | pass locally; the hosted Linux clippy step is the authority |
| The seventh reconciliation carries no code and loses no coverage | `origin/main` @ `726f3c645` changed four documentation files and nothing else; `git merge` auto-merged exactly those four. Both sides' edits to `plans/registry.md` and `architecture/blackboard_contract_matrix.md` were checked by md5 of the individual rows and of the file — the branch's row is byte-identical to its pre-merge copy, `main`'s row is byte-identical to `origin/main`'s, and the file both sides edited differs from both parents. The one fixture implementation the merge changed was checked by reading both implementations: the shared helper runs the same three Git operations in the same order, and nothing in the suite asserts on the two fields that differ | pass |

## 4. The counts, re-measured

Every row of `architecture/overview.md`'s Verified-counts table was re-measured
against **its own listed `Source`** on the merged tree, not carried over from
either side. Two rows moved; the rest did not.

| Row | Before | After | Source |
|---|---:|---:|---|
| Integration test files | 302 | **319** | `tests/*.rs` + `tests/*/*.inc`, `tests/common/` excluded — 4 top-level + 315 members |
| CI guard scripts | 68 | **76** | `scripts/check_*` — 6 from `main`, the branch's 6 already counted, plus the 2 this line adds |

The remaining rows were re-measured and did not move: `codegg-core` modules
**54**, `CoreRequest` variants **263** (239 struct + 24 unit), `CoreResponse`
variants **172** (171 struct + 1 unit), `CoreEvent` variants **92**,
`ProjectionEvent` variants **47**, `AppEvent` variants **61**, tool
registrations **67** (54 `register` + 13 `register_boxed`), distinct production
tool names **61** (66 production `impl Tool for` blocks, 5 resolving their name
at runtime, less the 7 test-only mock impls), tool source files **83**,
authorized operations **257** (247 representative requests), identity newtypes
**19**, built-in slash commands **168**, built-in agents **10**, bundled themes
**50**, TUI state modules **33** (34 `.rs` files), database tables **95
statements / 91 tables**, storage layout version **87**, Git operation variants
**54**, Git risk classes **11**, LSP server definitions **39**, architecture
docs **91**, workspace member crates **11 (+ root)**.

Two of these deserve their derivation stated, because both sides had changed
the source they are measured from:
- **The integration-test measure did not change.** It is M004's measure —
  root integration test *modules* — and the absorption moved a file without
  moving the count: 16 + 302 = 318 before the absorption, 3 + 315 = 318 after
  it. The count on `origin/main` is 318 too, by the other route: 302 top-level
  files + 16 predecessor members. (The sixth reconciliation of §2.7 added one
  more file, which **stays** top-level, so the row now reads 319: 4 top-level +
  315 members.)
- **The guard-script measure moved twice more, for this line's own guards.**
  `scripts/check_test_self_reexec_paths.py` and
  `scripts/check_test_common_binding.py` are the seventy-fifth and
  seventy-sixth `scripts/check_*` scripts, and both exist because this
  line's own hosted runs showed the class they close (§2.5 and §2.9). They
  are measured the same way as every other guard — `ls scripts/check_*` — and
  the integration-test row beside them is unchanged, because the absorption
  moved files without adding or removing one.
- **The tool-name row did not move, and it had been measured correctly.** The
  row's parenthetical names 66 production `impl Tool for` blocks and 7
  test-only mocks. The mock in `broker.rs` is spelled
  `impl super::super::Tool for TestTool`, so a scan for `impl Tool for` alone
  undercounts it and makes the row look stale by one. Measured on the merged
  tree, on `origin/main`, and on the merge base, the numbers are 66 / 7 / 61
  in all three.

## 5. Verification executed

### Commands run

```bash
git merge origin/main --no-commit            # the four conflicts of §2.1
cargo check --workspace --all-targets        # the merged and absorbed tree
cargo clippy --workspace --all-targets -- -D warnings
python3 scripts/check_test_ownership.py [--self-test]
python3 scripts/check_test_filter_execution.py [--self-test]
python3 scripts/check_windows_test_support_release.py [--self-test]
python3 scripts/check_test_assertion_quality.py [--self-test]
python3 scripts/check_test_duplication.py [--self-test]
python3 scripts/check_test_self_reexec_paths.py [--self-test]
python3 scripts/check_test_common_binding.py [--self-test]
cargo test -p codegg --test command_git git_fixture_isolation -- --test-threads=1
cargo test -p codegg --test security_permission
cargo test -p codegg --test agent_loop
cargo test -p codegg --test composed_two_agent_race_m011
(cd examples/plugins/sdk-rust && cargo check --tests)
bash scripts/verify.sh quick
```

### Results

- `cargo check --workspace --all-targets`: **0 errors**.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: **8
  errors**, all in files byte-identical to `origin/main`, all macOS-only
  artifacts of `#[cfg(not(target_os = "linux"))]` code that Linux CI never
  compiles. `main`'s own last green CI run executes "Workspace Clippy" to
  **success** on `ubuntu`. The merge introduces no new clippy finding. These
  eight are recorded as a **pre-existing `main` defect**, not as this line's.
- `cargo test -p codegg --test command_git git_fixture_isolation`: **2
  passed, 1 ignored** — the parent test re-executes the child with `--exact`
  and the qualified path selects it, which is the repair of §2.3.
- `cargo test -p codegg --test security_permission`: **325 passed, 0 failed**
  — the family still carries every member on Unix after the `#[cfg(unix)]`
  gate of §2.4, and the §2.5 repair to `sandbox_policy_wiring` costs the
  family nothing on a host that has a backend.
- `python3 scripts/check_test_self_reexec_paths.py`: **passes** on the merged
  tree, and its `--self-test` rejects each of the five defect classes — a bare
  name in a family member, a sibling member's module name, a test that does
  not exist, a macro-computed argument, and a module-qualified name in a
  standalone target — while accepting the unmodified copy.
- `cargo test -p codegg --test agent_loop`: **530 passed, 0 failed, 1
  ignored** — four consecutive runs on the merged tree, after the §2.8
  repair. At the branch tip before `main`'s advance the same command reported
  the same totals, and at the merged tip *before* the repair it reported
  **529 passed, 1 failed** with the identical total, which is how the
  cross-test race was attributed rather than assumed.
- `cargo test -p codegg --test composed_two_agent_race_m011`: **3 passed, 0
  failed** — the sixth reconciliation's new target, kept standalone (§2.7).
- `python3 scripts/check_test_common_binding.py`: **passes** on the merged
  tree, and its `--self-test` rejects a private binding in each kind of
  binder — a family root and a standalone target — while accepting the
  unmodified copy.
- `git merge-tree --write-tree origin/main HEAD`: a tree with **no** conflict
  entries, and exit 0 — the sixth merge was textually clean, and the tree it
  produced did not compile (§2.7).
- The **seventh** reconciliation (`726f3c645`): `git merge-tree --write-tree
  --name-only origin/main HEAD` → exit 0 with no conflict entries, and the
  merge itself auto-merged exactly the four documentation files `main` had
  changed. `cargo check --workspace --all-targets` on the merged tree →
  **0 errors**. `bash scripts/verify.sh quick` on the merged tree → **"Quick
  verification passed."** Both sides' documentation edits were checked by md5
  (§2.10's table).
- `examples/plugins/sdk-rust`: `cargo check --tests` **passes**.
- The guard tier: every guard named above passes, and every `--self-test`
  still rejects the failure class it exists to reject.
- `bash scripts/verify.sh quick`: see §7.

## 6. What this record does not claim

- **It does not re-measure M005.** The hosted before/after evidence in
  `plans/validation/ci-test-throughput-recovery/measurement-record.md` was
  measured on the pre-merge tree, at the candidate `3dfeb6e2f`, against the
  baseline `2c1953347`. The merge changed the tree it was measured on, so
  those numbers are **not** the numbers for this tree. They remain the record
  of what was measured, and this record does not restate them as current.
- **It does not re-run the hosted suite.** The hosted run is CI's job on the
  landed branch; this record reports the local gate and the hosted run's
  result is reported where it happens.
- **It does not claim the macOS clippy state is green.** It is not, on this
  platform, and it was not before the merge either.
- **It does not claim the AppContainer projection can run Git.** It cannot
  today, and §2.5's repair does not change that — it stops the test from
  pretending otherwise, and restores the seven host-denial probes that the
  failing Git step was discarding. The null-device limitation itself is an
  open item in §7.
- **It does not claim the §2.5 repairs are proven on Windows or Linux.** The
  local gate cannot run the Windows lane or the Linux M004 lane; the hosted
  runs are the authority, and this record's own §2.5 quotes the failure they
  must turn green.

## 7. Open items carried forward

- **The AppContainer child cannot open the null device.** `\Device\Null` is
  denied to the restricted child, so Git for Windows cannot run inside the
  projection at all (`fatal: could not open '/dev/null' … Permission denied`),
  and the C019 probe reports `null_device_access` as `state: denied` against
  `expected: allowed` alongside `git_init`, `git_status`, `python_version` and
  `cargo_version`. §2.5 makes C014 *report* that limitation instead of
  aborting on it, so the seven host-denial probes now run; it does not **fix**
  it. Granting the null device means changing a device object's DACL — a
  Windows security-hardening change to the backend, not a test repair. It
  belongs to the Windows native sandbox line
  (`plans/subsystems/windows-native-sandbox-roadmap.md`), not to this one.
- **The `contained_worktree_c008` target fails on macOS.** It reports
  `3 passed; 1 failed` locally —
  `the_grant_names_only_the_administration_directory` panics at
  `build_worktree_grant(…).expect("the contained worktree grant")`. The
  attribution is measured: the branch's **pre-merge** copy of the file fails
  the same way, and the file is `origin/main`'s apart from the binding
  change of §2.9, so it is a pre-existing macOS-platform failure in a
  Linux-targeted containment test rather than a regression. No local gate
  runs that target — the Linux M004 lane does — so it is recorded rather
  than repaired, and `architecture/testing.md`'s known-macOS-failures table
  does not yet list it.
- **The eight macOS-only clippy findings in `origin/main`'s files.** They are
  `#[cfg(not(target_os = "linux"))]` code that no macOS path calls. They
  belong to `main`, they are invisible to Linux CI, and fixing them means
  changing `main`'s platform gating — out of this line's scope. Recorded, not
  silently inherited.
- **The deferred rebase (roadmap §10 D9) is discharged by the merge.** The
  branch was reconciled with `origin/main` rather than rebased, which is the
  cheaper and safer of the two for a ten-commit branch and a 150-commit
  divergence, and it keeps the merge commit as the single place the two
  histories meet.

## 8. Recommendation

**Closed.** The branch is merged with `origin/main`, the integration tests
`main` carries are absorbed into the recovery families, the hosted workflow
references that the consolidation broke are repaired, the guards that the
merge exposed are repaired, the 98 test functions the workstream commit
stripped out of ten out-of-workspace files are restored, the `security_permission`
family compiles on Windows again, the three defects the Windows lane exposed
are repaired (§2.5), the sixth reconciliation's silent compile break is
repaired (§2.7), the cross-test race the consolidation left latent is repaired
(§2.8), the private shared-tree binding that only hosted clippy could see is
repaired and guarded (§2.9), the seventh reconciliation carried no code and
dropped no coverage (§2.10), and the recorded counts are re-measured against
their own sources. The tree compiles, the guard tier is green, and the
recorded counts are true on the merged tree.

Five findings are worth carrying forward beyond this line, because all five
are about **what the local gate does not cover**:

- **The root workspace's gate compiles the root workspace.** The ten files
  the workstream commit damaged are all outside it — the desktop crate, the
  research crates, the plugin SDK example — so `cargo check --workspace
  --all-targets`, `cargo clippy --workspace --all-targets`, every
  `verify.sh` tier, and the hosted `CI` job all passed on a tree whose
  desktop crate did not parse. A guard that compiles every Rust workspace
  in the repository, not just the root one, would have caught it.
- **A file-level `#[cfg]` gate is a coverage decision, not a portability
  detail.** M004's consolidation moved `sandbox_containment.rs` into a
  family that a Windows lane builds, and the member's Unix-only APIs broke
  that lane's build. The Windows lane is the authority on Windows
  compilation, and it is the only place this could be seen.
- **A test that cannot reach its own assertion is a green test that asserts
  nothing.** `sandbox_policy_wiring`'s refusal assertion was unreachable on
  every host: the guard returned early where a backend exists, and the tool
  failed for an unrelated reason where none does. It passed for years
  because passing was the only outcome it could produce. The same shape
  appeared in `linux_m004_process_ipc`, where a child that matched no test
  exited 0 having done nothing and the parent's "the child succeeded"
  assertion read that empty success as evidence — so the interrupted-publish
  recovery it exists to prove was never exercised. It was the *next*
  assertion, the one that checks the interrupted lock actually exists, that
  caught it. **A test that re-executes its own binary must name its test by a
  path that exists**, and `scripts/check_test_self_reexec_paths.py` now
  enforces exactly that.
- **A failing prerequisite can silently discard the assertions behind it.**
  C014's Git step aborted the child, so the seven host-denial probes after it
  never ran — the test was failing at step two while carrying no coverage of
  step three. A step that cannot succeed on a given host must *report* its
  outcome and let the rest of the test proceed, not abort the whole run.
- **A clean merge is not a merged tree.** The sixth merge reported no conflict
  and produced a tree that did not compile: `main`'s edit carried a
  `#[path]` attribute that is relative to the file holding it, and the file had
  moved directory. The two sides touched different paths, so git paired them
  by rename and the break was silent (§2.7). **A merge must be audited by
  compiling the merged tree, not by reading the conflict list** — and the
  conflict list is not even the whole story.
- **A family member that observes a process-global observes its siblings too.**
  The consolidation put the agent-loop harness in a binary with thirty other
  members, and one of its assertions read the *process-global* event bus
  without a filter (§2.8). The hosted `CI` job runs one process per test, so
  it cannot see that class; a plain `cargo test` can. **Consolidation changes
  what a test shares, and a test that reads a global must be re-read in that
  light.**

This record opens no milestone and rewrites no earlier record. The eight
macOS-only clippy findings are recorded as a pre-existing `origin/main`
defect, and M005's hosted numbers are recorded as the measurement of the
tree it was measured on, not as this tree's.
