# Testing Architecture

CodeGG's workspace test suite has substantially different resource
profiles. Unbounded parallelism has been observed to spawn many threads
plus subprocesses, with some processes consuming substantial memory. The
repository intentionally does not maintain a fragile exact global test
total; command output at a specific revision is the authoritative count.

## Canonical Verification Commands

```bash
scripts/verify.sh quick    # cheap sanity for ordinary iteration
scripts/verify.sh full     # broad verification before handoff or release
```

### `verify.sh quick`

1. `cargo fmt --check --all`
2. `generate_builtin_agents.py --check`
3. `check-core-boundary.sh`
4. `check-client-boundary.sh`
5. `check-desktop-boundary.sh`
6. `check_sandbox_contract.py`
7. `check_execution_ownership.py`
8. `check_tui_project_authority.py`
9. `check_tui_editor_text_authority.py`
10. `check_http_route_disposition.py`
11. `check_audit_coverage.py`
12. `check_scheduler_bypass.py`
13. `check_provider_wire_boundary.py`
14. `check_provider_wire_cutover.py`
15. `check_openai_endpoint_composition.py`
16. `check_provider_catalog_consistency.py`
17. `check_provider_resilience_ownership.py`
18. `check_eggwork_target_routing.py`
19. `check_eggwork_target_routing.py --self-test`
20. `cargo check --workspace --all-targets --locked`

Quick runs **20** steps (19 checks plus the workspace `cargo check`). The
boundary and provider-wire guards were added after this list was first
written; `scripts/verify.sh` is the authority.

### `verify.sh full`

Runs quick first, then:

1. `cargo clippy --workspace --all-targets --locked -- -D warnings`
2. `cargo nextest run --workspace --locked --profile ci`
3. `cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support --profile ci`

Both modes set `CARGO_BUILD_JOBS=2` by default. Test execution runs
under nextest profile `ci` (see below); plain `cargo test` broad runs
keep `--test-threads=1`.

## Test-suite totals

The suite's size is **measured**, not estimated, and the measurement is
reproducible:

```bash
python3 scripts/rust_test_scan.py --validate
```

That command parses every `.rs` and `.inc` file in `tests/`, `src/`, and
`crates/` with a Rust-aware scanner — one that handles lifetimes, raw and
byte strings, character literals, nested block comments, and `async fn` —
and asserts that the number of parsed test functions equals the number of
`#[test]` / `#[tokio::test]` attributes in every file, and that the
scanner's comment-and-literal-stripped mask preserves the original's
newline count exactly. A regex scan is **not** an acceptable substitute: on
this tree it undercounts `tests/` by 76%, because `&'a T` reads as a
character literal.

As of **2026-10-21**, after the CI/test-throughput recovery M006 and M007
milestones:

| Tree | Test functions | Test attributes |
|------|---------------|-----------------|
| `tests/` | 4,946 | 4,946 |
| `src/` | 7,387 | 7,388 (one `macro_rules!` test template) |
| `crates/` | 3,586 | 3,586 |
| **Total** | **15,919** | **15,920** |

The M007 milestone's 84 removals reconcile exactly against its decision
table, `docs/test-duplication.toml`:

| Tree | Before | Removed | After |
|------|--------|---------|-------|
| `tests/` | 4,980 | 34 | 4,946 |
| `src/` | 7,433 | 46 | 7,387 |
| `crates/` | 3,590 | 4 | 3,586 |
| **Total** | **16,003** | **84** | **15,919** |

Two guards enforce the properties these numbers express, and both run in
`verify.sh quick` and in CI:

- `scripts/check_test_assertion_quality.py` — every test body either
  asserts a checkable property, delegates to a named helper that does, or
  is recorded in `docs/test-assertion-quality.toml` with the reason it
  asserts nothing.
- `scripts/check_test_duplication.py` — no two test bodies assert the same
  thing twice, unless `docs/test-duplication.toml` records the survivor.

### Pass and skip accounting

A test that skips **before** any assertion reports *passed* having proved
nothing. Every such body must be listed in
`docs/test-assertion-quality.toml` with its class, the condition that
triggers it, and the workflow that satisfies it — silence is not a reason.

Measured on 2026-10-21 with `scripts/rust_test_scan.py`:

| Class | Bodies | Meaning |
|---|---|---|
| `opt-in-gated` | 20 | The body runs only under an explicit environment opt-in (`CODEGG_REGENERATE_ASSETS`, `CODEGG_RUN_LINUX_BWRAP_FIRST_HOP`, `CODEGG_RUN_LINUX_CGROUP_V2_PROBE`, and the Linux sandbox opt-ins) |
| `environment-absent` | 13 | The body runs only when a tool or a kernel capability the machine may not have is present — LSP server binaries, a spawnable helper shell, Landlock |
| `unconditional` | **0** | A skip that fires on every machine |

The guard's `--self-test` rejects an entry that names no satisfying
workflow, so the list cannot rot into a list of excuses.

Two of the twenty opt-ins — `CODEGG_RUN_LINUX_BWRAP_FIRST_HOP` and
`CODEGG_RUN_LINUX_CGROUP_V2_PROBE` — are set by **no** workflow. They are
manual probes, and their entries say so explicitly.

## Test Resource Classes

| Class | Description | Parallelism | Examples |
|-------|-------------|-------------|----------|
| `fast` | Pure/unit, parsing, config | Safe | `egggit::diff`, `eggsentry::profile` |
| `storage` | SQLite pool ops, CRUD | Serial or low | `tests/session_crud.rs` |
| `process-heavy` | Fake LSP stdio, daemon | Serial | `tests/lsp_test_support/lsp_composite_stdio.inc` |
| `plugin-heavy` | Wasmtime runtime | Serial | `tests/agent_loop/plugin.inc` |
| `adversarial` | Routing, sandbox, projection | Serial | `tests/command_git/command_routing_adversarial.inc` |
| `workspace` | Workspace isolation | Serial | `tests/agent_loop/workspace_isolation.inc` |
| `real-lsp` | Actual server smoke | Manual | `crates/egglsp/tests/real_server_smoke.rs` |
| `release-full` | Conservative full validation for main/tags | Serial | `scripts/verify.sh full` |

## Why Serial by Default

Key amplification factors:

- **LSP tests** spawn fake language-server subprocesses, create temp
  Rust workspaces, write scenario files, exercise async shutdown/restart.
- **Plugin tests** may instantiate Wasmtime runtime state.
- **Tokio default flavor** is single-threaded/current-thread. Bare
  `#[tokio::test]` already has the lightweight default runtime;
  `audit_tokio_tests.py` remains available to identify tests needing
  explicit concurrency review.
- **SQLite migration churn** — `isolated_pool()` runs full migrations
  on every call.

## Tokio Runtime Flavor Rules

### Default: `current_thread`

```rust
#[tokio::test]
async fn test_something() { /* ... */ }
```

Appropriate for: pure unit tests, SQLite pool ops, in-memory registry
tests, mock provider tests, shell projection fixtures.

