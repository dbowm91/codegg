# codegg-core Crate

## Purpose

`codegg-core` is a workspace crate containing the core runtime, session
management, storage, event bus, domain state, common error types, and
domain-specific subsystems (jobs, workspace, tool programs, project
discovery). It is designed to be a low-coupling foundation that root
`codegg` builds upon. The boundary is enforced by
`scripts/check-core-boundary.sh`.

## Where It Lives

`crates/codegg-core/` — workspace member, `Cargo.toml` at root.

## Modules

| Module | Purpose |
|--------|---------|
| `bus` | Event bus (GlobalEventBus), PermissionRegistry, QuestionRegistry |
| `context` | Context packing, projection, and compaction utilities |
| `error` | Central error taxonomy (AppError, ToolError, etc.) |
| `goal` | Long-horizon goal runtime, budget enforcement |
| `identity` | Identity validation and parsing |
| `jobs` | Durable jobs, attempts, schedules, recovery, idempotency |
| `memory` | Persistent memory patterns |
| `migration` | Legacy project database migration (idempotent) |
| `model_profile` | Declarative adapter resolution, model profiles, task state policy |
| `project_catalog` | Project catalog management and invariants |
| `project_discovery` | Bounded project discovery logic |
| `project_discovery_service` | Project discovery service layer |
| `project_storage` | Project-scoped storage |
| `projection_replay` | Projection replay for context reconstruction |
| `protocol_conversions` | Core-safe DTO↔domain conversions |
| `provider_connections` | Provider connection lifecycle and tombstone compat |
| `repository_lineage` | Repository lineage tracking |
| `resilience` | Circuit breaker re-export |
| `run_store` | Run store for persisting agent run artifacts |
| `session` | Session storage, schema, checkpoint |
| `snapshot` | File state capture and diff |
| `storage` | SQLite initialization, preferences, daemon catalog |
| `task_state` | Todo state management and projections |
| `tool_program` | Restricted-Python frontend for Tool Programs (IR, parser) |
| `workspace` | Workspace identity, registry, path policy, execution context |
| `workspace_services` | Per-workspace service bundle (RunStore, PathPolicy, etc.) |
| `worktree` | Git worktree operations |

## Dependencies

`codegg-core` depends on:

- `codegg-config` — configuration types and schema
- `codegg-git` — typed Git operation model, argv parser, risk
  classification
- `codegg-protocol` — protocol DTOs
- `codegg-providers` — provider types and circuit breaker
- `egggit` — read-only git facts (status, diff, log, blame, refs)
- `egglsp` — LSP client/service/operations
- `eggsentry` — deterministic security scanning

Notable third-party: `tokio`, `sqlx` (SQLite), `serde`, `serde_json`,
`anyhow`, `reqwest`, `chrono`, `uuid`, `sha2`, `similar`, `regex`,
`rustpython-parser` (Tool Programs), `dashmap`, `parking_lot`.

`codegg-core` does NOT depend on:

- `axum`, `tower-http`, `tokio-tungstenite` (server/client)
- `ratatui`, `crossterm` (TUI)
- `wasmtime` (plugins)
- `eggcontext` (token counting — consumed by root only)
- Root `codegg` crate

## Boundary Enforcement

`scripts/check-core-boundary.sh` scans `crates/codegg-core/src/` for
forbidden imports and `crates/codegg-core/Cargo.toml` for forbidden
dependencies.

**Forbidden imports** (root-domain modules):
`agent`, `tool` (except `tool_program`), `permission`, `mcp`, `plugin`,
`tui`, `server`, `client`, `auth`, `crypto`, `search`,
`search_backend`, `research`, `theme`, `tts`, `upgrade`.

**Forbidden dependencies**:
`ratatui`, `crossterm`, `ratatui_textarea`, `axum`, `tower_http`,
`tokio_tungstenite`, `wasmtime`, `wasmtime_wasi`.

Note: `tool_program` is an allowed module (it is a pure-language
compiler with no UI/server deps). The boundary regex uses `tool[^_]` to
exclude it.

## Error Module Split

The error module is split between `codegg-core` and root:

- `codegg-core/src/error.rs` — all error enums and their `From` impls
  (AppError, ConfigError, ToolError, AgentError, PermissionError,
  McpError, PluginError, LspError, ServerRuntimeError, ClientError,
  RunStoreError)
- Root `src/error.rs` — re-exports from codegg-core +
  `AxumAppError`/`AxumServerRuntimeError` wrappers with `IntoResponse`
  impls (behind `#[cfg(feature = "server")]`)

This split is necessary because axum is not a dependency of codegg-core.

## protocol_conversions Split

- `codegg-core/src/protocol_conversions.rs` — session, message,
  provider, config type conversions
- Root `src/protocol_conversions.rs` — agent-specific conversions +
  re-exports core conversions

Agent types are not in codegg-core, so agent conversions must stay
root-side.

## Key Subsystems

### Workspace (Phase 2+)

`WorkspaceId` is a typed opaque string newtype. `WorkspaceRegistry`
deduplicates canonical project roots, rejecting symlink/relative
aliases. `ExecutionContext` is passed by `Arc` through every
daemon-owned execution path so commands, tools, and subagents never
infer their working directory from process-global cwd.

### Jobs (Phase 4)

Typed IDs (`JobId`, `AttemptId`, `ScheduleId`, `DependencyId`,
`DaemonGeneration`) — opaque UUID strings. `JobState` and `AttemptState`
machines enforce terminal-never-regress. `JobStore` and `ScheduleStore`
traits live here (UI/server/auth-free).

### Tool Programs (M004–M020)

Restricted-Python frontend: parse → normalized AST → validate → static
bounds → compile IR → verify IR. The pipeline is parse-only; it never
executes source or loads modules. IR is deterministic, versioned,
verified, and content-addressed.

### Storage (Phase 3)

`init_daemon_catalog(&DaemonPaths)` owns the user-scoped catalog.
`init_legacy_project_store(root)` retains backward compat. `STORAGE_LAYOUT_VERSION = 38`.
`DaemonPaths` is the single source of truth for catalog and asset paths.

## Configuration Surface

None — `codegg-core` is a library crate. Configuration is provided by
callers via `codegg-config::schema::Config`.

## Invariants & Gotchas

- `#![deny(unsafe_code)]` at crate root — no unsafe allowed.
- `codegg-core` must NOT gain UI, server, plugin, or auth dependencies.
  Run `scripts/check-core-boundary.sh` after changes.
- The `tool_program` module uses `rustpython-parser` for AST parsing.
  This is a heavy dependency but is compile-only (no runtime Python).
- `reqwest` is a dependency (used by provider connections). It does not
  bring in server frameworks.
- `md5` is retained only for legacy project-memory namespace
  reads/migration. New writes use SHA-256 via `sha2`.

## Testing

```bash
cargo test -p codegg-core                    # all core tests
python3 scripts/check-core-boundary.sh       # boundary enforcement
```

Narrowest: `cargo test -p codegg-core -- <module>` for a specific
module. Key test modules:
- `model_profile::adapter::tests` — adapter matching
- `task_state::tests` — todo state machine
- `workspace` tests — registry, path policy
- `jobs` tests — state machines, recovery
- `tool_program` tests — parser, IR compilation

## Related Docs

- `architecture/model_profile_task_state.md` — model profiles and task
  state deep dive
- `architecture/native_crates.md` — library-first tool architecture
- `architecture/testing.md` — test resource taxonomy
