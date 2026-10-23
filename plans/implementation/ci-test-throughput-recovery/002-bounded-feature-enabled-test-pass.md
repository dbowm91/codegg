# CI and Test Throughput Recovery Milestone M002 — Bound the Feature-Enabled Test Pass

Status: ready for handoff

Repository baseline: `239f06e7fc876eaa36156dbf4658bb56314c9673` (the tree the plan was authored against)

Implementation baseline: `2c1953347896848c2f52c2dfbd9f04029d1c2cc2` (the tree the work was applied to, after the 2026-10-21 rebase onto `origin/main`)

Source roadmap:

- `plans/subsystems/ci-test-throughput-recovery-roadmap.md#milestone-m002--bound-the-feature-enabled-test-pass`

Long-term requirements:

- `plans/000-long-term-specification.md` §4.7 — correctness before transparent magic;
- `plans/003-planning-process.md` §2.3–2.4.

Applicable ADRs: none required.

Predecessor work (closed, historical, not reopened by this milestone):

- `plans/subsystems/ci-test-throughput-optimization-roadmap.md` M001–M005;
- `plans/subsystems/ci-test-throughput-optimization-post-closure-corrective-addendum.md` C001–C002.

Primary class: infrastructure, with a polish obligation.

---

## 1. Objective

The feature-enabled verification pass builds and runs **only** the test targets that the additional feature set actually unlocks, instead of the entire workspace a second time.

The set of tests that *execute* is unchanged. The set of tests that are *built* shrinks from the whole workspace to a small, enumerated set.

## 2. Why this milestone is ready

**Hard dependencies:** none.

**Interface dependencies:** none. This milestone changes the composition of one command invocation in `scripts/verify.sh`. There is no CI equivalent: no workflow in `.github/workflows/` runs the feature-enabled pass (verified 2026-10-21 — the only `--features` invocations in CI are the platform-specific `linux-git-test-support` and `windows-sandbox-test-support` qualifications). The milestone's benefit is therefore **local to `scripts/verify.sh full`**, and the closure record must say so rather than claim a hosted-CI saving it does not have.

**Operational dependency on M001:** M001 (profile alignment) should land first. The two changes are independent in code, but M001's effect is a compile-time claim and M002's effect is also a compile-time claim; changing both in one measurement window makes neither attributable. Sequence M001 first so each has its own hosted number.

**Why it is second:** it is the cheapest remaining structural win, and it is the last milestone whose savings do not depend on the target topology — which M004 changes.

## 3. Current implementation evidence

### 3.1 The second pass re-links every root test target under the new feature set

`scripts/verify.sh` in `full` mode performs, in order:

1. `cargo check --workspace --all-targets --locked`;
2. `cargo clippy --workspace --all-targets --locked -- -D warnings`;
3. `cargo nextest run --workspace --locked --profile ci`;
4. `cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support --profile ci`.

**Correction to an earlier claim in this plan.** The plan originally described step 4 as "the second pass runs the whole workspace again". It does not: step 4 is already scoped to `-p codegg`. The waste is more specific and larger than "the whole workspace":

Step 3 builds the root package's test targets **without** the extra features. Step 4 enables `server,plugins,lsp-test-support`, which changes the root library's compiled form — so the root library is rebuilt under the new feature set, and **every** root test binary that links it must be re-linked. That is 288 root test targets, of which 279 were already built and executed by step 3 and are re-linked only to be re-executed unchanged.

Measured on the implementation baseline `2c1953347`: the root package has 288 test targets (286 auto-discovered `tests/*.rs` files plus the two consolidated directory targets), 9 of which exist only under the extra features.

Step 4 is not *entirely* redundant, and that is the point of this milestone: it is the only place where the feature-gated tests run at all.

### 3.2 What the feature set actually unlocks

The root package's `Cargo.toml` declares fifteen `[[test]]` targets. Thirteen of them add a `required-features` gate:

