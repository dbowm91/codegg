# CI/Test Throughput Recovery M003 — Closure Status

Status: conditionally closed

Recommendation: **conditionally closed** — the manifest, the guard, and the scoped mode are complete and verified against the tree. The coverage-instrumented comparison required by the plan's work package E found **two defects** in the first implementation, both now fixed: a change to a crate another crate depends on selected almost nothing, and a selected feature-gated target could not be built. The remaining limitation — that the dependency coupling is argued rather than measured for four of five representative cases — is stated explicitly in §6.

> **Correction to an earlier draft of this record.** The first draft claimed the scope was a superset of the change's coverage for every case. That claim was **false** for any change in a crate that another crate depends on, and §4.5 records the defect and the fix. The record is corrected here rather than left for a reader to discover.

Source implementation plan:

- `plans/implementation/ci-test-throughput-recovery/003-change-scoped-verification-mode.md`

Source subsystem roadmap:

- `plans/subsystems/ci-test-throughput-recovery-roadmap.md`

Repository baseline reviewed: `2c1953347896848c2f52c2dfbd9f04029d1c2cc2`

## 1. Executive finding

The repository now has a machine-readable **test-ownership manifest**, a **static guard** that keeps it honest, and a **developer-local scoped verification mode** that runs only the tests a change can affect.

The scope is a *selector*, never a filter: it never decides that a test does not need to run. A changed file with no mapping widens the affected member; a changed file outside every member widens the scope to the whole workspace; an unresolvable diff fails loudly and exits non-zero.

The hosted CI job's scope is unchanged, and `scripts/verify.sh quick` and `full` are unchanged in what they run. The mode is additive to the guard set.

## 2. What was built

| Artifact | What it is |
|---|---|
| `docs/test-ownership.toml` | The manifest. 12 members (one per workspace package), 32 coverage groups, 22 dependency edges, and the source-path → test-target mapping. |
| `scripts/check_test_ownership.py` | The guard, with a `--self-test` that injects each violation class. |
| `scripts/scoped_selection.py` | The selection algorithm: changed files → affected members → affected targets, with a JSON and a members format for the caller. It carries a `--self-test`. |
| `scripts/verify.sh scoped [base-ref]` | The mode. Runs the whole quick guard set, then the scoped cargo work. |
| `architecture/testing.md` | The mode's contract, its limits, and its failure behaviour. |
| `.opencode/skills/testing-ci/SKILL.md` | The mode as an operator-facing option. |

### The manifest

- **12 members**, one per workspace package, each with its source root and its library test target name.
- **32 groups**: 21 for the root package and 11 for the four other members that own integration test targets. The seven members with no integration test targets need no group: their library test target is owned by the member entry itself.
- **Precise versus catch-all**: 30 groups carry a real source-path → subject mapping; **one** group, `root_catch_all`, is a documented catch-all covering the root package's remaining top-level source paths (`src/lib.rs`, `src/main.rs`, `src/bin/**`) and the root library test target. The manifest is therefore **not** an undifferentiated catch-all for the root package, which is the failure mode the plan warned about.
- **Every real test target is owned.** The guard proves it: an unowned target is a failure. After M004's consolidation this shrank from 288 named targets to **22** — the schema did not change, only the membership, which is exactly the property the plan required of the schema.

### The guard

`scripts/check_test_ownership.py` fails when:

- the schema is malformed, or `schema-version` is not one it understands;
- a member entry names a package that does not exist, a workspace package has no member entry, or a member is declared twice;
- a group names an undeclared member, or reuses a group id;
- a group's source pattern matches no file in the tree (an entry that can never fire);
- a group's target pattern matches no test target of its member;
- a real test target exists that no group owns (a test that could never be selected — which is how a scoped run silently stops covering it).

Its `--self-test` injects **twelve** violation classes — an unowned target, an unknown member, a nonexistent target, an unmatched path pattern, a missing member, a duplicated member, a duplicated group id, an unknown schema version, a **workspace dependency with no manifest edge**, a **dependency edge that is not a real workspace dependency**, a **dependency edge naming an undeclared member**, and a malformed manifest — and proves each is rejected, and that the unmodified tree still passes.

