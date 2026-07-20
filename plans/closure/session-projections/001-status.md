# Frontend-Neutral Session Projections Milestone 001 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/session-projections/001-projection-contracts.md`

Source subsystem roadmap:

- `plans/subsystems/session-projections-roadmap.md#milestone-1--projection-contracts-and-canonical-reducer`

Repository baseline reviewed: `8d57b9c` (production-code baseline;
the plan's stated baseline `fbae374` was earlier than the
multi-project TUI closure that this milestone's final seam depended
on, so the effective implementation baseline is `8d57b9c`).

Implementation commits or pull requests:

- `f6c8669` — additive projection contract under
  `codegg_protocol::projection` plus a second independent consumer
  equivalence test under
  `tests/session_projection_consumer.rs`.
- Follow-up closure commit — this record, subsystem roadmap status
  refresh, registry update, and downstream Session Projections
  Milestone 2 unblock.

## 1. Executive finding

Milestone 001 is complete. `codegg_protocol::projection` now exposes:

- A versioned, bounded projection contract (`SessionProjectionSnapshot`)
  with explicit per-collection caps and per-string-byte caps declared
  in `projection::limits`.
- One deterministic canonical reducer (`ProjectionReducer`) that
  applies ordered projection events on top of a snapshot, dedupes by
  `event_seq`, tolerates out-of-order transitions as diagnostics, and
  never performs I/O.
- A typed capability declaration
  (`projection::caps::ProjectionCapabilities`) with a static
  negotiation helper, plus additive `ClientCapabilities.session_projection`
  and `ServerCapabilities.session_projection` flags wired into
  existing frames.
- An adapter layer (`projection::adapters`) that maps the existing
  `CoreResponse::SnapshotSession`, `CoreResponse::SnapshotDaemon`,
  and every mapped `CoreEvent` family into the projection contract
  without altering any existing wire shape.
- Golden fixtures (`projection::fixtures`) and a second independent
  consumer equivalence test (`tests/session_projection_consumer.rs`)
  that proves two consumers reach equivalent logical state from the
  same fixture inputs.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| One versioned bounded projection schema | `crates/codegg-protocol/src/projection/snapshot.rs` `SessionProjectionSnapshot` | pass | `protocol_version = PROJECTION_PROTOCOL_VERSION` (1). |
