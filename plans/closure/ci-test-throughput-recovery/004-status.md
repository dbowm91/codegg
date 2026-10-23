# CI/Test Throughput Recovery M004 — Closure Status

Status: conditionally closed

Recommendation: **conditionally closed** — the consolidation is implemented, every family compiles (`cargo check -p codegg --all-targets` is green), the target count fell from 288 to 21, and the full workspace suite was run end to end: **15,580 tests, 15,572 passed, 7 failed, 1 timed out, 8 skipped**. Six of the seven failures and the one timeout were **proved pre-existing** by running the same tests at the baseline commit; the seventh was a **real regression this milestone introduced**, and it is fixed (§3.1). The hosted-CI wall-clock measurement and the family-level link-cost measurement remain outstanding.

> **Evidence note (2026-10-21).** An earlier draft of this line read "the whole workspace suite is green". That claim was **premature** — the run had not completed. The line is corrected here to state what is actually evidenced.

Source implementation plan:

- `plans/implementation/ci-test-throughput-recovery/004-root-test-target-family-consolidation.md`

Source subsystem roadmap:

- `plans/subsystems/ci-test-throughput-recovery-roadmap.md`

Repository baseline reviewed: `2c1953347896848c2f52c2dfbd9f04029d1c2cc2`

## 1. Executive finding

The root package's **288** integration-test executables are now **21**. Every test body, every assertion, and every feature gate is preserved; what changed is the *unit of execution* — one executable per subject family instead of one per file.

| | Before | After |
|---|---|---|
| Root test targets | **288** | **21** |
| Workspace test targets | **314** | **47** |
| Root integration test modules (files) | 286 `tests/*.rs` | 301 `tests/*/*.inc` (0 top-level) |
| Root library rlib links per run | 288 | 21 |

The 19 families are the `mod.rs` + `.inc` pattern the M003 `projection_replay` pilot established: cargo does not auto-discover `tests/<dir>/mod.rs`, so every family is declared explicitly in the root `Cargo.toml`; cargo *does* auto-discover `tests/<dir>/*.rs`, so the members carry a `.inc` extension and are never auto-discovered.

## 2. The partition, and its departure from the decided count

Roadmap §10 D4 was decided by the owner on 2026-10-21: **~15 families accepted**, with the count explicitly a *target, not a quota*, and with "unrelated subjects share a family purely to hit the count" as an explicit stop condition.

The partition that was implemented has **19** families. The departure is recorded here with its reason, which the plan's acceptance criterion #1 allows ("the closure record states why a different count is the better partition").

**Ten of the nineteen are structurally forced** — they cannot be merged with anything without breaking a stated property:

- **Six singleton families**, one per binary that `.config/nextest.toml` reserves the test runner for: `eggwork_remote_execution_live`, `interactive_process_attach_resume`, `interactive_process_sessions`, `interactive_terminal_tui`, `scheduler_cancellation`, `causal_observe_replay`. Merging any of these with another file would make the whole family exclusive, changing the suite's timing profile and reintroducing the recorded flake class. They are also the binaries the CI job filters by name.
- **Four feature-gated families**: `lsp_test_support` (4 targets, gate `lsp-test-support`), `server_feature` (5 targets, gate `server`), `windows_sandbox_test_support` (2, gate `windows-sandbox-test-support`), `linux_git_test_support` (2, gate `linux-git-test-support`). A family cannot mix a gated member with an ungated one — the gate would apply to the whole family — so the gated targets stand alone, one family per gate.

**Nine are coherent subject families**: `tool_programs` (72), `tui_render` (41), `core_daemon` (38), `scheduler_jobs` (32), `agent_loop` (31), `security_permission` (19), `command_git` (16), `config_provider` (12), `ide_protocols` (5).

Ten forced + nine coherent = **19**. Reaching 15 would have required merging four of the nine coherent families into subjects they do not share — which is precisely the stop condition the owner set. **The stop condition was honoured, and the count was allowed to move.**

### The exclusion-binary mapping — no collisions