### Multi-threaded (explicit)

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_concurrent_access() { /* ... */ }
```

Use only when the test:
- Spawns background `tokio::spawn` tasks requiring concurrency
- Uses `tokio::sync::broadcast`/`mpsc` with real concurrent producers
- Tests actual subprocess lifecycle (LSP, daemon, shell)
- Uses `tokio::time::sleep` for timing-dependent behavior

### Always serial (`--test-threads=1`)

LSP subprocess, plugin-heavy, and real-server tests must run serially
because they compete for fixed ports, global process state, or limited
system resources.

## Pool Strategy

### `isolated_pool()` — Fresh DB per test

Creates a named in-memory SQLite DB (`codegg_test_iso_{uuid}`) with
full migrations. Use when tests need a clean slate with hardcoded IDs.

**Do NOT add redundant `migrate()` calls** — migrations run internally.

### `shared_pool()` — Process-wide shared DB

Process-wide shared in-memory DB (`?cache=shared`). Migrations run
once via `OnceLock`. Use when tests tolerate other tests' data.

### Choosing a pool

| Scenario | Pool | Reason |
|----------|------|--------|
| Hardcoded IDs (`"test-session"`) | `isolated_pool()` | Avoids cross-test collision |
| Tests clean up own data | `shared_pool()` | No per-test migration cost |
| Exact DB state needed | `isolated_pool()` | Clean slate |
| High test count, simple ops | `shared_pool()` | Faster |

## Adding New Tests

1. Start with `current_thread` runtime.
2. Use `isolated_pool()` for storage tests unless you guarantee cleanup.
3. Never add redundant `migrate()` calls.
4. Don't spawn real language servers in default tests.
5. Don't use fixed ports, global paths, or shared env vars without
   serializing.
6. Prefer deterministic fakes over subprocesses.
7. Keep timeouts as failure bounds only.
8. For multi-threaded tests, set explicit `worker_threads = 2`.

## Resource-Class Checklist

| Class | Runtime | Pool | Parallelism |
|-------|---------|------|-------------|
| `fast` | `current_thread` | `shared_pool()` or none | Safe |
| `storage` | `current_thread` | `isolated_pool()` | Serial or low |
| `process-heavy` | `current_thread` or `multi_thread` | `shared_pool()` | Serial |
| `plugin-heavy` | `current_thread` | none | Serial |
| `adversarial` | `current_thread` | none | Serial |
| `workspace` | `current_thread` | `isolated_pool()` | Serial |
| `real-lsp` | `multi_thread` bounded | none | Manual |

**Quick decision**: if the test spawns `tokio::process::Command` or
needs concurrent background tasks, use explicitly bounded `multi_thread`;
otherwise prefer `current_thread`. If it touches SQLite, use
`isolated_pool()`.

## Local Commands

```bash
# Canonical verification
scripts/verify.sh quick
scripts/verify.sh full

# Release/installer fixture tests (offline, change-specific; not routine CI)
scripts/release/test-release-tools.sh
scripts/release/test-installer.sh
sh -n install.sh

# Fast feedback (cheap crates)
cargo test -p egggit -p eggsentry -p codegg-config -p codegg-protocol

# Single crate
cargo test -p codegg-core

# Capped workspace validation (nextest parallelizes across binaries)
cargo nextest run --workspace --locked --profile ci

# LSP integration (fake server, serial)
cargo test -p egglsp --features lsp-test-support --test scenario_engine
cargo test --features lsp-test-support --test lsp_composite_stdio

# Plugin tests (serial)
cargo test -p codegg --lib plugin --all-features

# Workspace isolation
cargo test --test agent_loop -- workspace_isolation

# Adversarial tests
cargo test --test command_git -- command_routing_adversarial
cargo test --test tool_programs -- python_sandbox_adversarial
cargo test --test command_git -- context_projection_adversarial

# Real LSP smoke tests (requires installed servers)
cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke -- rust_analyzer