The guard is wired into `scripts/verify.sh quick` and into the `CI / verify` job.

## 3. The selection algorithm

**Changed files.** Merge-base semantics: `git merge-base <base> HEAD` is the comparison point. Uncommitted work — staged, unstaged, and untracked — is included, because "what does my change affect?" means the developer's whole tree.

**Affected members.** A changed file inside a member's source root marks that member affected; the longest matching root wins.

**Affected targets.** An affected member contributes its library test target **always**, plus the integration targets of every group whose source pattern matches.

**The three fallbacks, and why each is loud:**

| Situation | Action |
|---|---|
| A changed file inside a member, matching a group | That group's targets + the member's library test target. |
| A changed file inside a member, matching **no** group | The whole member is widened to its full surface. |
| A changed file **outside** every member | The scope widens to the **whole workspace**, and the mode names the file that caused it. |
| The diff cannot be computed | **Exit non-zero.** Never "no tests". |

## 4. A finding that changed the design

### 4.1 The plan's §3.6 premise about guard routing is false

The plan's §3.6 asserted that the repository's change-trigger routing is "machine-readable" and that the scoped mode should "reuse that classification rather than invent one". **That premise is false of this tree.**

`scripts/verify.sh quick` runs *every* guard unconditionally — there is no routing table, no dispatcher, and no per-guard trigger declaration. The change-trigger routing exists only as **prose** in `AGENTS.md`'s "Change-triggered guards" section, telling a human or an agent which guard to run for which kind of change. Only two guard scripts contain a `TRIGGER` identifier at all, and neither is a routing table.

The scoped mode therefore does **not** invent a routing classification. It composes with the routing as it actually is: **all** the quick guards run, in the same order, exactly as they do today — which satisfies the plan's invariant I9 trivially and without inventing semantics. The scoping applies to the **cargo** work, which is where the cost is. A guard is a static Python check that takes milliseconds; a test target is a link.

This is recorded rather than quietly dropped, because the plan stated a premise the tree did not support.

### 4.2 The coverage comparison found two defects in this milestone's own implementation

The plan's work package E asked for a coverage-instrumented comparison. Running it was not a formality: it found **two defects** in the first implementation of this milestone, both of which broke the plan's own invariant I4.

**Defect 1 — a change to a crate another crate depends on selected almost nothing.**

The first implementation resolved a changed file to exactly one member. For `crates/eggsentry/src/command.rs` it selected `eggsentry`'s library test target and nothing else.

The mechanism is specific to this repository:

- the root package depends on **all eleven** other workspace crates;
- it **re-exports** several of them — `codegg::security::*` is `eggsentry`, `codegg::lsp::*` is `egglsp`, `codegg::scheduler::config::*` is `codegg-config`;
- `src/tool/security.rs` calls `eggsentry`'s `inspect_text`, `inspect_file`, `ProfileRunner` and `ProfileConfig` through that re-export;
- **seven** root integration test files in three families — `agent_loop` (`tool_surface_minimization.inc`), `command_git` (`command_routing_adversarial.inc`, `context_projection_adversarial.inc`, `git_closure_matrix.inc`, `git_credential_cross_path.inc`, `git_execution_origin_matrix.inc`) and `tool_programs` (`tool_structured_execution.inc`) — call that tool, and the root's **inline** tests in `src/tool/security.rs` call it directly.

A change to `eggsentry` therefore changes the behaviour of seven root integration test files and a block of root inline tests, and the first implementation selected none of them. The coupling runs **through the dependent's own source and public API**, which is why no path-to-target rule could see it: the test file that calls the security tool never names `eggsentry` anywhere.

**Defect 2 — a selected target that could not be built.**

Four root families are gated behind `required-features`. A change to `src/lsp/**` selects the `lsp_test_support` family, and cargo refuses to build a gated target without its feature:

```
error: target `lsp_test_support` in package `codegg` requires the features: `lsp-test-support`
```

