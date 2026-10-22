# CI and Test Throughput Recovery Milestone M001 — Align the Dev and Test Cargo Profiles

Status: ready for handoff

Repository baseline: `239f06e7fc876eaa36156dbf4658bb56314c9673` (the tree the plan was authored against)

Implementation baseline: `2c1953347896848c2f52c2dfbd9f04029d1c2cc2` (the tree the work was applied to, after the 2026-10-21 rebase onto `origin/main`)

Source roadmap:

- `plans/subsystems/ci-test-throughput-recovery-roadmap.md#milestone-m001--align-the-dev-and-test-cargo-profiles`

Long-term requirements:

- `plans/000-long-term-specification.md` §4.7 — correctness before transparent magic;
- `plans/003-planning-process.md` §2.3–2.4.

Applicable ADRs: none required. This milestone changes cargo profile configuration and CI job invocation only. It does not alter product authority, protocol, storage, dependency selection, or runtime architecture.

Predecessor work (closed, historical, not reopened by this milestone):

- `plans/subsystems/ci-test-throughput-optimization-roadmap.md` M001–M005;
- `plans/subsystems/ci-test-throughput-optimization-post-closure-corrective-addendum.md` C001–C002.

Primary class: infrastructure, with a polish obligation.

---

## 1. Objective

One compiled instance of the third-party dependency graph per verification invocation.

Concretely: after this milestone, a `cargo build` followed by a `cargo test` over the same tree compiles each dependency crate **once**, and the hosted `CI / verify` job compiles the third-party dependency graph **once** rather than twice.

## 2. Why this milestone is ready

**Hard dependencies:** none. The predecessor CI-throughput workstream and its corrective addendum are closed; the cargo profile and the CI job are stable surfaces with no owner conflict.

**Interface dependencies:** none. The two surfaces this milestone touches — `Cargo.toml`'s profile table and `.github/workflows/ci.yml`'s cargo invocations — are both owned by the verification harness and are not currently being modified by another registered plan.

**Why it is first:** it is the cheapest change in the roadmap, it removes an entire redundant compilation from every verification invocation, and every later milestone's compile-time claim is only interpretable against a single-graph baseline. M003 has a hard dependency on it for exactly that reason.

## 3. Current implementation evidence

### 3.1 The profile difference

`Cargo.toml` declares:

```toml
[profile.dev]
codegen-units = 16

[profile.test]
opt-level = 0
codegen-units = 16
# Intentionally strips debuginfo for size; when triaging a test failure
# locally, restore backtraces with: RUSTFLAGS=-C debuginfo=2 cargo test ...
strip = "debuginfo"
```

The resolved `dev` profile is `opt-level = 0`, `codegen-units = 16`, `debug = 2`, `strip = "none"`. The resolved `test` profile is `opt-level = 0`, `codegen-units = 16`, `debug = 2`, `strip = "debuginfo"`. **`strip` is the only differing field.**

Cargo hashes the *resolved* profile into each compilation unit's identity. Two profiles that differ in any hashed field produce distinct units for the same source, so the same crate is compiled and linked twice.

### 3.2 The duplicate artifacts are present at this baseline

On the baseline tree, `target/debug/deps/` contains:

- `libcodegg_client-*.rlib` — **two** files, **81.5 MB each**, byte-size identical and differing only in their metadata hash;
- `libtokio-*.rlib` — **four** distinct hashes (the dev/test profile pair plus distinct feature sets);
- `libcodegg-*.rlib` — 855 MB (the root library).

And with a fully warm tree, `cargo nextest run -p codegg --test test_util --no-run` recompiled `codegg` and `codegg-client` from source (4m47s wall) before running a single test, because the test-profile artifacts for those crates did not exist.

### 3.3 The CI job exercises both profiles

`.github/workflows/ci.yml` runs, in order:

- `cargo clippy --workspace --all-targets --locked -- -D warnings` (dev profile, check mode);
- `cargo build --locked --bin codegg-sandbox-helper` (**dev profile, full build**);
- `cargo nextest run --workspace --locked --profile ci` (**test profile, full build**);
- `cargo nextest run --workspace --locked --profile ci -E 'not binary(eggwork_remote_execution_live)'` (a second test-profile build).

