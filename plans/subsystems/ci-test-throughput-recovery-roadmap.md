# CI and Test Throughput Recovery Roadmap

Status: proposed

Repository planning baseline: `239f06e7fc876eaa36156dbf4658bb56314c9673`

Primary class: polish / development infrastructure.

Canonical references:

- `plans/000-long-term-specification.md` §4.7 — correctness before transparent magic;
- `plans/003-planning-process.md` §2.3–2.4 — subsystem roadmaps and executable milestone plans;
- `plans/subsystems/ci-test-throughput-optimization-roadmap.md` — **closed** predecessor that defined the minimal verification contract;
- `plans/subsystems/ci-test-throughput-optimization-post-closure-corrective-addendum.md` — **closed** predecessor that reconciled the predecessor's evidence and documentation defects;
- `architecture/testing.md` — the active test-resource and routine-CI contract;
- `.github/workflows/ci.yml`, `.config/nextest.toml`, `scripts/verify.sh`, `Cargo.toml` — the executable verification surfaces this roadmap changes.

Related ADRs: none required. This roadmap changes development and CI mechanics only. It does not alter product authority, protocol, storage, release ownership, dependency selection, or runtime architecture.

Predecessor relationship: this roadmap is a **successor**, not a corrective pass. The predecessor's M001–M005 and its corrective C001–C002 are closed historical work and remain authoritative for what they landed. This roadmap opens because a *new* measurement at a *new* baseline found that the routine CI build phase is structurally more expensive than the predecessor's closure baseline described, and that the predecessor's headline count was never verified against the repository. This roadmap does not reopen any predecessor milestone.

---

## 1. Purpose and ownership boundary

Routine hosted CI must give a developer usable feedback inside the working session, and the per-commit local loop must cost only what the change actually touched.

At the baseline recorded in §4, the measured shape of the problem is:

- the root package's **282** cargo test targets account for **44.1%** of all workspace compile unit-time, and the root crate's own two lib compilations account for a further **38.6%** — so the root package is **~85%** of all compile work in the workspace;
- the CI job compiles the **entire third-party dependency graph twice**, because `[profile.dev]` and `[profile.test]` differ in exactly one hashed field (`strip`);
- `scripts/verify.sh full` runs the workspace suite **twice**, the second time with a different feature set, re-linking essentially every binary it just linked;
- the predecessor's closure baseline recorded **84** root integration-test binaries, while the repository at that same revision carried **~203**, and carries **282** at this baseline.

**This roadmap owns:** the cargo profile configuration used for verification builds, the composition and invocation of the verification test passes, the topology of the root package's integration-test targets, and the change-scoped selection contract for per-commit verification.

**This roadmap does not own:** which tests exist and what they assert (test semantics are owned by the subsystem roadmaps that wrote the behaviour); product authority, permission, or execution boundaries; dependency selection and the workspace dependency policy; CI concurrency and hosted-runner provisioning decisions owned by the platform; release gating.

**Boundary rule:** a milestone in this roadmap may change *how many* test binaries exist, *when* they are built, and *which* of them run for a given change. It may not delete a behavioural assertion in order to save time. Where the two conflict, the assertion wins and the milestone stops.

## 2. Work classification

### Invariants

- **I1 — No behavioural assertion is removed for timing.** Every test that exists at a milestone's baseline still asserts the same thing afterwards, unless a closure record names it as a proven duplicate of a surviving test and records the surviving test by name. Timing never justifies deletion.
- **I2 — The routine CI job remains one bounded `CI / verify` job.** No new permanent CI matrix, lane, or job is introduced by this roadmap. A milestone that cannot be expressed inside the existing job stops and reports.
- **I3 — A change-scoped run is a superset of what the change touches, never a subset of what CI runs.** Scoped selection is an *addition* to the verification surface for the developer loop. The hosted job keeps running the full suite; scoped selection never becomes a way for CI to run less.
- **I4 — The recorded verification counts are verified, not copied.** Any count this roadmap publishes (test targets, test functions, binaries, wall-clock) states the command that produced it and the baseline it was produced at.
- **I5 — No new permanent framework.** The work uses Cargo, Nextest, and the repository's existing `scripts/check_*.py` static-guard idiom. It does not introduce a test-runner replacement, a build farm, a remote execution dependency, or a custom test-selection framework.

### Capabilities

- **C1 — A developer can run a verification pass bounded to what their change touched.** A change-scoped mode produces a defensible, evidence-backed answer to "what do I need to run for this change?" without running, linking, or maintaining the whole workspace suite.

### Infrastructure

- **N1 — A machine-readable test-ownership manifest** mapping source paths to the test targets that cover them, guarded by a static check so it cannot silently drift from the target list.
- **N2 — A single compiled instance of the third-party dependency graph per verification invocation**, so a verification job does not pay the dependency build twice.
- **N3 — A consolidated root integration-test target topology** that links materially fewer executables while preserving per-test isolation.

### Polish

- **P1 — Corrected verification counts and wall-clock baselines** in `architecture/testing.md`, replacing the predecessor's unverified figures with measured ones.
- **P2 — Removal of test-suite waste that is provably not capability** (dead files, self-contradicting copies, duplicated helpers), where the proof does not depend on the roadmap's larger changes. **Owned by M006 (assertion quality) and M007 (duplication).** The two milestones split the capability by the kind of proof each removal needs: M006 removes what is *false or vacuous*, M007 removes what is *repeated*. Neither advances the roadmap's throughput objective; both discharge the "provably not capability" half of P2.

