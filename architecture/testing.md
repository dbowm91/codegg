# Testing Architecture

Codegg's test suite contains ~1,219 async tests across 94 files with wildly different resource profiles. Unbounded parallelism has been observed to spawn 50-70 threads plus many subprocesses, with some processes consuming 1-2 GiB of memory. The default CI command intentionally serializes execution:

```bash
cargo test --workspace --all-features -- --test-threads=1
```

This document defines the test taxonomy, resource model, and guidance for adding new tests.

## Test Resource Classes

| Class | Description | Parallelism | Examples |
|-------|-------------|-------------|----------|
| `fast` | Pure/unit tests, in-memory registries, parsing, config logic | Safe with `current_thread` | `egggit::diff`, `eggsentry::profile`, `plugin::registry`, `command::mod` |
| `storage` | SQLite pool ops, session CRUD, snapshot capture/restore | Serial or low parallelism | `tests/session_crud.rs`, `tests/snapshot.rs`, `goal::store` |
| `process-heavy` | Fake LSP stdio, supervisor restart, daemon sockets, subprocess spawning | Serial (`--test-threads=1`) | `tests/lsp.rs`, `tests/lsp_composite_stdio.rs`, `tests/supervisor_restart_stdio.rs` |
| `plugin-heavy` | Wasmtime runtime, plugin install/registry/management | Serial | `tests/plugin.rs`, `src/plugin/management.rs` |
| `real-lsp` | Actual language server smoke tests (rust-analyzer, pyright, gopls) | Manual/scheduled only | `crates/egglsp/tests/real_server_smoke.rs` |
| `release-full` | Conservative full validation for main/tags | Serial | `cargo test --workspace --all-features -- --test-threads=1` |

## Why Serial by Default

The workspace mixes cheap pure-logic tests with heavyweight subprocess-spawning tests. Key amplification factors:

- **LSP tests** spawn fake language-server subprocesses per test, create temp Rust workspaces, write scenario files, and exercise async shutdown/restart. A single test binary (`tests/lsp.rs`) has 84 tests that each spawn a subprocess.
- **Plugin tests** may instantiate Wasmtime runtime state.
- **Tokio default flavor** is multi-threaded. 1,219 bare `#[tokio::test]` attributes each create a multi-threaded runtime with default worker threads. Converting to `current_thread` eliminates this overhead for tests that don't need concurrent workers.
- **SQLite migration churn** — `isolated_pool()` runs full migrations on every call. Some test files add redundant `migrate()` calls on top.

## Tokio Runtime Flavor Rules

### Default: `current_thread`

```rust
#[tokio::test(flavor = "current_thread")]
async fn test_something() {
    // ...
}
```

Use `current_thread` unless the test explicitly requires concurrent worker threads. This is the default for:
- Pure unit tests and parsing
- SQLite pool operations (in-memory)
- In-memory registry tests (PluginRegistry, PermissionRegistry, etc.)
- Mock provider tests
- Shell projection fixture tests

### Multi-threaded (explicit)

```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_concurrent_access() {
    // ...
}
```

Use multi-threaded only when the test:
- Spawns background tasks via `tokio::spawn` that must execute concurrently
- Uses `tokio::sync::broadcast` or `tokio::sync::mpsc` with real concurrent producers/consumers
- Tests actual subprocess lifecycle (LSP, daemon, shell)
- Uses `tokio::time::sleep` for timing-dependent behavior

### Always serial (`--test-threads=1`)

LSP subprocess tests, plugin heavy tests, and real-server tests must run serially regardless of runtime flavor because they compete for:
- Fixed ports or sockets
- Global process state
- Limited system resources (memory, file descriptors)

## Pool Strategy

### `isolated_pool()` — Fresh DB per test

```rust
let pool = common::pool::isolated_pool().await;
```

