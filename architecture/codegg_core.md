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
`crates/codegg-core/build.rs` is a build script that reads the model-adapter
TOML assets and generates `model_adapters.rs` into `OUT_DIR`
(`cargo:rerun-if-changed` on the asset directory); it depends only on
`regex`, `serde`, and `toml`.

## Modules

55 `pub mod` declarations in `crates/codegg-core/src/lib.rs` (the file is
exactly those 55 lines — there are no `pub use` re-exports and no crate-level
attributes there):

| Module | Purpose |
|--------|---------|
| `agent_convergence` | Agent convergence orchestration and cycle management |
| `agent_run` | Agent run lifecycle and state |
| `agent_run_control` | Agent run control signals and cancellation |
| `agent_run_group` | Agent run group coordination |
| `agent_run_transcript` | Durable delegated-run transcript, keyed by `AgentRunId` (ADR-0051) |
| `approval` | Approval routing domain and durable runtime preferences (M003) |
| `audit` | Audit logging and event recording |
| `audit_instrumentation` | Audit instrumentation hooks |
| `authorization` | Authorization policy and operation descriptors |
| `bus` | Event bus (GlobalEventBus), PermissionRegistry, QuestionRegistry |
| `checkout_reservation` | Authoritative checkout reservation check (M005 C004) |
| `collaboration` | Multi-user collaboration presence and chat |
| `context` | Context packing, projection, and compaction utilities |
| `decision` | Bounded, backend-neutral decision semantics for adapters |
| `error` | Central error taxonomy (AppError, ToolError, etc.) |
| `git_publication` | Durable journal for private Git index publication (C007) |
| `goal` | Long-horizon goal runtime, budget enforcement |
| `identity` | Identity validation and parsing |
| `jobs` | Durable jobs, attempts, schedules, recovery, idempotency |
| `memory` | Persistent memory patterns |
| `migration` | Legacy project database migration (idempotent) |
| `model_profile` | Declarative adapter resolution, model profiles, task state policy |
| `model_routing` | Adapter into the neutral `eggpool-model-routing` semantic router |
| `presence` | Ephemeral project-scoped presence leases (M001, task-free, bounded) |
| `process_admission` | Authoritative process admission authority (M005 C005) |
| `project_catalog` | Project catalog management and invariants |
| `project_discovery` | Bounded project discovery logic |
| `project_discovery_service` | Project discovery service layer |
| `project_storage` | Project-scoped storage |
| `projection_replay` | Projection replay for context reconstruction |
| `protocol_conversions` | Core-safe DTO↔domain conversions |
| `provider_connections` | Provider connection lifecycle and tombstone compat |
| `repository_lineage` | Repository lineage tracking |
| `resilience` | Circuit breaker re-export |
| `run_placement` | Where a delegated run is placed; shared-checkout availability |
| `run_result` | Agent run result types and persistence |
| `run_store` | Run store for persisting agent run artifacts |
| `session` | Session storage, schema, checkpoint |
| `session_control` | Turn-scoped shared-session controller lease (ADR-0007) |
| `session_launch` | Durable, recoverable session-launch saga (M006) |
| `shared_checkout` | The managed checkout a shared agent-run group uses (M005 C006) |
| `snapshot` | File state capture and diff |
| `storage` | SQLite initialization, preferences, daemon catalog |
| `task_state` | Todo state management and projections |
| `team` | Team membership and role management |
| `tool_program` | Restricted-Python frontend for Tool Programs (IR, parser) |
| `transport_auth` | Transport-level authentication types |
| `work_order` | Durable work orders: model, store, coordinator, trigger |
| `work_plan` | Bounded work plans: model, store, checkpoint, assessment, projection |
| `workspace` | Workspace identity, registry, path policy, execution context |
| `workspace_services` | Per-workspace service bundle (RunStore, PathPolicy, etc.) |
| `workstream_claim` | Durable intent claims for the Agent Workstream Blackboard |
| `worktree` | Git worktree operations |
| `worktree_service` | Worktree service layer |
| `writer_permit` | Durable, generation-fenced checkout writer permits (M005 C002) |

## Dependencies

`codegg-core` depends on:

- `codegg-config` — configuration types and schema
- `codegg-git` — typed Git operation model, argv parser, risk
  classification