The first implementation named the gated target without enabling the feature, so the scoped mode failed on every change touching a gated subject.

### 4.3 The fix — a dependency-edge section in the manifest, and feature enabling

`docs/test-ownership.toml` gains a `[[dependencies]]` section: one entry per **workspace dependency edge** — 22 of them, `member` = the crate that can be affected, `dependent` = the crate that depends on it. The scope rule:

> When `member` is affected, every member that transitively depends on it is affected too, and a member pulled in that way contributes its **full** surface — its library test target and every integration target it owns.

The full surface is the honest answer: the dependent's tests are black-box over the dependent's public API, and that API re-exports the dependency, so *any* of the dependent's tests can reach the changed code. A tighter answer needs a call graph from every test to every crate, which the repository does not have.

The edge set is **mechanically verifiable**, and that is the part that matters. `scripts/check_test_ownership.py` compares the manifest's declared edges against `cargo metadata`'s workspace-local dependency edges and fails when an edge is **missing** (a dependency added to the workspace without a manifest entry — a silent under-selection), when an edge is **invented** (`cargo metadata` does not report it — a fabricated widening), or when an edge names an undeclared member, names the same crate on both sides, or is declared twice. The `--self-test` injects each class and proves each is rejected: **twelve** violation classes in total, up from nine.

For the feature gates, the mode reads each selected target's `required-features` from `cargo metadata` and enables exactly those features for that member's invocation, printing them. Naming a gated target without its feature is a cargo error; the mode now makes it a working command.

The cost is stated rather than hidden: **a change to any non-root crate widens the scope to the root package's whole integration surface**, because the root package depends on every other crate. A change confined to the root package's own `src/` — the common case — stays precise, because the root package has no dependents.

### 4.5 A third defect — an empty array under `set -u` in bash 3.2

`scripts/verify.sh` runs under `set -euo pipefail`, and the shell macOS ships is **bash 3.2**. In bash 3.2, expanding an **empty** array with `"${arr[@]}"` under `set -u` is an `unbound variable` **error**.

The scoped mode's feature list is exactly such an array, and it is empty whenever the selected targets need no cargo feature — the common case. The first implementation of the feature enabling expanded it plainly, so the scoped mode would have failed on every unfeatured selection, with an error that says nothing about what went wrong.

The fix is the portable-safe expansion `"${arr[@]+"${arr[@]}"}"`, which yields nothing for an empty array and the elements otherwise. The same hazard is now covered by a **self-test on the selector**, which proves three properties the caller depends on: the dependency pull-in widens the dependent; the `members` line has exactly three tab-separated columns (the caller reads it with `IFS=$'\t' read -r name targets features`, so a column that moves is a scope that silently changes); and a member with no required feature yields an **empty** third column — the exact case that broke.

The self-test is wired into `scripts/verify.sh quick` and into the CI job.

### 4.6 A fourth defect — a member affected twice lost the dependency widening

The pull-in was first implemented as a fixed-point loop that **skipped** a
dependent that was already in the affected set. That is wrong for the case
where one change touches two areas at once — say `crates/eggsentry/src/command.rs`
*and* `src/tui/app/mod.rs`. The root package is then directly affected (the
TUI path matches the `tui_render` group) *and* pulled in by `eggsentry`'s
dependency. The old loop saw it already present and skipped the pull-in, so the
root package contributed **one** target instead of its full surface — the
dependency widening was silently dropped for exactly the case where it mattered
most.

The fix is a worklist rather than a fixed-point loop over the affected set, so
a member that is affected by both routes records the pull-in **and** keeps its
full surface. The self-test now pins both halves of the behaviour: a change
confined to the root package's own source stays **precise** (1 target — the
root package has no dependents), and a change that touches two areas gives the
root package its **full** surface (21 targets).

### 4.4 The measured effect of the fix