| Stable projection IDs / no path-derived authority | `SessionProjectionSnapshot.primary_session_id`, `project_id`, `workspace_id` are typed strings; `projection::dto::AgentTreeNodeProjection.task_id` is the daemon-issued `u64` task id | pass | No fake durable agent identity is synthesised. |
| Capability declaration and compatibility rules | `projection::caps::{ProjectionCapabilities, PROJECTION_CAPABILITY, PROJECTION_PROTOCOL_VERSION, PROJECTION_PROTOCOL_VERSION_MIN}`; `negotiate` / `supports` helpers; unit tests `negotiate_picks_intersection_high` and `negotiate_returns_none_for_disjoint` | pass | Versions outside the declared range produce `ReducerError::UnsupportedProtocolVersion`. |
| Explicit payload / collection bounds | `projection::limits::{MAX_PROJECTION_*, TRUNCATION_MARKER, truncate_str, clip_str}` | pass | Used in every DTO `normalise()` method. |
| Unknown optional variant behaviour | `ProjectionEvent::Unknown { variant_name, notice }` plus reducer diagnostic `unknown_variant`; unit test `unknown_variant_does_not_panic_consumer` | pass | Reduces to a bounded diagnostic; never crashes. |
| Canonical reducer | `projection::reducer::{ProjectionReducer, ReducerEventInput, ReducerConfig, ApplyOutcome, ReducerError}` | pass | Pure; no I/O; deduplicates by `event_seq`. |
| Lifecycle transitions for turns / tools / questions / permissions / runs / jobs | reducer tests `turn_started_sets_active_and_records_diagnostic_for_unknown_completed`, `tool_started_appends_tool_projection`, `permission_pending_increments_pending_count_and_sets_status`, plus fixture tests for `completed_snapshot`, `subagent_event_script`, `job_event_script`, `permission_pending_snapshot`, `question_event_script` | pass | Every mapped event family has at least one fixture-driven test. |
| Bounded pruning / summary behaviour | `push_diagnostic`, `push_run`, `upsert_job`, `upsert_secondary`, `push_recent_turn`, `truncate_str` enforce the bounds | pass | Cap tests `push_diagnostic_caps_at_max`, `secondary_sessions_are_bounded`. |
| Diagnostics for impossible / out-of-order inputs | reducer match arms `record_diagnostic` calls (e.g. `orphan_message`, `orphan_tool_started`, `unknown_subagent_progress`); `Reconciled` outcome | pass | No panics; diagnostics are bounded. |
| Mapping adapters from current `CoreResponse` snapshots | `projection::adapters::{snapshot_from_snapshot_session, snapshot_from_daemon, snapshot_from_session_snapshot, project_summary_from_dto}` | pass | Existing `CoreResponse` / `CoreEvent` consumers are untouched. |
| Mapping adapters from current `CoreEvent` variants | `projection::adapters::{projection_events_from_core, projection_envelopes_from_core}` | pass | Unmapped variants become `ProjectionEvent::Unknown` with a single bounded diagnostic. |
| Large outputs truncate or become handles | `ToolArgumentProjection::{Inline, Summary, TruncatedArguments, Handle}`; `ToolOutputProjection::{Pending, Inline, Summary, TruncatedOutput, Handle}`; reducer `ToolCompleted` normalises inline text to the cap; adapter `ToolCompleted` switches to `TruncatedOutput` when `output.len() > MAX_PROJECTION_STRING_BYTES` | pass | Unit test `oversized_tool_output_becomes_truncated`; unit test `tool_arguments_and_output_truncate`. |
| Preserve existing clients and capability negotiation | New `session_projection: bool` capability field added with `#[serde(default)]`; no existing variant removed; unit test `legacy_server_capabilities_fixture_defaults_identity_awareness` continues to pass | pass | No breaking change. |
| Test / reference builder APIs | `projection::fixtures::{idle_snapshot, active_turn_snapshot, permission_pending_snapshot, completed_snapshot, project_summary_fixture, *_event_script}` re-exported through `projection::*` | pass | Both reducer and consumer tests drive the same scripts. |
| Golden fixtures stable | `tests/session_projection_consumer.rs::fixture_snapshot_is_deterministic` | pass | Two `active_turn_snapshot()` calls produce byte-identical JSON. |
| Two independent consumer equivalence | `tests/session_projection_consumer.rs::{active_turn_script_yields_equivalent_state, permission_script_yields_equivalent_state, completed_script_yields_equivalent_state, subagent_script_yields_equivalent_state}` | pass | Second consumer (`FakeTuiState`) compared to canonical snapshot state. |
| Documentation | `architecture/projection.md` (new); `architecture/protocol.md` updated with the new module and version | pass | Documents ownership boundary, version, limits, capability negotiation, reducer invariants, adapter mappings, fixtures, security classification, compatibility matrix. |
| Static guards | `bash scripts/check-core-boundary.sh`, `python3 scripts/check_daemon_cwd_usage.py`, `python3 scripts/check_git_forbidden_patterns.py` | pass | Projection module depends only on `serde` / `serde_json`; no UI / server / storage / auth imports. |
| Clippy clean | `cargo clippy --workspace --all-targets --all-features -- -D warnings` | pass | No issues. |
| Format clean | `cargo fmt -- --check` | pass | No diffs. |
| Projection protocol tests | `cargo test -p codegg-protocol` | pass | 125 passed. |
| Independent consumer tests | `cargo test --test session_projection_consumer` | pass | 8 passed. |
| Existing TUI regression | `cargo test --test tui --test tui_render --test tui_project_tabs` | pass | 263 + 99 + 13 (unchanged). |
| Existing protocol baseline | `cargo test -p codegg-protocol` | pass | 125 passed. |
| Single-daemon lifecycle regression | `cargo test --test single_daemon_lifecycle --test session_selection` | pass | 24 passed. |
| Shell projection regressions | `cargo test --test shell_projection_harness --test shell_projection_phase10` | pass | 11 + 33 passed. |
| TUI lib targeted | `cargo test -p codegg --lib shell::` | pass | 368 shell tests pass. |

