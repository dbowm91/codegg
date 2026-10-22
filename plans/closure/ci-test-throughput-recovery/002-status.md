# CI/Test Throughput Recovery M002 — Closure Status

Status: conditionally closed

Recommendation: **conditionally closed** — the implementation is complete, the bounded invocation was **executed** (§6.1: 3 cargo test targets, 7,504 tests, every failure a pre-existing macOS platform failure), and the correctness property is verified by construction and by measurement of the target set. The one thing that cannot be produced locally is **hosted evidence**, and this milestone never had a way to produce it: no CI workflow runs the feature-enabled pass, so its benefit is local-only by construction (§1).

Source implementation plan:

- `plans/implementation/ci-test-throughput-recovery/002-bounded-feature-enabled-test-pass.md`

Source subsystem roadmap:

- `plans/subsystems/ci-test-throughput-recovery-roadmap.md`

Repository baseline reviewed: `2c1953347896848c2f52c2dfbd9f04029d1c2cc2`

## 1. Executive finding

The feature-enabled verification pass no longer builds and re-runs every root test target. It builds and runs **three** cargo targets — the root library test target and the two feature-gated families — instead of **288**.

There is **no hosted-CI benefit**, and the closure record says so plainly: no workflow in `.github/workflows/` runs the feature-enabled pass. The only `--features` invocations in CI are the platform-specific `linux-git-test-support` and `windows-sandbox-test-support` qualifications. The milestone's saving is entirely local to `scripts/verify.sh full`.

## 2. What changed

| | Before | After |
|---|---|---|
| Invocation | `cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support --profile ci` | `cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support --profile subset --lib --test lsp_test_support --test server_feature` |
| Cargo targets built and run | 288 root test targets | 3 (the root lib test target + 2 gated families) |
| Nextest profile | `ci` | `subset` (new) |

`.config/nextest.toml` gains a `subset` profile:

- `test-threads = 4` — the same bound as the `ci` profile, so the local pass keeps its current concurrency and its timing profile;
- `slow-timeout = { period = "60s", terminate-after = 2 }` — the same as `ci`;
- the **per-test** reservation for `same_scope_requests_coalesce_to_one_publication`, with `threads-required = "num-cpus"` — the same as `ci`.

The `ci` profile's **whole-binary** exclusion is deliberately *not* carried over. Those six binaries (`eggwork_remote_execution_live`, `interactive_process_attach_resume`, `interactive_process_sessions`, `interactive_terminal_tui`, `scheduler_cancellation`, `causal_observe_replay`) are integration targets and none is in the bounded set, so reserving the runner for them would be meaningless — and carrying them over would make the subset profile depend on binaries the pass never builds. The `ci` profile itself is untouched, so invariant I3 holds.

The per-test reservation **is** carried over, because that test is an inline library test (`src/agent/asset_refresh.rs:721`) and the bounded pass includes the root lib test target.

## 3. The executed-set mapping — invariant I4

The invariant is: *every test function the previous feature-enabled pass executed is still executed*. That statement is about the **verification as a whole**, not about the second pass in isolation, and the distinction matters:

| Set | Previous verification | Bounded verification |
|---|---|---|
| Root lib inline test functions | pass 1 (workspace) + pass 2 (feature-enabled) | pass 1 (workspace) + pass 2 (`--lib`) — **unchanged** |
| The 149 feature-gated integration test functions | pass 2 only | pass 2 only, via 2 family targets — **unchanged** |
| The 279 ungated root integration targets' test functions | pass 1 + pass 2 | pass 1 only — **unchanged overall** |

The third row is the whole point of the milestone: those 279 targets were built, linked and re-executed by pass 2 *solely* because pass 2 changed the root library's compiled form (the new feature set), which forces every dependent test binary to be re-linked. Their test functions are still executed, by pass 1. The **union** of the two passes executes exactly the same set of test functions as before; only the redundant second execution of the ungated targets is gone.