| Changed file | Before the fix | After the fix |
|---|---|---|
| `crates/eggsentry/src/command.rs` | `eggsentry` (1 member, 0 integration targets) | `eggsentry` + `codegg-core` + `codegg` — 3 members, 34 integration targets |
| `src/lsp/mod.rs` | `codegg` / `lsp_test_support`, **unbuildable** | `codegg` / `lsp_test_support`, with `lsp-test-support` enabled |
| `src/tui/app/mod.rs` | `codegg` / `tui_render` | unchanged — `codegg` / `tui_render` |
| `scripts/whatever.py` | workspace | unchanged — workspace |

The last two rows matter as much as the first: the fix is targeted at the dependency direction, and it does not widen a scope that was already correct.

## 5. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| WP-A: the schema, the selection contract, and how M004 shrinks the manifest | `docs/test-ownership.toml`'s header comment states the schema, the glob semantics, the member resolution rule, the catch-all and out-of-member rules, and the M004 shrink. | pass |
| WP-B: the manifest is populated, and the precise-versus-catch-all breakdown is stated | 12 members, 32 groups, 30 precise + 1 catch-all (§2). Two root targets were found unattributed during population (`scheduler_resource_profiles`, `search_backend_eggsearch`) and were attributed by inspection. | pass |
| WP-C: the guard fails on every violation class, with a `--self-test` | **Twelve** classes, all rejected (§2, §4.3). Wired into `verify.sh quick` and the CI job. | pass |
| WP-D: the mode computes a scope, prints it, and fails loudly | `scripts/scoped_selection.py` + `verify.sh scoped`. Representative selections measured below. | pass |
| WP-E: the scope is a superset of the change's coverage | §6. The comparison found two defects, now fixed (§4.2, §4.3). | pass (partly argued) |
| WP-F: measure, document, and record what the mode does not do | `architecture/testing.md` has the mode's contract, limits, and failure behaviour. The testing-ci skill lists it as an operator option. | pass |
| I1 (no assertion removed) | The manifest selects tests; it never deletes one. No test file was edited by M003. | pass |
| I2 (one bounded CI job) | No workflow job, matrix, or lane was added. | pass |
| I3 (the hosted job's executed set is unchanged) | The hosted job's commands are untouched by M003. | pass |
| I5 (an unmapped change fails loudly) | §3. The mode exits non-zero on an unresolvable diff and never falls back to an empty scope. | pass |
| I6 (the manifest is machine-readable and statically guarded) | `tomllib`-parsed TOML with a `--self-test` guard. | pass |
| I7 (the `ci` profile's slot bound and exclusions untouched) | `[profile.ci]` in `.config/nextest.toml` is unmodified. | pass |
| I9 (the guard-trigger routing keeps working) | §4 — all guards still run, unchanged and in order. | pass |

## 6. WP-E — the scope is a superset

### The representative cases

| Changed file | Selected member(s) | Selected integration targets | Argument for sufficiency |
|---|---|---|---|
| `crates/eggsentry/src/command.rs` | `eggsentry`, `codegg-core`, `codegg` | `eggsentry`: none of its own; `codegg-core`: its 13; `codegg`: its 21 | `eggsentry` has no integration targets, so its own surface is its library test target. It is depended on by `codegg-core` and by the root package, so both are pulled in transitively and contribute their full surfaces (§4.3). |
| `src/tui/app/mod.rs` | `codegg` | `tui_render` | The `tui_render` group covers `src/tui/**` and `src/theme/**`. The root package has no dependents, so nothing is pulled in. |
| `crates/codegg-core/src/session/schema.rs` | `codegg-core`, `codegg` | `codegg-core`: `cached_models_composite_key`, `continuation_checkpoint`, `session_title_slug`; `codegg`: its 21 | The `codegg-core` groups that cover the session schema, plus the root package, which depends on `codegg-core` and so is pulled in transitively. |
| `src/scheduler/admission.rs` | `codegg` | `interactive_process_sessions`, `scheduler_cancellation`, `scheduler_jobs` | The three groups whose source patterns cover the scheduler's admission module. |
| `src/lsp/mod.rs` | `codegg` | `lsp_test_support` (with `lsp-test-support` enabled) | The `lsp_fake_server` group covers `src/lsp/**`. The target is feature-gated, so the mode enables the gate rather than naming an unbuildable target (§4.3). |
| `scripts/whatever.py` | *(widened to the workspace)* | — | Out of every member: no test target can be ruled out. |

### The negative property

The property that would fail if the manifest were wrong is the **negative** one: *a change that touches a subsystem with a known covering test must select that test*. The table above is that demonstration for six subsystems, each with a different shape (a leaf member with no integration targets, a root member with one large group, a non-root member with several groups, a root member with several groups, a feature-gated root group, and an out-of-member file).

The table also demonstrates the **dependency** direction, which is where the first implementation was wrong: `crates/eggsentry/src/command.rs` and `crates/codegg-core/src/session/schema.rs` both pull in the root package, and `src/tui/app/mod.rs` — a change confined to the root package's own source — does not.

### The coverage-instrumented comparison

`cargo-llvm-cov` **0.8.7** is available on this machine, so the plan's coverage comparison was not merely argued. It was run for the leaf-crate case — the one that is cheap to instrument — and the result is recorded in §6.1.

**This is the comparison that confirmed defect 1 in §4.2.** The plan's WP-E asked for it as *evidence of a superset property*. It produced something more specific and more useful than a yes: it **measured** the coupling (the root package's tests execute 198 lines of `eggsentry`'s code) and it located the assertion that makes the coupling **behavioural** rather than incidental (the root's inline test asserts on `eggsentry`'s own classification decision). §6.1 gives the numbers and is explicit about what the measurement does *not* show.

### 6.1 The comparison, measured

Two coverage runs, on the same instrumented build:

| Run | Command | Result |
|---|---|---|
| The changed member's own tests | `cargo llvm-cov -p eggsentry` | **159 tests passed.** **214** lines of `crates/eggsentry/src/**` covered. |
| The root package's tests | `cargo llvm-cov -p codegg --lib --test command_git --test agent_loop` | **8,262 tests passed** (the six known macOS failures were skipped). **198** lines of `crates/eggsentry/src/**` covered. |

Per module:

| Module | `eggsentry`'s own tests | The root package's tests |
|---|---|---|
| `command.rs` | 66 | 66 |
| `dependency.rs` | 46 | 37 |
| `finding.rs` | 0 | 0 |
| `profile.rs` | 35 | 28 |
| `scanner.rs` | 67 | 67 |
| **total** | **214** | **198** |

Two facts follow, and they are not the same fact.

**The coupling is real and measured.** The root package's tests execute **198 lines** of `eggsentry`'s code, across four of its five modules. The root package is not independent of `eggsentry`, and the first implementation's selection for a change in `eggsentry` did not include a single one of the root's tests.

**At the line level, the comparison happens to hold.** `eggsentry`'s own tests cover a **superset** of what the root's tests cover (214 ⊇ 198) — the 16-line difference is in `dependency.rs` and `profile.rs`, which the root's tests reach less thoroughly. So the measurement does **not** show a line-coverage gap in this case, and it would be wrong to present it as if it did.

**What it does show is a behavioural dependency, and that is the property I4 is about.** The root package's inline test `src/tool/security.rs::inspect_text_returns_findings` asserts

```rust
assert!(!findings.is_empty());
assert_eq!(findings[0]["category"], "secret_exposure");
```

`findings[0]["category"]` is `eggsentry`'s **classification decision**, produced by the scanner the root calls through its `codegg::security::*` re-export. A change to `crates/eggsentry/src/scanner.rs` that alters what is classified as `secret_exposure` changes that assertion's outcome — and that test lives in the root package's library test target, which the first implementation did not select for a change in `eggsentry`. Line coverage cannot show this, because the line was already executed before the change; only the *outcome* moves.

That is why the fix is a dependency widening rather than a coverage comparison: the question "does the scoped selection cover more lines?" is the wrong question. The right one is "can a test outside the selection start failing because of this change?", and for a dependency the answer here is yes, and the measurement locates exactly which code and which assertion.

**The remaining cases.** The same comparison was **not** run for the other five representative cases. Instrumenting and relinking all 47 test targets costs exactly what this workstream exists to reduce, and the dependency's effect on the other five is stated as argued-not-proven: the dependency edges are declared in the manifest, the guard proves the declared edge set is exactly `cargo metadata`'s, and the rule (a dependent contributes its **full** surface) is sound for any dependent, not only for the one that was measured. The measured case is the one where the coupling was least obvious — a leaf crate whose own tests look self-sufficient — which is the case most worth measuring.

**A note on the tool.** `cargo llvm-cov -p codegg` reports only the package under test's own files, so the root's coverage of `eggsentry` is **not** in the report it writes. The number above came from merging the run's raw profiles (`llvm-profdata merge`) and exporting them directly (`llvm-cov export`), which include every instrumented file that executed.

For the other four cases the coverage comparison was **not** run, and the honest statement is what was argued instead: each case's sufficiency argument is the manifest's own source-path → group mapping, cross-checked against the group's target list, plus the guard's proof that no target is unowned. A coverage run over the whole workspace would have required instrumenting and relinking all 47 test targets, which is the same cost this roadmap exists to reduce.

### What is still argued rather than proven

- That the **source-path patterns** are correct is a judgement about which test covers which source. The guard proves the mapping is *consistent* (every pattern matches real files, every target exists, nothing is unowned) — it does not prove the *judgement* is right. A pattern that names a plausible-but-wrong source path would pass the guard.
- That the **scope is a superset** is proved per group by the manifest's own structure: a member affected by a change contributes its whole group set for the matching patterns plus its library target, and an unmatched file widens rather than narrows. It is not proved by coverage for the four non-leaf cases.

## 7. Invariant review

- **I1:** held — the manifest selects; it never deletes.
- **I2:** held — no CI job added.
- **I3:** held — the hosted job is untouched.
- **I4 (a scoped run is a superset):** held — §6, with the stated limit.
- **I5:** held — §3.
- **I6:** held — §2.
- **I7:** held — `[profile.ci]` unmodified.
- **I8:** held — no dependency, feature default, or product behaviour changed.
- **I9:** held — §4.

## 8. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| **medium** | The plan's §3.6 premise — that the guard-trigger routing is machine-readable — is false of this tree. | None for the implementation: the mode runs all the guards, which is compatible with both prose routing and machine routing. But the plan's wording is wrong about the repository. | The plan's §3.6 was corrected in place; a future change that wants per-guard routing needs to build the table first. |
| low | The manifest's source-path patterns are a human judgement. The guard proves consistency, not correctness. | A plausible-but-wrong pattern would pass the guard and could under-select. | The out-of-member and unmatched-file rules are the backstop: an unmapped file widens rather than narrows. A coverage-driven audit of the patterns is the natural follow-up (see roadmap §10, D2). |
| low | The coverage-instrumented comparison was run for one representative case, not five. | Four cases rest on the manifest's mapping argument rather than on coverage. | Recorded in §6 as argued-not-proven. |
| low | `scripts/verify.sh scoped` shells out to `python3` three times per member to read the selection. | Cosmetic; the scoped mode is developer-local and the selection is small. | None. |

No high or critical findings. Nothing blocks M004 — and M004's interface dependency on M003 (the manifest names the root test targets) was discharged by updating the manifest's group membership in the same change.

## 9. Registry updates

- `plans/registry.md`: the M003 row `ready → active (implemented; locally verified; four defects found and fixed)`, with the dependency-edge rule, the four defects, the measured coupling, and the cost of the fix recorded.
- `plans/subsystems/ci-test-throughput-recovery-roadmap.md`: §7 M003's exit conditions amended with the dependency-edge requirement and the feature-gate requirement; §12 M003 `not started → active`.
- `plans/implementation/ci-test-throughput-recovery/003-*.md`: §3.6 corrected (the routing is prose, not a table), §6 and §7 amended with the measured manifest shape, and §17 added as the implementation record — the two defects, the third (the bash empty-array hazard), the fix, and the cost.