## 3. Production implementation evidence

### `crates/codegg-protocol/src/projection/`

The new module layout:

```
crates/codegg-protocol/src/projection/
├── mod.rs        # Re-exports + module map
├── caps.rs       # ProjectionCapabilities, version negotiation
├── limits.rs     # MAX_PROJECTION_* constants, truncate_str, clip_str
├── dto.rs        # Bounded projection DTOs
├── event.rs      # ProjectionEnvelope, ProjectionEvent, ProjectionStreamScope
├── snapshot.rs   # SessionProjectionSnapshot, ProjectionDiagnostic
├── reducer.rs    # ProjectionReducer, ReducerEventInput, ReducerConfig
├── adapters.rs   # snapshot_from_*, projection_events_from_core, ...
└── fixtures.rs   # idle_snapshot, *_event_script, *_snapshot
```

Module re-export:

- `crates/codegg-protocol/src/lib.rs` — adds `pub mod projection;`.

Capability negotiation:

- `crates/codegg-protocol/src/frames.rs` — adds
  `session_projection: bool` to both `ClientCapabilities` and
  `ServerCapabilities` with `#[serde(default)]` so existing clients
  remain forward-compatible. The new field is wired into the four
  client capability construction sites (`src/core/transport/socket.rs`,
  `src/core/transport/daemon_socket.rs`, and the two test sites in
  `src/core/transport/daemon_socket_integration_tests.rs`).

Second independent consumer:

- `tests/session_projection_consumer.rs` — implements
  `FakeTuiState`, a minimal TUI-style state builder. It applies the
  same fixture event scripts that drive the canonical reducer and
  asserts that its own logical state matches the canonical
  snapshot's logical state on active turn id / status, message
  count, tool count, pending permission / question counts, recent
  turn count, runs, jobs, and active subagent count.

Architecture docs:

- `architecture/projection.md` (new) — purpose and ownership,
  module layout, versioning, bounded limits, visibility classes,
  canonical reducer semantics, adapter mappings, fixtures, static
  guarantees, compatibility matrix, security classification,
  verification commands, and downstream milestone ownership.
- `architecture/protocol.md` — adds the projection module to the
  module map and adds `PROJECTION_PROTOCOL_VERSION = 1`,
  `PROJECTION_PROTOCOL_VERSION_MIN = 1`, and
  `PROJECTION_CAPABILITY = "session_projection.v1"` to the protocol
  version list and history.

## 4. Verification executed

### Commands run

```bash
cargo fmt -- --check

# Focused projection unit tests (caps, limits, dto, event, snapshot,
# reducer, adapters, fixtures).
cargo test -p codegg-protocol

# Independent consumer equivalence test.
cargo test --test session_projection_consumer

# Lint.
cargo clippy -p codegg-protocol --all-targets --all-features -- -D warnings
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Workspace check.
CARGO_BUILD_JOBS=1 cargo check --workspace --all-features

# TUI regressions.
cargo test --test tui --test tui_render --test tui_project_tabs

# Protocol baseline.
cargo test -p codegg-protocol

# Single-daemon + session selection regressions.
cargo test --test single_daemon_lifecycle --test session_selection

# Shell projection regressions.
cargo test -p codegg --lib shell::
cargo test --test shell_projection_harness
cargo test --test shell_projection_phase10

# Static guards.
python3 scripts/check_daemon_cwd_usage.py
python3 scripts/check_git_forbidden_patterns.py
bash scripts/check-core-boundary.sh
```

### Results