| Feature | Gated targets |
|---|---|
| `lsp-test-support` | `lsp_composite_stdio`, `document_lsp_integration`, `lsp_completion_trajectory`, `lsp_read_delivery_m013` |
| `server` | `projection_transport_real`, `identity_m002_transport_auth`, `team_collaboration_m001_http_auth`, `team_collaboration_m006_trajectory`, `team_collaboration_postclosure_m001_registration_auth` |
| `windows-sandbox-test-support` | `windows_acl_lease`, `windows_restricted_managed_process` |
| `linux-git-test-support` | `linux_m004_process_ipc`, `linux_git_c002_performance` |

Two declared targets (`projection_replay`, `session_family`) are consolidated directory targets with no gate. The remaining root test files are auto-discovered and ungated.

The nine `server` and `lsp-test-support` targets above are the only integration targets whose existence or behaviour depends on the extra feature set. Measured test-function counts for the gated sets are 93 (`server`), 56 (`lsp-test-support`), 10 (`windows-sandbox-test-support`) and 18 (`linux-git-test-support`).

### 3.3 The feature-gated inline tests live in the root lib test target

The root lib test target (`cargo test --lib`) carries the inline `#[cfg(test)]` modules, which include the feature-gated ones. **A subset run that omits the root lib test target silently drops every feature-gated inline test.** This is the single most important correctness constraint in this milestone.

Measured on the implementation baseline `2c1953347` by resolving the module tree from `src/lib.rs` and counting every `#[test]`/`#[tokio::test]` function whose enclosing `#[cfg(...)]` predicate requires a feature — including gates inherited through a `mod` declaration in a parent file, which is where the count and the gate-site count diverge:

| Feature | Feature-gated inline test functions |
|---|---|
| `server` | **43** (11 in `src/error.rs`, 1 in `src/client/sdk.rs`, 31 in `src/server/`) |
| `plugins` | **8** (all in `src/plugin/runtime/wasm.rs`) |
| `lsp-test-support` | **0** |
| `image` | 7 |
| `decision-runtime-sdm` | 4 |

**Correction to an earlier figure in this plan.** The plan originally claimed "approximately 22 gated on `server`, 15 on `plugins`, and 2 on `server` among the inline tests". Those were the counts of `#[cfg(feature = ...)]` **gate sites** in `src/`, not of gated test functions. The measured counts are 43 and 8. The direction of the error matters: a gate site count *undercounts*, because a single `#[cfg(feature = "plugins")] pub mod wasm;` in a parent file gates every test in that module. The milestone's conclusion is unchanged — the root lib test target must be in the bounded set, and omitting it would drop **51** feature-gated inline test functions — but the number is now measured rather than assumed, per the successor roadmap's invariant I4.

### 3.4 The `ci` profile is not appropriate for a subset run

`.config/nextest.toml` states the constraint in its own words:

> Workspace-wide only: subset runs (`-p <crate>`) should use the default or timing profile, because the heavy-binary filter below strictly requires its binaries to exist.

The `ci` profile carries `[[profile.ci.overrides]]` entries with `threads-required = "num-cpus"` for six whole binaries (`eggwork_remote_execution_live`, `interactive_process_attach_resume`, `interactive_process_sessions`, `interactive_terminal_tui`, `scheduler_cancellation`, `causal_observe_replay`) and one whole test (`same_scope_requests_coalesce_to_one_publication`). A subset run that does not build those binaries cannot use the `ci` profile's exclusion machinery as written.

Consequence: **the bounded pass must use a subset-appropriate profile** (`default`, which sets `test-threads = 14` locally), and the plan must record that the `ci` profile's 4-slot bound does not apply to it. The implementing agent must state which profile the bounded pass uses and why, in the closure record.

### 3.5 The mechanism is proven

`cargo nextest run --test <name>` restricts the **cargo build**, not merely Nextest's run list. Verified on an isolated probe workspace with a clean target directory per variant:

| Command | Test executables produced |
|---|---|
| `cargo nextest list --test beta` | `beta` only |
| `cargo nextest list --tests` | `alpha`, `beta`, `gamma`, `delta` |
| `cargo nextest list --test alpha --test gamma` | `alpha`, `gamma` only |

**Repeated `--test` flags are accepted** and restrict the build to exactly the named targets. This is the mechanism the milestone depends on, and it is proven rather than assumed.

