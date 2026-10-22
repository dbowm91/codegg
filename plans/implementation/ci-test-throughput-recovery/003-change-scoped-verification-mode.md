# CI and Test Throughput Recovery Milestone M003 — Change-Scoped Verification Mode

Status: implemented (2026-10-21); closure `plans/closure/ci-test-throughput-recovery/003-status.md`

Repository baseline: `239f06e7fc876eaa36156dbf4658bb56314c9673` (the tree the plan was authored against)

Implementation baseline: `2c1953347896848c2f52c2dfbd9f04029d1c2cc2` (the tree the work was applied to, after the 2026-10-21 rebase onto `origin/main`)

Source roadmap:

- `plans/subsystems/ci-test-throughput-recovery-roadmap.md#milestone-m003--change-scoped-verification-mode`

Long-term requirements:

- `plans/000-long-term-specification.md` §4.7 — correctness before transparent magic;
- `plans/003-planning-process.md` §2.3–2.4.

Applicable ADRs: none required. This milestone adds a developer-facing verification mode and a machine-readable ownership manifest. It does not alter product authority, protocol, storage, dependency selection, or runtime architecture.

Predecessor work (closed, historical, not reopened by this milestone):

- `plans/subsystems/ci-test-throughput-optimization-roadmap.md` M001–M005;
- `plans/subsystems/ci-test-throughput-optimization-post-closure-corrective-addendum.md` C001–C002.

Primary class: capability, with an infrastructure obligation.

---

## 1. Objective

A developer can run a verification pass **bounded to the change under review**, so the per-commit loop costs what the change touched rather than the whole workspace.

Concretely: given a base ref and a working tree, the harness computes the affected workspace members and the affected test targets from a machine-readable ownership manifest, and runs exactly those — using cargo-level target selection so that unselected test targets are never linked.

The hosted CI job's coverage is **not** changed by this milestone.

## 2. Why this milestone is ready

**Hard dependency: M001** (`plans/implementation/ci-test-throughput-recovery/001-profile-alignment-single-dependency-graph.md`). M003's headline claim is a compile-time saving. It is only interpretable against a single-compiled-dependency-graph baseline; measured against a baseline where the dependency graph is compiled twice, the scoped saving is confounded. M001 must be closed before M003's measurement is taken.

**Interface dependencies:** none beyond the Nextest CLI contract, which is stable at the baseline (`cargo-nextest` 0.9.140) and whose `--test` semantics are proven in §3.3.

**Why it is third:** it delivers the capability the owner asked for, and it is sequenced before M004 so that M004 can then shrink the target set the manifest has to describe. The reverse order was considered and rejected (successor roadmap §10, D3).

## 3. Current implementation evidence

### 3.1 The current local loop is unbounded

`scripts/verify.sh quick` ends with `cargo check --workspace --all-targets --locked`, and `full` adds a workspace-wide Clippy pass and two workspace-wide Nextest passes. Every one of those is workspace-wide. A developer who changes one file in one crate pays for a type-check of all 282 root test targets, a lint of all of them, and the linking and execution of all of them.

### 3.2 The structural constraint that bounds what scoping can achieve

The root `codegg` package depends **directly** on all eleven other workspace crates. `cargo metadata --no-deps` confirms it: `codegg-client`, `codegg-config`, `codegg-core`, `codegg-document`, `codegg-git`, `codegg-protocol`, `codegg-providers` (twice — once with an extra feature), `eggcontext`, `egggit`, `egglsp`, `eggsentry`.

Therefore **a change to any workspace member invalidates the root library and re-links every root test target that is selected to be built.** Scoping cannot shorten the *rebuild chain*; it can only shorten the *number of links*.

That is still the dominant term, because the measured unit-time model at the baseline is:

| | Unit-secs |
|---|---|
| root lib `--test` harness | 1,993.3 |
| root lib (dev rlib) | 1,535.2 |
| **271 root integration-test targets** | **4,040.1** |
| everything else | ~1,587 |