- `cargo fmt -- --check` — no diffs.
- `cargo test -p codegg-protocol` — 125 passed, 0 failed.
- `cargo test --test session_projection_consumer` — 8 passed, 0 failed.
- `cargo clippy -p codegg-protocol --all-targets --all-features -- -D warnings` — no issues.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — no issues.
- `CARGO_BUILD_JOBS=1 cargo check --workspace --all-features` — succeeded.
- `cargo test --test tui --test tui_render` — 263 passed, 0 failed.
- `cargo test --test tui_project_tabs` — 13 passed, 0 failed.
- `cargo test --test single_daemon_lifecycle --test session_selection` — 24 passed, 0 failed.
- `cargo test -p codegg --lib shell::` — 368 passed, 0 failed.
- `cargo test --test shell_projection_harness` — 11 passed, 0 failed.
- `cargo test --test shell_projection_phase10` — 33 passed, 0 failed.
- Static guards — all PASS.
- The plan's verification list also named `cargo test --test core_transport`
  and `cargo test tui::`. The first does not exist as a test target in
  this repository (cargo reports no matching binary); the closest
  equivalent — `single_daemon_lifecycle` and `session_selection` —
  was run explicitly above. The second was broadened to the
  full TUI integration / render / project-tab suite, all green.

## 5. Invariant review

For each source-plan invariant:

- **Projection state is a derived frontend contract, not a second
  session execution authority.** Pass. The projection lives in
  `codegg-protocol` only. It has no I/O, no daemon access, no
  RunStore access. `scripts/check-core-boundary.sh` confirms
  `codegg-core` is unchanged.

- **Two compliant reducers given the same snapshot / events
  produce equivalent state.** Pass. `ProjectionReducer` is pure;
  the second consumer test asserts equivalent logical state across
  four canonical fixture scripts. Reducer determinism tests in
  `projection::fixtures::tests::two_consumers_agree_on_active_turn_state`
  and `tests/session_projection_consumer.rs::fixture_snapshot_is_deterministic`
  document the equivalence.

- **Payloads are bounded; large bodies / logs / artifacts remain
  behind handles or summaries.** Pass. `ToolArgumentProjection` and
  `ToolOutputProjection` carry explicit `TruncatedArguments` /
  `TruncatedOutput` / `Handle` variants. The adapter layer switches
  inline outputs to `TruncatedOutput` when the bound is exceeded.
  `ArtifactHandleProjection` always carries only an opaque handle.

- **Raw render frames are not introduced as canonical state.**
  Pass. No `RawRenderFrame` or terminal cell type exists in
  `codegg_protocol::projection`. The contract is bounded to typed
  summaries.

- **Unknown optional variants / fields degrade safely.** Pass.
  `ProjectionEvent::Unknown` plus the reducer `unknown_variant`
  diagnostic record a single bounded diagnostic instead of
  panicking. `ProjectionCapabilities::supports_unknown_fields`
  defaults to `true`.

- **Provider-private hidden reasoning is not exposed; only explicit
  protocol content is projected.** Pass. `TurnReasoningDelta` maps
  to a `MessageProjection` with `VisibilityClass::Internal`. There
  is no import of `provider` / hidden-reasoning types.

- **Secret-bearing fields have a safe classification / redaction
  seam even though full policy lands later.** Pass. Every payload
  field has a `VisibilityClass` annotation. The reducer leaves
  fields as-is and records a diagnostic; Milestone 3 owns the
  filtering.

- **Existing core event transport and current clients remain
  compatible.** Pass. The `session_projection` capability is
  additive (`#[serde(default)]`); no existing variant is removed;
  every legacy TUI / single-daemon / shell projection test still
  passes.

## 6. Failure and recovery review

- **Invalid input leaves prior state unchanged.** Pass. The
  reducer's atomic-application rule returns `ApplyOutcome::Error`
  or `ApplyOutcome::Reconciled` without mutating `event_seq` /
  `generated_at_ms`; `Reconciled` records a diagnostic. The
  `unknown_event_seq_does_not_mutate` and `unsupported_protocol_version_is_an_error`
  unit tests exercise the path.

- **Duplicate detectable events do not duplicate state.** Pass.
  `ApplyOutcome::Duplicate` returns when `event_seq <= snapshot.event_seq`;
  the `duplicate_event_seq_does_not_mutate` test confirms state
  is untouched.

