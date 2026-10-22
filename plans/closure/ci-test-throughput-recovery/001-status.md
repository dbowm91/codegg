# CI/Test Throughput Recovery M001 — Closure Status

Status: conditionally closed

Recommendation: **conditionally closed** — the implementation is complete and locally verified against every acceptance criterion; the hosted-CI measurement required by the plan's exit condition has not been produced and remains the one outstanding item.

Source implementation plan:

- `plans/implementation/ci-test-throughput-recovery/001-profile-alignment-single-dependency-graph.md`

Source subsystem roadmap:

- `plans/subsystems/ci-test-throughput-recovery-roadmap.md`

Repository baseline reviewed: `2c1953347896848c2f52c2dfbd9f04029d1c2cc2` (the tree the work was applied to, after the 2026-10-21 rebase onto `origin/main`; the plan itself was authored against `239f06e7fc876eaa36156dbf4658bb56314c9673`)

Implementation commits or pull requests:

- Not yet committed. The change set is: `Cargo.toml`, `scripts/check_cargo_profile_alignment.py` (new), `scripts/verify.sh`, `.github/workflows/ci.yml`, `.github/workflows/desktop-e2e.yml`, `architecture/testing.md`.

## 1. Executive finding

The `dev` and `test` cargo profiles now resolve identically, so the third-party dependency graph and the root library rlib are compiled **once** per verification invocation instead of twice. The change is a one-line profile setting plus a static guard; no test scope, assertion, feature boundary, or CI job shape changed.

The measured effect, on the real repository with a fresh target directory per arm:

| Arm | Distinct dependency crates compiled | Crates compiled **more than once** | `libcodegg_client-*.rlib` | `libtokio-*.rlib` | `libserde_json-*.rlib` |
|---|---|---|---|---|---|
| baseline (`profile.dev.strip = "none"`) | 45 | **45** | 2 | 2 | 2 |
| aligned (`profile.dev.strip = "debuginfo"`) | 45 | **2** | 2 | 2 | **1** |

The two crates that still compile twice (`tokio`, `codegg-client`) are **feature**-attributable, not profile-attributable: `codegg-client`'s `[dev-dependencies]` enables tokio's `macros, rt, rt-multi-thread`, so the test graph resolves a different tokio feature union than the build graph. See §5.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| WP-A1: reproduce the double compile in isolation and show a change that removes it | Two-crate probe at `/tmp/ct4` (workspace `a` → `b`), **profiles in the workspace root manifest** (an earlier attempt put them in a member manifest, where cargo ignores them — the probe then showed no difference and was discarded). Control (`dev` strip `none` vs `test` strip `debuginfo`): `cargo test` **recompiles** `a`, two distinct `liba-*.rlib` hashes. Variant (`dev` strip `debuginfo`): `cargo test` does **not** recompile `a`, one hash. | pass | The mechanism is cargo's profile hash, not the stripping itself. |
| WP-A2: the change removes the double compile on the repository | Fresh-target-dir A/B on `codegg-client` (a workspace member with a real third-party graph), `CARGO_BUILD_JOBS=2`: baseline 45/45 crates compiled twice; aligned 2/45. Artifact identity: `libserde_json-*.rlib` 2 → **1**; `libcodegg_client-*.rlib` 2 (feature-attributable); `libtokio-*.rlib` 2 (feature-attributable). | pass | See §4 for the exact commands. |
| WP-B: the CI job compiles each third-party crate once; dev-profile build and test-profile build share one graph | The CI job's dev-profile step is `cargo build --locked --bin codegg-sandbox-helper`; with the profiles aligned it resolves to the same units as the `cargo nextest run --workspace` step. Baseline evidence: the repository's own `target/debug/deps` held **two** `libcodegg_client-*.rlib` copies (85,468,040 and 85,445,208 bytes — the dev copy keeps debuginfo). | pass (local) | A whole-workspace CI-shape measurement was attempted and **failed on this machine**: the root library compile was SIGTERM'd under memory pressure from three concurrent worktrees. The member-level A/B above carries the same claim at 1/45 the cost. Hosted evidence outstanding — see §10. |
| WP-C: a static guard prevents regression | `scripts/check_cargo_profile_alignment.py` resolves both profiles the way cargo does (`test` inherits `dev`) and fails when any hashed field — or any per-package override — differs. | pass | See §3. |
| WP-D: the guard fails on a deliberately mis-aligned tree | The guard was run against the **real** baseline `Cargo.toml` (restored from `HEAD`): it fails with `profile.dev.strip = 'none' but profile.test.strip = 'debuginfo'`. Restored to the aligned manifest: it passes. | pass | The repository-side evidence does not rest on the `--self-test` alone. |
| Acceptance: the documented backtrace-recovery path still works | **It did not work before this milestone, and the plan's suggested alternative did not work either.** Measured: `RUSTFLAGS="-C debuginfo=2"` produces a byte-identical artifact to the plain build (404,528 bytes both, zero DWARF sections) because the profile's `strip` still applies. `cargo build --config 'profile.dev.strip="none"'` produces the unstripped artifact (439,944 bytes). | pass (corrected) | The `Cargo.toml` comment and `architecture/testing.md` now name the **working** recovery command. |
| Acceptance: test executables are not materially larger | The `test` profile's `strip = "debuginfo"` is unchanged. The `dev` profile now also strips, which *shrinks* dev artifacts. | pass | Invariant I4 holds. |
| Acceptance: no behavioural assertion removed for timing | No test file, assertion, feature gate, or Nextest filter changed. `git diff` touches build configuration, one workflow, one doc, and the plan set. | pass | Invariant I1 holds. |
| Acceptance: one bounded CI job | `.github/workflows/ci.yml` gains two guard steps; no job, lane, or matrix is added or removed. | pass | Invariant I2 holds. |
| Acceptance: hosted CI scope never shrinks | No `--features`, `--test`, or `-E` filter changes in any workflow. The hosted job runs exactly the targets it ran before. | pass | Invariant I3 holds. |