Counts, each against its own listed Source:

- **7,433** inline test functions in `src/` — measured by scanning `src/**/*.rs` for `#[test]`/`#[tokio::test]` and resolving the enclosing `#[cfg(...)]` feature gates through the module tree from `src/lib.rs`.
- **51** of those are feature-gated: **43** on `server` (11 in `src/error.rs`, 1 in `src/client/sdk.rs`, 31 in `src/server/`) and **8** on `plugins` (all in `src/plugin/runtime/wasm.rs`). These live in the root lib test target, which is why `--lib` is in the bounded set.
- **149** feature-gated integration test functions: **93** in the five `server` targets and **56** in the four `lsp-test-support` targets.

### Correction to an earlier figure in this milestone's plan

The plan originally claimed the root lib test target carried "approximately 22 gated on `server`, 15 on `plugins`, and 2 on `server`". Those were the counts of `#[cfg(feature = ...)]` **gate sites** in `src/`, not of gated test functions. The measured counts are **43** and **8**. The direction of the error matters: a gate-site count *undercounts*, because one `#[cfg(feature = "plugins")] pub mod wasm;` in a parent file gates every test in that module. The milestone's conclusion is unchanged — the root lib test target must be in the bounded set, and omitting it would drop **51** feature-gated inline test functions — but the number is now measured rather than assumed, per roadmap invariant I4.

### Correction to the milestone's scope claim

The plan originally described the second pass as "the second pass runs the whole workspace again" and claimed a hosted-job benefit. Both were wrong:

- The second pass was already scoped to `-p codegg`; it never ran the whole workspace.
- No CI workflow runs the feature-enabled pass at all.

The roadmap's M002 block was corrected in place, and the plan's §2 and §3.1 were corrected in place, rather than the discrepancy being left for a reader to discover.

## 4. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| The bounded pass builds and runs exactly the root lib test target plus the feature-gated integration targets | `scripts/verify.sh`'s `feature_pass_targets` array names `--lib`, `--test lsp_test_support`, `--test server_feature`. `cargo metadata` confirms all three exist. | pass |
| Every test function the previous feature-enabled pass executed is still executed | §3's mapping. The union of the two passes is unchanged; the 51 feature-gated inline functions and the 149 feature-gated integration functions are all in the bounded set. | pass |
| The pass uses a Nextest profile appropriate for a subset run | A new `subset` profile: `ci`'s slot bound and slow-timeout, `ci`'s per-test reservation, and none of `ci`'s whole-binary exclusions (whose binaries are not in the bounded set). `ci` itself is unmodified. | pass |
| A gated target whose feature is not enabled fails loudly | Cargo fails on an unknown `--test` name. The bounded set names only targets that exist under `server,plugins,lsp-test-support`; `windows_sandbox_test_support` and `linux_git_test_support` are excluded because their features are not enabled, and they are qualified by the platform workflows. | pass |
| The `windows-sandbox-test-support` targets are not silently dropped | They run in `.github/workflows/windows-process-launcher.yml` and `windows-native-probe-contract.yml`, and the bounded pass's exclusion of them is explicit and commented. | pass |
| No dependency, feature default, or product behaviour changes | The feature set is still exactly `server,plugins,lsp-test-support`. No manifest dependency changed. | pass |
| No behavioural assertion removed | No test file was edited by this milestone. | pass |

## 5. Invariant review

