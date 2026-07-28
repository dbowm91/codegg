# Development Verification and Release Milestone 002 — Canonical Local Verification Contract

Status: ready for handoff

Repository baseline: `39d0720f9748cabc978ad9b0a3a32c31c6bc84d1` plus the development-verification-release planning registration series

Source roadmap:

- `plans/subsystems/development-verification-release-roadmap.md#milestone-002--canonical-local-verification-contract`

Long-term requirements:

- `plans/000-long-term-specification.md#2-primary-product-goals`
- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/002-long-term-roadmap.md#cross-phase-execution-rules`
- `plans/002-long-term-roadmap.md#phase-19--operational-hardening-and-scale-closure`

Applicable ADRs:

- None.

Primary class: invariant

## 1. Objective

Create one authoritative, bounded local verification contract with three clearly separated concepts:

- `quick`: cheap repository sanity suitable for ordinary iteration;
- `full`: broad maintainer/developer verification suitable before substantial handoff or release preparation;
- change-specific checks: optional feature, plugin, LSP, example, platform, audit, and packaging commands selected because the changed surface requires them.

The contract must be executable without introducing a new build system. It must reconcile `AGENTS.md`, `CONTRIBUTING.md`, `architecture/testing.md`, Cargo aliases, Nextest configuration, and routine CI around one source of truth. It must also restore truthful resource terminology and conservative broad-test limits.

## 2. Why this milestone is ready

This plan becomes ready only after Milestone 001 is closed and the reduced routine-CI boundary is accepted. That closure establishes which commands belong in hosted CI and prevents this milestone from documenting a moving workflow target.

The repository already contains:

- a large production test suite;
- focused crate and integration-test commands;
- static ownership and policy guards;
- fake-server and real-server LSP harnesses;
- plugin SDK and WASM examples;
- Cargo aliases;
- a Nextest configuration and timing helper;
- detailed but overlapping testing documentation.

The work is primarily policy consolidation and small scripting. No product architecture decision is unresolved.

## 3. Current implementation evidence

At the reviewed baseline:

- `AGENTS.md` presents `CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=14` as the full capped suite;
- `architecture/testing.md` calls the same fourteen-thread command serial or conservative in several places;
- the test taxonomy correctly recognizes process-heavy, plugin-heavy, adversarial, workspace, and real-server classes as serial or manual;
- `.config/nextest.toml` contains `default`, `ci-fast`, `ci-heavy`, and `ci-release` profiles even though routine CI uses Cargo test;
- `scripts/capture-nextest-timing.sh` exists to produce timing evidence and baseline comparisons;
- active documentation lists many focused commands but does not define one authoritative selection policy;
- `--all-features` activates `lsp-real-server-tests`, even though actual server availability is checked at runtime;
- root features mix production surfaces (`server`, `plugins`, `image`) with test-only surfaces (`lsp-test-support`, `lsp-real-server-tests`).

The repository has a real resource constraint: previous unbounded execution produced dozens of threads and subprocesses with high memory and I/O pressure. Broad verification therefore must remain deliberately serialized or tightly bounded. Faster feedback should come from selecting less work, not increasing fan-out.

## 4. Invariants that must not regress

- There must be exactly one authoritative definition of quick and full verification.
- Routine CI must call commands compatible with that contract rather than maintaining a separate hidden policy.
- Broad compile and test commands must set explicit resource bounds.
- Process-heavy, plugin-heavy, adversarial, workspace, and real-server work must not be mislabeled as freely parallel.
- `--test-threads=14` must not be described as serial.
- Real language servers must remain opt-in and must never be installed or spawned by quick verification.
- Optional feature coverage must be explicit; omission from routine CI must not be confused with deletion.
- Existing static guards must remain discoverable and tied to their change triggers.
- A verification helper must propagate the first failing command's nonzero status.
- The helper must not mutate source, regenerate checked-in files without an explicit mode, modify user configuration, or publish artifacts.
- No contributor must install Nextest merely to run the canonical quick or full paths.

## 5. Scope

### In scope