- `codegg-protocol` — protocol DTOs
- `codegg-providers` — provider types and circuit breaker
- `eggpool-model-routing` — neutral semantic model-routing compiler
  (git dep, rev-pinned in `Cargo.toml`; reached through `model_routing`)
- `egggit` — read-only git facts (status, diff, log, blame, refs)
- `egglsp` — LSP client/service/operations
- `eggsentry` — deterministic security scanning

Notable third-party: `tokio`, `sqlx` (SQLite), `serde`, `serde_json`,
`anyhow`, `url`, `chrono`, `uuid`, `sha2`, `similar`, `regex`,
`rustpython-parser` (Tool Programs), `dashmap`, `parking_lot`, `thiserror`,
`async-trait`, `tracing`, `unicode-normalization`, `dirs`, `rand`,
`base64`, `subtle`, `once_cell`, `tokio-util`. On Unix only: `libc`,
`rustix` (the `fs` feature).

`md5 = "0.8"` is the one **local** (non-workspace) third-party pin in this
manifest; it is retained only for legacy project-memory namespace
reads/migration.

`codegg-core` does NOT depend on:

- `axum`, `tower-http`, `tokio-tungstenite` (server/client)
- `ratatui`, `crossterm` (TUI)
- `wasmtime` (plugins)
- `eggcontext` (token counting — consumed by root only)
- `codegg-document` (text transaction core — consumed by root only)
- Root `codegg` crate
- Any `eggplan-*` crate (the work-plan repository binding is
  application-layer only)

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
`init_legacy_project_store(root)` retains backward compat. The current layout
version is defined by `storage::STORAGE_LAYOUT_VERSION` (currently **93**,
and `scripts/check_project_catalog_invariants.py` fails the build if it drifts
below the highest migration number in `session/schema.rs`). `DaemonPaths` is
the single source of truth for catalog and asset paths.

`storage::immediate` owns the `BEGIN IMMEDIATE` helper required by the C002
queue-write transaction-shape guard; `storage::preferences` owns the durable
preference store; `storage::paths` owns `DaemonPaths`.

### Writer Authority (M005)

Five core modules together are the checkout-writing authority, and they are
**not** the scheduler and **not** the worktree lease:

| Module | Authority |
|--------|-----------|
| `writer_permit` | Durable, generation-fenced "who may write this checkout *now*" permit |
| `checkout_reservation` | "Is this checkout reserved, and by whom?" for writers that arrive without a run |
| `process_admission` | Pre-effect admission for OS processes (which name no file) |
| `shared_checkout` | Which managed checkout a bounded agent-run group shares |
| `run_placement` | Where a delegated run is placed; shared profile availability (platform-gated) |

`session_launch` adds the durable, recoverable launch saga that spans a
Git worktree on disk and a session row in SQLite.

### Delegated-Run Transcript (ADR-0051 option B)

`agent_run_transcript` owns the durable conversation of a **delegated run**.
A delegated run is not a session: it has no `session` row, it never enters
session enumeration, and it is never compacted by the parent session's
policy. Its conversation therefore cannot live in the `message` table —
`message.session_id` is a foreign key with `ON DELETE CASCADE`, so writing
one would fail the constraint rather than create a transcript.

| Property | Where |
|----------|-------|
| The key | `AgentRunId` — the same durable identity `agent_run` already owns |
| The row | `(run_id, ordinal, role, kind, redacted_body, byte_len, created_at)` |
| The table | `agent_run_transcript`, created by migration **v89** |
| The aggregate | `agent_run_transcript_state` — next ordinal, stored bytes, truncation flag |
| Redaction | `redact_transcript_body`, at **write**, before the body is durable |
| The row bound | `MAX_TRANSCRIPT_ROW_BYTES` — 8 KiB, marked in the row |
| The run bound | `MAX_TRANSCRIPT_RUN_BYTES` — 256 KiB, marked by a truncation marker row |
| The read bound | `MAX_TRANSCRIPT_READ_LIMIT` — 256 rows; there is no unbounded read |

Three properties are load-bearing:

- **Redaction happens at write.** The recogniser is the existing
  `ProjectionFieldRedactor`, the same one the projection seam and the run's
  durable failure message use — there is no second secret recogniser. A
  read-time redaction would leave the secret durable.