So: full root test-target build (7,568.6 unit-secs) versus a scoped build of the root library plus dependencies plus a small number of selected targets (~2,422 unit-secs) — a **~68% reduction** in compile work for a one-target scope. The plan must present this honestly as a *link-count* saving with an irreducible rebuild chain, not as "scoping avoids the rebuild".

### 3.3 The selection mechanism is proven

`cargo nextest run --test <name>` restricts the **cargo build**, not merely Nextest's run list. Verified on an isolated probe workspace with a clean target directory per variant:

| Command | Test executables produced |
|---|---|
| `cargo nextest list --test beta` | `beta` only |
| `cargo nextest list --tests` | `alpha`, `beta`, `gamma`, `delta` |
| `cargo nextest list --test alpha --test gamma` | `alpha`, `gamma` only |

**Repeated `--test` flags are accepted** and restrict the build to exactly the named targets. By contrast, `-E 'binary(...)'` filters only the run list — every target of the selected package is still built — so `-E` cannot deliver this milestone's saving.

### 3.4 There is no source-to-test mapping today

The root package's test files are named by milestone and feature (`tests/async_background_c003_idle_lifecycle.rs`, `tests/team_collaboration_m006_trajectory.rs`), not by source module. A name-prefix heuristic is unreliable: matching test-file name prefixes against `src/` module names maps 62 files to `src/tool/` and 30 to `src/tui/`, while large families (`work_orders_*`, `team_collaboration_*`, `projection_*`, `session_*`) do not correspond to any `src/` subdirectory at all.

Seven of the twelve workspace members have **zero** integration test targets — all their tests live in the lib target — so for those members package-level selection is already exact and needs no manifest entry beyond the package itself.

### 3.5 The repository already owns the idiom this milestone should copy

`docs/execution-ownership.toml` plus `scripts/check_execution_ownership.py` is the established pattern for a machine-readable ownership manifest with a static guard that keeps it honest, and it carries a `--self-test` that injects a violation to prove the guard is not vacuous. `docs/validation/`-style closure evidence and the `scripts/check_*.py` convention are the house style. This milestone should produce a **sibling** of that pattern, not a new mechanism.

### 3.6 The guard layer is already change-scoped

The ~60 `scripts/check_*.py` guards are already routed by change-trigger: each declares which source paths trigger it, and the routine verification path runs only the guards whose trigger matches the change. The existing guard-trigger routing is therefore the precedent — and a possible source of truth — for the manifest's path-matching semantics. The implementing agent must read the existing routing before inventing new semantics.

## 4. Invariants that must not regress

- **I1 — No behavioural assertion is removed for timing.** The manifest selects which tests run; it never deletes one.
- **I2 — One bounded `CI / verify` job.** No new job, matrix, or lane. The scoped mode is a local developer affordance.
- **I3 — The hosted CI job's executed set is unchanged.** Scoping is additive to the developer's options and is never a way for CI to run less. This is roadmap invariant I3 and it is non-negotiable here.
- **I4 — A scoped run is a superset of the change's coverage.** Every test that could be affected by the change must be selected. If the manifest cannot determine that, the mode must widen to the affected package's whole surface, or refuse.
- **I5 — An unmapped or unresolvable change fails loudly.** Silently skipping a changed file is the one failure mode this milestone must never exhibit.
- **I6 — The manifest is machine-readable and statically guarded**, with a `--self-test` proving the guard rejects an injected violation.
- **I7 — The `ci` Nextest profile's slot bound and whole-binary exclusions are untouched.**
- **I8 — No dependency, feature default, or product behaviour changes.**
- **I9 — The existing guard-trigger routing keeps working.** The scoped mode composes with it; it does not replace or reorder it.

## 5. Scope

### In scope