Creates a named in-memory SQLite DB (`codegg_test_iso_{uuid}`) and runs full migrations. Use when tests need a clean slate with hardcoded IDs that would collide in a shared pool.

**Warning:** Do NOT add redundant `migrate()` calls after `isolated_pool()` — migrations run internally via `build_pool()`.

### `shared_pool()` — Process-wide shared DB

```rust
let pool = common::pool::shared_pool().await;
```

Process-wide shared in-memory DB (`?cache=shared`). Migrations run once via `OnceLock`. Use when tests can tolerate other tests' data or clean up after themselves.

### Choosing a pool

| Scenario | Pool | Reason |
|----------|------|--------|
| Tests use hardcoded IDs (`"test-session"`) | `isolated_pool()` | Avoids cross-test collision |
| Tests clean up their own data | `shared_pool()` | Avoids redundant migration overhead |
| Tests need exact DB state | `isolated_pool()` | Clean slate guaranteed |
| High test count, simple ops | `shared_pool()` | Faster — no per-test migration cost |

## Adding New Tests

1. **Choose the lightest runtime flavor** that works. Start with `current_thread`.
2. **Use `isolated_pool()`** for storage tests unless you can guarantee cleanup.
3. **Never add redundant `migrate()` calls** after `isolated_pool()` or `shared_pool()`.
4. **Don't spawn real language servers** in default tests. Use fake transports.
5. **Don't use fixed ports, global paths, or shared env vars** without serializing.
6. **Prefer deterministic fakes** over subprocesses when process lifecycle isn't under test.
7. **Keep timeouts as failure bounds only** — don't use `sleep` as synchronization.
8. **For multi-threaded tests**, set explicit `worker_threads = 2` rather than using the default.

## Local Commands

```bash
# Fast feedback (cheap tests only, low parallelism)
cargo test -p egggit -p eggsentry -p codegg-config -p codegg-protocol

# Single crate
cargo test -p codegg-core

# Full serial validation (conservative, CI baseline)
CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=1

# LSP integration (fake server, serial)
cargo test -p egglsp --features lsp-test-support --test scenario_engine
cargo test --features lsp-test-support --test lsp_composite_stdio

# Plugin tests (serial)
cargo test -p codegg --lib plugin --all-features

# Real LSP smoke tests (manual, requires installed servers)
cargo test -p egglsp --features lsp-real-server-tests --test real_server_smoke -- rust_analyzer
```

## Test Timing with Nextest

Nextest is configured in `.config/nextest.toml` with profiles for different workloads. Use it to identify slow tests and capture baseline timing data.

### Profiles

| Profile | Timeout | Threads | Use Case |
|---------|---------|---------|----------|
| `default` | 30s | Auto | Local development |
| `ci-fast` | 20s | Auto | Quick CI validation |
| `ci-heavy` | 60s | Serial | Full workspace timing |
| `ci-release` | 120s | Serial | Release validation |

### Local Timing Commands

```bash
# Install nextest (one-time)
cargo install cargo-nextest

# Run full workspace with timing
cargo nextest run --workspace --profile ci-heavy --all-features

# Run specific crate with timing
cargo nextest run -p codegg-core --profile ci-heavy

# Generate timing report (slowest tests first)
cargo nextest run --workspace --profile ci-heavy --all-features --json | \
  python3 -c "import sys,json; data=json.load(sys.stdin); tests=data.get('test',{}).get('executed',[]); tests.sort(key=lambda t: t.get('time',{}).get('duration',0), reverse=True); [print(f\"{t['time']['duration']:.2f}s  {t['name']}\") for t in tests[:20]]"

# Find tests exceeding timeout threshold
cargo nextest run --workspace --profile ci-heavy --all-features 2>&1 | grep -E " TIMEOUT|slow"
```

### Baseline Metrics

To capture baseline timing data for comparison:

```bash
# Full workspace timing (single run)
cargo nextest run --workspace --profile ci-heavy --all-features 2>&1 | \
  grep -E "^test result:|slow test" > /tmp/nextest-baseline.txt

# Compare against future runs
diff /tmp/nextest-baseline.txt <(cargo nextest run --workspace --profile ci-heavy --all-features 2>&1 | grep -E "^test result:|slow test")
```

## CI Structure

The CI pipeline runs jobs in sequence: `agent-assets` → `fmt` → `check` → `clippy` → `test` → `plugin-focused` → `examples`.

### `test` job

Runs the full serial workspace suite:

```bash
cargo test --workspace --all-features -- --test-threads=1
```

This is the primary validation gate. All test resource classes are covered here.

### `plugin-focused` job

Runs targeted plugin tests with explicit serial flags (`-- --test-threads=1`). **This job is duplicate-by-design**: the `test` job already runs `--workspace --all-features` which includes plugin tests. The `plugin-focused` job exists for:

1. **Focused diagnostic logs** — easier to identify plugin-specific failures in CI logs
2. **Core boundary validation** — runs `scripts/check-core-boundary.sh` to enforce the codegg-core isolation contract
3. **Defense-in-depth** — if the broad test lane is narrowed in the future, plugin coverage is already isolated

All plugin test commands run serially because plugin paths may instantiate Wasmtime runtime state.

### Real LSP tests

Real-server compatibility tests live in a separate workflow (`lsp-real-server.yml`) and are **not part of routine PR validation**. The workflow triggers on:

- **Manual dispatch** (`workflow_dispatch`)
- **Weekly schedule** — Monday 6am UTC
- **Push to `main`** touching `crates/egglsp/**`, `src/lsp/**`, or `.github/workflows/lsp-real-server.yml`

Real-server tests must not be pulled into routine PR validation. They verify compatibility with actual language server binaries (rust-analyzer, basedpyright, gopls, typescript-language-server, clangd) and require installed server binaries.

### `--all-features` and real-server tests

The root `--all-features` flag enables the `lsp-real-server-tests` feature, which compiles `crates/egglsp/tests/real_server_smoke.rs`. This is safe because:

1. **Tests skip at runtime** — each test calls `require_server_binary()` which returns `None` when the server binary is not installed, causing the test to skip with `eprintln!("SKIP: ...")`
2. **No subprocess launched** — skipped tests never spawn a language server process
3. **CI does not install real servers** — the default `test` job does not set `CODEGG_RA_BIN` or similar env vars

The separate `lsp-real-server.yml` workflow explicitly installs server binaries and runs with `--features lsp-real-server-tests` to exercise the real compatibility path.

See `AGENTS.md` for the full test command catalog.

## CI Lane Roadmap Decision

**Decision: Conservative keep** — maintain the current monolithic serial test lane.

The closure pass evaluation determined that splitting the CI into resource lanes (fast, storage, process-heavy, plugin-heavy) would add complexity without sufficient benefit at the current test count (~1,219 tests). The conservative approach is retained because:

1. **Documentation is sufficient** — the test resource taxonomy, process-heavy file headers, and audit scripts provide visibility into resource usage without CI complexity.
2. **Regression guards are in place** — `check-tokio-test-flavors.py` prevents new bare tokio tests, and `audit_tokio_tests.py` identifies concurrency-sensitive tests.
3. **Serial execution is reliable** — `--test-threads=1` eliminates resource contention issues entirely.
4. **Wall-clock is acceptable** — the full serial suite completes within CI timeout limits.

### Future Considerations

If test count grows significantly or wall-clock becomes problematic, consider:

1. **Nextest adoption** — use `.config/nextest.toml` profiles for timing data and potential parallelism.
2. **Resource-aware splitting** — split into sequential lanes: fast/default → storage → process-heavy → plugin-heavy.
3. **Selective feature flags** — use `--features` instead of `--all-features` for targeted CI runs.

These changes should be driven by measured regressions, not speculative optimization.