By contrast, `-E 'binary(...)'` filters only the *run* list — every target of the selected package is still built — so the existing CI invocation that excludes `eggwork_remote_execution_live` with `-E` saves execution time and no build time.

### 3.6 The CI job's own second invocation

`.github/workflows/ci.yml` runs two Nextest invocations: a full workspace run, and a second full workspace run that excludes `eggwork_remote_execution_live` via `-E`. That second CI invocation is a *different* concern from `verify.sh`'s feature-enabled pass — it is the same workspace scope with one binary excluded, and it exists to avoid double-running the live Eggwork qualification. The implementing agent must be careful to distinguish the two and must not conflate them; see §7 work package B.

## 4. Invariants that must not regress

- **I1 — No behavioural assertion is removed for timing.** No test file changes.
- **I2 — One bounded `CI / verify` job.** No new job, matrix, or lane.
- **I3 — The `ci` profile's 4-slot bound and whole-binary exclusions are untouched.** The bounded pass uses a different profile; it does not modify the `ci` profile.
- **I4 — Every test function the previous feature-enabled pass executed is still executed.** This is the milestone's core correctness property.
- **I5 — The feature-gated integration targets still build and run** under the feature set. The milestone must not turn a gated target into a permanently-skipped one.
- **I6 — The root lib test target runs in the bounded pass**, so every feature-gated inline test still executes (see §3.3).
- **I7 — The `windows-sandbox-test-support` targets are not silently dropped.** They are gated on a feature the bounded pass does not enable, so they do not run in either pass today; their disposition must be recorded, not changed.
- **I8 — No dependency, feature default, or product behaviour changes.** The feature set used by the bounded pass is exactly `server,plugins,lsp-test-support`, unchanged.

## 5. Scope

### In scope

- The second Nextest invocation in `scripts/verify.sh`.
- The CI job's Nextest invocation(s), to the extent the bounded pass applies there.
- The enumeration of the bounded target set, and whatever mechanism makes that enumeration auditable (a named list in the script, or a small generated list).
- Documentation of which profile the bounded pass uses and why.

### Explicitly out of scope