# Tokio flavor audit
python3 scripts/audit_tokio_tests.py
```

## Windows Process Launcher Smoke

The Windows process-supervision milestone has a focused native test lane. It
checks Unicode argv, exit and timeout classification, cancellation of a
Job-owned descendant tree, Job cleanup after root exit, and fail-closed
filesystem requests. Run it on Windows with:

```powershell
cargo test --locked -p codegg --test windows_process_launcher -- --nocapture
```

`.github/workflows/windows-process-launcher.yml` runs the launcher and its
qualified positive regression suites on `windows-latest` for the
native-sandbox work branch and supports manual dispatch. The historically
negative C014 AppContainer/Git experiment remains runnable as an opt-in job
when manually dispatching that workflow with `run_c014_research: true`; its
expected failure is research evidence, never a passing launcher gate.
Launcher tests qualify process ownership only; they do not qualify filesystem
ACL or network enforcement.

The same Windows workflow also builds the restricted service path with the
release-disabled `windows-sandbox-test-support` feature and runs
`tests/windows_sandbox_test_support/windows_restricted_managed_process.inc`. That target verifies owned
stdin/stdout/stderr, private status despite forged stdout markers, token and Job
checks before resume, timeout cleanup, and a non-`Enforced` test-only result.
The workflow also runs an AppContainer control launch with the service's
profile-local environment block and the status-frame rejection tests. It does
The same workflow runs `tests/windows_sandbox_test_support/windows_acl_lease.inc` and the ACL lease unit
tests. They verify AppContainer read/write on a runner-owned disposable root,
read-only grants, denial on an empty sibling root, exact descriptor restoration
after normal exit and cancellation, simulated parent-crash recovery, and refusal
to overwrite concurrent descriptor drift. A hosted existing-root adversarial
test found that a nonempty denied tree allowed synthetic credential reads via
its direct path, a junction, and a hardlink (`38220136385`). The test-only path
now rejects deny roots containing entries before child creation; separate
positive fixtures cover allowed workspace access. These checks remain test-only
and do not qualify production journal protection, existing-workspace authority,
reparse/hardlink safety, or full M004 security. C011 remains blocked on its
broader privileged qualification; C012 is conditionally closed with path
replacement still unqualified.

The same workflow exercises the native shell argv builder and Windows policy
input fixtures for system roots, profile credential denies, PATH-independent
shell resolution, case normalization, alternate data streams, UNC/device
namespaces, traversal, junction components, and distinct sibling worktree
roots. It runs PowerShell and cmd smokes, including cmd's raw-command channel
and `COMSPEC` value, plus Git Bash when installed; WSL selection is argv-checked
because a hosted runner may not have a provisioned Linux distribution. These checks are
policy-input coverage only; M004/M006 own effective ACL and filesystem
enforcement tests.

## Test Execution with Nextest

Routine broad execution uses nextest profile `ci`, configured in
`.config/nextest.toml` (requires `cargo install cargo-nextest --locked`;
CI installs it via `taiki-e/install-action@nextest`):

| Profile | Threads | Scope | Use Case |
|---------|---------|-------|----------|
| `default` | 14 | per-test processes | Local development |
| `timing` | Serial | per-test processes | Local timing diagnostics |
| `ci` | 4 concurrent tests | workspace-wide | CI + `verify.sh full` |

`ci` runs each test in its own process (nextest's execution model), up
to 4 concurrently (= runner vCPUs; 8 was tried and reverted after
starving subprocess timing tests). That is why it is sound despite
process-global env
mutation throughout the test code: unlike `cargo test --test-threads=N`,
no two tests ever share an address space, so intra-suite env races are
impossible by construction (verified by audit 2026-09-25). The four
sleep/timeout-bound heavy binaries (`eggwork_remote_execution_live`,
`interactive_process_attach_resume`, `scheduler_cancellation`,
`causal_observe_replay`) run alone via `threads-required = "num-cpus"`.
`scheduler_cancellation` joined
this list 2026-09-25 after real-`sleep` token assertions flaked twice under
parallel load while passing serially; `causal_observe_replay` joined
2026-10-16 after two hosted CI runs exceeded its frozen p95 gate while other
workspace binaries ran concurrently.
`interactive_process_sessions` and `interactive_terminal_tui` left this list
on 2026-10-23: both were absorbed into the `interactive_process_attach_resume`
family, so their executables no longer exist, and the merged executable keeps
this family's exclusive slot. Three exclusive executables cost three serial
slots for 40 test functions; one costs one.
The profile is workspace-wide only — subset runs (`-p <crate>`) use the
default or timing profile, because the heavy-binary filter strictly
requires its binaries to exist.

```bash
cargo install cargo-nextest --locked
cargo nextest run --workspace --profile ci
scripts/capture-nextest-timing.sh --top 20
```

### The sweep's private cache root

The scratch allocator resolves its per-user cache root from
`dirs::cache_dir()` and refuses a root another local user could write to (see
`architecture/security.md`). A broad sweep therefore inherits the operator's
cache root, and its result would be a property of that machine's home
directory rather than of the product. Both canonical runners give the sweep a
private cache root of its own:

- `scripts/verify.sh full` creates one with `mktemp -d` and exports
  `XDG_CACHE_HOME` for the duration of the run.
- `.github/workflows/ci.yml` sets `XDG_CACHE_HOME` to a directory under the
  runner's temporary directory.

A manual broad run that has to be independent of the operator's home directory
does the same:

```bash
XDG_CACHE_HOME="$(mktemp -d)" cargo nextest run --workspace --locked --profile ci
```

The root stays **outside** the workspace on purpose: several qualifications
grant the repository itself as the workspace root, so a scratch root inside it
would no longer be a distinct root from the granted workspace.

## CI Structure

Routine CI is one bounded `verify` job in `.github/workflows/ci.yml`
for PRs and pushes to `main`. Historical baseline was ~37 min per run
(historical measurement 2026-09-25: ~45 s setup/guards/fmt, ~5 min
clippy, ~31 min workspace tests, of which only ~10 min is test
execution and ~20 min is serial compile/link of ~100 test binaries).
Historical post-tuning steady state was ~19 min green (2026-09-25:
~2 min clippy, ~16 min test step with ~7 min execution of 11781
tests). Final M001-M005 state is 17m17s on main/live run
`36196068239` attempt 3 (7m51s test build, 362.537s execution,
11,726 passed / 1 skipped, live Eggwork included) with
`CARGO_BUILD_JOBS=4`. The **test-function** counts in that record
(11,726 passed / 1 skipped; 11,781 pre-consolidation) stand. The
**binary** counts in the same sentence do not: it reads "84 root
integration-test binaries after M003/M004 consolidation (historical 100
pre-consolidation)", but the tree at the revision that recorded them
(`99117577f`) holds **208** top-level `tests/*.rs` files and 8 declared
`[[test]]` entries — roughly 2.5x the recorded figure — so those counts
are not reproducible from the tree and are treated here as unverified.
The successor roadmap (`plans/subsystems/ci-test-throughput-recovery-roadmap.md`
§4.4) records the drift; its M004 milestone arms a guard that keeps the
recorded target count and the real one in agreement.

Current root test-target count, measured 2026-10-21 at `2c1953347`:
**288** root test targets — 286 top-level `tests/*.rs` files plus 2
directory-based consolidated targets (`projection_replay`,
`session_family`) — of which **13** are gated behind `required-features`:
4 on `lsp-test-support`, 5 on `server`, 2 on `windows-sandbox-test-support`,
2 on `linux-git-test-support`. Workspace-wide the count is **314**. Method:
`cargo metadata --no-deps --format-version 1`, counting targets of kind
`test` per package.

After the M004 family consolidation (same day, same baseline), the root
package has **21** test targets and the workspace has **47**: the 286
top-level files became 19 families (plus the 2 pre-existing directory
targets), and the 13 gated targets became 4 gated families. The target
topology is described under "Root integration-test families" below.

After the fifth merge (2026-10-22, at `f4019a410`), the root package has
**24** test targets and the workspace has **52**: `main`'s sixteen new
top-level files were absorbed into the families that owned their subject,
thirteen of them, and the three that stay top-level are named by a hosted
qualification lane. `codegg-core`'s two (`session_launch` and `writer_permit`)
are counted in the workspace figure and are outside the root package's family
mechanism. Method: `cargo metadata --no-deps --format-version 1`, counting
targets of kind `test` per package — the same method as the two figures
above.

### Known macOS-local failures

On **macOS** the workspace suite has **seven** failures that are **not**
regressions. The first six were proved pre-existing by checking out the
baseline commit `2c1953347` into a separate worktree and running the same
tests there; the baseline probe is recorded in
`plans/closure/ci-test-throughput-recovery/004-status.md` §8.1. The seventh
arrived on `main` after that probe, so it is proved against `origin/main`'s
own copy of the file instead; it is recorded in
`plans/closure/ci-test-throughput-recovery/008-merge-and-absorption-status.md`
§7.

| Test | Why it fails on macOS |
|---|---|
| `core::daemon_refresh::tests::session_projection_rebuilds_owner_scoped_live_jobs` | The test registers `/tmp` as a workspace root, and the workspace registry rejects a **symlinked** root. On macOS `/tmp` is a symlink to `/private/tmp`. |
| `core::daemon_refresh::tests::session_projection_snapshot_includes_in_process_live_child_instances` | Same cause. |
| `managed_process::tests::offline_required_scheduler_carried_intent_refuses_disabled_sandbox_before_spawn` | The `OfflineRequired` network intent needs a Linux sandbox, so the refusal arrives earlier than the test expects. |
| `core::daemon::tests::poolless_standalone_human_shell_uses_registered_workspace` | The standalone shell's output does not arrive within the test's own wait. |
| `scratch_cache_root_contract::the_default_cache_root_is_the_home_directory_cache` | The test expects `$HOME/.cache`; macOS's cache directory is `$HOME/Library/Caches`. |
| `scratch_cache_root_contract::an_explicit_cache_home_is_the_root_the_allocator_uses` | Same convention mismatch. |
| `contained_worktree_c008::the_grant_names_only_the_administration_directory` | The test needs the Linux protected-mount view, so the containment grant cannot be built here: `the worktree child could not be contained: permission denied: protected mount view is available only on Linux`. The target is Linux-targeted and is exercised by the hosted `Linux M004 process and IPC qualification` lane, not by a local gate. |

Do not "fix" these by editing the assertions — they are testing the Linux
contract on a platform that does not provide it. When a *new* failure appears
next to these, it is real: check it against the baseline before assuming the
same class.

### Known macOS-local gate failures

The **clippy gate** is also red on macOS, and for `origin/main`'s own reasons
rather than for anything the test layout did. The gate's command is the one
`scripts/verify.sh full` runs:

```bash
cargo clippy --workspace --all-targets --locked -- -D warnings
```

On macOS it reports **nine** distinct findings across two targets — the **lib**
(7) and the **lib test** (2: `new` and `new_with_network_intent` at
`src/lsp/m004.rs:106`, and `spec_with` at `src/managed_process.rs:2715`) —
because the lib's failure stops the build before the **bin** is reached. A run
**without** `-D warnings` reaches the bin and reports a **tenth**: `unused
import: Seek` at `src/bin/codegg-sandbox-helper.rs:30`, whose only use is the
`file.seek(…)` at line 594 inside a `#[cfg(target_os = "linux")]` function.
Every one of the ten is code that no macOS path calls, so the hosted Linux
runner — where the code *is* called — sees no finding at all.

**So `scripts/verify.sh full` is red on macOS on `origin/main` today, for this
reason and no other; `scripts/verify.sh quick` is green there.** The affected
files are `origin/main`'s, byte-identical to the ones the CI/test-throughput
line recorded, and the disposition is recorded in
`plans/closure/ci-test-throughput-recovery/008-merge-and-absorption-status.md`
§7. The local consequence worth stating is the one the line's own §2.9 repair
showed: a dead-code finding in a **test target** is invisible to local macOS
clippy, because the lib fails first, so the hosted Linux runner is the only
authority for that class.

### Root integration-test families
The root package's integration tests are **families**: a directory under
`tests/` whose `mod.rs` is one Cargo test executable and whose members are
`.inc` files included with `#[path]`. Two cargo behaviours make this
necessary:

- cargo does **not** auto-discover `tests/<dir>/mod.rs`, so every family is
  declared explicitly in the root `Cargo.toml`;
- cargo **does** auto-discover `tests/<dir>/*.rs` as separate test targets,
  so the members carry a `.inc` extension and are never auto-discovered.

This is the pattern the M003 `projection_replay` pilot established; M004
applied it across the root package. The root package's test-target count
fell from **288 to 21**, and the number of root test executables linked per
run fell from 288 to 21.

Consequences worth knowing:

- **A test's id changed** from `member::test_name` to
  `family::member::test_name`. The test body and every assertion are
  unchanged. Anything that filters by full test path — `cargo test -- --exact
  <path>`, a libtest substring filter — must use the new path.
- **`cargo test --test <member>` no longer selects a single member.** The
  unit of selection is the family. `cargo test --test <family> -- <member>`
  still narrows to one member, because libtest's filter is a substring match
  on the full test path.
- **The `ci` profile's whole-binary exclusions still resolve.** The four
  binaries that reserve the runner for themselves
  (`eggwork_remote_execution_live`, `interactive_process_attach_resume`,
  `scheduler_cancellation`, `causal_observe_replay`) each keep their exact
  target name. Their flake rationale stays attached
  to the binary that flaked, not to the family.
  `interactive_process_attach_resume` is the one **family of three** in the
  list since 2026-10-23, when it absorbed `interactive_process_sessions` and
  `interactive_terminal_tui`: two more exclusive executables became one, and
  the merged binary keeps the slot the three had between them.
- **`tests/common/` is a helper tree, not a target.** It is bound once per
  family that uses it, through `#[path = "../common/mod.rs"]`, and the
  members reach it with `use super::common;`.
- The manifest `docs/test-ownership.toml` and the filters in
  `.config/nextest.toml` and `.github/workflows/ci.yml` name these targets,
  and `scripts/check_test_target_references.py` fails if any of them names a
  target that does not exist — a renamed target would otherwise make a
  filter silently stop reserving the runner for it.

### The families after the fifth merge (2026-10-22)

`origin/main` never received M004's consolidation: it still carries its root
integration tests as top-level `tests/*.rs` files, and its advance to
`f4019a410` added **sixteen** more of them. The fifth merge therefore had to
reconcile two *shapes* — `main`'s top-level files against this branch's
families — rather than two values.

Sixteen of those files arrived as top-level cargo targets. **Thirteen** were
absorbed into the family that already owned their subject, and **three** stayed
top-level because a hosted qualification lane names them:

| Absorbed file | Family | Member |
|---|---|---|
| `tests/blackboard_economics_m008.rs` | `agent_loop` | `blackboard_economics_m008.inc` |
| `tests/blackboard_trajectory_m008.rs` | `agent_loop` | `blackboard_trajectory_m008.inc` |
| `tests/build_quota_admission.rs` | `scheduler_jobs` | `build_quota_admission.inc` |
| `tests/checkout_reservation_c004.rs` | `core_daemon` | `checkout_reservation_c004.inc` |
| `tests/checkout_reservation_c004_http.rs` | `server_feature` | `checkout_reservation_c004_http.inc` |
| `tests/git_fixture_isolation.rs` | `command_git` | `git_fixture_isolation.inc` |
| `tests/host_admission_manifest.rs` | `security_permission` | `host_admission_manifest.inc` |
| `tests/process_denial_c005.rs` | `security_permission` | `process_denial_c005.inc` |
| `tests/session_launch_service.rs` | `core_daemon` | `session_launch_service.inc` |
| `tests/tool_program_c002_delegated_ceiling.rs` | `tool_programs` | `tool_program_c002_delegated_ceiling.inc` |
| `tests/writer_disposition_guards.rs` | `tool_programs` | `writer_disposition_guards.inc` |
| `tests/writer_fence_c003.rs` | `core_daemon` | `writer_fence_c003.inc` |
| `tests/writer_permit_service.rs` | `core_daemon` | `writer_permit_service.inc` |

The three that stayed top-level are named by a hosted qualification lane, and
**a target a CI lane names keeps its own target** — the rule M004 already
applied to the branch's own six "family of one" targets:

| Target | Named by |
|---|---|
| `shared_checkout_c006` | `.github/workflows/shared-checkout-macos-qualification.yml` |
| `macos_native_shared_checkout_containment` | the same macOS lane |
| `contained_worktree_c008` | `.github/workflows/linux-m004-native.yml` |

The root package's test-target count therefore falls from **37** to **24** (21
families + 3), and the workspace's from **65** to **52** — the root's 13
absorbed targets plus `codegg-core`'s two (`session_launch` and `writer_permit`,
which M004 deliberately left outside the root package's family mechanism). (At
that merge; the sixth pass below re-measured both.)

Three consequences follow from the move, and each is a change a reader has to
know about:

- **A member reaches the shared helper with `use super::common::…`, not
  `use common::…`.** A standalone test file declares `mod common;` because
  `tests/common/` is its sibling directory; inside a family the helper is one
  level up, so the family binds it with `#[path = "../common/mod.rs"]` and the
  member imports the helper's items directly. The `security_permission` family
  gained that binding for this merge, because `process_denial_c005` uses
  `common::checkout_reservation`.
- **`crate::` in an integration test means the test crate root, not the
  library.** `crate::scheduler_admission_service(…)` resolved through the
  standalone file's own crate-root import; as a family member it has to name
  the helper directly.
- **A member that re-invokes its own test binary must use the qualified
  path.** `git_fixture_isolation` re-executes `current_exe() --exact <name>`,
  and `--exact` matches the *full* path, which is now
  `git_fixture_isolation::child_prints_its_own_process_unique_fixture_path`. A
  bare test name selects nothing, and the parent reads the empty child report
  as a failure to derive a fixture path.

### The families after the sixth merge (2026-10-22)

`main` advanced again, to `7d3c6ed09` (PR #166, the Linux trusted-Git C008
contained worktree), so this line reconciled with it a **sixth** time. The
advance added **one** top-level root integration-test file,
`tests/composed_two_agent_race_m011.rs`, and no family member.

**That file stays top-level, and the reason is a shared global rather than a
lane.** The composed two-agent race drives two live agents against one
checkout, and the agent-loop harness suites **drain the process-global event
bus** and assert that nothing was left pending on it
(`agent_loop_harness::temp_root_write_following_the_offer_needs_no_human`
counts `AppEvent::PermissionPending` on a global broadcast receiver). A second
agent on that same bus turns that assertion into a cross-test race: absorbing
the file into `agent_loop` made the harness fail with *"no attempt may reach
the human when the model follows the offer"* — `left: 1, right: 0` — while the
file passed in its own binary. **A test that observes a process-global cannot
share a binary with the tests that observe it too.** So the file keeps its own
target, the way `main` shipped it, and it gets its own ownership-manifest
group.

The root package's test-target count is therefore **25** (21 families + 4) and
the workspace's **53**; the root integration-test *module* count rises
318 → **319** (4 top-level + 315 members).

Two consequences of this merge are worth naming, because neither is a textual
conflict:

- **A `#[path]` attribute is relative to the file that carries it.** `main`'s
  edit to the agent-loop harness included a shared helper with
  `#[path = "common/scripted_provider.rs"]`. The harness had moved into
  `tests/agent_loop/`, so that path resolved to `tests/agent_loop/common/`,
  which does not exist — the merged tree did not compile, and no merge tool
  reported it, because the two sides touched different paths and git paired
  them by rename. The family already binds the shared tree as `common`, so the
  harness reaches the provider through `use super::common::scripted_provider::…`
  and the private include is gone. **A move that changes a file's directory
  can break a path inside it that nothing else mentions.**
- **A standalone test file declares `mod common;`; a family member does not.**
  `tests/common/` is a standalone file's sibling directory, so it declares the
  tree itself. Inside a family the root already binds that tree, so a second
  declaration would compile it twice; the member reaches it with
  `use super::common::…` instead.
- **Every binder of the shared helper tree binds it `pub`.** The tree's items
  are `pub`. Most files in the tree carry their own per-file
  `#[allow(dead_code)]`, but `tests/common/secret_scan.rs` deliberately carries
  **none** — that suppression was removed on purpose, so the lint keeps its
  teeth in a file whose whole purpose is to be checked. A *private* `mod
  common;` makes those items unreachable from the binding crate root, and
  rustc reports every one of them as dead code: a hosted clippy failure under
  `-D warnings`, and one that only a **Linux** runner sees, because this tree's
  macOS build fails earlier on unrelated findings. `origin/main` still carries
  the blanket suppression in that file, which is why a private binding is
  harmless there and fatal here. `scripts/check_test_common_binding.py`
  enforces it for every binder of the root package's tree.
- **A member that observes a process-global must scope what it observes.** The
  consolidation puts a member in a binary — and a process — with its
  siblings. The agent-loop harness subscribes to the **process-global** event
  bus, and one of its assertions counted *every* `PermissionPending` on that
  bus with no session filter; in a family of thirty that assertion is a
  cross-test race, and it fired. A member that reads a global must filter it
  by what it owns — the file's own `EventCollector` helper already filters by
  session id, and the assertion that bypassed it now filters the same way.
  **The hosted `CI` job cannot see this class:** the `ci` nextest profile runs
  **one process per test**, so two tests never share a process-global. A plain
  `cargo test`, which runs a binary's tests as threads in one process, does.

### The families after the seventh merge (2026-10-22)

`main` advanced a third time, to `726f3c645` (PR #167, the Linux trusted-Git
C008 contained worktree), so this line reconciled with it a **seventh** time.
**The advance is documentation only** — four records, and no code and no
coverage — so the family topology is unchanged: the root package's test-target
count stays **25** (21 families + 4), the workspace's **53**, and the root
integration-test *module* count stays **319** (4 top-level + 315 members).

Both sides edited `plans/registry.md`, which is exactly where a documentation
merge can quietly overwrite one side's row, so the merge was checked rather
than assumed: the row the branch owns is byte-identical to its pre-merge copy,
the row `main` owns is byte-identical to `origin/main`'s, and the file both
sides edited (`architecture/blackboard_contract_matrix.md`) differs from
**both** parents.

One fixture implementation changed during the **sixth** merge and is worth
naming, because it is the kind of change a reviewer would look for: the C008
suite's `FakeMode::LinkedWorktree` arm stopped carrying its own inline copy of
the linked-worktree creation and now calls the shared helper
`common::linked_worktree::LinkedWorktreeCreator`. Git took that change
automatically — only `main` had edited the region since the merge base — and
it is **not** a loss of coverage: the helper runs the same three Git operations
in the same order (`branch`, `worktree add --detach`, `symbolic-ref HEAD`), and
nothing in the suite asserts on the two fields that differ (`backend` and
`network_enforced`).

### The measured link cost (M005)

The family consolidation was justified by a **projection** (M004's closure
record: a ~41 % workspace-compile saving, computed from the baseline's
median link time applied to the new target count). M005 **measured** it
instead, on the same machine, in the same session, with the same command.

**Method.** A *fresh* `CARGO_TARGET_DIR` — a warm tree reports the cost of
*checking*, not of linking — and then:

```bash
CARGO_TARGET_DIR="$(mktemp -d)" cargo test --workspace --locked --no-run --timings
```

The emitted report is parsed from the `const UNIT_DATA = [...]` block with
`re.S`. **`--timings=json` and `--timings-dir` are not usable here**: they
are unstable on cargo 1.89. The parse command, the raw unit data and the
per-family table are in
`plans/validation/ci-test-throughput-recovery/measurement-record.md` §3.

**Machine (recorded because unit-seconds are only comparable within a
machine).** 14 cores / 24 GiB, aarch64-apple-darwin, cargo and rustc
1.89.0, Apple clang 21.0.0, **no `mold`** — the hosted runner has `mold`
and this machine does not, so the *hosted* numbers below are not
comparable to these local ones, and are not mixed with them.

**The result.**

| | before (`2c1953347`) | after (`3dfeb6e2f`) | change |
|---|---:|---:|---:|
| Root integration-test executables linked | 275 | 17 | **−93.8 %** |
| Linked test executables, whole workspace | 305 | 47 | **−84.6 %** |
| Total compilation units | 988 | 730 | **−26.1 %** |
| Aggregate unit-seconds, whole workspace | 4,300.8 | 2,618.9 | **−39.1 %** |
| Aggregate unit-seconds, root integration bucket | 2,123.6 | 423.0 | **−80.1 %** |

**M004's ~41 % projection reproduces within 1.9 percentage points**
(−39.1 % measured). The candidate was repeated twice in the same session;
the two runs agreed to within ±0.6 % on the aggregate and ±5.5 % on the
17-unit root bucket.

**What this measurement does not show.** The aggregate carries a
contention-mediated component — the two sides' unrelated buckets disagree
in opposite directions, and the baseline's build-script executions alone
were 724.1 s against the candidate's ~115 s. M004's closure record's own
§4.1 table (271 targets / 4,040.1 s / 9,155.6 s total) does **not**
reproduce on this machine. The `pub mod` repair that M005's candidate
carries makes the candidate's link work marginally *higher*, so the saving
above is the conservative reading.

**The per-family cost.** A family's cost is dominated by the size of the
code it links, not by the number of test files it holds: `core_daemon` at
51.7 s carries the daemon and its server wiring, while `command_git` at
24.6 s carries the Git policy surface. The baseline's median was **13.9 s
per *file*** across 271 separate executables; the candidate's is **24.7 s
per *family*** across 17 — a family costs more than a file because it links
more code into one binary, and 17 of them replace 275. The full 47-row
table is in the measurement record §3.5; the root package's families are
the 21 `codegg` rows of it.

### The hosted two-phase wall-clock (M005)

The link cost above is a **local** measurement. The hosted CI job is
where the change was meant to pay off, so the same claim was measured
there too, against a preregistered method.

**Method.** Two refs, each pinned, each dispatched through the same
`ci.yml`: `ci-baseline/2c1953347` (the before side) and
`m005/candidate-3dfeb6e2f` (the after side). The job's two phases are
recovered from the run log rather than from a new step — the last
`Finished … in X` line of the `test` profile is the **build phase** and
nextest's `Summary [ Xs]` line is the **execution phase** — because
adding a build step would change the job being measured. The preregistration
is `plans/validation/ci-test-throughput-recovery/measurement-preregistration.md`;
the run-by-run record is the measurement record §4.

**Machine and cache state.** Every run is `ubuntu-24.04` with `mold`
present. **Every run on both sides is warm-cache** (a rust-cache hit),
which is a property of the workflow rather than a choice: the cache key
is shared across refs with an identical lockfile, so a cold sample is
not obtainable unilaterally.

| | before median (n = 4) | after median (n = 3) | ratio | threshold | verdict |
|---|---:|---:|---:|---|---|
| build phase (T2) | 683.000 s | 359.000 s | **52.6 %** | ≤ 60 % | **PASS** |
| execution phase (T3) | 487.818 s | 377.692 s | **77.4 %** | ≥ 90 % | **FAIL** |

**Raw values, both sides, all warm-cache.**

| side | build phase (s) | execution phase (s) | `Workspace tests` step (s) | job total (s) |
|---|---|---|---|---|
| before | 615.0, 646.0, 720.0, 744.0 | 436.888, 470.185, 505.452, 527.888 | 1060, 1124, 1232, 1282 | 1495, 1542, 1705, 1775 |
| after | 308.0, 359.0, 433.0 | 360.489, 377.692, 527.838 | 672, 740, 964 | 1125, 1215, 1594 |

**The wall-clock is reported with its dispersion and is not quoted as a
precise effect size.** The after side's execution phase spans 167 s
(360.489 → 527.838), and its `after-4` is the slowest execution phase on
either side — slower than three of the four before-side runs. The
**host-speed term is the same order as the effect being measured**: one
test whose source is **byte-identical** on both sides
(`response_budget_trims_trailing_nodes_with_a_reason`) took 61.1 / 84.3 /
82.6 / 81.8 s on the before side and **38.7 / 82.1 / 43.9 s** on the
after side — a **2.13× spread within the after side alone**, on
identical code. The after side's *internal* dispersion therefore exceeds
the whole before-vs-after difference the execution phase reports (a
1.29× ratio), and it also explains the after side's dispersion directly:
`after-4`'s host ran that test as slowly as the before side's hosts did.
A hosted wall-clock difference of that size cannot be separated from a
difference in the runners' speed, and the number above is the
measurement's *direction and magnitude class*, not a figure to quote to
one decimal place.

**Why T3 does not hold, and why that is not a defect.** T3 expected the
execution phase to be roughly unchanged. It is materially **shorter**, for
two reasons of very different kinds: 313 fewer test executions ran (real,
intended, and exactly reconciled — every one attributed to a recorded
removal), and the same executions ran faster on the after side. The
threshold's *purpose* — that the workstream did not silently drop
assertions — is discharged instead by **invariant I1**, which is a
counting check rather than a timing one, and which holds with zero
unexplained losses. The measurement record §4.4 carries the full
argument.

**The countable claim is the one that carries the milestone.** The
hosted nextest binary count fell **308 → 50** (−83.8 %) — the
preregistered prediction of 50, reconciled exactly against the observed
308 — and the linked root test-executable count fell **288 → 21**. The
wall-clock above is corroboration with its dispersion attached, not the
claim.

### Change-scoped verification (`scripts/verify.sh scoped`)

`scoped` is a **developer-local** affordance: it runs only the tests a change
can affect. It is additive to the guard set — every `quick` guard still runs,
in the same order — and it never changes what the hosted CI job runs.

```bash
scripts/verify.sh scoped              # base: origin/main
scripts/verify.sh scoped HEAD~3       # any base ref
```

**How the scope is computed.** `scripts/scoped_selection.py` resolves the
changed files against the base ref using **merge-base** semantics
(`git merge-base <base> HEAD`), so a branch that has already been merged into
the base is still compared against the commit it forked from. Uncommitted work
— staged, unstaged, and untracked — is included: a developer asking "what
does my change affect?" means everything in their tree. A changed file inside
a member's source root marks that member affected; the longest matching member
root wins. The affected member contributes its library test target **always**,
plus the integration targets of every group in `docs/test-ownership.toml`
whose source pattern matches.

**Dependency pull-in.** A member that **depends on** an affected member is
affected too, and it contributes its **full** surface — its library test target
and every integration target it owns. The pull-in is transitive.

This is not a refinement; it is the difference between a correct scope and a
wrong one. The root package depends on all eleven other workspace crates and
re-exports several of them (`codegg::security::*` is `eggsentry`,
`codegg::lsp::*` is `egglsp`, `codegg::scheduler::config::*` is
`codegg-config`). A change to `crates/eggsentry/src/command.rs` therefore
changes the behaviour of `codegg::security::scanner::inspect_text`, which the
root package's security tool calls, which the root package's integration tests
call — across seven files in three families, plus the root's inline
`src/tool/security.rs` tests. The coupling runs **through the dependent's own
source**, so a path-to-target rule cannot see it: the test file that calls the
security tool never names `eggsentry`.

The dependency edges are declared in `docs/test-ownership.toml`'s
`[[dependencies]]` section, and `scripts/check_test_ownership.py` compares
that set against `cargo metadata`'s workspace-local edges. An edge the
workspace has and the manifest does not is a **guard failure**, not a silent
under-selection.

The price is stated plainly: a change to any non-root crate widens the scope
to the root package's whole integration surface, because the root package
depends on every other crate. A change confined to the root package's own
`src/` — the common case — stays precise, because the root package has no
dependents.

**Feature gates.** A selected target that is gated behind `required-features`
cannot be built without its features, so the mode enables exactly the features
its selected targets are gated on and prints them. Naming a gated target
without enabling its feature is a cargo error, not a silent skip.

**What it does when it cannot be precise.** It never silently skips.

| Situation | Action |
|---|---|
| A changed file inside a member, matching a group | That group's targets, plus the member's library test target. |
| A changed file inside a member, matching **no** group | The whole member is widened to its full surface: every integration target it owns, plus its library test target. |
| A changed file in a crate another crate **depends on** | Every transitive dependent is affected, and each contributes its full surface. |
| A changed file **outside** every member (a script, a doc, a workflow, an asset) | The scope widens to the **whole workspace**, and the mode says which file caused it. |
| The diff cannot be computed (unknown base ref, shallow clone with no merge base) | The mode **fails loudly** and exits non-zero. It never falls back to "no tests". |

The out-of-member rule is deliberately conservative. The repository's
change-trigger routing classifies those files for the *guards* — and the
guards still run in full — but no test target can be *ruled out* for them, so
the safe scope is the entire workspace.

**What it prints.** The changed files it considered, the files outside every
member, the files inside a member that no group covers, the members and
targets it selected, and the reason for each. A scope that is invisible is a
scope that cannot be trusted.

**Its limits.** It selects *tests*, not *coverage*: it is a superset of what
the change actually affects, and it says so. It does not narrow Clippy or
`cargo check` — those remain workspace-wide. It does not change the hosted
job's scope in any way. `docs/test-ownership.toml` is its only input, and
`scripts/check_test_ownership.py` is what keeps that input honest.

The mode is **not** a way to skip work in CI, and it is not a substitute for
`full` before a handoff. Steps in order:
1. Generated-agent schema sync (`generate_builtin_agents.py --check`)
2. Core boundary guard (`check-core-boundary.sh`)
3. Sandbox contract guard (`check_sandbox_contract.py`)
4. Execution ownership guard (`check_execution_ownership.py`)
5. TUI project authority guard (`check_tui_project_authority.py`)
6. TUI editor text-authority guard (`check_tui_editor_text_authority.py`)
7. HTTP route disposition guard (`check_http_route_disposition.py`)
8. Audit coverage guard (`check_audit_coverage.py`)
9. Scheduler bypass guard (`check_scheduler_bypass.py`)
10. Formatting (`cargo fmt --check --all`)
11. Workspace Clippy (`cargo clippy --workspace --all-targets --locked`)
12. Live-Eggwork relevance detection
(`scripts/detect-live-eggwork-changes.sh`; main pushes always
require live; PRs require live only for live-relevant paths)
13. Conditional Eggwork fixture prebuild
(`scripts/prebuild-eggwork-fixtures.sh`; skipped when live is omitted)
14. Workspace tests (`cargo nextest run --workspace --locked --profile ci`;
full suite on main/relevant-PR live path, `-E 'not
binary(eggwork_remote_execution_live)'` on the ordinary unrelated-PR
fast path).

CI uses default features, bounded resources. Optional feature, plugin,
example, LSP, and cross-platform checks remain local.

### Live Eggwork fixture trust

The live qualification (`tests/eggwork_remote_execution_live/eggwork_remote_execution_live.inc`) starts a
real Eggwork node as a bounded subprocess. The pinned node resolves its
sandbox helper as a sibling of its own executable and **refuses a helper it
cannot trust**: the helper must be executable, owned by the invoking user,
and neither it nor any directory above it may be group- or world-writable.
An untrusted helper makes the node drop
`isolation.landlock.workspace-rw.v1`, and every required-isolation run is
then refused by the executor.

Cargo installs build artifacts with the *process umask* and does not narrow
them, and `tempfile` creates its directories the same way. On a host with a
permissive umask (`0002` is common on shared workstations) the helper and its
directory therefore land group-writable, the node refuses them, and the
live isolation qualifications fail on a tree, a fixture, and a code base
that are all identical to a green run. The outcome followed the operator's
umask instead of the product.

The qualification therefore **stages** the node and the helper in a private
directory before starting the node: the staging directory is set to `0700`,
the helper is placed there as an independent copy with mode `0755`, and the
node is started from that directory. The trust chain then depends on the
fixture alone — the staged directory, and the system temp root above it
(sticky and root-owned, which the trust check exempts) — and not on the
build tree's own modes or on the umask. The build artifacts are left
untouched; the prebuild script only has to produce them, not trust them.

`a_staged_sandbox_helper_is_trusted_whatever_the_umask` pins the contract:
the mirror of the node's trust check rejects a group-writable helper in a
group-writable directory, and accepts the staged pair.

### CI economy policy

Four deliberate deviations from the local defaults, all confined to the
hosted runner (4 vCPU / 16 GB). Local `verify.sh` behavior is unchanged
except `CARGO_BUILD_JOBS=2`:

1. **Superseded-run cancellation** — `concurrency` with
   `cancel-in-progress: true` per ref. A new push cancels the previous
   in-progress run instead of verifying both.
2. **Docs-only skip** — `paths-ignore` for `plans/**`, `docs/**`,
   `architecture/**`, `**.md`, `.opencode/skills/**`. Safe because every
   guard verifies code -> docs direction; `assets/agents/**` and
   `assets/prompts/**` stay gated.
3. **mold linker** — `rui314/setup-mold@v1` as the default linker. The
   workspace-test step is dominated by linking large test binaries, and
   mold is several times faster than GNU ld there; the suite itself
   validates the linked output.
4. **Build jobs fixed for the runner** — `CARGO_BUILD_JOBS=4` in
   `.github/workflows/ci.yml` (M001 disposition: == runner vCPUs ==
   nextest `ci` slots; 8 oversubscribes because each rustc already
   threads codegen internally, 2 leaves headroom unused).
5. **Nextest `ci` profile** — 4 concurrent per-test processes
   (= runner vCPUs; 8 tried 2026-09-25, reverted after a load-induced
   flake in `scheduler_cancellation`), run-alone heavies. Final
   M001-M005 measurement: 11726 tests in 362s execution on run
   `36196068239` (historical 11781 pre-consolidation in ~7 min vs
   ~10.6 min under serial `cargo test`); the sleep-bound heavies still
   take their wall-clock.
   Does not change `--test-threads` semantics of plain `cargo test`
   runs.

Do not generalize these: intra-binary `--test-threads` stays 1 — test
code mutates process-global env vars (audited, not assumed) — and
splitting CI into parallel jobs that each compile the workspace
duplicates the dominant cost instead of removing it.

### Cargo profile alignment — one compiled dependency graph

Cargo hashes the **resolved** profile into every compilation unit's identity.
The `dev` profile (used by `cargo build`, `cargo check`, `cargo clippy`) and
the `test` profile (used by `cargo test` / `cargo nextest run`) are therefore
two different builds of the same source — and any field that differs between
them makes the same crate compile **twice**.

`strip` was the only differing field: `[profile.dev]` kept debuginfo,
`[profile.test]` stripped it. The cost was a full second compile of the
third-party dependency graph *and* of the root library rlib in every
verification invocation:

- the hosted job's `cargo build --locked --bin codegg-sandbox-helper` step
  built the whole graph under `dev`, and `cargo nextest run` then rebuilt it
  under `test`;
- the local `cargo build` → `cargo test` loop paid the same double compile.

Both profiles now set `strip = "debuginfo"`, so they resolve identically and
the graph is compiled once. The `strip = "debuginfo"` intent for test
artifacts is preserved — it is what keeps the root integration-test
executables at 135-208 MB each and the root library rlib at 855 MB, and
unstripping them is not an option (the hosted runner does not have the disk).

`scripts/check_cargo_profile_alignment.py` guards the property: it resolves
both profiles the way cargo does (`test` inherits `dev`) and fails when any
hashed field — or any per-package override — differs. Its `--self-test`
injects each violation class and proves the guard rejects it. The guard is
wired into `scripts/verify.sh quick` and into the `CI / verify` job.

**Recovering full debuginfo.** `strip = "debuginfo"` removes DWARF but keeps
the symbol table, so a panic backtrace still names its frames and shows the
panic location. For full debuginfo in a single invocation, override the
profile on the command line:

```bash
cargo build --config 'profile.dev.strip="none"'
cargo test  --config 'profile.test.strip="none"'
```

`RUSTFLAGS="-C debuginfo=2"` does **not** do this: the profile's `strip`
still applies, so the debuginfo is removed again. The `--config` override
replaces the profile setting itself.

The one place that needs the unstripped binary is the desktop bundle, which
ships the root daemon: `.github/workflows/desktop-e2e.yml` builds it with
`--config 'profile.dev.strip="none"'` so crash symbolication is preserved.
That workflow runs no test suite, so it has no double compile to reclaim.
**That workflow is manual dispatch only since 2026-10-23** — the desktop
foundation's development is paused, so the hosted visible-window qualification
no longer runs on push or on pull requests. See `desktop.md` §"The hosted
qualification is paused (2026-10-23)" for the evidence and for what restoring
it costs.

Release artifacts are unaffected: `[profile.release]` already had
`strip = true`, and it is a separate profile.

### Build/link vs execution diagnostics (M001 recipe)

To separate compile/link wall time from test execution on any run:

1. **Hosted step times** — the Actions `Workspace tests` step spans
   both phases. The build phase ends at the Cargo line
   `Finished 'test' profile [unoptimized + debuginfo] target(s) in Xs`
   in the step log; everything after is Nextest execution.
2. **Nextest execution total** — the run-closing
   `Summary [Xs] N tests run` line is pure execution across all
   binaries (historical M001 baseline: 414 s for 11781 tests;
   final M004 baseline: 362.537s for 11726 passed / 1 skipped on run
   `36196068239`).
3. **Local representative-unit probe** — rebuild one large test target
   after touching the root lib (keeps deps warm, isolates one
   lib-codegen + link unit):
   ```bash
   touch src/lib.rs && time cargo test --locked --no-run --test session_crud
   ```
   Compare candidates with `CARGO_INCREMENTAL=0` to remove
   incremental-cache-maturity bias, and repeat warm builds to confirm
   the steady state; a cold fingerprint (e.g. right after a profile
   change) is not comparable to a warm baseline.
4. **Cargo timing (ephemeral)** — for a one-off breakdown of where
   rustc time goes in a representative build:
   ```bash
   cargo test --workspace --locked --no-run --timings
   ```
   Timing output stays local; it is not a CI artifact or gate.

M001 dispositions (2026-09-25): `CARGO_PROFILE_TEST_DEBUG=0` rejected
— codegen-isolated local A/B on a representative large test target
showed wall parity (2m22s vs 2m23s non-incremental) with only CPU-time
reduction, so no CI-only override without hosted proof; hosted
`Finished ... + debuginfo` confirms generation happens but the
8m50s parallel build phase across ~100 binaries is link-count
dominated (evidence handed to M003). `codegen-units` not pursued —
single-variable sweep unjustified while executable count dominates.
`CARGO_BUILD_JOBS=4` retained as the measured final value.

### Release-footprint measurements

```bash
CARGO_TARGET_DIR=/tmp/codegg-release-default \
  cargo build --release --locked --bin codegg

CARGO_TARGET_DIR=/tmp/codegg-release-production \
  cargo build --release --locked --bin codegg \
  --features server,plugins,lsp-test-support
```

Binary size is evidence, not a CI gate.

### `--all-features` and real-server tests

`--all-features` enables `lsp-real-server-tests` which compiles
`real_server_smoke.rs`. Tests skip at runtime when server binaries
are not installed. CI does not install real servers.

`verify.sh full` uses `--features server,plugins,lsp-test-support`
instead of `--all-features` to avoid activating `lsp-real-server-tests`.

The agent-facing `verify` tool follows the same conservative rule: its default
Cargo lint does not enable every workspace feature. A caller may select one
workspace package and validated package features through typed fields;
`all_features` is an explicit package-only opt-in. This does not replace
`scripts/verify.sh quick/full`, which remain the canonical workspace checks.

### Test-support features and the provider capture seam

| Feature | Crate | Enabled by | Why |
|---|---|---|---|
| `lsp-test-support` | `egglsp` | root feature flags | Fake-LSP harness; real-server tests stay out of default sweeps. |
| `capture-test-support` | `codegg-providers` | root `[dev-dependencies]` **only** | Origin override for `tests/agent_loop/opencode_go_connection_trajectory.inc`. |

`capture-test-support` is deliberately a **dev-dependency** feature rather than
a root feature. Under resolver 2 that keeps it out of `cargo build`, out of the
release binary, and out of `--features` sweeps, while still letting `cargo test`
compile the seam for `tests/`. It is the same technique as `lsp-test-support`
with a narrower blast radius.

Consequences to preserve:

- The seam has no production setter. Nothing in config, the protocol, an
  environment variable, or a CLI path can enable it.
- The redirect replaces the **origin only**. The shared profile still owns the
  path and per-surface auth shape, so a captured request is the production
  request.
- Because it lives on a `ProviderConnectionFactory` instance, it is safe for
  tests running concurrently in one process. Do not convert it into process or
  environment global state.

Run the trajectory with:

```bash
cargo test -p codegg --test opencode_go_connection_trajectory
```

### Session projection transport closure

```bash
python3 scripts/check_projection_transport_isolation.py
python3 scripts/check_websocket_bounds.py
cargo test -p codegg-protocol
cargo test -p codegg --lib core::transport::projection
cargo test -p codegg --lib server::ws
cargo test -p codegg --lib core::transport::daemon_socket
cargo test --test projection_replay                 # consolidated binary (M003 pilot): 11 submodules
cargo test --test projection_disclosure_invariants
cargo test --test projection_artifact_handles
```

### CI Lane Roadmap Decision

**Conservative keep, parallelized execution** — the single-job bounded
test lane stays (no resource-lane split: each extra job would recompile
the workspace and duplicate the dominant cost). Wall-clock pressure is
handled inside the lane via nextest profile `ci` (see above), adopted
2026-09-25 after measurement showed execution, not just compile, needed
parallelism.

### Future Considerations

Nextest adoption (phase 1, 2026-09-25) covers cross-binary parallelism
with serial-within and run-alone heavies. If wall-clock regresses:

1. **Extend the run-alone list** — any new sleep/subprocess-bound file
   gets a `binary(...)` entry in the `ci` profile's heavy override.
2. **Tune slot count** — `test-threads` in profile `ci` trades memory
   (one libcodegg-loaded binary per slot) against throughput; measure
   before changing.
3. **Selective feature flags** — use `--features` instead of
   `--all-features` for targeted CI runs (unchanged).

### CI/test throughput final state (M001–M005 closure, 2026-09-25;
### C001 post-closure baselines, 2026-09-30)

Routine CI remains one bounded non-release job. The current settings
and consolidation are the result of the closed M001–M005 workstream
(`plans/subsystems/ci-test-throughput-optimization-roadmap.md`,
closures under `plans/closure/ci-test-throughput-optimization/`).
M001: `CARGO_BUILD_JOBS=4`, `CARGO_PROFILE_TEST_DEBUG=0` rejected
(debug-info generation is not wall-time bound after the JOBS
decision), codegen-units sweep declined as a single-variable
experiment. M002: explicit Eggwork fixture prebuild
(`scripts/prebuild-eggwork-fixtures.sh`), repository-owned
change-sensitive live qualification
(`scripts/detect-live-eggwork-changes.sh`), whole-binary Nextest
exclusivity retained for the 5 heavy binaries with a dated audit.
M003: 11 `projection_replay_*` binaries → 1 `projection_replay`
target. M004: 5 default-feature `session_*` binaries → 1
`session_family` target. Feature-gated `team_collaboration_*`
binaries (`required-features = ["server"]`) are explicitly outside
the consolidation scope per the M004 plan's incompatible-feature
boundary. The total binary count of root integration tests fell
from 100 to 84; workspace test count fell from 11,781 to 11,726;
hosted steady-state landed at 17m17s (run `36196068239`,
clippy 2m08s, prebuild warm 4 s, build phase 7m51s, exec 362s,
11726 tests passed / 1 skipped). The full CI economy policy
section above remains the authoritative summary.

C001 post-C002 baselines (final reverted tree, no sccache, no
`CARGO_INCREMENTAL` override; test growth to 11830 from Eggplan M002
+ plugin hermetic fixes):

- Main/relevant-change live path (authoritative): control PR
  `c001-control-incremental` run `36771911506` attempt 1 — 18m13s,
  build 8m30s, exec 369s, 11830/5, `live_required=true`; attempt 2
  (warm rerun) — 17m55s, build 8m22s, exec 383s; main push
  `36760308368` — 17m45s, 11830 pass, live included.
- Ordinary unrelated-PR fast path: PR `c001-unrelated-fastpath-v2` run
  `36776772880` — 18m15s total, Clippy 2m06s, build 9m12s, exec 356s,
  11817 passed / 5 skipped, `live_required=false`, prebuild skipped,
  `eggwork_remote_execution_live` omitted via `-E 'not
  binary(eggwork_remote_execution_live)'`. Detector correctly reports
  false for non-live changes (e.g. `src/tool_advisor/` comment-only);
  docs-only PRs skip CI via `paths-ignore` and are not baselines.

### M005 — Compiler-result cache and same-job overlap (negative dispositions;
### C001 measured sccache disposition supersedes the unmeasured M005 cache reasoning)

**sccache disposition: not retained (C001 measured negative).** The routine
workflow uses `Swatinem/rust-cache@v2` (M001 review, expanded at M002 to
cover `crates/eggwork-test-node`) backed by the GitHub Actions cache. C001
executed the previously omitted bounded hosted A/B (preregistration
`plans/closure/ci-test-throughput-optimization-post-closure-corrective/001-measurement-preregistration.md`;
candidate `mozilla-actions/sccache-action@v0.0.11`, sccache v0.18.0,
`SCCACHE_GHA_ENABLED=true`, `RUSTC_WRAPPER=sccache`,
`CARGO_INCREMENTAL=0` explicit in both arms, `contents: read` retained, no
secret):

- Control (post-C002, `CARGO_INCREMENTAL=0` explicit, no sccache): PR
  `c001-control-incremental` run `36771911506` attempt 1 — 18m13s total,
  Clippy 2m22s, build 8m30s, exec 369s, 11830 passed / 5 skipped, live
  included; attempt 2 (warm rerun) — 17m55s, Clippy 2m16s, build 8m22s,
  exec 383s, same scope.
- Candidate seed/cold (same + sccache, first population) run `36772006989` —
  15m33s, Clippy 1m50s, build 6m58s, exec 330s, sccache 0 hits / 10 misses
  (477 non-cacheable: multiple-input-files 238, crate-type 230), 0 errors.
- Candidate warm (same branch, cache restored) run `36776697828` — 16m26s,
  Clippy 2m14s, build 6m52s, exec 389s, sccache 10/10 hits (100% on the 10
  executed), 0 misses, 0 errors.
- Candidate reuse (small leaf comment, most crates unchanged) run
  `36778759740` — 17m41s, Clippy 2m16s, build 8m04s, exec 384s, sccache 6
  hits / 4 misses (60%), 0 errors.

Only 10/487 (2%) compile requests are sccache-cacheable in this workspace;
warm-vs-seed gain from 100% hits is 6s build. Seed (0 hits) already beats
control by 92s build, proving the large vs-control delta is confounding
(runner/rust-cache variance), not sccache hits. Realistic reuse gains only
18–26s build vs warm control, below the preregistered 45s build / 30s total
threshold requiring two comparable observations. Retention thresholds not
met; candidate fully reverted (no `RUSTC_WRAPPER`, no sccache action, no
`CARGO_INCREMENTAL` override in the final tree).

M005's unmeasured premises are superseded and corrected: `Swatinem/rust-cache`
does not provide equivalent compiler-result caching (workspace crates are
opt-in, not default), and the supported sccache GHA backend uses
Actions-provided runtime credentials with no external secret. The negative
disposition stands on measured evidence (low cacheable fraction, minimal
hit benefit, threshold miss on reuse), not on the prior assumptions. M001–M004
conclusions and the same-job-overlap rejection below are unchanged.

**Critical-path overlap disposition: rejected.** Same-job overlap
candidates (parallelizing cheap static guards against the Cargo
build, for example) were considered and rejected:

- The `git identity` + `agent TOML` + 7 guards + `fmt` + `clippy`
  steps all touch the working tree but each is sub-second; the
  Cargo build phase needs the rust toolchain whose install is the
  preceding step's gate.
- Backgrounding any step against another Cargo invocation is
  excluded by the single shared `target/` directory — Cargo target
  locks would contend.
- The next strict improvement (a second bounded CI job for clippy
  + tests against a separate target directory) would re-compile
  the workspace twice, doubling the dominant cost.

The workstream therefore closes with one bounded non-release job
plus a single reentrant warm-cache key, exactly the
"Conservative keep, parallelized execution" disposition recorded
above.

**No superseded experiments to clean.** All current workflow
comments reflect M001–M004 measured dispositions; no probe or
trial comment remains in `ci.yml`, `.config/nextest.toml`,
`scripts/verify.sh`, or `architecture/testing.md`. Per the M001
diagnostic recipe, `cargo --timings` remains an ephemeral local
diagnostic only.

## Related Docs

- `AGENTS.md` — full test command catalog
- `.config/nextest.toml` — nextest profiles
- `scripts/audit_tokio_tests.py` — Tokio runtime flavor audit

## Source Verification

Verified 2026-10-06 against `scripts/verify.sh`, `.config/nextest.toml`,
`.github/workflows/ci.yml`, `tests/common/pool.rs`, and the workspace tree.
The many dated CI wall-clock/baseline measurements further down this document
are historical records and were deliberately left untouched.

Corrected:
- **`verify.sh quick` step list 10 → 19 steps.** Nine guards were added after
  the list was written: `check-client-boundary.sh`,
  `check-desktop-boundary.sh`, `check_tui_editor_text_authority.py`,
  `check_provider_wire_boundary.py`, `check_provider_wire_cutover.py`,
  `check_openai_endpoint_composition.py`,
  `check_provider_catalog_consistency.py`,
  `check_provider_resilience_ownership.py`, and
  `check_eggwork_target_routing.py`. Verified against the `==> ` echo order in
  `scripts/verify.sh:50`-`:107`.
- **CI step list 13 → 14 steps.** Added the TUI editor text-authority guard
  between the TUI project authority and HTTP route disposition guards
  (`.github/workflows/ci.yml:110`).
- **Run-alone heavies "four" → "five".** The `ci` profile override lists five
  binaries; `scheduler_cancellation` is the fifth and was missing
  (`.config/nextest.toml`).

Confirmed accurate, no change:
- Nextest profiles match exactly: `default` 14 threads, `timing` serial
  (1 thread), `ci` 4 threads, all with `slow-timeout`
  (`terminate-after = 2`, `60s` period for `timing`/`ci`, `30s` for `default`).
- `isolated_pool()` (`tests/common/pool.rs:52`) does build a named in-memory
  DB as `codegg_test_iso_{uuid}` (`:53`) and runs migrations internally, so
  the "do NOT add redundant `migrate()`" rule holds. `shared_pool()`
  (`:37`) is the `OnceLock` process-wide pool, as documented.
- `scripts/audit_tokio_tests.py` exists.
- `verify.sh full` uses `--features server,plugins,lsp-test-support` with
  `--profile ci`, not `--all-features`, avoiding `lsp-real-server-tests`.
- Plain `cargo test` broad runs keep `--test-threads=1`
  (`scripts/verify.sh:43`).