The plan's stated hazard was that a family inheriting two previously-exclusive binaries would serialise them against each other. **That hazard did not materialise**: each of the six is a family of one, and **each keeps its exact target name**. The consequences are:

- `.config/nextest.toml`'s whole-binary filter is **byte-identical** — no re-pointing was needed, and the flake rationale stays attached to the binary that flaked, not to a family.
- The CI job's `-E 'not binary(eggwork_remote_execution_live)'` filter is **byte-identical**.
- No family's exclusivity changed.

## 3. What else changed, and why it was not optional

### 3.1 A regression this milestone introduced, found by the suite and fixed

`tests/tool_programs/tool_program_m012_process_recovery.inc` carries a
**structural** test, `c30_all_m012_test_files_exist`, whose C-30 requirement is
"verify all required M012 test files exist". It asserted seven **pre-consolidation**
paths — `tests/tool_program_m012_authority.rs` and six siblings — which the
consolidation moved into `tests/tool_programs/` with the `.inc` extension. The
assertion therefore failed after the move:

```
FAIL codegg::tool_programs tool_program_m012_process_recovery::c30_all_m012_test_files_exist
```

This is a **real regression**, and the closure record's earlier claim — that
every member file was byte-identical to its original — did not catch it, because
the file *was* byte-identical: the **repository layout it asserted** was what
changed. A path-dependent structural test is invisible to a file-by-file diff
and is exactly the class of breakage a consolidation introduces. The record's
claim is corrected here rather than left standing.

**The fix** updates the seven paths to their new locations and resolves them
against `env!("CARGO_MANIFEST_DIR")` rather than the process working directory,
so the assertion no longer depends on where the test is run from. The test's
intent — "the M012 test suite is complete" — is preserved; only the paths it
names changed. Verified: the test passes after the fix.

A sweep of the whole test tree for other repository-layout assertions found
**no other instance**: every other `exists()` assertion in `tests/` is about a
temporary directory the test created, and the two `tests/fixtures/**` joins
still resolve because the fixtures tree was not moved.

### The `ci` profile and the CI job's filters did not change

Because the six binaries kept their names, the two filters that name targets are unchanged. `scripts/check_test_target_references.py` (new) now fails if either filter names a target that does not exist, or if the recorded integration-test count in `architecture/overview.md`'s Verified counts table disagrees with the tree — so a later rename cannot silently stop reserving the runner for a binary that needs it. Its `--self-test` injects a bad nextest filter, a bad CI filter, and a stale recorded count, and proves each is rejected.

### The production test-runner resolver had to change

`src/test_runner/resolve.rs`'s `resolve_file` maps a test **file** to a cargo **target**: it took `tests/foo.rs` → `cargo test --test foo`. After the move, `tests/command_git/git_closure_matrix.inc` would have produced `--test git_closure_matrix`, which does not exist — so the product's "run this test file" feature would have failed for most test files.

It now resolves a file inside a family directory to that family's target, reading the manifest's `[[test]]` declarations rather than guessing the directory name — because cargo does not auto-discover `tests/<dir>/mod.rs`, a directory under `tests/` is only a target because the manifest says so, and the declared name is authoritative. A new unit test covers it.

This is a production change in a milestone the plan classified as "no runtime behaviour changes". It is recorded as a **material deviation**: the plan's WP-C listed "three consumers" that name a test target, and this is a **fourth** consumer — one that *derives* a target name rather than naming one, and that therefore did not appear in the plan's inventory. Leaving it broken would have been a capability loss.

### Three classes of latent breakage that only a compile could find

The consolidation changes a file's *directory*, and Rust has two path-relative constructs that follow the file:

| Class | Instance | Fix |
|---|---|---|
| `include_str!` of production source | `tests/agent_loop/context_epoch_handoff.inc`, `tests/agent_loop/long_horizon_trajectory_qualification.inc` (4 occurrences) | `../src/` → `../../src/`, `../crates/` → `../../crates/` |
| `include_str!` of test fixtures | `tests/config_provider/fake_eggsearch_mcp.inc` (4 occurrences) | `fixtures/…` → `../fixtures/…` |
| A dead `mod common;` | `bash_background_m002_routes`, `job_background_test_slice`, `eggwork_remote_execution_live`, and `subagent` (which used `crate::common::`, so the substituted import was unused) | The declaration was removed where it was unused; `subagent`'s `use super::common;` was removed because `crate::common::` resolves through the family root without it. |

The last class is a small **cleanup**, not a loss: all three files declared `mod common;` and never used it — `tests/common/mod.rs` (518 lines) was being compiled into those binaries for nothing. The `eggwork_remote_execution_live` family's `common` binding was therefore removed as well, since it had exactly one member and that member used nothing.

### Dead code removed

`tests/test_util.rs` was deleted: 63 lines, **zero** test functions, **zero** references anywhere in the repository. Cargo auto-discovered it as its own test target, so the repository was linking an executable that compiled 63 lines of dead code and ran nothing.

### The ownership manifest shrank, as the schema was designed to allow

`docs/test-ownership.toml`'s root groups went from naming **288** targets to naming **22** — the schema, the field names, and the guard's rules did not change. That is the property M003's work package A required of the schema, and it is now demonstrated rather than asserted.

### Documentation and guard references re-pointed

71 files referenced a moved test file by path — architecture documents, skills, `CHANGELOG.md`, `bugs.md`, the execution-ownership manifest, five guard scripts, and four CI workflows. All were re-pointed at the new paths.

`docs/validation/` was **excluded** and reverted: `AGENTS.md` records that its closure records are pinned to old SHAs by design and must not be "refreshed". The rewrite also skipped `plans/`, whose documents describe the baseline they were authored against.

## 4. Failure-granularity cost

A family is one executable, so a failure inside it is reported with its family and member in the path (`family::member::test_name`) — the member is still identified. What a family costs is that **a failure in one member aborts the rest of that family's tests**, where a per-file target would have isolated them.

This is the same trade the M003 pilot already made for `projection_replay`, and the families were chosen so that the members in each share a subject area — so a failure in one is likely to be relevant to its neighbours. The two families that mix the widest range of subjects are `tool_programs` (72 members: the tool program runtime, the broker, the language, the deterministic tools, the Python harness, and the tool registry) and `core_daemon` (38 members: the daemon, session lifecycle, identity authority, work orders, and the projection). Those are the two places where the isolation a per-file target gave is genuinely lost, and they are the first places to look if a family-wide abort becomes a recurring cost.

## 5. Projected saving

The baseline model, from the measured cargo timings at the plan's baseline: 271 root integration test targets consumed **4,040.1 of 9,155.6 unit-seconds (44.1%)** of all workspace compile work, at a **median of 13.9 s** and a mean of 14.9 s per target.

Applying that model to 21 root targets gives **≈292 unit-seconds**, a saving of **≈3,748 unit-seconds**, or about **41%** of the workspace's total compile work.

**The model understates the saving and should not be read as a measurement.** A family is not one file: `tool_programs` links 72 modules into one executable, so its link cost is not 13.9 s. The model also ignores that the *root library rlib* is still linked into 21 binaries rather than 288 — a saving the per-target model does not capture either. The honest statement is: **the link count fell by a factor of 13.7, and the wall-clock effect has not been measured on the hosted runner.**