## 3. The guard

`scripts/check_cargo_profile_alignment.py`:

- resolves `[profile.dev]` and `[profile.test]` the way cargo does — `test` inherits `dev`, then the manifest's own table applies on top — over cargo's hashed field set (`opt-level`, `debug`, `strip`, `debug-assertions`, `overflow-checks`, `codegen-units`, `incremental`, `panic`, `rpath`, `lto`, `split-debuginfo`);
- compares the per-package overrides too (`[profile.dev.package."*"]` and friends), because a dependency resolving to different units under the two profiles is the same double compile;
- `--self-test` injects four violation classes — an un-stripped `dev` profile, a changed `codegen-units` on `test`, a dev-only per-package override, and a malformed manifest — and fails if any of them is *not* rejected. All four are rejected, and the unmodified tree still passes.

The guard is wired into `scripts/verify.sh quick` and into the `CI / verify` job. It runs **before** the expensive compile, so a regression is caught in milliseconds rather than after a full dependency build.

## 4. Verification executed

### Commands run

```bash
# Mechanism, isolated (workspace root manifest — cargo ignores member profiles)
CARGO_TARGET_DIR=/tmp/ct4-t1 cargo build -p b && CARGO_TARGET_DIR=/tmp/ct4-t1 cargo test -p b --no-run
CARGO_TARGET_DIR=/tmp/ct4-t2 cargo build -p b && CARGO_TARGET_DIR=/tmp/ct4-t2 cargo test -p b --no-run

# Repository A/B, fresh target dir per arm
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/tmp/m001-lean-baseline \
  cargo build --locked -p codegg-client --config 'profile.dev.strip="none"'
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/tmp/m001-lean-baseline cargo nextest list -p codegg-client
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/tmp/m001-lean-aligned cargo build --locked -p codegg-client
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/tmp/m001-lean-aligned cargo nextest list -p codegg-client

# Guard
python3 scripts/check_cargo_profile_alignment.py
python3 scripts/check_cargo_profile_alignment.py --self-test
git show HEAD:Cargo.toml > Cargo.toml && python3 scripts/check_cargo_profile_alignment.py   # expects exit 1
```

### Results

