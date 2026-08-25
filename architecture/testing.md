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
6. `cargo check --workspace --all-targets --locked`

### `verify.sh full`

Runs quick first, then:

1. `cargo clippy --workspace --all-targets --locked -- -D warnings`
2. `cargo test --workspace --locked -- --test-threads=1`
3. `cargo check -p codegg --locked --features server,plugins,lsp-test-support`

Both modes set `CARGO_BUILD_JOBS=1` by default.

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

# Fast feedback (cheap crates)
cargo test -p egggit -p eggsentry -p codegg-config -p codegg-protocol

# Single crate
cargo test -p codegg-core

# Capped workspace validation
CARGO_BUILD_JOBS=1 cargo test --workspace --locked -- --test-threads=1

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

## Test Timing with Nextest

Optional diagnostic tooling configured in `.config/nextest.toml`.

| Profile | Timeout | Threads | Use Case |
|---------|---------|---------|----------|
| `default` | 30s | Auto | Local development |
| `timing` | 60s | Serial | Local timing diagnostics |

```bash
cargo install cargo-nextest
cargo nextest run --workspace --profile timing --all-features
scripts/capture-nextest-timing.sh --top 20
```

## CI Structure

Routine CI is one bounded `verify` job in `.github/workflows/ci.yml`
for PRs and pushes to `main`. Steps in order:

1. Generated-agent schema sync (`generate_builtin_agents.py --check`)
2. Core boundary guard (`check-core-boundary.sh`)
3. Sandbox contract guard (`check_sandbox_contract.py`)
4. Execution ownership guard (`check_execution_ownership.py`)
5. Formatting (`cargo fmt --check --all`)
6. Workspace Clippy (`cargo clippy --workspace --all-targets --locked`)
7. Workspace tests (`cargo test --workspace --locked -- --test-threads=1`)

CI uses default features, bounded resources. Optional feature, plugin,
example, LSP, audit, and cross-platform checks remain local.

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
cargo test --test projection_replay_daemon_protocol
cargo test --test projection_replay_subscription
cargo test --test projection_replay_resume
cargo test --test projection_disclosure_invariants
cargo test --test projection_artifact_handles
```

### CI Lane Roadmap Decision

**Conservative keep** — maintain the current single-job bounded test
lane. Splitting into resource lanes would add complexity without
measured need. If test count grows or wall-clock becomes problematic,
consider nextest adoption, resource-aware splitting, or selective
feature flags. These changes should be driven by measured regressions.

### Future Considerations

If test count grows significantly or wall-clock becomes problematic,
consider:

1. **Nextest adoption** — use `.config/nextest.toml` profiles for
   timing data and potential parallelism.
2. **Resource-aware splitting** — split into sequential lanes:
   fast/default → storage → process-heavy → plugin-heavy.
3. **Selective feature flags** — use `--features` instead of
   `--all-features` for targeted CI runs.

## Related Docs

- `AGENTS.md` — full test command catalog
- `.config/nextest.toml` — nextest profiles
- `scripts/audit_tokio_tests.py` — Tokio runtime flavor audit