The dev-profile binary build and the test-profile test build therefore compile the entire third-party graph twice. Measured against the baseline, that is **422 third-party crates plus the root library and the twelve workspace path dependencies** compiled twice per job.

### 3.4 The mechanism is proven in isolation

On a two-crate probe workspace (`a` is a dependency of `b`), with a clean target directory per variant:

- **Control** (profiles differing in `strip`): `cargo build -p b` then `cargo test -p b --no-run` prints `Compiling a v0.1.0` — the dependency is recompiled.
- **Variant** (`CARGO_PROFILE_DEV_STRIP=debuginfo` applied to both invocations): the same sequence does **not** print `Compiling a` — the dependency is reused.

The mechanism is therefore understood and reproducible; this milestone's work is to apply it correctly to a job whose cargo invocations are spread across several steps and two different profiles.

### 3.5 The predecessor's related measurement, and why it does not settle this

The predecessor recorded a measured-negative disposition for `CARGO_PROFILE_TEST_DEBUG=0`: a codegen-isolated local A/B on one representative large test target showed wall parity (2m22s vs 2m23s, non-incremental) with only CPU-time reduction, and required hosted proof before any CI-only override.

That measurement is about **`debug`**, a different field, and it was taken on a single target rather than on the whole graph. It is not evidence about `strip`, and it does not bear on this milestone. The predecessor's `strip = "debuginfo"` choice — keeping test binaries small — **is** preserved by this milestone; what changes is that the *dev* profile must resolve to the same `strip` value so the two profiles collapse onto one unit. The predecessor's own comment says the intent was size ("Intentionally strips debuginfo for size"), and that intent is honoured.

## 4. Invariants that must not regress

- **I1 — No behavioural assertion is removed for timing.** This milestone touches no test file, no test target, and no test assertion.
- **I2 — One bounded `CI / verify` job.** No new job, matrix, or lane.
- **I3 — The `ci` Nextest profile's slot bound and whole-binary exclusions are untouched.** This milestone changes no Nextest profile.
- **I4 — Test binaries stay small.** The `strip = "debuginfo"` intent for the test profile is preserved. The milestone must not make test executables materially larger in order to collapse the profiles.
- **I5 — `verify.sh quick` and `verify.sh full` keep their existing step order and semantics**, except that cargo invocations may share a resolved profile.
- **I6 — The routine CI job keeps `contents: read` and no release or publication authority.**
- **I7 — The dependency policy is unchanged.** No dependency is added, removed, upgraded, or re-versioned. This milestone changes *when* a crate is compiled, never *whether* it is a dependency.

## 5. Scope

### In scope

- The `strip` field in the root `Cargo.toml`'s `[profile.dev]` and `[profile.test]`.
- The cargo invocations in `.github/workflows/ci.yml` that build or check the workspace, and any equivalent job-scoped environment that must be applied to keep **every** cargo invocation in the job on one resolved profile.
- Any equivalent alignment needed in `scripts/verify.sh` so that the local `full` path does not compile the graph twice either.
- A structural check that the alignment holds (see §7, work package C).

### Explicitly out of scope

- Changing `debug`, `opt-level`, `codegen-units`, `incremental`, `lto`, or `panic` in any profile. The predecessor measured `CARGO_PROFILE_TEST_DEBUG=0` as wall-parity and that disposition stands.
- Changing the release profile.
- Any dependency change (I7).
- Any test, test target, or test assertion (I1).
- The Nextest profiles and their exclusions (I3).
- The `verify.sh` second-pass restructure (M002) and the scoped mode (M003).

## 6. Required production changes

### Core/domain

None. No runtime behaviour changes.

### Storage and migrations

None.

### Protocol and DTOs

None.

### Runtime and concurrency

None. This is a build-configuration change; it must not alter how any process is spawned, scheduled, or cancelled.

### Frontend or operator surface

None.

### Security and authorization

None. No permission, approval, sandbox, or execution-authority change.

### Documentation and static guards

