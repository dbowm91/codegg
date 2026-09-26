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
4. `check_sandbox_contract.py`
5. `check_execution_ownership.py`
6. `check_tui_project_authority.py`
7. `check_http_route_disposition.py`
8. `check_audit_coverage.py`
9. `check_scheduler_bypass.py`
10. `cargo check --workspace --all-targets --locked`

### `verify.sh full`

Runs quick first, then:

1. `cargo clippy --workspace --all-targets --locked -- -D warnings`
2. `cargo nextest run --workspace --locked --profile ci`
3. `cargo nextest run -p codegg --locked --features server,plugins,lsp-test-support --profile ci`

Both modes set `CARGO_BUILD_JOBS=2` by default. Test execution runs
under nextest profile `ci` (see below); plain `cargo test` broad runs
keep `--test-threads=1`.

## Test Resource Classes

| Class | Description | Parallelism | Examples |
|-------|-------------|-------------|----------|
| `fast` | Pure/unit, parsing, config | Safe | `egggit::diff`, `eggsentry::profile` |
| `storage` | SQLite pool ops, CRUD | Serial or low | `tests/session_crud.rs` |
| `process-heavy` | Fake LSP stdio, daemon | Serial | `tests/lsp_composite_stdio.rs` |
| `plugin-heavy` | Wasmtime runtime | Serial | `tests/plugin.rs` |
| `adversarial` | Routing, sandbox, projection | Serial | `tests/command_routing_adversarial.rs` |
| `workspace` | Workspace isolation | Serial | `tests/workspace_isolation.rs` |
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
cargo test --test workspace_isolation

# Adversarial tests
cargo test --test command_routing_adversarial
cargo test --test python_sandbox_adversarial
cargo test --test context_projection_adversarial

# Real LSP smoke tests (requires installed servers)
cargo test -p egglsp --features lsp-real-server-tests \
  --test real_server_smoke -- rust_analyzer

# Tokio flavor audit
python3 scripts/audit_tokio_tests.py
```

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
sleep/timeout-bound heavy files (`eggwork_remote_execution_live`,
`interactive_process_attach_resume`, `interactive_process_sessions`,
`interactive_terminal_tui`) run alone via `threads-required = "num-cpus"`.
The profile is workspace-wide only — subset runs (`-p <crate>`) use the
default or timing profile, because the heavy-binary filter strictly
requires its binaries to exist.

```bash
cargo install cargo-nextest --locked
cargo nextest run --workspace --profile ci
scripts/capture-nextest-timing.sh --top 20
```

## CI Structure

Routine CI is one bounded `verify` job in `.github/workflows/ci.yml`
for PRs and pushes to `main`. The historical pre-optimization baseline
was about 37 minutes (2026-09-25: ~45 s setup/guards/fmt, ~5 min
Clippy, ~31 min workspace tests). The intermediate ~19-minute result
and 11,781-test count are historical; final M001-M005 evidence is
17m17s on main/live run `36196068239` attempt 3, with 11,726 passed
and 1 skipped. The final measurement recorded ~2m08s Clippy, 7m51s
workspace-test build, and 362.537s Nextest execution. The unrelated-PR
fast path has not yet been measured; C001 owns that hosted measurement
and the compiler-cache experiment. Steps run in this order:

1. Checkout, stable Rust + rustfmt/Clippy, mold linker, Rust/Cargo cache, and Nextest setup.
2. Configure test Git identity.
3. Validate generated-agent schema, core boundary, sandbox contract, execution ownership,
   TUI authority, HTTP route disposition, audit coverage, and scheduler bypass.
4. Formatting.
5. Workspace Clippy.
6. Detect whether changed paths require live Eggwork qualification.
7. Prebuild/cache the workspace-excluded Eggwork fixture only when live qualification is required.
8. Workspace tests with `--profile ci`; main pushes and relevant PRs include
   `eggwork_remote_execution_live`, while unrelated PRs omit that one binary.

The ordinary unrelated-PR path and the main/relevant-change live path are
separate baselines. Main always includes live qualification. PR relevance
is determined by `scripts/detect-live-eggwork-changes.sh`; detection errors
fail open and include the live target. `scripts/prebuild-eggwork-fixtures.sh`
is conditional on the same decision.
2. Core boundary guard (`check-core-boundary.sh`)
3. Sandbox contract guard (`check_sandbox_contract.py`)
4. Execution ownership guard (`check_execution_ownership.py`)
5. TUI project authority guard (`check_tui_project_authority.py`)
6. HTTP route disposition guard (`check_http_route_disposition.py`)
7. Audit coverage guard (`check_audit_coverage.py`)
8. Scheduler bypass guard (`check_scheduler_bypass.py`)
9. Formatting (`cargo fmt --check --all`)
10. Workspace Clippy (`cargo clippy --workspace --all-targets --locked`)
11. Workspace tests (`cargo nextest run --workspace --locked --profile ci`)

CI uses default features, bounded resources. Optional feature, plugin,
example, LSP, and cross-platform checks remain local.

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
   flake in `scheduler_cancellation`), run-alone heavies. Measured:
  historical 11,781 tests in ~7 min execution (vs ~10.6 min under serial
   `cargo test`); the sleep-bound heavies still take their wall-clock.
   Does not change `--test-threads` semantics of plain `cargo test`
   runs.

Do not generalize these: intra-binary `--test-threads` stays 1 — test
code mutates process-global env vars (audited, not assumed) — and
splitting CI into parallel jobs that each compile the workspace
duplicates the dominant cost instead of removing it.

### Build/link vs execution diagnostics (M001 recipe)

To separate compile/link wall time from test execution on any run:

1. **Hosted step times** — the Actions `Workspace tests` step spans
   both phases. The build phase ends at the Cargo line
   `Finished 'test' profile [unoptimized + debuginfo] target(s) in Xs`
   in the step log; everything after is Nextest execution.
2. **Nextest execution total** — the run-closing
   `Summary [Xs] N tests run` line is pure execution across all
   binaries (M001 baseline: 414 s for 11781 tests).
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

### CI/test throughput final state (M001–M005 closure, 2026-09-25)

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

### M005 — Compiler-result cache and same-job overlap

**Historical sccache disposition superseded by C001 measurement.**
M005 rejected a hosted sccache experiment based on cache-equivalence
and external-credential assumptions. Those assumptions were not
established by the cited cache documentation. C001 in
`plans/subsystems/ci-test-throughput-optimization-post-closure-corrective-addendum.md`
owns the measured disposition; see
`plans/closure/ci-test-throughput-optimization-post-closure-corrective/001-status.md`.
The current experiment uses the GitHub Actions cache backend and its
runtime credentials, while retaining `Swatinem/rust-cache` for Cargo
dependency/target reuse. No user-managed secret or external cache is
in scope. Until C001 closes, do not treat either retention or rejection
as a measured result.

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

The workstream retains one bounded non-release job and the
"Conservative keep, parallelized execution" disposition for
same-job overlap.

The compiler-cache configuration remains subject to C001's measured
retain/revert decision. Per the M001 diagnostic recipe,
`cargo --timings` remains an ephemeral local diagnostic only.

## Related Docs

- `AGENTS.md` — full test command catalog
- `.config/nextest.toml` — nextest profiles
- `scripts/audit_tokio_tests.py` — Tokio runtime flavor audit