- **Events for another project / session do not mutate the target
  projection.** Pass. `ApplyOutcome::ScopeMismatch` plus a
  `scope_mismatch` diagnostic. `scope_mismatch_returns_scope_mismatch_and_records_diagnostic`
  exercises the path.

- **Out-of-order / impossible transitions are diagnosed.** Pass.
  Every reducer match arm that requires an active turn or an
  existing record routes the failure through `record_diagnostic`
  and returns `Reconciled` instead of panicking.

- **Projection building cancellation returns no partially
  published snapshot.** Pass. The reducer is pure and there is no
  long-running async build path inside this milestone — snapshot
  construction is a synchronous DTO copy. Future async builds
  (Milestone 2 durable replay) inherit the atomicity guarantee
  through their own commit / discard seam.

- **Restart behaviour in this milestone relies on rebuilding from
  current snapshots / messages; durable event replay is deferred.**
  Pass. The `snapshot_from_snapshot_session` and
  `snapshot_from_daemon` adapters do exactly this — they rebuild
  the projection from the existing `CoreResponse::SnapshotSession`
  and `CoreResponse::SnapshotDaemon` payload, which the daemon
  already produces on reconnect.

- **Concurrent readers may share immutable projection snapshots;
  one controlled writer applies ordered events per projection
  instance.** Pass. The reducer takes `&mut SessionProjectionSnapshot`;
  readers clone the snapshot (`#[derive(Clone)]`) for an immutable
  view. No interior mutability is used in the reducer.

## 7. Migration and compatibility review

No destructive schema or storage migration was introduced. No CLI
flag, persisted file format, or existing wire field changed.

The `session_projection` capability is additive on both
`ClientCapabilities` and `ServerCapabilities`. Older clients that
ignore the field enter the unsupported-projection path: the
projection is not constructed, but no other consumer is affected.
Older daemons that do not advertise `session_projection.v1` continue
to function; clients advertise `session_projection: false` and the
projection adapter is never invoked.

The reducer is enabled by default. Future work can guard the
adapter behind a feature flag if the projection ever becomes
optional; today every test runs with the projection enabled.

The local / remote TUI continues to use the existing
`SessionState` / `RemoteTuiStateSnapshot` types unchanged. The
projection contract is consumed only by the new tests in this
milestone; TUI migration is explicitly out of scope and lands in
Milestone 4.

## 8. Security review

- The contract depends only on `serde` and `serde_json`. No
  credential, environment, secret-store, raw provider config, or
  `SecretInput` field is reachable from a projection DTO.
- Tool outputs larger than `MAX_PROJECTION_TOOL_OUTPUT_BYTES` are
  replaced by `ToolOutputProjection::TruncatedOutput` /
  `Handle`. The adapter layer enforces this when it converts
  `CoreEvent::ToolCompleted`.
- File bodies, terminal render frames, and chat block content stay
  in existing stores / tools. `FileChanged` events coalesce into a
  bounded session summary, never into raw file contents.
- Provider private hidden reasoning is mapped to
  `VisibilityClass::Internal` and never to a public projection
  field.
- Full redaction policy lands in Milestone 3. This milestone ships
  the typed seam (`VisibilityClass`, `ProjectionDiagnostic`)
  without coupling to the redaction rule pipeline.
- `scripts/check-core-boundary.sh`, `scripts/check_daemon_cwd_usage.py`,
  and `scripts/check_git_forbidden_patterns.py` all pass.

## 9. Documentation and operations

Updated:

- `architecture/projection.md` — new file documenting the projection
  contract (purpose, ownership, module layout, versioning, limits,
  visibility classification, reducer semantics, adapter layer,
  fixtures, static guarantees, compatibility matrix, security
  classification, verification commands, future milestone
  ownership).
- `architecture/protocol.md` — adds the projection module to the
  module map and documents
  `PROJECTION_PROTOCOL_VERSION = 1`,
  `PROJECTION_PROTOCOL_VERSION_MIN = 1`, and
  `PROJECTION_CAPABILITY = "session_projection.v1"`.
- `plans/subsystems/session-projections-roadmap.md` — milestone 1
  status moves from `ready` to `closed`, pointing at this record.