- A new verification mode (a `scoped` mode on `scripts/verify.sh`, or a sibling script — the agent's choice, recorded in the closure).
- A machine-readable ownership manifest mapping source paths to the test targets that cover them, in the `docs/execution-ownership.toml` idiom.
- A static guard keeping the manifest honest, in the `scripts/check_*.py` idiom, with a `--self-test`.
- The changed-path → affected-members → affected-targets computation.
- Documentation of the mode's contract, its limits, and its failure behaviour.

### Explicitly out of scope

- Changing the hosted CI job's scope. The full suite keeps running there.
- Deleting, merging, or rewriting any test.
- Inferring coverage from test *names*, commit messages, or LLM judgement. The manifest is explicit and human-maintained.
- Automatic generation of the manifest from runtime coverage. That is a different mechanism with different costs; see §10, D2.
- Consolidating test targets into families (M004). The manifest must be written so M004 can shrink it rather than rewrite it.
- The `cargo check` / `cargo clippy` overlap between `quick` and `full`.
- The 149 feature-gated test functions CI never executes (successor roadmap §10, D1).

## 6. Required production changes

### Core/domain

None. No runtime behaviour changes.

### Storage and migrations

None.

### Protocol and DTOs

None.

### Runtime and concurrency

None. The mode composes existing cargo and Nextest invocations.

### Frontend or operator surface

The developer-facing surface is the mode's command line: its arguments, its output, and — critically — its failure output. The mode must print which members and targets it selected, which it skipped, and why, so a developer can see the scope of what just ran. A scope that is invisible is a scope that cannot be trusted.

### Security and authorization

None. The mode reads the repository's working tree and the manifest; it writes nothing outside the target directory.

### Documentation and static guards

- `docs/test-ownership.toml` (or an equivalently-named file) — the manifest.
- `scripts/check_test_ownership.py` — the guard, with `--self-test`.
- `architecture/testing.md` — the mode's contract, its limits, and the fact that the hosted job's scope is unchanged.
- `.opencode/skills/testing-ci/SKILL.md` — the skills index points at this module; if the skill documents the verification modes, it must learn the new one. The AGENTS.md skills index must continue to resolve.
- `scripts/verify.sh` — the mode's entry point.

## 7. Ordered work packages

### Work package A — Establish the manifest schema and the selection contract

Intent: define, before writing any mapping, what the manifest says and what the mode guarantees.

Required changes:

1. Choose the manifest's shape, following `docs/execution-ownership.toml` as the model. It must express, at minimum:
   - a source-path pattern → the workspace member(s) it belongs to;
   - a source-path pattern → the test target(s) that cover it, where the target list may be empty (meaning "this member's lib test target only" — for the seven members with no integration targets);
   - a **catch-all** rule for paths not matched by any entry, because the mode must never silently skip a changed file.
2. Define the selection algorithm precisely, in prose, before implementing it:
   - changed files are computed against a base ref (the agent must choose the ref semantics and document them);
   - a changed file that falls inside a member's directory marks that member affected;
   - an affected member contributes its lib test target (always) plus the integration targets the manifest names for the matching patterns;
   - a changed file matching no pattern takes the documented catch-all action — widen to the whole affected member, or refuse. **Silently skipping is forbidden** (I5).
   - a changed file **outside** every workspace member (for example a top-level script, a doc, or an asset) takes the documented action — the guard-trigger routing already classifies these, so the mode should reuse that classification rather than invent one (§3.6).
3. Decide how the manifest references targets so that **M004's consolidation shrinks the manifest rather than rewriting it.** The natural shape is a target-name pattern (a glob over target names) plus a family concept, so that when 271 targets become ~15 families, a pattern that matched `tui_*` still matches `tui_family`. The agent must pick a shape and state how M004 will use it. This is a design constraint on the schema, not an implementation of M004.

Acceptance evidence:

- The schema, with a worked example per member type (a member with integration targets, a member with only a lib target, a path outside every member).
- The selection algorithm written out, including the catch-all and out-of-member behaviours.
- A written statement of how the schema survives M004's consolidation.

### Work package B — Populate the manifest

Intent: a manifest that is complete enough to be useful and honest enough not to lie.

Required changes:

1. Populate the manifest for all twelve workspace members.
2. For the root package — 282 targets at the baseline — group targets by concern into a set of **coverage groups**, each naming the source paths it covers and the targets in the group. This is the bulk of the work and it is inherently a judgement call about which test covers which source.
3. Prefer an explicit group over a broad catch-all, but accept a catch-all for the root package where a defensible per-file mapping is not available. **The agent must record which groups are precise and which are catch-alls** — an undifferentiated catch-all for the whole root package would make the mode correct but useless, and the agent should say so plainly if that is the honest outcome.
4. Cross-check every group against the tests that exist, so no group names a target that does not exist and no target is silently unowned.

Acceptance evidence:

- The populated manifest.
- The precise-versus-catch-all breakdown, stated honestly.
- A report of any target that could not be attributed to any group.

### Work package C — Implement the guard

Intent: the manifest cannot drift from reality.

Required changes:

1. `scripts/check_test_ownership.py` must fail when:
   - a manifest entry names a test target that does not exist;
   - a test target exists but is named by no manifest entry;
   - a manifest entry names a workspace member that does not exist;
   - a manifest source pattern matches no file (an entry that can never fire);
   - the manifest's own schema is malformed.
2. The guard must carry a `--self-test` that injects each violation class and proves the guard rejects it, in the same style as the repository's other change-triggered guards.
3. Wire the guard into `scripts/verify.sh quick` and the CI job.

Acceptance evidence:

- The guard passing on the changed tree.
- The `--self-test` output for every violation class.
- The guard listed in `scripts/verify.sh quick` and in the CI job's guard invocation.

### Work package D — Implement the scoped mode

Intent: the developer-facing command.

Required changes:

1. The mode accepts a base ref (and must document its default and its ref semantics — merge-base versus direct diff, and what happens on a shallow clone).
2. The mode computes the affected members and targets, then runs:
   - the affected members' checks and lints;
   - the affected members' test targets, using repeated `--test` flags so that unselected targets are not linked.
3. The mode's output must state, plainly: the changed files it considered, the members it selected, the targets it selected, the targets it skipped, and the reason for each skip.
4. The mode must fail loudly (non-zero) when it cannot determine a safe scope — an unresolvable diff, a changed file with no mapping and no defensible catch-all, or a manifest that fails its guard.
5. The mode must compose with the existing guard-trigger routing: the change-triggered guards still run exactly as they do today (I9).
6. The mode must never reduce the set of tests below the manifest's declared coverage for the affected members. If the manifest says a member is covered by a set of targets, all of them run.

Acceptance evidence:

- The mode's invocation, its output format, and its failure output.
- A demonstration on a representative change: one file in one leaf crate (for example a file in `crates/eggsentry`) selects the expected members and targets, and the built-executable count is materially below the workspace-wide form.

### Work package E — Prove the scope is a superset

Intent: establish the milestone's core correctness property.

Required changes:

1. For a set of representative changes spanning several subsystems, compute the scoped selection and compare it against the union of what the workspace-wide run would execute.
2. Prove that the scoped set **contains** the tests that actually depend on the change. This is the hard part, and it cannot be established by inspection alone for every case; the honest evidence is:
   - a per-change argument for the representative cases, naming the covering test targets;
   - a coverage-instrumented run for at least one representative case, if the toolchain permits — that is, compile with coverage and compare the covered code against the scoped selection's targets;
   - where coverage instrumentation is unavailable, an explicit statement of what was argued and what was not proven.
3. Establish the **negative** property: a change that touches a subsystem with a known covering test must select that test. This is the property that would fail if the manifest were wrong, and it is the one that must be demonstrated.

Acceptance evidence:

- The representative-case table: change → selected members → selected targets → the argument for sufficiency.
- The coverage-instrumented comparison for at least one case, or an explicit, itemised statement of what was argued instead.
- The negative-case demonstration.

### Work package F — Measure and record

Intent: produce the evidence the closure requires and correct the documentation.

Required changes:

1. Measure the scoped pass's cost against the workspace-wide form on the same tree, with the method stated — and against the **post-M001** baseline, since M001 is a hard dependency.
2. Run the hosted `CI / verify` job on the change and confirm the hosted scope is unchanged.
3. Update `architecture/testing.md` and the testing-ci skill.
4. Record what the mode does not do.

Acceptance evidence:

- The scoped-versus-workspace-wide comparison, with the unit-time or wall-clock method stated.
- The hosted run id, with a statement that the hosted job's executed set is unchanged.
- Documentation updates.

## 8. Failure, cancellation, restart, and contention semantics

- **Unresolvable scope must fail, not default.** If the base ref cannot be resolved (a shallow clone, a missing ref, a first commit), the mode must exit non-zero with an actionable message. It must not fall back to running everything, because "running everything" as a silent fallback hides the reason the scoped path was unavailable.
- **A changed file with no mapping must widen or refuse.** Both are acceptable; silently skipping is not (I5).
- **Cancellation.** The mode must be interruptible in the same way the existing modes are, and must not leave a partially-written scope artifact that a later run would trust.
- **Contention.** The scoped mode inherits the `ci` profile's slot bound and exclusions when it runs under that profile. A scoped run selecting one of the six whole-binary exclusion binaries must preserve the exclusion's semantics; if the scoped set cannot be run under a profile that carries the exclusion, that is a stop condition (§14).
- **Restart.** The mode holds no durable state. A re-run recomputes the scope from the tree and the base ref.
- **Concurrent invocations.** Two scoped modes against the same target directory contend on cargo's lock, as the existing modes do. The mode must not introduce a second lock ordering.

## 9. Compatibility and migration

- **Developer-local compatibility.** The mode is additive. The existing `quick` and `full` modes keep their exact behaviour and are the default when no mode is named.
- **CI compatibility.** None required — the hosted job's scope is unchanged by design (I3). The manifest's guard does run in CI, which is new but cheap (the guard is a static check over a manifest and a target list).
- **Migration.** None. No persisted state, no protocol, no stored configuration.
- **Manifest evolution.** When M004 consolidates targets, the manifest must be updated in the same change (M004 carries that interface dependency). The schema chosen in work package A must make that an update of group membership rather than a rewrite of the schema.

## 10. Required tests

### Focused unit tests

The manifest-matching and scope-computation logic is the natural subject of focused tests:
- a changed file inside a member's directory selects that member;
- a changed file matching a group's pattern selects that group's targets;
- a changed file matching no pattern takes the documented catch-all action;
- a changed file outside every member takes the documented action;
- a member with no integration targets contributes only its lib test target;
- an empty change set (no changed files) produces the documented empty scope — and the mode's behaviour for an empty scope must be explicit, not accidental;
- a base ref that cannot be resolved fails rather than defaulting.

### Integration tests

- The mode against a real working tree: the representative-change table from work package E.
- The mode's composition with the existing change-triggered guard routing: a change that triggers a guard still runs that guard (I9).

### Restart and recovery tests

None. The mode holds no durable state.

### Contention and cancellation tests

- The mode is interruptible mid-run and leaves no stale scope artifact.
- A scoped run selecting an exclusion binary preserves the exclusion's semantics.

### Security and negative tests

- The guard's `--self-test`, covering every violation class in work package C.
- The guard run against a deliberately broken manifest (a manifest naming a nonexistent target), proving the repository-side rejection independently of the self-test.
- The mode refusing an unresolvable base ref, with a non-zero exit and an actionable message.
- The mode refusing (or widening on) a changed file with no mapping — proven to be non-silent.

### Migration and compatibility tests

- The hosted job's executed set is unchanged: compare the hosted test-function count against the baseline count, and record it.

## 11. Required verification commands

```bash
# the new guard, including its self-test
python3 scripts/check_test_ownership.py
python3 scripts/check_test_ownership.py --self-test

# the scoped mode, against a representative base ref
scripts/verify.sh scoped <base-ref>

# the scope computation in isolation, for the work-package-E table
git diff --name-only <base-ref>...HEAD

# formatting and linting
cargo fmt --check
cargo clippy --workspace --all-targets --locked -- -D warnings

# routine guard sweep (now including the new guard)
scripts/verify.sh quick

# broader suite — the hosted scope must still be the full suite
cargo nextest run --workspace --locked --profile ci

# hosted evidence
# .github/workflows/ci.yml — CI / verify, on the change
```

## 12. Documentation updates

- `architecture/testing.md` — the scoped mode's contract, its guarantees and limits, the manifest's location, the catch-all and out-of-member behaviours, and the statement that the hosted job's scope is unchanged.
- `docs/test-ownership.toml` — the manifest itself, with a header comment stating its schema in prose, in the `docs/execution-ownership.toml` style.
- `.opencode/skills/testing-ci/SKILL.md` — the mode, if the skill enumerates verification modes.
- `AGENTS.md` — the skills index and the "Quick start" section if they enumerate the verification modes. The AGENTS.md index must continue to resolve to `architecture/testing.md`.
- `plans/subsystems/ci-test-throughput-recovery-roadmap.md` — M003's status row.
- `plans/registry.md` — M003's status transitions.

## 13. Acceptance criteria

1. A scoped mode exists, accepts a base ref, computes an affected scope, and runs only the selected targets — with the unselected targets demonstrably **not built** (artifact evidence, not assertion).
2. The mode's output states the changed files considered, the members selected, the targets selected, the targets skipped, and the reason for each skip.
3. The manifest is machine-readable, is statically guarded, and the guard carries a `--self-test` that rejects each violation class; the guard runs in `scripts/verify.sh quick` and in CI.
4. A scoped run is demonstrated to be a **superset** of the affected change's coverage, with a per-case argument and at least one coverage-instrumented comparison, or an explicit statement of what was argued instead.
5. A changed file with no mapping widens or refuses — proven non-silent — and an unresolvable base ref fails with a non-zero exit.
6. The hosted `CI / verify` job's executed test set is unchanged, proven by a before-and-after count from a hosted run.
7. The `ci` Nextest profile is byte-identical to the baseline.
8. No test file, test target, or test assertion changed; no dependency, feature default, or product behaviour changed.
9. `scripts/verify.sh quick` and `verify.sh full` both pass unchanged.
10. The measured scoped cost is reported against the post-M001 baseline with the measurement method stated, together with an explicit statement of the irreducible rebuild chain — the saving is a link-count saving, not a rebuild-chain saving.

## 14. Stop conditions

The implementing agent must stop and report rather than improvise when:

- the manifest cannot be populated to a precision that makes the scoped run meaningfully cheaper than the workspace-wide run — in that case, say so and propose the honest alternative (a catch-all with an honest label) rather than shipping a mode that is correct but useless;
- the root package's coverage cannot be grouped defensibly, and the only available grouping would be a single whole-root catch-all;
- the scoped selection would reduce the executed set below the manifest's declared coverage for an affected member;
- the mode would need to modify the `ci` Nextest profile or its exclusions (I7);
- a scoped run selecting an exclusion binary cannot preserve that exclusion's semantics;
- the mode would require changing the hosted job's scope (I3) — that is not this milestone;
- coverage instrumentation is unavailable **and** the per-case sufficiency argument cannot be made for a representative case, such that work-package E's superset property would be unevidenced;
- scope would expand into M004's consolidation, or into the D1 coverage decision.

## 15. Closure evidence required

The closure record `plans/closure/ci-test-throughput-recovery/003-status.md` must contain:

1. the manifest schema and the selection algorithm as implemented;
2. the populated manifest, with the precise-versus-catch-all breakdown stated honestly;
3. the guard's name, its `--self-test` output for every violation class, and its output against a deliberately broken manifest;
4. the scoped-versus-workspace-wide measurement, with the method stated, measured against the **post-M001** baseline, and an explicit statement of the irreducible rebuild chain;
5. the artifact evidence that unselected targets were not built;
6. the work-package-E table: representative change → selected members → selected targets → sufficiency argument;
7. the coverage-instrumented comparison, or the explicit itemised statement of what was argued instead;
8. the negative-case demonstration (a change touching a subsystem with a known covering test selects that test);
9. the non-silence evidence: an unmapped changed file widens or refuses, and an unresolvable base ref exits non-zero;
10. the hosted run id and the before-and-after hosted test-function count, showing the hosted scope unchanged;
11. a confirmation that the `ci` profile is byte-identical to the baseline, and that no test file, test target, or assertion changed;
12. the documentation diffs for `architecture/testing.md`, the manifest, the testing-ci skill, and `AGENTS.md`;
13. the registry updates performed, including the dependency audit against the registry's blocked-work section.

## 16. Handoff notes

- **Resource constraint.** The root library rlib is 855 MB and root test executables are 135–208 MB each, totalling 13.7 GB for the root package. The scoped mode still builds the root library; it saves links, not the library. Do not describe it as skipping the root build.
- **The mechanism is proven; do not re-derive it.** `cargo nextest run --test <name>` restricts the cargo build; repeated `--test` flags work. `-E` does not. If the agent finds `--test` behaving differently on a newer `cargo-nextest`, that is a finding to record, not a reason to redesign.
- **Coverage instrumentation may not be available.** The repository's dependencies do not obviously include a coverage tool; `cargo llvm-cov` may not be installed. The plan's work package E anticipates this and requires an explicit statement of what was argued instead — it is not a licence to claim the superset property without evidence.
- **Manifest effort is the real cost.** The bulk of this milestone is judgement about which test covers which source, for 282 root targets. Budget for it. The schema in work package A is designed so that M004's consolidation shrinks the manifest — do not hand-author 282 individual entries if a group structure will serve.
- **Toolchain.** Rust 1.89+, edition 2021, stable. `cargo-nextest` 0.9.140 at the baseline. `cargo --timings=json` is nightly-only — use plain `--timings`.
- **Preserved user changes.** The worktree may carry unrelated local modifications; do not stash, reset, or discard them.

## 17. Implementation record — and two defects the coverage comparison caught

Status: **implemented** (2026-10-21); closure `plans/closure/ci-test-throughput-recovery/003-status.md`.

### 17.1 What the plan got wrong about the repository

Work package E asks for a coverage-instrumented comparison. Running it is not a formality here: it found **two defects** in the first implementation of this milestone, both of which broke the plan's own invariant I4 ("a scoped run is a superset of the change's coverage").

**Defect 1 — a change to a crate another crate depends on selected almost nothing.**

The first implementation resolved a changed file to exactly one member and selected that member's targets. For `crates/eggsentry/src/command.rs` that selected `eggsentry`'s library test target and nothing else.

That is wrong, and the mechanism is specific to this repository:

- the root package depends on **all eleven** other workspace crates;
- the root package **re-exports** several of them — `codegg::security::*` is `eggsentry`, `codegg::lsp::*` is `egglsp`, `codegg::scheduler::config::*` is `codegg-config`;
- `src/tool/security.rs` calls `eggsentry`'s `inspect_text`, `inspect_file`, `ProfileRunner` and `ProfileConfig` through that re-export;
- **seven** root integration test files in three families (`agent_loop`, `command_git`, `tool_programs`) call that tool, and the root's **inline** tests in `src/tool/security.rs` call it directly.

So a change to `eggsentry` changes the behaviour of seven root integration test files and a block of root inline tests, and the first implementation selected none of them. The coupling runs **through the dependent's own source**, which is why no path-to-target rule could ever see it: the test file that calls the security tool never names `eggsentry` anywhere.

**Defect 2 — a selected target that could not be built.**

Four root families are gated behind `required-features`. A change to `src/lsp/**` selects the `lsp_test_support` family, and cargo refuses to build a gated target without its feature:

```
error: target `lsp_test_support` in package `codegg` requires the features: `lsp-test-support`
```

The first implementation named the gated target without enabling the feature, so the scoped mode failed on every change touching a gated subject.

### 17.2 The fix — a dependency-edge section in the manifest

`docs/test-ownership.toml` gains a `[[dependencies]]` section: one entry per **workspace dependency edge**, `member` = the crate that can be affected, `dependent` = the crate that depends on it. The scope rule is:

> When `member` is affected, every member that transitively depends on it is affected too, and a member pulled in that way contributes its **full** surface — its library test target and every integration target it owns.

The full surface is the honest answer, and the reason is stated in the manifest: the dependent's tests are black-box over the dependent's public API, and that API re-exports the dependency, so *any* of the dependent's tests can reach the changed code. A tighter answer needs a call graph from every test to every crate, which the repository does not have.

The edge set is **mechanically verifiable**, which is the part that matters. `scripts/check_test_ownership.py` compares the manifest's declared edges against `cargo metadata`'s workspace-local dependency edges and fails when:

- an edge is **missing** — a dependency added to the workspace without a manifest entry is a silent under-selection, and the guard catches it;
- an edge is **invented** — a declared edge `cargo metadata` does not report is a fabricated widening;
- an edge names an **undeclared member**, names the same crate on both sides, or is declared twice.

The guard's `--self-test` injects each of those classes and proves each is rejected (twelve violation classes in total, up from nine).

The cost is stated rather than hidden: a change to any non-root crate widens the scope to the root package's whole integration surface, because the root package depends on every other crate. A change confined to the root package's own `src/` — the common case — stays precise, because the root package has no dependents.

For the feature gates, the mode reads each selected target's `required-features` from `cargo metadata` and enables exactly those features for that member's invocation, printing them. Naming a gated target without its feature is a cargo error; the mode now makes it a working command.

### 17.3 A third defect — an empty array under `set -u` in bash 3.2

`scripts/verify.sh` runs under `set -euo pipefail`, and the shell macOS ships is **bash 3.2**. In bash 3.2, expanding an **empty** array with `"${arr[@]}"` under `set -u` is an `unbound variable` **error**.

The scoped mode's feature list is exactly such an array, and it is empty whenever the selected targets need no cargo feature — the common case. Expanding it plainly would have failed the scoped mode on every unfeatured selection, with an error that says nothing about what went wrong.

The fix is the portable-safe expansion `"${arr[@]+"${arr[@]}"}"`. The hazard is now covered by a **self-test on the selector**, which proves three properties the caller depends on: the dependency pull-in widens the dependent; the `members` line has exactly three tab-separated columns (the caller reads it with `IFS=$'\t' read -r name targets features`, so a column that moves is a scope that silently changes); and a member with no required feature yields an **empty** third column — the exact case that broke. The self-test is wired into `scripts/verify.sh quick` and into the CI job.

### 17.4 A fourth defect — a member affected twice lost the dependency widening

The pull-in was first implemented as a fixed-point loop that **skipped** a
dependent already in the affected set. That is wrong for a change touching two
areas at once — say `crates/eggsentry/src/command.rs` *and*
`src/tui/app/mod.rs`. The root package is then directly affected *and* pulled
in by `eggsentry`'s dependency; the old loop skipped the pull-in, so the root
package contributed **one** target instead of its full surface, and the
dependency widening was silently dropped for exactly the case where it mattered
most.

The fix is a worklist rather than a fixed-point loop over the affected set.
The self-test pins both halves: a change confined to the root package's own
source stays **precise** (1 target — the root package has no dependents), and a
change touching two areas gives the root package its **full** surface (21
targets).

### 17.5 What the fix costs and preserves

**Preserved.** The scope is now a genuine superset for the dependency direction as well as the path direction. The guard's "every workspace dependency has an edge" rule means the direction cannot rot silently. The hosted CI scope is untouched. The guard set is untouched.

**Dropped.** Nothing. The change makes the mode's output *true* where it was previously false.

### 17.6 What is measured and what is still argued

**Measured** (closure record §6.1): the leaf-crate case, with `cargo llvm-cov`. The root package's tests execute **198 lines** of `eggsentry`'s code across four of its five modules, and the root's inline test `src/tool/security.rs::inspect_text_returns_findings` asserts on `eggsentry`'s own classification decision (`findings[0]["category"] == "secret_exposure"`). A change to the scanner changes that assertion's outcome, and that test was outside the first implementation's selection. The measurement is explicit about what it does **not** show: at line level `eggsentry`'s own tests already cover a superset of what the root's tests cover (214 ⊇ 198), so the coverage comparison does not demonstrate a line-level gap. It demonstrates the **dependency**, which is the thing a scoped run has to respect.

**Still argued, not proven**: the other five representative cases. Instrumenting and relinking all 47 test targets costs exactly what this workstream exists to reduce. The rule that covers them — a dependent contributes its **full** surface — is sound for any dependent and does not depend on the measured case; the guard proves the declared dependency edge set is exactly `cargo metadata`'s, so the direction cannot rot silently.