- `architecture/testing.md` — the profile description and the CI baseline section must be updated to record the single-graph property, the resolved-profile change, and the hosted measurement. The existing "verified counts" material must not be extended with a number this milestone did not measure.
- `Cargo.toml` — a comment recording *why* the two profiles are aligned, so a later edit that reintroduces a difference is visibly a regression rather than a cleanup. The predecessor's own comment style (a rationale above the setting) is the right shape.
- A static guard, if the alignment is implemented as a manifest change, that fails when the resolved dev and test profiles differ in any hashed field. If the alignment is implemented as a job-scoped environment override (§10, D6), the guard's subject changes to the job's cargo invocation set instead — see §7 work package C for both forms.

## 7. Ordered work packages

### Work package A — Choose and implement the alignment mechanism

Intent: collapse the dev and test profiles onto one unit identity for every cargo invocation in the verification job.

Required changes:

1. Resolve roadmap decision **D6** (job-scoped environment override vs manifest alignment). The trade-off, which the implementing agent must weigh rather than pick blindly:
   - **Job-scoped environment override** (for example `CARGO_PROFILE_DEV_STRIP=debuginfo` exported into the CI job): invisible to a developer's local `cargo build`, but it is applied *only* to the invocations that carry it. If the job's clippy step, its binary-build step, and its Nextest steps do not all carry the same override, the graph is still compiled twice. It must also be applied to `scripts/verify.sh` separately for local benefit, or the local loop keeps paying the double compile.
   - **Manifest alignment** (set the same `strip` in both profile tables): durable and applies everywhere automatically, including a developer's local `cargo build` → `cargo test` sequence. It does mean the dev-profile debug binary is stripped too, which is a local-debugging consideration, and it must be checked against I4 (test binaries stay small) and against the AGENTS.md note that backtraces are restored for test triage via `RUSTFLAGS=-C debuginfo=2 cargo test …`.
   - Either way, **the overriding constraint is that every cargo invocation within one verification invocation must resolve to the same profile.** A partial application is worse than none, because it looks fixed and is not.
2. Apply the chosen mechanism to the CI job's cargo invocations and to `scripts/verify.sh`.
3. Preserve the existing intent that test executables stay small (I4).

Acceptance evidence:

- The mechanism's effect is demonstrated on an isolated two-crate probe exactly as in §3.4 — control recompiles the dependency, the applied mechanism does not — with the probe's commands and output recorded.
- The mechanism is then re-demonstrated on the repository: after the change, a `cargo build` followed by a `cargo test` over the same tree does **not** produce a second distinct artifact for any dependency crate, and `libcodegg_client-*.rlib` does not gain a second hash variant attributable to the profile difference.

### Work package B — Prove the job compiles the graph once

Intent: show the redundancy is gone in the shape the CI job actually exercises.

Required changes:

1. Instrument or otherwise observe the CI job's cargo invocations and record how many times each third-party crate is compiled.
2. Confirm that the dev-profile binary build and the test-profile test build now share one compiled graph.

Acceptance evidence:

- A per-crate compile-count table for the CI job's cargo steps, showing each third-party crate compiled once.
- The root library and the twelve workspace path dependencies each compiled once.

### Work package C — Arm a check so the alignment cannot silently regress

Intent: a later edit that reintroduces a hashed profile difference must fail loudly rather than quietly restoring the double compile.

Required changes:

1. Add the check in whichever form matches the mechanism chosen in work package A:
   - manifest alignment → a static guard that resolves the root `Cargo.toml`'s dev and test profiles and fails when any hashed field differs;
   - job-scoped override → a static guard that fails when a cargo invocation in the CI job or in `scripts/verify.sh` is not covered by the override.
2. The check must carry a `--self-test` that injects a violating profile difference (or an uncovered invocation) and proves the check rejects it, in the same style as the repository's other change-triggered guards. A check that cannot fail is not a check.
3. Wire the check into `scripts/verify.sh quick` and the CI job, following the repository's convention that a change-triggered guard lives in `scripts/check_*.py`.

Acceptance evidence:

- The check passes on the changed tree.
- The check's `--self-test` injects a violation and the check rejects it, with both outputs recorded.
- The check is listed in `scripts/verify.sh quick` and in the CI job's guard invocation.

### Work package D — Measure and record

Intent: produce the hosted evidence the closure requires, and correct the documentation.

Required changes:

1. Run the hosted `CI / verify` job on the change and record the total wall-clock and the test-step build and execution split, labelled as hosted.
2. Compare against the §4.1 baseline and against the predecessor's recorded run, stating clearly that the predecessor's figure was taken against a different (and, per the successor roadmap §4.4, inaccurate) target count.
3. Update `architecture/testing.md`: the profile description, the CI baseline, and the corrected counts.
4. Do **not** record a count this milestone did not measure. The successor roadmap's invariant I4 applies to this milestone's own outputs.

Acceptance evidence:

- Hosted run id, wall-clock, build/execution split, and the method used to extract them.
- `architecture/testing.md` updated, with the profile rationale recorded next to the setting.
- A statement of what the change did **not** improve, if the hosted number moved less than the local model predicted.

## 8. Failure, cancellation, restart, and contention semantics

- **Partial failure.** If the check in work package C fails on a tree where the alignment is correct, the guard is wrong, not the tree. The guard's `--self-test` is the arbiter.
- **Rebuild races.** Two cargo invocations in the same job writing the same target directory is not a new condition — the job already serialises its steps — but the implementing agent must confirm the alignment does not introduce concurrent invocation of the same unit.
- **Cache invalidation.** The alignment changes the resolved profile, so every existing cached artifact under the old profile is invalidated once. That is expected and is a one-time cost; it must not be mistaken for a regression in the hosted wall-clock, and the hosted measurement must be taken from a run that is not serving stale-cache compensation.
- **No runtime effect.** There is no process, job, or scheduler semantics to define. If the implementation appears to require any, it has left scope.

## 9. Compatibility and migration

- **Developer-local compatibility.** A developer who relies on a dev-profile binary carrying debuginfo (for example `cargo run` backtraces) is affected if the manifest-alignment form is chosen. This must be stated in the plan's handoff notes and, if it is a real regression for that workflow, resolved — for example by keeping `RUSTFLAGS=-C debuginfo=2` available as the documented recovery path, consistent with the precedent already recorded in `Cargo.toml`'s own comment. It must not be resolved by silently reverting the alignment.
- **CI compatibility.** No compatibility surface. The job's inputs, outputs, permissions, and step structure are unchanged.
- **Migration.** None. No persisted state.

## 10. Required tests

### Focused unit tests

None required. This milestone changes build configuration; it introduces no Rust logic. If the work-package-C check is a Python static guard, its correctness is established by its `--self-test`, not by a Rust unit test.

### Integration tests

None required for the mechanism itself.

The **mechanism demonstration** in work package A is the required evidence, and it must be run on an isolated probe workspace with a clean target directory per variant, because the artifact-identity claim is false if the target directory is warm.

### Restart and recovery tests

None.

### Contention and cancellation tests

None. The Nextest profiles and their exclusions are untouched (I3).

### Security and negative tests

- The static check's `--self-test` is the negative test: it must demonstrate the check rejects an injected violating profile difference.
- The check must additionally be shown to fail on a deliberately mis-aligned tree, so the repository-side evidence does not rest on the `--self-test` alone.

### Migration and compatibility tests

- If the manifest-alignment form is chosen: demonstrate that the documented backtrace-recovery path (`RUSTFLAGS=-C debuginfo=2 cargo test …`) still restores backtraces for test triage after the change, since the milestone must not remove the repository's existing debugging affordance.

## 11. Required verification commands