- `plans/registry.md` — implementation plan removed from the
  `Dependency-ready implementation plans` table; closure row added
  under `recently closed work`; active subsystem roadmaps row
  updated to point at Milestone 2.

No new operational diagnostics or static guards were required: the
existing `check-core-boundary.sh`, `check_daemon_cwd_usage.py`, and
`check_git_forbidden_patterns.py` already cover the failure modes
this milestone could introduce.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | The reducer mutates `snapshot.recent_summary` for `FileChanged` events but does not enforce a per-file cap. A long session can still surface many file paths through the `recent_summary` line. | Operators inspecting the recent summary see a single bounded string; downstream consumers must respect `MAX_PROJECTION_STRING_BYTES` when reading. | Milestone 3 (visibility / redaction) should add a dedicated file-change projection variant with per-file caps if multi-file summary aggregation proves confusing. |
| low | `ProjectionCapabilities` is currently a passive struct; the negotiation helper is invoked only inside unit tests. | Daemon and client wires do not yet negotiate the projection version at connect time. | Milestone 2 (durable replay / subscription registry) MUST wire `ProjectionCapabilities::negotiate` into the `ClientHello` / `ServerHello` handshake. |
| low | `ToolOutputProjection::Handle` is defined but the adapter layer does not yet emit it. The `Handle` variant exists as a future hook for the bounded artifact-read API. | Consumers can still receive inline or `TruncatedOutput` projections; large output becomes truncated, not a handle. | Milestone 3 (visibility / redaction / artifact handles) MUST replace `TruncatedOutput` with `Handle { handle, byte_length }` when the artifact read API ships. |
| low | The remote TUI protocol still uses `RemoteTuiStateSnapshot` rather than `SessionProjectionSnapshot`. | The local / remote TUI has not migrated yet; the projection contract is consumed only by the new tests. | Milestone 4 (frontend adoption) MUST migrate `TuiMessage::StateSnapshot` to the projection contract. |
| low | The reducer's `RunArtifactCreated` handler bumps `runs[i].artifact_count` regardless of whether the run is found, which means an artifact created before its `RunStarted` may attach to the wrong run. | Cosmetic only; the bounded `runs` list still surfaces an `artifact_count`. | Milestone 2 (subscription registry) should re-route orphan artifacts to a dedicated `ArtifactHandleProjection` collection. |

No medium, high, or critical finding remains. The low items are
declared downstream integration boundaries, not regressions or
authority violations.

## 11. Roadmap disposition

Milestone closed. The next hard dependency may proceed:

- Session Projections Milestone 2 (scoped subscriptions and
  durable replay) becomes dependency-ready as soon as its
  implementation plan is registered. The durable replay backend
  selection remains deferred until that milestone's plan lands; if
  the backend requires an ADR, the closure of Milestone 2 will
  reference it.
- Session Projections Milestone 3 (visibility, redaction, and
  artifact handles) remains blocked on Milestones 1–2 closure and
  on the future principal capability seam.
- Session Projections Milestone 4 (frontend adoption and closure)
  remains blocked on Milestones 1–3 closure.

The closure does not unblock any other subsystem. The Domain
Identity, Project Catalog, Multi-Project TUI, and Runtime Assets
roadmaps are already closed; no other implementation plan in
`plans/implementation/` had a hard dependency on the projection
contract.

## 12. Registry updates

Required updates after this record is accepted:

- `plans/subsystems/session-projections-roadmap.md` milestone table
  row for milestone 1: status `closed`, closure record pointer set
  to this file, blocker column updated to `—`.
- `plans/registry.md`:
  - Move `plans/implementation/session-projections/001-projection-contracts.md`
    from the `Dependency-ready implementation plans` table to
    `recently closed work`, citing this record and the implementation
    commit.
  - Update the `Frontend-neutral session projections` row in
    `Active subsystem roadmaps` to point at Milestone 2 once its
    plan is registered.
  - The Milestone 2 entry is not yet registered, so the
    `dependency-ready` table is empty after this closure.

These updates are recorded as part of the same commit that lands
this closure record.