- Removing the first workspace-wide pass. It remains the full-suite gate.
- Enabling `server` or `lsp-test-support` in the **first** pass or in the hosted job's default scope. The 149 never-executed CI test functions are a coverage decision owned elsewhere (successor roadmap §4.6, §10 D1) and this milestone does not make that decision.
- The `windows-sandbox-test-support` targets. They run in the Windows-specific workflow, not in either pass; see I7.
- Changing the `ci` profile, its slot count, or its exclusions (I3).
- Removing the `-E` exclusion of `eggwork_remote_execution_live` from the CI job's second invocation — that exclusion has a separate purpose (§3.6) and is not this milestone's target.
- The `cargo check` / `cargo clippy` duplication between `verify.sh quick` and `verify.sh full` (§10, D4 of the successor roadmap notes the check/clippy overlap as an observation; it is not this milestone's target because it is a separate pass-order question).
- Any change to M003's scoped mode or M004's consolidation.

## 6. Required production changes

### Core/domain

None.

### Storage and migrations

None.

### Protocol and DTOs

None.

### Runtime and concurrency

None. The milestone changes which test executables are built and run, never how any process is spawned or scheduled.

### Frontend or operator surface

None.

### Security and authorization

None.

### Documentation and static guards

- `scripts/verify.sh` — the bounded pass must be self-documenting: the enumeration of targets it builds must be readable at the point of use, with a comment stating *why* the root lib test target is in the list (the feature-gated inline tests) and *why* the gated integration targets are in the list.
- `architecture/testing.md` — the verification-pass description must record the bounded pass's scope, the profile it uses, and the measured effect. The "verified counts" material must not be extended with a number this milestone did not measure.
- A static guard is **not** required for this milestone. If the implementing agent introduces a generated enumeration, it must be guarded in the repository's `scripts/check_*.py` style with a `--self-test`; if it is a literal list in the script, no guard is needed.

## 7. Ordered work packages

### Work package A — Enumerate the bounded target set

Intent: know exactly which targets the extra feature set unlocks, and prove the enumeration complete rather than assumed.

Required changes:

1. Enumerate the feature-gated integration targets from the root `Cargo.toml`'s `[[test]]` declarations (§3.2).
2. Include the root lib test target in the bounded set — non-optional, because the feature-gated inline tests live there (§3.3, I6).
3. Determine empirically whether the root lib test target's *name* is selectable via `--test`. If the lib test target cannot be named with `--test`, the bounded pass must use the mechanism that does select it, and the closure record must state which mechanism was used and why. This is a **work package discovery step**, not an assumption: the plan must not presuppose the answer.
4. Cross-check the enumeration against the feature-gated inline tests: search the root package's inline test modules for `#[cfg(feature = ...)]` gates and confirm every gated feature is covered by the bounded pass's feature set.

Acceptance evidence:

- The enumeration, with the `Cargo.toml` declaration each entry comes from.
- A confirmed answer to work package A step 3, with the command that established it.
- The cross-check from step 4, with the count of feature-gated inline test functions found and confirmation that the bounded pass's feature set covers every gate encountered.

### Work package B — Rewrite the feature-enabled pass

Intent: the pass builds and runs only the bounded set.

Required changes:

1. Replace the workspace-wide feature-enabled invocation with a bounded one that names the target set from work package A.
2. Select the profile appropriate to a subset run, per §3.4, and record the choice.
3. Keep the pass's **exit status semantics** identical: a failure in any bounded target must fail the pass and therefore the script.
4. Preserve the script's existing ordering and reporting so a reader can still tell which pass is running.
5. Leave the CI job's separate `-E`-excluded invocation alone unless the implementing agent can show it is genuinely the same redundancy; if it is, the change must preserve the live-Eggwork exclusion's purpose exactly. If in doubt, leave it and record why.

Acceptance evidence:

- The rewritten invocation, with the profile choice and its rationale recorded at the point of use.
- A demonstration that the bounded pass produces a materially smaller set of built test executables than the workspace-wide form — the artifact or build-count difference recorded, not asserted.

### Work package C — Prove the executed set is unchanged

Intent: establish the milestone's core correctness property — that bounding the build did not drop any executed test.

Required changes:

1. Enumerate the test functions the previous workspace-wide feature-enabled pass executed, and the test functions the bounded pass executes.
2. Compute the difference and account for every entry.
3. Any test function that the previous pass executed and the bounded pass does not is either (a) a test that the first workspace-wide pass already covers with the same assertion — which must be argued per test, not assumed, because a feature can change behaviour — or (b) a regression to fix before closing.

Acceptance evidence:

- The two enumerations and their difference.
- A per-difference disposition. Category (a) entries must state why the feature cannot alter that test's outcome, with the specific `#[cfg(feature = ...)]` gate or the absence of any feature-dependent code path in the test's subject.
- A count of test functions executed before and after. The count must not drop. If it does, the milestone has failed its correctness property and must be corrected or reverted.

### Work package D — Measure and record

Intent: produce the hosted evidence and correct the documentation.

Required changes:

1. Run the hosted `CI / verify` job on the change; record total wall-clock and the test-step build/execution split, labelled hosted.
2. Compare against the successor roadmap §4.1 baseline and against the predecessor's recorded run, stating that the predecessor's figure was taken against a different target count.
3. Update `architecture/testing.md`.
4. Record what the change did not improve.

Acceptance evidence:

- Hosted run id, wall-clock, build/execution split, extraction method.
- `architecture/testing.md` updated.
- An explicit statement of what did not improve.

## 8. Failure, cancellation, restart, and contention semantics

- **A failure in a bounded target must fail the pass.** The bounded pass must not swallow a target's non-zero exit. This is the milestone's most important failure-semantics property, because a bounded pass with a permissive exit-status rule would silently convert a red suite into a green one.
- **A target that fails to build must fail the pass**, not be skipped. The pass must not treat an unbuildable gated target as "nothing to run".
- **A gated target whose feature is not enabled must not be silently skipped by the script.** If the bounded set names a target that does not exist under the selected feature set, that is a configuration error and must be loud.
- **Cancellation.** The bounded pass must remain interruptible in the same way the workspace-wide pass is; no new supervision or waiting behaviour is introduced.
- **Concurrency.** The bounded pass uses a different profile from the workspace-wide pass. The `ci` profile's whole-binary exclusivity was recorded to keep subprocess- and PTY-driving binaries deterministic. A bounded pass that runs several of those binaries concurrently under a wider slot count could reintroduce the flake class the exclusions exist to prevent. The implementing agent must therefore check whether any bounded target is one of the six exclusion binaries, and if so, either exclude it from the bounded pass (recording why) or run the bounded pass under a profile that preserves the exclusion. **This is a stop condition if it cannot be resolved safely** — see §14.

## 9. Compatibility and migration

- **Developer-local compatibility.** `verify.sh full` becomes faster and produces the same green/red signal. A developer who relied on the second pass's full-workspace output to see every binary's result loses that visibility; the pass must print what it did and did not run so the reduced scope is visible rather than silent.
- **CI compatibility.** The hosted job's coverage must not change. If the CI job's own second invocation is left alone, the CI job's executed set is unchanged and the milestone's CI benefit comes only from the `verify.sh` path — the closure record must say which, honestly.
- **Migration.** None. No persisted state, no protocol, no stored configuration.

## 10. Required tests

### Focused unit tests

None required. The milestone changes a command composition, not Rust logic.

### Integration tests

None new. The milestone's own correctness evidence is the work-package-C enumeration comparison, which is the integration-level check for this change.

### Restart and recovery tests

None.

### Contention and cancellation tests

- The bounded pass's exit-status semantics must be demonstrated: a deliberately failing bounded target must produce a non-zero pass exit. This is a required negative test and is the direct guard against the failure mode described in §8.
- A bounded target that cannot be built under the selected feature set must be loud, not silent.

### Security and negative tests

- The exit-status negative test above.
- The "missing target is loud" negative test above.
- If the enumeration is generated rather than literal, the generator's self-test must demonstrate it rejects a malformed entry, in the repository's `scripts/check_*.py` style.

### Migration and compatibility tests

- The executed-set comparison from work package C, with the count of executed test functions before and after.

## 11. Required verification commands

```bash
# narrow: the bounded pass in isolation, with its target enumeration visible
cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support --profile default \
  --test <target> --test <target> ...   # exact list per work package A

# the enumeration comparison (work package C)
cargo nextest list --workspace --locked --features server,plugins,lsp-test-support --profile default --json-type terse
cargo nextest list -p codegg --locked --features server,plugins,lsp-test-support --profile default --json-type terse

# formatting and linting
cargo fmt --check
cargo clippy --workspace --all-targets --locked -- -D warnings

# routine guard sweep
scripts/verify.sh quick

# broader suite
cargo nextest run --workspace --locked --profile ci
cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support --profile ci

# the full local path
scripts/verify.sh full

# hosted evidence
# .github/workflows/ci.yml — CI / verify, on the change
```

The `--json-type terse` listing form is the mechanism for the executed-set comparison; the implementing agent must confirm the exact flag against the installed `cargo-nextest` version (0.9.140 at the baseline) and record what was used.

## 12. Documentation updates

- `architecture/testing.md` — the verification-pass description: what the bounded pass covers, the profile it uses and why, the measured effect, and the corrected counts per successor roadmap §4.4.
- `scripts/verify.sh` — an inline comment at the bounded invocation explaining the target list's derivation and the two entries that are easy to get wrong (the root lib test target, and the feature-gated integration targets).
- `.config/nextest.toml` — its existing note that the `ci` profile is "workspace-wide only" already anticipates this change; it should be updated only if the implementing agent adds a named subset profile, and then only to record that profile's existence.
- `plans/subsystems/ci-test-throughput-recovery-roadmap.md` — M002's status row.
- `plans/registry.md` — M002's status transitions.

## 13. Acceptance criteria

1. The feature-enabled pass builds and runs only the enumerated bounded target set; the set of built test executables is materially smaller than the workspace-wide form, measured.
2. The count of test functions executed by the bounded pass is **not lower** than the count the previous workspace-wide feature-enabled pass executed, and every difference is individually accounted for.
3. The root lib test target runs in the bounded pass, and every feature-gated inline test function executes.
4. All nine feature-gated integration targets build and run under the feature set.
5. A deliberately failing bounded target produces a non-zero pass exit; a bounded target that cannot be built is loud.
6. The `ci` Nextest profile — its 4-slot bound and all seven `threads-required = "num-cpus"` exclusions — is byte-identical to the baseline.
7. No test file, test target, or test assertion changed; no dependency, feature default, or product behaviour changed.
8. `scripts/verify.sh quick` and `verify.sh full` both pass, and `cargo clippy --workspace --all-targets --locked -- -D warnings` passes.
9. `architecture/testing.md` records the hosted measurement, the method, the corrected counts, and what did not improve.

## 14. Stop conditions

The implementing agent must stop and report rather than improvise when:

- the root lib test target cannot be selected by any cargo-level mechanism, and the only way to run the feature-gated inline tests is a full workspace build — in that case the milestone's central saving is not achievable as specified and the approach must be re-planned;
- bounding the pass would drop a test function whose outcome can change with the feature set, and no per-test argument that the feature cannot affect it can be made;
- any bounded target is one of the six `ci`-profile whole-binary exclusion binaries, and the bounded pass cannot preserve that exclusion without either dropping the target or running the pass under a profile that does not carry it — a flaky bounded pass is worse than a slow one;
- the pass's exit-status semantics cannot be made to fail on a failing or unbuildable target;
- the fix would require changing the `ci` profile (I3);
- the change would require enabling `server` or `lsp-test-support` in the hosted job's default scope, which is the D1 coverage decision and is out of scope here;
- scope would expand into M003's scoped mode or M004's consolidation.

## 15. Closure evidence required

The closure record `plans/closure/ci-test-throughput-recovery/002-status.md` must contain:

1. the enumerated bounded target set, with the `Cargo.toml` declaration each entry derives from;
2. the answer to work package A step 3 (how the root lib test target is selected), with the command that established it;
3. the feature-gated inline test cross-check, with the count of gated inline test functions found;
4. the executed-set comparison — before and after enumerations, their difference, and a per-difference disposition;
5. the executed test-function count before and after, showing no decrease;
6. the built-executable count before and after, measured;
7. the exit-status negative test's output (a deliberately failing bounded target producing a non-zero exit);
8. the "unbuildable target is loud" negative test's output;
9. the hosted run id, wall-clock, and test-step build/execution split, labelled hosted, with the extraction method;
10. a comparison against the successor roadmap §4.1 baseline and against the predecessor's recorded run, stating explicitly that the predecessor's figure was taken against a different target count;
11. a confirmation that the `ci` profile is byte-identical to the baseline;
12. the documentation diff for `architecture/testing.md` and `scripts/verify.sh`;
13. the registry updates performed, including the dependency audit against the registry's blocked-work section.

## 16. Handoff notes

- **Resource constraint.** The root library rlib is 855 MB and root test executables are 135–208 MB each. The bounded pass still builds the root library and its test target, so it is not cheap in absolute terms — it is cheap *relative to* the workspace-wide form. Do not describe it as cheap.
- **Profile choice matters.** The bounded pass will not use the `ci` profile (§3.4). Its slot count therefore differs from the workspace-wide pass's 4. State the slot count in the closure record.
- **Toolchain.** Rust 1.89+, edition 2021, stable. `cargo-nextest` 0.9.140 at the baseline. `cargo --timings=json` is nightly-only — use plain `--timings`.
- **Do not treat `-E` as a build filter.** `-E 'binary(...)'` filters the run list only; every target of the package is still built. The milestone's saving depends on `--test`, not on `-E`.
- **The CI job has two Nextest invocations for different reasons.** The second one excludes the live Eggwork binary; the `verify.sh` second one enables extra features. Do not conflate them.
- **Preserved user changes.** The worktree may carry unrelated local modifications; do not stash, reset, or discard them.