```bash
# narrow: the new static guard, including its self-test
python3 scripts/check_cargo_profile_alignment.py
python3 scripts/check_cargo_profile_alignment.py --self-test

# the mechanism probe (isolated workspace, clean target dir per variant)
# — control and variant commands recorded in the closure record

# narrow Rust check: the workspace still type-checks under the aligned profile
cargo check --workspace --all-targets --locked

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

The guard script's exact name is the implementing agent's choice provided it follows the repository's `scripts/check_*.py` convention; the closure record must state the name actually used.

## 12. Documentation updates

- `architecture/testing.md` — the cargo profile description (record *why* the profiles are aligned, next to the setting), the CI baseline section (hosted measurement and method), and the corrected counts per successor roadmap §4.4.
- `Cargo.toml` — a rationale comment above the profile settings, matching the existing comment style.
- `plans/subsystems/ci-test-throughput-recovery-roadmap.md` — M001's status row moves to `implemented`/`closing` as closure proceeds; the roadmap is not rewritten to match the outcome.
- `plans/registry.md` — M001 moves from `ready` to `active` on implementation start, then to `closing`, then to `closed` per the planning process's status rules.
- `docs/` — no user-facing documentation changes; this milestone is invisible to users of the product.

## 13. Acceptance criteria

1. A `cargo build` followed by a `cargo test` over the same tree compiles each dependency crate once, demonstrated on an isolated probe with a clean target directory and re-demonstrated on the repository by artifact identity.
2. The CI job compiles each third-party crate once; the dev-profile binary build and the test-profile test build share one compiled graph.
3. No dependency was added, removed, upgraded, or re-versioned (I7).
4. No test file, test target, or test assertion was changed (I1).
5. The Nextest profiles and their whole-binary exclusions are byte-identical to the baseline (I3).
6. Test executables are not materially larger than at the baseline (I4).
7. A static check with a passing `--self-test` is wired into `scripts/verify.sh quick` and the CI job, and the check rejects an injected violating tree.
8. `scripts/verify.sh quick` passes and `cargo clippy --workspace --all-targets --locked -- -D warnings` passes.
9. `architecture/testing.md` records the hosted measurement, the method, and the corrected counts, and states plainly what did not improve if the hosted number moved less than the local model predicted.

## 14. Stop conditions

The implementing agent must stop and report rather than improvise when:

- aligning the profiles turns out to require changing `debug`, `codegen-units`, `opt-level`, or any other hashed field beyond `strip` — that is a different milestone and reopens the predecessor's measured-negative `debug` disposition;
- the job-scoped override cannot be applied to *every* cargo invocation in the job, because a partial application would leave the redundancy in place while looking fixed;
- the alignment requires changing a dependency's feature set or version (I7);
- the alignment materially enlarges test executables, violating I4;
- the hosted measurement shows no material reduction **and** no structural reduction in compile count — in that case record the negative result and revert, rather than keeping a change that only complicates the configuration;
- any owned path of the change would require altering product authority, protocol, or storage — out of scope for this roadmap.

## 15. Closure evidence required

The closure record `plans/closure/ci-test-throughput-recovery/001-status.md` must contain:

1. the probe experiment's exact commands and outputs, control and variant, with clean target directories;
2. the repository-side artifact evidence — the dependency-crate artifact set before and after, showing no second profile-attributable artifact;
3. the per-crate compile-count table for the CI job's cargo steps;
4. the hosted run id, total wall-clock, and the test-step build/execution split, labelled hosted, with the extraction method;
5. a comparison against the successor roadmap §4.1 baseline and against the predecessor's recorded run, **stating explicitly** that the predecessor's figure was taken against a different target count;
6. the static check's name, its `--self-test` output, and the output of the check run against a deliberately mis-aligned tree;
7. a confirmation that no test file, test target, or test assertion changed, and that the Nextest profiles are byte-identical to the baseline;
8. a statement of what the change did not improve;
9. the documentation diff for `architecture/testing.md` and `Cargo.toml`;
10. the registry updates performed, including the dependency audit against the registry's blocked-work section.

## 16. Handoff notes

- **Resource constraint.** The baseline build is heavy: the root library rlib is 855 MB and root test executables are 135–208 MB each, totalling 13.7 GB for the root package. Ensure the machine has adequate disk and do not run parallel verification invocations against the same target directory.
- **Contention.** Baseline measurements were taken while other worktrees were compiling. Take the hosted measurement on the hosted runner and label it hosted; do not compare it to a local number taken under contention without saying so.
- **Toolchain.** Rust 1.89+, edition 2021, stable. `cargo-nextest` 0.9.140 is required for the broader suite command; `cargo --timings=json` is **nightly-only** and must not be used — use plain `--timings` and read `target/cargo-timings/cargo-timing.html`.
- **Preserved user changes.** The worktree may carry unrelated local modifications; do not stash, reset, or discard them.
- **Known hazard.** The alignment invalidates every artifact cached under the old profile exactly once. A hosted run that reports an unexpectedly long first step is likely serving that invalidation, not a regression.
- **Serial-test requirement.** This milestone does not touch the Nextest `ci` profile. Do not adjust its exclusions; the recorded flake history (8-slot oversubscription reverted 2026-09-25; `scheduler_cancellation` and `causal_observe_replay` added for measured hosted flakes) still governs.