## 3. Non-goals

This roadmap does not authorize:

- deleting a behavioural assertion to improve timing (see I1);
- reducing the coverage the hosted CI job executes;
- introducing self-hosted runners, a general build farm, or a remote execution dependency;
- replacing Cargo or Nextest with a custom test runner;
- changing product behaviour, feature defaults, permission semantics, or runtime architecture;
- broad workspace dependency refactoring unrelated to measured compile cost — in particular the `sqlx`→`rusqlite` consolidation is explicitly **out of scope** and is recorded in §10 as a decision point, not a task;
- raising test parallelism past the demonstrated runner limit without re-validating a full green run (the `ci` profile's 4-slot bound is set by a recorded flake history and stays);
- introducing `cargo deny` / `cargo audit` supply-chain gating — that is **new capability**, owned elsewhere, and is recorded in §10 as a finding rather than a task here;
- making release depend on CI performance thresholds.

## 4. Current state

All figures below were measured at baseline `239f06e7` and state their method. Wall-clock figures were taken on a 14-core machine while three unrelated worktrees were compiling concurrently, so absolute seconds are inflated; **unit counts, ratios, and artifact sizes are not affected by that contention.**

### 4.1 Where compile time goes

`cargo build --workspace --all-targets --timings` produced 984 units and 9,155.6 s of aggregate unit-time (2,158 s wall on 14 cores). Unit-time, not wall-clock, is the right currency here because it is insensitive to how many cores are available:

| Group | Units | Unit-secs | Share |
|---|---|---|---|
| root lib `--test` harness (the inline unit tests) | 1 | 1,993.3 | 21.8% |
| root lib (dev-profile rlib) | 1 | 1,535.2 | 16.8% |
| **root integration-test targets** | **271** | **4,040.1** | **44.1%** |
| other workspace test targets | 19 | 120.2 | 1.3% |
| binaries | 4 | 166.1 | 1.8% |
| dependency crates | 565 | 871.9 | 9.5% |
| build scripts | 112 | 144.0 | 1.6% |
| other | 11 | 284.8 | 3.1% |

Median root integration-test target: **13.9 s** of unit-time. The root `codegg` rlib is **855 MB**; each linked root test binary is **135–208 MB**; the root package's test binaries total **13.7 GB**.

**Consequence:** the build phase of the verification job is a *link* phase. 71% of all compile work is producing 304 test executables, and the root package alone is ~85% of the workspace's compile work.

### 4.2 The dependency graph is compiled twice per CI run

`Cargo.toml`'s `[profile.dev]` and `[profile.test]` differ in exactly one field, `strip` (`"debuginfo"` in the test profile only). Cargo hashes the resolved profile into each unit's identity, so the two profiles are **distinct units**. The CI job runs `cargo build --locked --bin codegg-sandbox-helper` (dev profile) and then `cargo nextest run` (test profile), so **422 third-party crates plus the root lib and the twelve workspace path dependencies are compiled twice per job.**

Verified on the repository at this baseline: `libcodegg_client-*.rlib` exists as **two identical 81.5 MB copies**, and `libtokio-*.rlib` as **four distinct hashes**. Verified on an isolated two-crate probe: with the profiles differing, a `cargo build` followed by a `cargo test` recompiles the dependency; with the `strip` field aligned, it does not.

This also affects the local loop: on a fully warm tree, `cargo nextest run -p codegg --test <name>` recompiled `codegg` and `codegg-client` (4m47s) because the test-profile artifacts did not exist.

### 4.3 The verification script runs the suite twice

`scripts/verify.sh` performs, in `full` mode, a workspace-wide check, a workspace-wide Clippy pass, a workspace-wide Nextest pass, and then a second workspace-wide Nextest pass with `server,plugins,lsp-test-support` enabled. The second pass re-links and re-executes essentially every binary the first pass already covered; the second pass's genuinely new surface is the root lib test target plus nine feature-gated integration targets.

### 4.4 The recorded baseline does not match the repository

`architecture/testing.md` records the predecessor's closure baseline as **"84 root integration-test binaries"** with a 7m51s test build and 362.5 s execution, from hosted run `36196068239` (2026-09-25). Measured against git at that same revision, `tests/` held **207** top-level `tests/*.rs` files plus eight declared `[[test]]` targets — roughly **203** cargo test targets, not 84. The recorded figure is off by about 2.4×, and the AGENTS.md rule that counts be *"verified against the listed `Source` rather than copied between docs"* was not applied.

The real target count has since grown further: 215 top-level test files on 2026-10-01, 256 on 2026-10-15, **280** at this baseline — **282** cargo test targets in the root package and **308** across the workspace, confirmed by `cargo metadata --no-deps`.

The consequence is not only a wrong number in a document. The predecessor's M001–M005 dispositions — "consolidate to reduce link steps", "job splitting rejected because it duplicates the dominant cost", "sccache rejected, 2% hit rate" — were taken against a count that was roughly 2.4× lower than reality, and against a topology that has since grown by a further ~39%.

### 4.5 Test-suite waste that is provably not capability

A static audit of all 280 root integration-test files and all inline test modules across the twelve workspace members found, among other items:

- a dead helper at the top level of `tests/` that Cargo auto-discovers as an empty test target and that **no file in the repository references**;
- twenty tests asserting against a **pasted copy** of production SSRF logic that has drifted from the production function, and which currently **contradicts** the test that exercises the real function;
- six service-level tests duplicated verbatim in a sibling file that carries twelve further unique tests;
- a shared helper's five self-tests compiled into **46** binaries, i.e. 230 redundant executions of five assertions;
- twenty-two crate roots carrying an inert `#![cfg(test)]` gate (208 test functions) that is a no-op in a `--test` build and a silent-zeroing hazard if the files are ever included elsewhere;
- three inline tests whose bodies cannot fail (one literally empty, two asserting a no-op macro);
- twenty-four tests that report **pass** after skipping, including nine that skip when `git` is not on `PATH`;
- four `#[ignore]`d "tests" in `src/tool_advisor/` that are repository-writing code generators.

### 4.5.1 The §4.5 figures that did not reproduce, corrected

Every figure below was re-measured with `scripts/rust_test_scan.py` (the
shared Rust-aware scanner) after M006 and M007. The audit's own figures are
kept above; the corrections and their sources are recorded here.

| §4.5 item | Audit figure | Measured | Source |
|---|---|---|---|
| The dead top-level `tests/` helper | "280-line" | **63 lines, 0 test functions, 0 references** — it was `tests/test_util.rs`, and **M004 removed it** | `git show HEAD:tests/test_util.rs` |
| Tests asserting against a pasted copy of production SSRF logic | twenty, and the copy "currently contradicts" the real test | **0 copies remain.** M006 deleted the local `ipv6_segments_to_ipv4` / `is_internal_ip` copies and pointed all twenty at `codegg::security::ssrf::is_internal_ip`; the contradiction was real — the copy asserted `fc01::` was internal while production blocks `fc00::/7` — and is fixed | `tests/config_provider/mcp.inc`; `src/security/ssrf.rs` |
| Service-level tests duplicated verbatim in a sibling file | six | The measured cross-file duplication is **62 groups with 65 redundant copies**, and the largest single group is **seven** members (`group-028`, the `lsp.inc` read-only cluster). No group of six survives | `docs/test-duplication.toml` |
| A shared helper's five self-tests compiled into **46** binaries | 46 binaries, 230 executions | **Post-M004 scale: eight families, forty executions.** **Discharged by M007 work package D: one family, five executions** | `tests/common/` declarations; `tests/command_git/secret_scan_self_tests.inc` |
| Inert `#![cfg(test)]` gates | "twenty-two **crate roots** (208 test functions)" | The **208 test functions reproduce exactly**, but the gates were **not** on crate roots: they were **22 inner `#![cfg(test)]` attributes** in 22 `tests/tool_programs/*.inc` files, plus **15 `#[cfg(test)] mod` gates** across 14 more test-target files — **37** in all. All 37 are removed | `git show HEAD:tests/tool_program_*.rs`; `scripts/check_test_assertion_quality.py` reports **0** |
| Tests whose bodies cannot fail | three | **0.** The three in `crates/codegg-core/src/tool_program/guards.rs` are deleted, and the fourth (`crates/egglsp/tests/real_server_smoke.rs`) now asserts the property it was named for | `crates/codegg-core/src/tool_program/guards.rs`; `scripts/check_tool_program_parse_only.py` |
| Tests that report **pass** after skipping | twenty-four, "including nine that skip when `git` is not on `PATH`" | **33** bodies reach a skip and return before any assertion. **The "nine that skip when `git` is not on `PATH`" does not reproduce: no skip is gated on `git`'s absence.** The measured classes are **20 `opt-in-gated`** and **13 `environment-absent`** — of the latter, 10 are LSP-server binaries or helper-process spawns in `crates/egglsp/tests/real_server_smoke.rs` and 3 are a kernel without Landlock in `tests/security_permission/sandbox_landlock.inc`. **0 are unconditional.** All 33 are listed with class, condition and `satisfied-by` | `docs/test-assertion-quality.toml`; `scripts/check_test_assertion_quality.py` |
| `#[ignore]`d repository-writing "tests" in `src/tool_advisor/` | four | **0 are reachable by a routine `cargo test -- --ignored`.** Each write path is gated on `CODEGG_REGENERATE_ASSETS=1` | `src/tool_advisor/causal_frontier.rs`; `src/tool_advisor/retrieval_relevance.rs` |

### 4.5.2 M007's disposition of the shared-helper item

§4.5's shared-helper bullet — *"a shared helper's five self-tests compiled
into 46 binaries"* — was re-measured at the **post-M004** scale, because
M004's family consolidation changed the shape of the problem:
`tests/common/` is declared by **eight** root families, so the five
`secret_scan` self-tests ran **forty** times (eight × five), not 230. It was
then **discharged** by M007's work package D: the five self-tests moved out
of the shared helper into the one family that uses it,
`tests/command_git/secret_scan_self_tests.inc`, so they now compile and
execute in **one** target, **five** times. The assertions are unchanged;
only their home moved. The sibling item — `crates/egglsp/tests/common/`,
which carried **no** assertions and was therefore compile-time duplication
only — was resolved in the same work package by binding each of its six
consumers to the single harness module it uses.

### 4.5.3 M007's measured duplication figures

M007's decision table, `docs/test-duplication.toml`, is the authoritative
record of the duplication this workstream found. Measured with the
content-preserving detector (a literal is replaced by a marker derived from
its content, so two calls that differ only in a string argument stay
distinct):

| Figure | Value |
|---|---|
| Duplicate groups | **90** |
| Members | **189** |
| Redundant copies | **99** |
| — in cross-file groups | **62 groups, 65 redundant copies** |
| — in same-file groups | **28 groups, 34 redundant copies** |
| Removals applied | **85** (35 in `tests/`, 46 in `src/`, 4 in `crates/`) |
| Groups deliberately kept whole | **14** — every one of them crosses a crate boundary, where the pair *is* the boundary check |
| Remaining duplicate groups on the tree | **14**, all cross-crate |

Blanking every literal instead of deriving a marker from its content reports
**456** groups where the true figure is **90** — a 5× over-report.

The same audit found **no** provable overlap between behavioural tests and the `scripts/check_*.py` guards — the guards are source-text lints, the tests drive real behaviour — so "a static guard already covers it" is **not** a general pruning rationale in this repository. The audit also identified a set of **sole-guard** tests that must not be pruned, including compile-time `Send + Sync` assertions, sixteen identity-newtype round-trip tests, a static source guard implemented as an inline Rust test, and the three-way argv parse/render/risk triples in `codegg-git`.

### 4.6 A coverage gap that runs the other way

Nine integration targets are gated behind `required-features` — four LSP targets and five `server` targets — totalling **149 test functions**. The CI job invokes Nextest without `--features`, and no workflow enables `server` or `lsp-test-support`, so **149 test functions never execute in CI**. This is a coverage gap, not waste, and it is deliberately *not* a target of this roadmap; it is recorded in §10 as a decision for the owner.

### 4.7 What is already in place and must not be re-proposed

The following are existing, working, and out of scope: the single bounded `CI / verify` job with `concurrency` cancellation; the `mold` linker; `Swatinem/rust-cache`; the `ci` Nextest profile's 4-slot bound and its six whole-binary plus one whole-test `threads-required = "num-cpus"` exclusions; `CARGO_BUILD_JOBS` caps; the prebuilt Eggwork fixture path; `scripts/detect-live-eggwork-changes.sh`; the ~60 `scripts/check_*.py` guards with their change-triggered routing, which total **17.5 s** and are therefore not a time problem; and the M003/M004 precedent of consolidating test targets into `mod.rs` + `.inc` families.

## 5. Target architecture

The end state has four properties.

**One compiled dependency graph per verification invocation.** The dev and test profiles resolve to the same unit identity, so a verification job compiles the third-party graph once and reuses it for both the binary build and the test build.

**A verification job whose build phase is proportional to the number of *distinct behaviours under test*, not to the number of files that happen to contain them.** The root package's integration tests are organised into a small number of family targets, each of which still isolates its tests in separate processes.

**A per-commit verification path that is a function of the change.** Given a base ref and a working tree, the harness computes the affected workspace members and the affected test targets, and runs exactly those, using Nextest's cargo-level target selection so that unselected targets are never linked.

**Counts that are published with their method.** Every count this roadmap produces states the command that produced it and the baseline it was produced at, and a static check prevents the target count from moving without the recorded figure being revisited.

## 6. Dependency graph

```text
M001  profile alignment (single compiled dependency graph)
  |
  +--> M002  bounded feature-enabled test pass
  |
  +--> M003  change-scoped verification mode          [hard dep: M001]
        |
        +--> M004  root test-target family consolidation   [interface dep: M003]

M006  test-suite assertion quality                   [no dependency]
  |
  +--> M007  remove provably duplicate test bodies    [hard dep: M006]

M005  hosted verification and measurement closure     [ready: owner push + before-run dispatch]
```

| Dependency | Class | Why |
|---|---|---|
| M002 → M001 | operational | Independent code changes; sequenced so each is measurable in isolation. M001's saving is measured against the same job, and measuring two simultaneous changes would not attribute either. |
| M003 → M001 | **hard** | The scoped mode's saving is a compile-time claim. It must be measured against the post-M001 single-graph baseline, or the number is not interpretable. |
| M004 → M003 | **interface** | M004 changes root test-target names. M003's ownership manifest names those targets. M004 must land with a matching manifest update or the manifest's target references become stale. |
| M006 → — | none | M006 edits test bodies only and depends on no other milestone's output. It is gated on M004's *interface* (the family topology it works inside), not on M004's completion. |
| M007 → M006 | **hard** | M006 delivers the Rust-aware scanner M007's duplicate detector is built on, and removes the drifted SSRF copy and the three empty test bodies that M007's detector would otherwise flag. Writing a second scanner would reproduce the four measured failure modes and produce a different answer. |
| M005 → — | none in the graph | M005 is the workstream's closure milestone. It has no predecessor in the graph: M001–M004, M006 and M007 are implemented and committed. It is gated on **mechanical owner actions** — push the branch, dispatch the hosted before-run, and confirm the before-run baseline — not on another milestone and not on a design decision. |

M004 is sequenced *after* M003 deliberately: M003 delivers the change-scoped capability the owner asked for, and M004 then makes that capability cheaper by shrinking the target set the manifest has to describe. The alternative order was considered and rejected — see §10, D3.

M006 and M007 are a separate branch of the graph because they are a **different kind of work**: M001–M004 change *how the suite is invoked and laid out*, while M006 and M007 change *what the suite asserts*. They are independent of the throughput chain, they do not advance the throughput objective, and they must not be measured as if they did. They are sequenced M006 → M007 because M007's removals must not remove a copy of an assertion that is itself wrong, which is exactly the case the SSRF cluster presents.

## 7. Milestones

### Milestone M001 — Align the dev and test cargo profiles

Class: infrastructure, with a polish obligation.

Objective: one compiled instance of the third-party dependency graph per verification invocation.

Dependencies: none.

Deliverable boundary: the cargo profile configuration and the CI job's cargo invocations. No test file, test target, or test assertion is touched.

User or operator value: the hosted verification job stops paying for a second full dependency build; the local loop stops recompiling the dependency graph when switching between build and test.

Exit conditions:

- a `cargo build` followed by a `cargo test` over the same tree compiles each dependency crate **once**, demonstrated on a two-crate probe and re-demonstrated on the repository by the absence of a second identical artifact;
- the CI job's dependency build happens exactly once per run;
- the change is either a job-scoped environment override or a manifest alignment, and the choice is recorded with its measured effect;
- hosted evidence shows a material reduction against the §4.1 baseline, or the closure record explains why not and reverts the change.

Deferred work: changing `debug` or `codegen-units` for the test profile. The predecessor measured `CARGO_PROFILE_TEST_DEBUG=0` as wall-parity locally and required hosted proof before a CI-only override; that measurement stands and is not re-litigated here.

### Milestone M002 — Bound the feature-enabled test pass

Class: infrastructure, with a polish obligation.

Objective: the second verification pass links and executes only the binaries the additional features actually unlock.

Dependencies: M001 (operational).

Deliverable boundary: the second Nextest invocation in `scripts/verify.sh`. The set of executed tests is unchanged; the set of *built* targets is reduced.

User or operator value: `verify.sh full` stops re-linking and re-running every root test target under the new feature set. **There is no hosted-job benefit.** No workflow in `.github/workflows/` runs the feature-enabled pass — verified 2026-10-21 — so this milestone's saving is entirely local to `scripts/verify.sh full`. An earlier revision of this roadmap claimed "the hosted job stops re-linking and re-running the whole workspace a second time"; that was wrong on two counts, and the correction is recorded here rather than quietly dropped.

Exit conditions:

- the feature-enabled pass builds and runs exactly the root lib test target plus the feature-gated integration targets;
- every test function that the previous second pass executed is still executed, and the closure record enumerates the mapping;
- the pass uses a Nextest profile appropriate for a subset run, per the standing constraint in `.config/nextest.toml` that the `ci` profile's whole-binary exclusions require their binaries to exist;
- local evidence shows the second pass no longer dominates `verify.sh full`'s build phase, measured on the implementation baseline and labelled as local.

Deferred work: the 149 feature-gated test functions that CI never executes (§4.6). Enabling them is a coverage decision, not a throughput decision; see §10, D1.

### Milestone M003 — Change-scoped verification mode

Class: capability, with an infrastructure obligation.

Objective: a developer can run a verification pass bounded to the change under review.

Dependencies: M001 (hard).

Deliverable boundary: a new verification mode, a machine-readable ownership manifest, and a static guard that keeps the manifest honest. No test is deleted, and the hosted CI job's coverage is unchanged.

User or operator value: the per-commit loop costs what the change touched, not the whole workspace.

**Implemented with a dependency-edge rule the plan did not specify** (2026-10-21). A path rule maps a changed *file* to the tests that cover it; it cannot map a changed *crate* to the tests of a crate that depends on it. In this workspace the root package depends on all eleven other crates and re-exports several of them, so a change to a leaf crate reaches the root package's tests through the root's own source — a coupling no path rule can see. The manifest therefore carries the workspace's dependency edges, the guard proves the declared edge set is exactly `cargo metadata`'s, and a member pulled in that way contributes its full surface. The details and the measured before/after are in `plans/closure/ci-test-throughput-recovery/003-status.md` §4.

Exit conditions:

- the mode accepts a base ref, computes the affected workspace members and test targets from the manifest, and runs exactly those;
- the manifest is machine-readable, is checked by a static guard wired into the routine verification path, and the guard fails when a manifest entry names a target that does not exist or a target that exists but is unowned;
- the manifest declares the workspace's dependency edges, and the guard fails when an edge is missing or invented — a dependency direction that rots silently is a scoped run that silently stops covering a crate;
- the guard carries a self-test proving it rejects an injected violation, so it cannot pass vacuously;
- a scoped run is demonstrated to be a **superset** of the change's coverage, and the hosted job is demonstrated to still run the full suite;
- a selected target that is feature-gated is built with its feature enabled, so the scope never names a target cargo would refuse to build;
- the measured scoped cost is reported against the §4.1 baseline with the measurement method stated.

Deferred work: any coverage inference the manifest cannot justify. An unmapped changed file must fail loudly rather than be silently skipped, and the dependency direction widens to the dependent's full surface rather than guessing at a narrower set.

### Milestone M004 — Consolidate root integration-test targets into families

Class: infrastructure, with a polish obligation.

Objective: materially fewer linked root test executables, with per-test isolation preserved.

The owner has decided (roadmap §10, D4) a target of approximately **15** family targets.

**Implemented at 19 families** (2026-10-21). Ten are structurally forced — six singleton families for the binaries the `ci` profile reserves the runner for, and four feature-gated families, one per gate — and nine are coherent subject families. Reaching 15 would have required merging four unrelated subjects into one executable, which is the milestone's own stop condition. The departure from the target is recorded in `plans/closure/ci-test-throughput-recovery/004-status.md` §2, as the exit condition below requires.

Dependencies: M003 (interface).

Deliverable boundary: the root package's integration-test target layout and the ownership manifest it references. Test bodies and assertions are not changed by consolidation.

User or operator value: the hosted job's build phase shrinks in proportion to the number of families rather than the number of files.

Exit conditions:

- the root package links a materially smaller number of test executables, with the before/after counts measured and stated;
- the partition lands at approximately the decided ~15 families, or the closure record states why a different count is the better partition — the count is a target, not a quota, and a partition that needs 17 families to keep unrelated subjects apart is preferable to one bent to 15;
- every test function that existed before consolidation still exists and is still executed in exactly one target;
- the Nextest `ci` profile's whole-binary exclusions are re-validated against the new target names, and any exclusion whose binary no longer exists is either re-pointed or removed with a recorded reason;
- the ownership manifest is updated in the same change so no target reference is stale;
- hosted evidence shows the reduction, and the closure record states the measured per-family link cost.

Deferred work: consolidating inline unit tests. The root lib test harness is a single unit and already links once.

### Milestone M005 — Hosted verification and measurement closure — **ready**

Class: infrastructure, with a polish obligation.

Objective: produce the hosted evidence the workstream's closure definition requires, and close the workstream.

**The milestone number was reserved so the workstream's milestones would be numbered in the order they are *executed*.** The plan is now written — `plans/implementation/ci-test-throughput-recovery/005-hosted-verification-and-measurement-closure.md` — and the reservation has discharged. The milestone changes **no product source, no test, no build configuration, and no CI job definition**; it produces measurements and records.

Dependencies: **none in the graph.** M001–M004, M006 and M007 are implemented, locally verified, and committed at `4f89b4b05`. What remains is mechanical owner action, not design:

1. the change set is now **committed** — `4f89b4b05`, 468 files — so the hosted "after" run can exist as soon as the branch is **pushed** (outstanding; owner);
2. the hosted before-run baseline must be confirmed — **`2c1953347`** is the clean before (the same tree, pre-change, already pushed) and `239f06e7` is 323 commits further back and would confound that much unrelated change;
3. the rebase is **deferred past the measurement** — see §10, D9. The roadmap previously listed it here as a dependency; the deviation is recorded rather than hidden, and the rebase becomes an ordinary, separately-verified merge activity after the closure.

The milestone's deliverable is a **measurement**, and a measurement that is not preregistered is an anecdote. Its plan therefore fixes the method before any run is dispatched: the primary claim is a **countable** one (the linked root-test-executable count: 288 → 21), the hosted wall-clock is the **secondary** claim and is reported with dispersion and cache state rather than as a best-of, the method is written to `plans/validation/ci-test-throughput-recovery/measurement-preregistration.md` **before** the candidate run, and every reported number names the command, the SHA, and the date that produced it.

It also discharges **M004's unmeasured ~41 % link-cost projection** and **M003's four argued-not-proven scoped-cost cases** — the two claims the workstream still carries as unsupported.

Deliverable boundary: the hosted `CI / verify` runs, the measurement preregistration, the per-family link-cost measurement, M003's measured scoped cost, and the republication of `architecture/testing.md`.

User or operator value: the workstream's stated benefit becomes a measured fact rather than a projection.

Exit conditions: the roadmap's §11 completion definition is satisfied, with each bullet either discharged or explicitly waived with a recorded reason.

Deferred work: the feature-gated coverage decision (§10, D1), which is an owner coverage decision and not part of the closure.

### Milestone M006 — Test-suite assertion quality

Class: polish, with a correctness obligation.

Objective: every test function asserts a real, checkable property of production code, or is removed — and no test asserts a property that production contradicts.

Dependencies: none. Gated on M004's *interface* (the family topology), not on M004's completion.

Deliverable boundary: **test files only**, plus the shared Rust-aware scanner and the static guard `scripts/check_test_assertion_quality.py`. No product source is changed.

User or operator value: the suite's pass signal means what it says. Two green tests currently assert opposite answers to the same question — one against a pasted copy of production's IP classifier that has drifted — and thirteen tests in a family report *passed* having skipped everything.

Exit conditions:

- the twenty SSRF assertions in `tests/config_provider/mcp.inc` call production rather than a copy of it, and the `fc01::` assertion agrees with production;
- no test body is byte-identical to an empty body without a recorded disposition;
- a routine `cargo test -- --ignored` run changes no tracked file;
- no inert `cfg(test)` gate remains in a test-target source file;
- every skip that precedes an assertion is reported as a skip, or named as an exception with the workflow that satisfies it;
- a static guard with a `--self-test` proves the four classes and is wired into `verify.sh quick` and the CI job.

Deferred work: the duplication half of P2, which is M007.

### Milestone M007 — Remove provably duplicate test bodies

Class: infrastructure, with a polish obligation.

Objective: no two test functions assert the same thing twice, except where a recorded exception names the survivor — and a static guard proves it.

Dependencies: M006 (hard) — it delivers the content-preserving scanner this milestone's detector is built on, and removes the drifted copy and the empty bodies that the detector would otherwise flag.

Deliverable boundary: **test files only**, plus the static guard `scripts/check_test_duplication.py`. No product source is changed.

User or operator value: the suite stops paying twice for the same assertion, and the shared integration-test helper stops compiling its own five assertions into eight binaries it does not use.

Exit conditions:

- every removed test function is recorded in a removal table naming the survivor that keeps the assertion;
- where a unit copy and an integration copy were identical, the survivor is the one that proves the stronger reach;
- the detector is content-preserving — it reports zero duplicates for bodies that differ only in a string argument, and that negative case is pinned by the `--self-test`;
- the detector's parsed test-function counts equal the ground-truth attribute counts;
- the shared helper's five assertions execute in exactly one target;
- a static guard with a `--self-test` proves the three classes and is wired into `verify.sh quick` and the CI job.

Deferred work: consolidating the inline unit tests into fewer harnesses, which is the same deferral M004 records.

## 8. Cross-cutting requirements

### Storage and migration

None. No persistent state changes.

### Protocol and compatibility

None. No wire, DTO, or API surface changes. The Nextest profiles and the CI job's cargo invocations are developer-facing configuration, not a compatibility contract.

### Security and authorization

None. No milestone changes permission, approval, execution authority, or sandbox semantics. The CI job keeps `contents: read` and no release or publication authority.

### Concurrency, cancellation, and recovery

The `ci` profile's 4-slot bound and its whole-binary exclusions exist because of a recorded flake history (8 slots were tried and reverted on 2026-09-25; `scheduler_cancellation` and `causal_observe_replay` joined the exclusion list for measured hosted flakes). No milestone may raise parallelism or remove an exclusion without re-validating a full green run. A scoped run is a subset of the same profile semantics, not a new execution model.

### Observability and audit

Every published count states the command that produced it and the baseline. A static guard prevents the root package's test-target count from drifting away from the recorded figure without the record being revisited.

### Performance and resource use

This is the roadmap's whole subject. The governing metric is **compile unit-time**, because it is insensitive to runner core count, and the governing artifact measure is **linked test-executable count and size**.

### Documentation and operations

`architecture/testing.md` carries the active test-resource and routine-CI contract and must be corrected by the milestones that change it. `plans/registry.md` carries the control surface. The AGENTS.md skills index points at `architecture/testing.md` for the testing module and must continue to resolve.

## 9. Verification strategy

- **Static guards.** The new ownership-manifest guard must carry a `--self-test` that injects a violation and proves the guard rejects it, in the same style as the repository's other change-triggered guards.
- **Focused tests.** Each milestone names the narrowest target that covers its change. Per AGENTS.md, prefer the narrowest target; do not run the whole workspace to check a manifest.
- **Structural assertions.** The mechanism claims are asserted structurally, not by wall-clock alone: "one compiled dependency graph" is asserted by artifact identity (a dependency crate does not acquire a second distinct artifact between a build and a test), and "subset selection restricts the build" is asserted by the set of produced executables.
- **Hosted evidence.** The CI job's wall-clock is a closure requirement, measured on the hosted runner and labelled as hosted, per the repository's standing rule that local and hosted evidence are not interchangeable.
- **Negative results are recorded.** An optimization that measures neutral or negative closes with the measurement and the revert, not with a quiet claim.

## 10. Risks and decision points

| ID | Decision / risk | Disposition |
|---|---|---|
| D1 | The 149 feature-gated test functions that CI never executes (§4.6). | **Owner decision, out of scope here.** Enabling `server,plugins,lsp-test-support` in CI would add real coverage and add build cost. This roadmap records the gap and does not close it. |
| D2 | The `sqlx` → `rusqlite` consolidation. | **Rejected for this roadmap.** It is the only real dependency-graph consolidation available, and it is a large migration touching async API shape and compile-time query checking. The measured compile-time win in §4.1 does not come from dependency removal. |
| D3 | Sequence M003 before M004, or M004 before M003? | **M003 first.** M003 delivers the capability the owner asked for; M004 then shrinks the target set the manifest describes. The reverse order would let M003's manifest be written against the post-consolidation topology, but it delays the per-commit capability behind a large refactor. M004 carries the interface dependency so the manifest and the target names move together. |
| D4 | Family granularity for M004. | **Decided 2026-10-21: ~15 families accepted.** The owner accepted a target of approximately **15** family targets for the root package's integration tests, in exchange for coarser failure isolation and a coarser scoped run. This resolves the decision but **not** the partition: which files land in which family, which families mix unrelated subjects, and how the six whole-binary exclusion binaries map into families remain M004's work-package A deliverable, to be presented for review against the measured cost rather than assumed from the family count. The count is a target, not a quota — a partition that needs 17 families to keep unrelated subjects apart is a better partition than one forced to 15, and the implementing agent must say so if it departs from 15 rather than bending subjects to fit. |
| D5 | The `cargo deny` / `cargo audit` supply-chain enforcement cited by an inline test comment but absent from the repository. | **Recorded as a finding, not a task.** Adding supply-chain gating is new capability owned elsewhere. This roadmap notes the discrepancy so it is not mistaken for coverage. |
| D6 | M001 could be implemented as a job-scoped environment override or as a manifest change. | **Decided 2026-10-21: manifest alignment.** `[profile.dev]` and `[profile.test]` now both set `strip = "debuginfo"`, so the two profiles resolve identically and every cargo invocation in every context — the hosted job, `scripts/verify.sh`, and a developer's own `cargo build` → `cargo test` loop — compiles the dependency graph once. The job-scoped override was rejected because it cannot cover the local loop, and a partial application is worse than none: it looks fixed and is not. The DX cost is bounded and recoverable — `strip = "debuginfo"` keeps the symbol table, so backtraces still name their frames, and `cargo build --config 'profile.dev.strip="none"'` restores full debuginfo for one invocation. The plan's suggested `RUSTFLAGS="-C debuginfo=2"` recovery was measured and **does not work**: the profile's `strip` still applies. |
| D7 | The predecessor's recorded baseline is wrong by ~2.4× and its dispositions were taken against it. | **Mitigated by I4 and §4.4.** The dispositions themselves are re-tested rather than assumed: M001 and M002 are the two the wrong count most affected, and both require hosted evidence. |
| D8 | P2 was one capability with no owning milestone. How is it split? | **Split by the kind of proof each removal needs: M006 removes what is false or vacuous, M007 removes what is repeated.** A single milestone would have had to carry two incompatible exit conditions — "this assertion is wrong" and "this assertion is already made elsewhere" — with different proofs, different survivors, and different risks of capability loss. The split also fixes the ordering hazard: M007 must not remove a copy of an assertion that is itself wrong, which is exactly the case the SSRF cluster presents, where one of the three copies asserts the opposite of production. M006 re-points that copy first; M007 then removes the genuine repetitions. Both milestones are independent of the throughput chain and neither advances the throughput objective. |
| D9 | M005's measurement order: rebase onto the advanced `origin/main` **before** the hosted runs, or measure the tree that was actually verified? | **Measure first, rebase after — a deviation from the roadmap's own M005 dependency list, recorded here rather than hidden.** The hosted "after" run must measure *the tree that was verified*: `4f89b4b05` is that tree, and it ran 15,466 tests locally with only the six pre-existing platform failures M004 proved against baseline `2c1953347`. Rebasing first would produce a **different** tree that needs its own full local suite run and its own verification before it could be measured, and absorbing `origin/main`'s twelve new top-level `tests/*.rs` files would change the very target count the measurement is about (21 → 33). Measuring first turns the rebase into an ordinary, separately-verified merge activity instead of something entangled with the workstream's closing evidence. The rebase is therefore **deferred past the measurement** and carried into M005's closure record as an open item. The **before-run baseline** is `2c1953347` — the clean before, the same tree pre-change, already pushed — not `239f06e7`, which is 323 commits further back and would confound the delta with that much unrelated change. |

## 11. Completion definition

This roadmap closes when:

- the hosted `CI / verify` job links and builds materially fewer test executables than the §4.1 baseline, with hosted wall-clock evidence and the measurement method stated;
- a change-scoped verification mode exists, is guarded against drift, and has demonstrated that it covers the change it was scoped to;
- every test function that existed at the §4.1 baseline still exists and is still executed somewhere, with any removal recorded as a proven duplicate naming its survivor;
- the recorded verification counts in `architecture/testing.md` match the repository, and a static check prevents the target count from drifting away from the record;
- the predecessor's M001–M005 and corrective C001–C002 remain closed and unmodified;
- the final hosted evidence demonstrates a material reduction from the §4.1 baseline, or the closure record explains why further reduction was not justified without weakening correctness.

A target wall-clock below 10 minutes is desirable but is not a correctness gate. A negative optimization experiment may close if it is reverted and documented.

The third bullet governs M007's removals directly: a test function removed by M007 is removed **only** as a proven duplicate that names its survivor, and M006's removals are governed by the same bullet through the narrower clause that a removed assertion must have been provably absent. Neither milestone may reduce the coverage the hosted CI job executes in order to reach a number.

## 12. Milestone status

| Milestone | Status | Implementation plan | Closure record | Blockers |
|---|---|---|---|---|
| M001 | active (implemented; locally verified; hosted evidence outstanding) | `plans/implementation/ci-test-throughput-recovery/001-profile-alignment-single-dependency-graph.md` | `plans/closure/ci-test-throughput-recovery/001-status.md` | none |
| M002 | active (implemented; bounded invocation executed; hosted evidence outstanding) | `plans/implementation/ci-test-throughput-recovery/002-bounded-feature-enabled-test-pass.md` | `plans/closure/ci-test-throughput-recovery/002-status.md` | M001 (operational) |
| M003 | active (implemented; locally verified; four defects found by the coverage comparison and fixed) | `plans/implementation/ci-test-throughput-recovery/003-change-scoped-verification-mode.md` | `plans/closure/ci-test-throughput-recovery/003-status.md` | M001 (hard) |
| M004 | active (implemented; full suite run; one regression found and fixed; hosted evidence outstanding) | `plans/implementation/ci-test-throughput-recovery/004-root-test-target-family-consolidation.md` | `plans/closure/ci-test-throughput-recovery/004-status.md` | M003 (interface) |
| M005 | **ready** (plan written; the change set it measures is committed at `4f89b4b05`) | `plans/implementation/ci-test-throughput-recovery/005-hosted-verification-and-measurement-closure.md` | (not written — written at closure) | owner action: push the branch and dispatch the hosted before-run; confirm the before-run baseline (`2c1953347` recommended, §10 D9); the rebase is deferred past the measurement |
| M006 | **closed** (implemented; three guards ship with `--self-test` coverage; the workspace compiles and its suite runs) | `plans/implementation/ci-test-throughput-recovery/006-test-suite-assertion-quality.md` | `plans/closure/ci-test-throughput-recovery/006-status.md` | none |
| M007 | **closed** (implemented; 90 duplicate groups triaged, 85 removals applied and verified; the shared-helper fan-out discharged) | `plans/implementation/ci-test-throughput-recovery/007-test-suite-duplication.md` | `plans/closure/ci-test-throughput-recovery/007-status.md` | none |