- **I1 (no assertion removed for timing):** held — no test file changed.
- **I2 (one bounded CI job):** held — no workflow changed.
- **I3 (`ci` profile's slot bound and whole-binary exclusions untouched):** held — `.config/nextest.toml`'s `[profile.ci]` is byte-identical; the new profile is additive.
- **I4 (every test function still executed):** held — §3.
- **I5 (the feature-gated integration targets still build and run):** held — they are in the bounded set, by family.
- **I6 (the root lib test target runs in the bounded pass):** held — `--lib` is in the bounded set.
- **I7 (`windows-sandbox-test-support` targets not silently dropped):** held — §4.
- **I8 (no dependency, feature default, or product behaviour change):** held.

## 6. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | The plan's inline-test counts were gate-site counts, not gated-test counts. | Corrected in §3; the plan and the roadmap were corrected in place. | None. |
| low | The plan claimed a hosted-CI benefit that does not exist. | Corrected in §1 and §3; the roadmap's M002 block was corrected in place. | None. |
| low | The feature-enabled pass's failures are the same six macOS-specific failures the whole suite has (see `architecture/testing.md`'s "Known macOS-local failures"). They are pre-existing and platform-bound, not caused by the bounded set. | None for the milestone. | None. |

No high or critical findings. Nothing blocks M003.

### 6.1 The bounded invocation, executed

The invocation was run end to end:

```bash
cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support \
  --profile subset --lib --test lsp_test_support --test server_feature --no-fail-fast
```

```
Starting 7504 tests across 3 binaries (4 tests skipped)
Summary [ 243.722s] 7504 tests run: 7501 passed, 3 failed, 4 skipped
```

The pass was run twice. The first run was on a machine shared with three other
cargo builds and produced one extra failure — a **timeout** of
`agent::asset_refresh::tests::same_scope_requests_coalesce_to_one_publication`
at exactly its 300 s reserved slow-timeout. Re-run on a quieter machine, that
timeout does not occur, and the run is clean apart from the three macOS
failures below. The test is an inline library test in `src/agent/asset_refresh.rs`
— a file this milestone does not touch — and it passes in 0.132 s when run
alone.

**Three binaries.** The root library test target, `lsp_test_support`, and `server_feature`. The unbounded form of the same invocation builds every root integration target under the new feature set — **21** of them after M004's consolidation, **288** before it. The bounded set is **3** cargo test targets.

**The failures are the known macOS ones** and are identical to the whole suite's: the two `core::daemon_refresh` tests that register `/tmp` as a workspace root (macOS's `/tmp` is a symlink, and the workspace registry rejects a symlinked root) and the `OfflineRequired` managed-process test, which needs a Linux sandbox. All three were proved pre-existing against the baseline commit `2c1953347` — see `plans/closure/ci-test-throughput-recovery/004-status.md` §8.1.

**What this discharges.** The finding that was open in an earlier draft of this record — "the bounded invocation has not been executed green" — is discharged: the invocation runs, it builds three cargo test targets instead of 21, and every failure it produces is a pre-existing platform failure that the unbounded form produces too. The milestone's recommendation moves from *conditionally closed pending execution* to *conditionally closed pending hosted evidence only*, which this milestone never had a way to produce locally: no CI workflow runs the feature-enabled pass (see §1).

## 7. Roadmap disposition

Milestone conditionally closed; M003 may proceed — M003's dependency on M001 is hard, and M001's own effect is already landed and measured, so the sequence M001 → M002 → M003 → M004 holds.

Note for the sequence: M004 subsequently changed the *shape* of the bounded set. The nine feature-gated integration targets became two families (`lsp_test_support` and `server_feature`), so the bounded set is now `--lib --test lsp_test_support --test server_feature`. The invariant, the profile, and the reasoning are unchanged; only the target names moved. That update is part of M004's change set.

## 8. Registry updates

- `plans/registry.md`: the M002 row `ready → active (implemented; execution evidence outstanding)`.
- `plans/subsystems/ci-test-throughput-recovery-roadmap.md`: §7 M002's deliverable boundary and user-value corrected (no hosted equivalent), §12 M002 `not started → active`.
- `plans/implementation/ci-test-throughput-recovery/002-*.md`: §2 (no CI equivalent), §3.1 (the pass is `-p codegg`, and what it actually costs), §3.2 (the 13 gated targets and their measured function counts), §3.3 (the measured 43/8 inline counts and the correction of the earlier 22/15 figure).