## 6. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| WP-A: the partition, its counts, its projected saving, its failure-granularity cost, and the exclusion-binary mapping | §2, §4, §5. | pass |
| WP-B: the family mechanism, with the `[[test]]` declarations, per-family module-path resolution, and the gated/ungated split | 21 `[[test]]` entries in `Cargo.toml`; `cargo check -p codegg --all-targets` is **green** (exit 0); the four gated families are each homogeneous in their gate. | pass |
| WP-C: the three target-name consumers re-pointed | The nextest filter and the CI `-E` filter are **unchanged** (the six binaries kept their names); the manifest's group membership was updated in the same change; the **fourth** consumer — the production test-runner resolver — was found and fixed (§3). | pass |
| WP-D: the guard that keeps the consumers and the target list in agreement | `scripts/check_test_target_references.py`, wired into `verify.sh quick` and CI, with a `--self-test` covering three violation classes. | pass |
| I1 (no behavioural assertion removed) | Every member file is byte-identical to its original except the three path fixes, the four dead `mod common;` removals (§3), and the one **path-dependent structural assertion** this milestone invalidated (§3.1) — which was found by the suite, fixed, and re-verified. No test's *behaviour* changed; one test's *expectation about the repository layout* did, because the layout it named is what this milestone moves. | pass (with the §3.1 correction) |
| I5 (the `ci` profile's per-test process isolation preserved) | The `ci` profile is unmodified; nextest still runs each test in its own process. | pass |
| I8 (the feature-gated targets keep their gates) | 13 gated targets → 4 gated families, each with its original gate. No family mixes a gated and an ungated member. | pass |
| Test removal is only of a proven duplicate with its survivor named | Nothing was removed as a duplicate. The one deletion, `tests/test_util.rs`, had zero test functions and zero references. | pass |

## 7. Invariant review

- **I1:** held — §6.
- **I2 (one bounded CI job):** held — no job, matrix, or lane changed.
- **I3 (the hosted CI job's scope never shrinks):** held — the hosted job's commands are unchanged, and the suite it runs now *contains* the same test functions in 21 executables instead of 288.
- **I4 (counts verified against the listed Source):** held — the integration-test-file row in `architecture/overview.md` was **re-measured and its measure re-pinned**, not copied: `tests/*.rs` (0) + `tests/*/*.inc` (301), with the `tests/common/` helper tree excluded. The row's prose records why the value moved (286 → 301: one file deleted, 16 previously-invisible `.inc` members now counted).
- **I5 (no new permanent framework):** held — one guard script in the repository's existing `scripts/check_*.py` idiom.
- **Roadmap I3 (the hosted job's scope):** held — §6.

## 8. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| **medium** | The hosted wall-clock effect is unmeasured. The per-target model in §5 is a projection from the baseline measurement, not a measurement of the consolidated tree. | The direction and the magnitude of the link-count reduction are established; the seconds are not. | Run the hosted `CI / verify` job on this tree and record the build-phase delta here before the milestone is closed outright. |
| **medium** | The family-level link cost is unmeasured. §5's model assumes a family's link cost is one target's. | The projection may under- or over-state the saving; `tool_programs` (72 modules) and `core_daemon` (38) are the outliers. | Measure the per-family link cost on the hosted runner, or locally with `cargo build --timings`, and refine §5. |
| low | A family aborts the rest of its family on a failure. | A failure in one member of `tool_programs` or `core_daemon` hides the rest of that family's results for that run. | §4. The first place to look if this becomes a recurring cost. |
| low | `cargo test --test <member>` no longer selects a single member. | A developer or workflow that selected one member by target name now selects a family. | Documented in `architecture/testing.md` and the testing-ci skill: use `cargo test --test <family> -- <member>`. The two workflows that did this were re-pointed to the family, which **broadens** their scope rather than narrowing it — chosen deliberately, because a narrow filter that matched nothing would make a qualification step silently vacuous. |
| low | `docs/validation/` still names the pre-consolidation paths. | The references point at files that no longer exist at those paths. | Deliberate — `AGENTS.md` pins those records to their revisions. |
| low | The workspace suite has **six** failures on macOS that are not regressions: the two `core::daemon_refresh` tests that register `/tmp` as a workspace root (macOS's `/tmp` is a symlink, and the registry rejects a symlinked root), the `OfflineRequired` managed-process test (Linux-only), the standalone-shell delivery test, and the two `scratch_cache_root_contract` tests (they expect Linux's `$HOME/.cache`; macOS uses `$HOME/Library/Caches`). | A developer on macOS sees six red tests that the hosted Linux job does not. | Proved pre-existing against the baseline commit (§8.1) and documented in `architecture/testing.md` under "Known macOS-local failures", so the next developer does not spend time re-diagnosing them. **Do not "fix" them by editing the assertions** — they are testing the Linux contract on a platform that does not provide it. |

No high or critical findings.

### 8.1 The full workspace suite — run, and what it proved

`cargo nextest run --workspace --locked --profile ci --no-fail-fast` on the
consolidated tree:

```
Summary [ 828.585s] 15580 tests run: 15572 passed, 7 failed, 1 timed out, 8 skipped
```

`--no-fail-fast` was used deliberately: the fail-fast run stopped at test 1,219
of 15,580 and would have proved nothing about the other 14,361.

**Every failure was accounted for against the baseline**, not explained away.
The baseline commit `2c1953347` was checked out into a separate worktree with
its own target directory and the failing tests were run there:

| Test | Baseline `2c1953347` | Consolidated tree | Disposition |
|---|---|---|---|
| `core::daemon_refresh::tests::session_projection_rebuilds_owner_scoped_live_jobs` | **FAILS** | fails | pre-existing: the test registers `/tmp` as a workspace root, and the registry rejects a symlinked root — `/tmp` is a symlink to `/private/tmp` on macOS |
| `core::daemon_refresh::tests::session_projection_snapshot_includes_in_process_live_child_instances` | **FAILS** | fails | pre-existing, same cause |
| `managed_process::tests::offline_required_scheduler_carried_intent_refuses_disabled_sandbox_before_spawn` | **FAILS** | fails | pre-existing: `OfflineRequired` is unavailable on this platform, so the refusal arrives earlier than the test's expected message |
| `core::daemon::tests::poolless_standalone_human_shell_uses_registered_workspace` | **FAILS** | fails | pre-existing: the standalone shell's output was not delivered within the test's own wait |
| `scratch_cache_root_contract::the_default_cache_root_is_the_home_directory_cache` | **FAILS** | fails | pre-existing: the test expects `$HOME/.cache`; macOS's cache directory is `$HOME/Library/Caches` |
| `scratch_cache_root_contract::an_explicit_cache_home_is_the_root_the_allocator_uses` | **FAILS** | fails | pre-existing, same convention mismatch |
| `tool_program_m012_process_recovery::c30_all_m012_test_files_exist` | **passes** | **failed** | **a regression this milestone introduced — found and fixed (§3.1)** |

The one **timeout** was `agent::asset_refresh::tests::same_scope_requests_coalesce_to_one_publication`, which the `ci` profile reserves the runner for and gives a 300 s slow-timeout. It ran while three other cargo builds shared the machine; run alone it **passes in 0.132 s**. It is a contention artefact of the measurement window, not a behaviour change — the test is an inline library test in `src/agent/asset_refresh.rs`, which this milestone does not touch.

The baseline probe is the part of this evidence that matters: it distinguishes
"the suite is red for a reason that predates the change" from "the suite is red
because of the change". Six of the seven are the former, proved by running the
same tests on the baseline commit. The seventh was the latter, and it was fixed.

**What this establishes.** Every test function that existed before the
consolidation still exists and is still executed: 15,572 of 15,580 pass, and
every failure is either proved pre-existing or fixed. The milestone's
no-regression claim now rests on a measurement, not on a file-by-file diff.

## 9. Registry updates

- `plans/registry.md`: the M004 row `ready → active (implemented; locally verified; hosted evidence outstanding)`, with the partition count and the reason for the departure from the ~15 target.
- `plans/subsystems/ci-test-throughput-recovery-roadmap.md`: §7 M004's exit conditions and the ~15-family count recorded against the achieved 19 with the reason; §10 D4 annotated with what was decided and what the implementation measured; §12 M004 `not started → active`.
- `plans/implementation/ci-test-throughput-recovery/004-*.md`: the partition, the exclusion-binary mapping, and the fourth consumer recorded as a material deviation.