- Isolated probe: control produced two `liba-*.rlib` hashes and recompiled `a` under `cargo test`; variant produced one hash and did not recompile `a`.
- Repository A/B: baseline arm — 45 distinct crates, **all 45** compiled more than once; `libcodegg_client-*.rlib` 2, `libtokio-*.rlib` 2, `libserde_json-*.rlib` 2. Aligned arm — 45 distinct crates, **2** compiled more than once (`tokio`, `codegg-client`); `libserde_json-*.rlib` **1**.
- Guard: pass on the aligned tree; `--self-test` rejected all four injected violations; exit 1 with the specific field diff on the restored baseline manifest.
- Backtrace probe: `strip = "debuginfo"` leaves the symbol table intact, so a panic backtrace still names its frames (`stripprobe::helper`, `stripprobe::main`) and shows the panic location. `RUSTFLAGS="-C debuginfo=2"` did **not** restore DWARF (byte-identical artifact). `--config 'profile.dev.strip="none"'` did.

## 5. Why two crates still compile twice

`tokio` and `codegg-client` compile twice in the aligned arm as well. The cause is **not** the profile:

```
crates/codegg-client/Cargo.toml
[dependencies]      tokio = { workspace = true, features = ["io-util","macros","net","process","rt","sync","time"] }
[dev-dependencies]  tokio = { workspace = true, features = ["macros","rt","rt-multi-thread"] }
```

The test graph resolves tokio with the union of both feature sets (`bytes,default,io-util,libc,macros,mio,net,process,rt,rt-multi-thread,signal-hook-registry,socket2,sync,time,tokio-macros`), the build graph without `rt-multi-thread`. Different feature sets are different units regardless of profile, so tokio and the crate that pulls it are compiled once per feature union. This is the plan's §3.2 "two profiles × two feature sets" observation, now isolated to the feature axis.

**Profile-attributable duplication is zero after the change.** Every other dependency crate in the graph — 43 of 45 — now compiles exactly once across a dev-profile build and a test-profile build.

## 6. Invariant review

- **I1 (no behavioural assertion removed for timing):** held. No test file, assertion, `required-features` gate, or Nextest filter changed.
- **I2 (one bounded CI job):** held. Two guard steps added to the existing `verify` job; no new job, lane, or matrix.
- **I3 (hosted CI scope never shrinks):** held. The hosted job's executed set is byte-identical to before.
- **I4 (counts verified against the listed Source, never copied):** held, and this milestone corrected three unverified counts inherited from the plan — see §9.
- **I5 (no new permanent framework):** held. The guard is a single static script in the repository's existing `scripts/check_*.py` idiom, with a `--self-test`, exactly like its siblings.

## 7. Migration and compatibility review

No schema, protocol, storage, or public-surface change.

**Developer-local compatibility — the one real cost.** `[profile.dev]` now strips debuginfo, so a locally built binary no longer carries DWARF. What is preserved, measured:

- the **symbol table** is kept, so panic backtraces still name their frames and still show the panic location;
- full debuginfo is one flag away: `cargo build --config 'profile.dev.strip="none"'` (and `cargo test --config 'profile.test.strip="none"'`).

What was **lost** unless explicitly restored: per-frame line numbers and inline-frame detail in a local `cargo run` backtrace.

**The plan's suggested recovery path was wrong and is corrected here.** The plan offered `RUSTFLAGS=-C debuginfo=2 cargo test …` as the recovery, consistent with the comment already in `Cargo.toml`. Measured: that does **not** restore debuginfo, because the profile's `strip` still applies to the artifact. The `Cargo.toml` comment and `architecture/testing.md` now name the command that works.

**The one place that needs the unstripped binary is preserved.** `.github/workflows/desktop-e2e.yml` bundles the root daemon binary into the Tauri desktop app, where macOS crash symbolication is worth keeping. That build now runs `cargo build --locked --config 'profile.dev.strip="none"'`. The workflow runs no test suite, so it has no double compile to reclaim and nothing is lost by the override.

Release artifacts are unaffected: `[profile.release]` already had `strip = true` and is a separate profile.

## 8. Security review