- **The bounds are enforced at write.** A truncated transcript says so: a
  bounded row carries the in-row marker, and a bounded run carries a
  truncation-marker row instead of the body it refused.
- **The store is append-only and ordinal-stable.** A row is never rewritten,
  and a concurrent append for one run receives a distinct ordinal. The
  ordinal and the per-run byte total are read and advanced in one
  `BEGIN IMMEDIATE` transaction, so the bound check costs the same on the
  hundredth append as on the first.

The transcript is written by the child's own capture path, **while the child
runs**. `src/agent/transcript_capture.rs` owns the sink;
`src/agent/loop.rs` reports the semantic checkpoints it reaches, and
`src/agent/worker.rs` installs the sink on the child's loop before the run
starts. A transcript write failure is logged and the delegated work continues.

A tool result in the transcript is the **outcome the loop actually executed**,
not an event the provider chose to emit. The loop records one `ToolResult` row
at the point it delivers the result to the model, so a recorded result is one
the model actually received. The reason the result comes from the loop is that
no provider adapter emits a tool-result event: the child's event vector carries
the tool *calls*, and the tool *outputs* exist only where the tools ran.
Reading the outputs from the event stream would therefore record a transcript
whose tool results are empty in production.

A tool *call* is recorded when the loop accepts it — before any tool runs — so
a reader sees a call that is still pending rather than a transcript that only
appears once the child has finished. A call the child never executed stays a
call with no result; nothing fabricates an outcome for it.

The model's own words are recorded once per provider response, at the
response boundary — never one row per text delta, and never the provider's
private reasoning. A child that dies mid-turn keeps the observations it had
already made.

The retained output is the path-redacted text the loop published and the model
received, kept as a bounded **excerpt**: a head, the truncation marker, and a
tail. The tail matters — a tool's conclusion is at the end of its output, so a
head-only cut would discard exactly the line a caller reads the output for.
The capture path never writes into the child's event vector: that vector is
what the parent's turn transcript is built from, so a tool call with no
matching result must stay `Pending`.

### The M002 tail read (authorized page)

`agent_run_transcript` gained a **tail read** in M002: `read(run_id,
from_ordinal, limit)` is the store's own cursor-addressed page, and
`projection_replay::agent_run_transcript_page` is the adapter that turns one
store page into the wire page (`AgentRunTranscriptPageDto`).

Three server-side clamps are applied by the adapter, in this order, and each
is a separate bound:

| Clamp | Value | Where |
|-------|-------|-------|
| Cursor | non-negative | the adapter; a negative `from_ordinal` reads from zero |
| Rows | `MAX_PROJECTION_TRANSCRIPT_PAGE_ROWS` (64) | the adapter, before the store is asked |
| Bytes | `MAX_PROJECTION_TRANSCRIPT_PAGE_BYTES` (8 KiB) | the adapter, on the **already-redacted** bodies |

The byte ceiling is applied *after* the store's write-time redaction and
*before* serialization, so a page can never widen what the store already
withheld. The cut is always at a row boundary: the page's `next_ordinal` is
the first row it did **not** carry, and `has_more` is true whenever the store
holds rows beyond the page. A full last page therefore does not read as
exhausted history — the store's own `recording_stopped` is reported
separately, in `AgentRunTranscriptSourceState`.

The adapter refuses an over-bound page rather than shipping a short one:
`AgentRunTranscriptPageDto::checked` returns
`AgentRunTranscriptPageError::BoundExceeded`, which is a *different* fact from
a storage failure (`…::Storage`). A failed read says nothing about what is in
the store; it never renders as "absent".

### The M002 advisory (body-free)

The **advisory** is how a delegated run's transcript growth is signalled. It
is not the transcript: it carries the run id, the committed high-water
ordinal, and whether the run's recording stopped. No text, no tool arguments,
no reasoning, no credentials.

Its rules are:

- **Post-commit.** The capture sink calls the publisher only after the
  store's append has returned, so a hint can never claim a row the store
  does not hold. The transcript's write transaction is never held across the
  publication.
- **Per run, not per child task.** The watermark is per `AgentRunId`, so a
  continuation that appends to the same run id advances the same watermark
  rather than starting a new one. The canonical storage ordinal dominates:
  a duplicate or out-of-order advisory never moves it backwards.