- one small verification entry point, preferably `scripts/verify.sh`;
- `quick` and `full` modes;
- a `--help` or usage surface;
- explicit `CARGO_BUILD_JOBS=1` and test-thread limits for broad commands;
- a documented matrix mapping changed surfaces to focused commands;
- correction of active test terminology and CI descriptions;
- consolidation of `AGENTS.md`, `CONTRIBUTING.md`, and `architecture/testing.md`;
- review of `.cargo/config.toml` aliases for consistency;
- simplification of `.config/nextest.toml` to optional local profiles only, or deletion if no current non-historical use remains;
- deprecation or narrowing of timing helpers whose only purpose was CI evidence production;
- focused tests for the verification helper's argument parsing and failure propagation where practical.

### Explicitly out of scope

- deleting production tests;
- changing product semantics to make tests pass;
- wholesale conversion from Cargo test to Nextest;
- introducing Make, Just, Task, Nix, Bazel, or a custom Rust verification binary;
- redesigning Cargo features across the workspace unless a very small correction is required to keep real-server tests opt-in;
- deleting the release workflow;
- changing package versions or crates.io metadata;
- deleting the real-server workflow;
- adding code coverage thresholds, benchmark gates, flaky-test quarantine infrastructure, or evidence dashboards;
- rewriting historical closure records.

## 6. Required production changes

### Core/domain

No product-domain changes are expected.

### Storage and migrations

None.

### Protocol and DTOs

None.

### Runtime and concurrency

Add one small shell entry point unless repository evidence demonstrates that an existing script already provides a simpler canonical surface:

```text
scripts/verify.sh quick
scripts/verify.sh full
scripts/verify.sh help
```

The script must:

- use Bash already assumed by repository scripts;
- begin with `set -euo pipefail` or equivalent strict behavior;
- resolve the repository root from the script location rather than caller cwd;
- print the selected tier and each command before execution;
- propagate nonzero status without `|| true` or hidden aggregation;
- set `CARGO_BUILD_JOBS=1` for broad Cargo commands;
- pass `--test-threads=1` to broad workspace tests;
- accept no arbitrary command injection or eval-based argument expansion;
- have no third-party runtime dependency beyond the tools already required to build CodeGG.

The implementation agent must derive the exact command sets from the closed Milestone 001 workflow and current repository evidence. The intended minimum contract is:

### Quick tier

Quick verification should finish early and avoid heavyweight optional features. It should include:

```bash
cargo fmt --check --all
python3 scripts/generate_builtin_agents.py --check
python3 scripts/check_builtin_agents.py
python3 scripts/check-tokio-test-flavors.py
./scripts/check-core-boundary.sh
CARGO_BUILD_JOBS=1 cargo check --workspace --all-targets --locked
```

Add a small deterministic test subset only if measurement shows it remains suitable for ordinary iteration. Preferred candidates are extracted core crates with low subprocess/storage overhead. Do not include the root all-features suite merely to make `quick` appear comprehensive.

The quick contract must explicitly state that developers run focused tests for the code they changed.

### Full tier

Full verification should include:

- every quick check;
- all active static ownership/security guards that are repository-wide invariants rather than subsystem-specific historical evidence;
- strict default-feature workspace Clippy;
- default-feature broad workspace tests with one test thread;
- a bounded production-feature compile/test set that excludes real external language servers;
- fake-server LSP integration where it is part of the supported default development surface;
- focused plugin/server checks if those features are supported production configurations.

A candidate shape is:

```bash
CARGO_BUILD_JOBS=1 cargo clippy --workspace --all-targets --locked -- -D warnings
CARGO_BUILD_JOBS=1 cargo test --workspace --locked -- --test-threads=1
CARGO_BUILD_JOBS=1 cargo check -p codegg --locked --features server,plugins,lsp-test-support
```

The exact production-feature command must be proven against current Cargo features. Do not use `--all-features` when doing so unintentionally activates real-server tests or unrelated optional UI dependencies. If a broad all-features command remains useful, document it as release/change-specific rather than canonical routine full verification.

### Change-specific matrix

Create a concise table with at least these trigger categories:

| Changed surface | Required focused verification |
|---|---|
| `src/server/**`, WebSocket transport, `server` feature | server feature compile plus relevant transport tests and bounds guards |
| `src/plugin/**`, plugin manifests, SDK/API | plugin feature tests, core-boundary guard, Rust/Python SDK tests, affected WASM examples |
| `crates/egglsp/**`, `src/lsp/**` | egglsp unit tests, fake-server scenario tests, optional selected real-server smoke |
| generated agents or agent TOML | generator check, source check, schema validation |
| scheduler/process ownership | scheduler-bypass and execution-ownership guards plus focused tests |
| projection transport | projection disclosure/publication/transport/WebSocket guards plus focused replay tests |
| Cargo dependency changes | full verification and manual `cargo audit` when preparing merge/release |
| package/release metadata | full verification plus `cargo package`/`cargo publish --dry-run` from Milestone 003 |
| platform-specific code | build/test on the affected host or target |

Do not turn this table into an automatically inferred changed-files engine. Human/agent selection is sufficient and less fragile.

### Frontend or operator surface

The operator surface is the script output and active documentation. It must be obvious:

- which tier ran;
- which command failed;
- which optional checks were not included;
- how to run a focused command next.

### Security and authorization

- Do not read or require credentials.
- Do not invoke registry or GitHub release operations.
- Do not source arbitrary project files.
- Do not use `eval`.
- Quote paths and arguments.
- Static security guards must remain fail-closed.

### Documentation and static guards

Rework active guidance so that:

- `architecture/testing.md` explains the resource model and canonical tiers without reproducing every historical CI lane;
- `AGENTS.md` points to `scripts/verify.sh quick|full` and retains a focused-command catalog where useful;
- `CONTRIBUTING.md` gives the contributor expectation without requiring every optional integration;
- `.github/workflows/ci.yml` uses the same commands or a clearly documented subset;
- `.cargo/config.toml` aliases do not contradict the canonical commands;
- Nextest is described as optional timing/diagnostic tooling only if retained.

## 7. Ordered work packages

### Work package A — Inventory active verification contracts

Intent:

Identify the commands that are true repository invariants, commands that are subsystem-triggered, and commands that exist only for historical evidence production.

Required changes:

- Review `.github/workflows/ci.yml` after Milestone 001.
- Review `AGENTS.md`, `CONTRIBUTING.md`, `architecture/testing.md`, `.cargo/config.toml`, `.config/nextest.toml`, and verification scripts.
- Classify every static guard as global, change-specific, obsolete, or historical-only.
- Classify each expensive check as full, change-specific, release-specific, or removable.
- Measure representative quick and full candidates on the available host when feasible.

Acceptance evidence:

- A command inventory in the closure record.
- No active command is assigned to two contradictory tiers.
- Historical-only commands are not silently promoted into the canonical path.

### Work package B — Implement the canonical verification entry point

Intent:

Provide a small executable contract rather than several prose variants.

Required changes:

- Add `scripts/verify.sh` with `quick`, `full`, and help handling.
- Reject unknown modes with a nonzero exit and usage text.
- Run from the repository root even when invoked elsewhere.
- Print commands before running them.
- Set conservative resource bounds.
- Avoid optional external tools in both canonical modes.
- Keep mode logic linear and readable; do not build a plugin architecture or command registry.

Acceptance evidence:

- `scripts/verify.sh quick` succeeds on the implementation revision.
- `scripts/verify.sh full` succeeds or reports an independently existing failure precisely.
- A deliberately failing injected command in a test fixture or temporary script copy proves nonzero propagation without modifying the production script.
- Unknown mode returns nonzero.

### Work package C — Define production-feature and change-specific checks

Intent:

Preserve correctness depth without burdening every ordinary run.

Required changes:

- Determine which root features represent supported production configurations.
- Exclude `lsp-real-server-tests` from canonical quick/full invocation.
- Document plugin, server, fake-LSP, real-LSP, example, audit, packaging, and platform commands.
- Retain focused commands already used successfully by subsystem closure work.
- Remove duplicate commands from active docs when one canonical form is sufficient.

Acceptance evidence:

- Every non-default production feature has a documented compile/test trigger.
- Real-server tests are clearly opt-in.
- Example and audit commands remain available.
- No documentation implies optional checks ran when they did not.

### Work package D — Simplify Nextest and timing policy

Intent:

Prevent optional diagnostic tooling from becoming a second verification system.

Required changes:

- Search active references to `.config/nextest.toml` profiles and `scripts/capture-nextest-timing.sh`.
- If Nextest is not used by active CI or a current developer contract, remove CI-named profiles and keep at most a simple default plus one explicitly optional heavy/timing profile.
- If the timing script exists solely to produce historical closure evidence, deprecate it in documentation and leave final deletion to Milestone 004, or delete it now if no active reference remains and the change is unambiguous.
- Do not require `cargo-nextest` for quick/full verification.

Acceptance evidence:

- Active docs contain one verification system.
- Any retained Nextest profile has a current named use.
- No profile named `ci-*` remains unless routine CI actually uses it.

### Work package E — Reconcile active documentation and CI

Intent:

Make all active contributor surfaces describe the same policy.

Required changes:

- Rewrite the CI section of `architecture/testing.md` around the reduced workflow.
- Correct serial/parallel terminology.
- Point `AGENTS.md` and `CONTRIBUTING.md` to the canonical script.
- Retain focused command examples without duplicating full policy prose.
- Update `.github/workflows/ci.yml` to invoke the script or the exact same constituent commands. Prefer invoking `scripts/verify.sh quick` only if it does not obscure step-level CI diagnostics; otherwise document the hosted subset explicitly.

Acceptance evidence:

- Repository search finds no active claim that fourteen test threads are serial.
- Quick/full commands agree across active documents.
- Routine CI is identified as a subset, not a release candidate gate.

## 8. Failure, cancellation, restart, and contention semantics

The verification helper must stop at the first failed command and return its status. It must not generate a success summary after failure.

Interrupted quick/full execution may be rerun from the beginning. No resume database, checkpoint, cache manifest, or persisted verification state is required.

Broad tests must remain one test thread unless the owning test is run as a focused command with a justified local setting. The script must not dynamically detect CPU count and increase concurrency.

Cargo cache and target directories remain Cargo-owned. The script must not delete `target/` automatically or run `cargo clean` as routine behavior.

Concurrent invocations are not coordinated by the script. They may contend for Cargo locks. The script must not invent lockfiles or daemonize work; users should avoid concurrent full runs on constrained systems.

## 9. Compatibility and migration

Existing direct Cargo commands remain valid. The canonical script is a convenience and policy entry point, not a wrapper that forbids focused commands.

If CI cannot invoke the script while preserving useful named diagnostics, CI may invoke the same commands directly. Documentation must state the exact relationship.

Nextest users may continue to run it locally if configuration is retained. Removal of CI-named profiles is not a product compatibility break.

Historical closure documents keep their original evidence commands. Active docs must not be made inaccurate to preserve historical wording.

## 10. Required tests

### Focused unit tests

If the repository already has shell-script testing support, add focused tests for mode parsing, unknown-mode rejection, repository-root resolution, and failure propagation. Do not add a new shell-test framework solely for this script.

### Integration tests

- Invoke `scripts/verify.sh quick` from repository root.
- Invoke it from a nested directory.
- Invoke `scripts/verify.sh full` from repository root.
- Run representative commands from the plugin, server, and fake-LSP change-specific matrix.

### Restart and recovery tests

Not applicable beyond rerunning after interruption. Confirm the script leaves no persistent state requiring cleanup.

### Contention and cancellation tests

No automated stress test required. Record that broad execution is bounded and that concurrent full runs are unsupported on constrained hosts.

### Security and negative tests

- Unknown mode fails.
- Extra positional arguments fail unless explicitly documented.
- Script contains no `eval`, credential reads, publication commands, or source of arbitrary repository configuration.
- Real-server commands are absent from quick/full execution.

### Migration and compatibility tests

- Direct focused Cargo commands documented in active guides still work.
- Routine CI remains consistent with the canonical contract.
- Any retained Cargo aliases expand to commands that do not contradict resource policy.

## 11. Required verification commands

Script behavior:

```bash
bash -n scripts/verify.sh
scripts/verify.sh help
scripts/verify.sh quick
( cd crates/codegg-core && ../../scripts/verify.sh quick )
if scripts/verify.sh unknown-mode; then
  echo 'unknown mode unexpectedly succeeded' >&2
  exit 1
fi
```

Canonical full path:

```bash
scripts/verify.sh full
```