No authorization, secret, network, privilege, or dependency change. The guard reads `Cargo.toml` and writes nothing. The `--self-test` writes a temporary sibling file (`Cargo.toml.cargo-profile-alignment-self-test`) and deletes it in a `finally` block; it never touches the real manifest.

## 9. Documentation and operations

- `Cargo.toml`: the profile rationale, the recovery command, and the pointer to the guard.
- `architecture/testing.md`: a new "Cargo profile alignment — one compiled dependency graph" section; the CI baseline section's **unverifiable binary counts** corrected (see §10); the current measured root test-target count recorded.
- `.github/workflows/ci.yml`: two guard steps.
- `.github/workflows/desktop-e2e.yml`: the unstripped-build override, with its reason.
- `scripts/verify.sh`: the guard, before the compile.

**Counts corrected in this milestone** (the successor roadmap's invariant I4 requires verification against the listed Source, never copying a number between documents):

1. `architecture/testing.md` recorded "84 root integration-test binaries after M003/M004 consolidation (historical 100 pre-consolidation)" from hosted run `36196068239`. The tree at the revision that recorded them (`99117577f`) holds **208** top-level `tests/*.rs` files and 8 declared `[[test]]` entries — about 2.5× the recorded figure — so those counts are not reproducible from the tree and are now labelled unverified. The **test-function** counts in the same record (11,726 passed / 1 skipped; 11,781 pre-consolidation) stand and were not touched.
2. The current root test-target count is **288** (286 auto-discovered `tests/*.rs` files plus the two consolidated directory targets), with **13** feature-gated targets and **314** workspace-wide. Method: `cargo metadata --no-deps --format-version 1`, counting targets of kind `test` per package.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| **medium** | The plan's hosted-CI measurement has not been produced. Local numbers are additionally contaminated: three concurrent owner worktrees were compiling on the same 14-core / 24 GB machine throughout, and a whole-workspace CI-shape measurement was SIGTERM'd under memory pressure. | The mechanism and the effect are measured and reproduced on the real repository; only the hosted wall-clock delta is unmeasured. | Produce one hosted run on the aligned tree and record the build-phase delta in §11. Until then the milestone stays **conditionally closed**, not closed. |
| low | `tokio` and `codegg-client` still compile twice across the build and test graphs. | None for this milestone — the duplication is feature-attributable, and it is the same on the baseline. | None. Recorded in §5 so a future reader does not mistake it for a profile regression. |
| low | The dev profile no longer carries debuginfo. | Local `cargo run` backtraces lose per-frame line numbers. Function names and the panic location are preserved. | None — the recovery command is documented in `Cargo.toml` and `architecture/testing.md`, and the desktop bundle keeps DWARF. |
| low | `architecture/testing.md`'s historical "84 binaries" figure could not be reconciled with the tree. | The figure is now labelled unverified; it was not used as an input to any measurement in this milestone. | The successor roadmap's M004 milestone arms a guard that keeps the recorded target count and the real one in agreement. |

No high or critical findings. Nothing blocks M002.

## 11. Roadmap disposition

Milestone conditionally closed; M002 may proceed — M002's dependency on M001 is operational, not hard, and the two changes are in disjoint files (`Cargo.toml` / the guard / `ci.yml` / `desktop-e2e.yml` versus `.config/nextest.toml` / the second `verify.sh` pass).

Handoff evidence for M002: the profile alignment removes one whole-graph compile from the job, so the second `verify.sh` pass now pays only for the feature-set difference. M002's own saving — bounding that pass to the ten targets the extra features unlock — is independent of M001 and was implemented against the same baseline.

Outstanding for full closure: one hosted run on the aligned tree, with the build-phase delta recorded here.

## 12. Registry updates

- `plans/registry.md`: the M001 row `ready → active (locally verified; hosted evidence outstanding)`.
- `plans/subsystems/ci-test-throughput-recovery-roadmap.md`: §10 decision D6 recorded as **decided 2026-10-21 — manifest alignment**, with the reason and the measured recovery-path correction; §12 M001 `not started → active`.
- `plans/implementation/ci-test-throughput-recovery/001-*.md`: baseline recorded as `2c1953347`, D6 resolved in the plan, and the `--config` recovery path corrected in §9.