- **Coalesced.** `src/agent/transcript_publisher.rs` collapses a run's
  appends under a row cap (`ADVANCE_ROW_CAP`) and an interval
  (`ADVANCE_INTERVAL`), and publishes immediately when the run's recording
  stopped — that is the run's last word, and holding it back would lose it.
- **Injected, never global.** The delegated child's runtime holds an
  `Arc<dyn TranscriptAdvancePublisher>`. The daemon installs it on the
  subagent pool it owns; there is no process-wide registry, no TUI
  dependency, and no direct call into the projection replay handle. The
  publisher reaches the daemon's canonical `EventLog`, which is the only
  producer.

## Configuration Surface

None — `codegg-core` is a library crate. Configuration is provided by
callers via `codegg-config::schema::Config`.

## Invariants & Gotchas

- Unsafe code is denied for this crate, but **not** by a crate-level
  attribute: `crates/codegg-core/Cargo.toml` carries `[lints] workspace = true`
  and the root `Cargo.toml` sets `[workspace.lints.rust] unsafe_code = "deny"`.
  The root `codegg` package deliberately does **not** inherit that table
  (`src/bin/codegg-sandbox-helper.rs` has reviewed `unsafe`); it keeps
  `#![deny(unsafe_code)]` in `src/lib.rs` instead. Do not add a duplicate
  `#![deny(unsafe_code)]` to `codegg-core/src/lib.rs` — the crate root is
  exactly the 54 `pub mod` lines.
- `codegg-core` must NOT gain UI, server, plugin, or auth dependencies.
  Run `scripts/check-core-boundary.sh` after changes.
- The `tool_program` module uses `rustpython-parser` for AST parsing.
  This is a heavy dependency but is compile-only (no runtime Python).
- HTTP transport clients are not owned by `codegg-core`; URL parsing uses the
  transport-neutral `url` crate and provider transport remains outside core.
- `md5` is retained only for legacy project-memory namespace
  reads/migration. New writes use SHA-256 via `sha2`.

## Testing

```bash
cargo test -p codegg-core                    # all core tests
bash scripts/check-core-boundary.sh         # boundary enforcement
```

Narrowest: `cargo test -p codegg-core -- <module>` for a specific
module. Key test modules:
- `model_profile::adapter::tests` — adapter matching
- `task_state::tests` — todo state machine
- `workspace` tests — registry, path policy
- `jobs` tests — state machines, recovery
- `tool_program` tests — parser, IR compilation

## Related Docs

- `architecture/presence.md` — presence lease domain (M001)
- `architecture/model_profile_task_state.md` — model profiles and task
  state deep dive
- `architecture/native_crates.md` — library-first tool architecture
- `architecture/testing.md` — test resource taxonomy
- `architecture/writer_permit.md` — checkout writer permit
- `architecture/session_launch.md` — session launch saga

## Source verification

Verified 2026-10-23 against `crates/codegg-core/src/lib.rs` (54 `pub mod`
lines; the module table above was diffed against it name-for-name and the
count corrected from 45 to 54 — `checkout_reservation`, `decision`,
`git_publication`, `process_admission`, `run_placement`, `session_launch`,
`shared_checkout`, `workstream_claim`, and `writer_permit` were missing),
`crates/codegg-core/Cargo.toml` (dependency and `[lints]` tables),
`crates/codegg-core/build.rs`, the root `Cargo.toml` (workspace members,
`[workspace.lints.rust]`, the root package's deliberate non-inheritance of
the lints table), `crates/codegg-core/src/error.rs` (all 11 error enums
present), root `src/error.rs` (`AxumAppError`, `AxumServerRuntimeError`),
root `src/protocol_conversions.rs` (agent-side conversions + core re-export),
`scripts/check-core-boundary.sh` (the forbidden-import regex and the
forbidden-dependency list both match what this doc claims), and
`crates/codegg-core/src/storage/mod.rs`
(`STORAGE_LAYOUT_VERSION = 87`, `init_daemon_catalog`,
`init_legacy_project_store`).

Two corrections made in this pass: the module count (45 → 54) and the
unsafe-code story — `codegg-core` denies unsafe through
`[lints] workspace = true`, **not** through a crate-level
`#![deny(unsafe_code)]`; `crates/codegg-core/src/lib.rs` contains nothing but
the 54 `pub mod` lines.