Representative change-specific checks, adjusted to the final documented command set:

```bash
CARGO_BUILD_JOBS=1 cargo check -p codegg --locked --features server
CARGO_BUILD_JOBS=1 cargo test -p codegg --locked --features plugins --lib plugin -- --test-threads=1
CARGO_BUILD_JOBS=1 cargo test -p egglsp --locked --features lsp-test-support --test scenario_engine -- --test-threads=1
CARGO_BUILD_JOBS=1 cargo test --locked --features lsp-test-support --test lsp_composite_stdio -- --test-threads=1
```

Documentation consistency searches:

```bash
rg --line-number --glob '!plans/archive/**' --glob '!plans/closure/**' \
  'test-threads=14|ci-fast|ci-heavy|ci-release|serial validation|serial workspace' \
  AGENTS.md CONTRIBUTING.md architecture .config .github scripts

rg --line-number 'scripts/verify\.sh|Quick verification|Full verification' \
  AGENTS.md CONTRIBUTING.md architecture/testing.md .github/workflows/ci.yml
```

Do not claim environment-dependent optional commands as passed unless actually run.

## 12. Documentation updates

Required active-document updates:

- `architecture/testing.md` — authoritative resource model, tiers, change-specific matrix, and CI relationship;
- `AGENTS.md` — concise quick/full entry points, focused command catalog, and resource rules;
- `CONTRIBUTING.md` — contributor expectations and when full verification is required;
- `.github/workflows/ci.yml` comments or steps — routine subset relationship;
- `.config/nextest.toml` comments if retained;
- `.cargo/config.toml` comments or aliases if changed.

Do not add `RELEASING.md`; Milestone 003 owns release documentation.

## 13. Acceptance criteria

- `scripts/verify.sh quick` and `scripts/verify.sh full` are the authoritative local entry points, or an equally small existing entry point is formally designated.
- Quick mode avoids all-features execution, real servers, release builds, examples, audit installation, package dry-runs, and cross-target builds.
- Full mode runs broad deterministic verification with `CARGO_BUILD_JOBS=1` and `--test-threads=1`.
- Full mode does not spawn real external language servers.
- Optional production features have documented change-triggered commands.
- Real-server, example, audit, packaging, and platform checks are opt-in.
- Unknown verification mode fails clearly.
- Nextest is optional and no longer presented as a second CI policy.
- Active documentation contains no claim that fourteen test threads are serial.
- CI and local documentation agree on the hosted subset.
- No product behavior, protocol, or storage schema changes.

## 14. Stop conditions

The agent must stop and report rather than improvise when:

- Milestone 001 is not strictly closed;
- a production feature cannot be represented without `--all-features` and its ownership is unclear;
- excluding real-server tests from full verification requires a broad Cargo feature redesign;
- a static guard's current ownership cannot be determined from code and active architecture docs;
- full verification exposes unrelated existing failures that require product changes outside this workstream;
- simplifying Nextest would remove a currently used non-historical developer workflow;
- the proposed script grows into a command scheduler or duplicates CodeGG's runtime test runner;
- a new external dependency is required solely to parse or dispatch verification commands;
- unrelated user changes cannot be preserved.

## 15. Closure evidence required

The closure record must contain:

- Milestone 001 closure dependency;
- final quick and full command lists;
- script content summary and argument contract;
- command results and wall-clock/resource observations where available;
- static-guard classification;
- feature/change-specific verification matrix;
- Nextest profile/script disposition;
- active documentation consistency search results;
- proof that real-server and publication commands are absent from quick/full modes;
- proof of unknown-mode failure and nested-directory invocation;
- residual optional checks not run and why;
- confirmation that no production test source was deleted.

## 16. Handoff notes

Favor a short, transparent script over abstraction. A sequence of explicit commands is the design.

The purpose of quick mode is not to prove release readiness. It is to catch cheap failures early. Focused tests remain the responsibility of the implementation agent changing a subsystem.

The purpose of full mode is broad deterministic confidence on a maintainer-controlled machine. It may take time, but it must not generate uncontrolled parallel load.

Do not optimize by raising test threads. Reduce duplicated invocation, use focused commands during iteration, and reserve the broad serialized path for handoff or release preparation.
