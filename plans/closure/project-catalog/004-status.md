# Project Catalog Milestone 004 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/project-catalog/004-protocol-server-migration-and-closure.md`

Source subsystem roadmap:

- `plans/subsystems/project-catalog-roadmap.md#milestone-4--protocol-server-migration-and-closure`

Repository baseline reviewed: `a827ae8`

Implementation commits or pull requests:

- `d1e5b70` — migrate project catalog operations through the native protocol and remove server process-global project scope
- Follow-up closure/documentation commit — this record, roadmap/registry closure, and downstream dependency decisions

## 1. Executive finding

Milestone 004 is complete. A daemon/server can expose bounded project catalog list/get/register/archive/restore/health operations through the native protocol, negotiate project-catalog capability, and serve multiple projects without `ServerState.project_dir`. HTTP, JSON-RPC, WebSocket, session, workspace, and file paths now require explicit project/workspace scope or uniquely resolve a legacy directory locator through `ProjectContextResolver`.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Bounded project DTOs and catalog wire operations | `crates/codegg-protocol/src/dto.rs`, `src/core/daemon.rs`, protocol round-trip test | pass | List and relation bounds are applied at the protocol boundary. |
| Capability negotiation and older-client defaults | `crates/codegg-protocol/src/frames.rs`, transport handshake tests | pass | New capability fields use serde defaults. |
| Lifecycle and health events carry explicit identity | `CoreEvent` variants and daemon project-catalog test | pass | Project/workspace IDs are payload identity, never paths. |
| Archive/restore preserves durable catalog state | `core::daemon::project_catalog_protocol_lists_lifecycle_and_health_by_scope`, catalog tests | pass | Operations are logical and retry-safe through existing catalog APIs. |
| Server has no process-global project authority | `src/server/state.rs`, `src/server/http.rs`, `rg project_dir src/server` | pass | `ServerState.project_dir` was removed. |
| HTTP/WS scope isolation and legacy locator behavior | `src/server/scope.rs` and migrated route handlers | pass | Explicit IDs are required together; directory lookup is unique-resolution-only. |
| Restart/lazy activation behavior remains bounded | `tests/project_activation.rs` and storage migration tests | pass | Restart hydration does not eagerly acquire activation leases. |
| Security and path boundaries remain enforced | path-traversal, Git-forbidden-pattern, core-boundary, and full all-features suites | pass | No high or critical finding remains. |
| Broad workspace verification | `cargo test --workspace --all-features -- --test-threads=14` | pass | All observed workspace binaries passed; main library suite: 3,834 passed, 0 failed. |

## 3. Production implementation evidence

- `codegg-protocol` now defines bounded project summaries, workspace summaries, health layers, details, register requests, catalog requests/responses, lifecycle/health events, and capability flags.
- `codegg-core` supplies conversions from catalog/workspace/health records to wire DTOs without moving storage authority across the crate boundary.
- `CoreDaemon` dispatches catalog list/get/register/archive/restore/health requests, validates typed IDs, clamps list output, and publishes identity-bearing lifecycle/health events.
- Server adapters use request-scoped project/workspace context. The new `src/server/scope.rs` centralizes explicit resolution and uniquely-resolved directory compatibility errors.
- HTTP project list/get/archive/restore routes are additive. Session, workspace, file, config, and JSON-RPC paths no longer infer a project from server state or process cwd.
- Architecture documentation was updated for catalog ownership, protocol DTOs/capabilities, server routes, compatibility locators, and restart/activation boundaries.

## 4. Verification executed

### Commands run

```bash
rtk cargo fmt -- --check
rtk cargo check --workspace --all-targets --all-features
rtk cargo clippy --workspace --all-targets --all-features -- -D warnings
rtk cargo test -p codegg-protocol
rtk cargo test -p codegg-core project_catalog
rtk cargo test -p codegg --lib core::daemon
rtk cargo test --features server --lib server
rtk env CARGO_BUILD_JOBS=1 cargo test --workspace --all-features -- --test-threads=14
rtk scripts/check-core-boundary.sh
rtk python3 scripts/check_daemon_cwd_usage.py
rtk python3 scripts/check_project_catalog_invariants.py
rtk python3 scripts/check_project_agent_pwd_inference.py
rtk python3 scripts/check_discovery_invariants.py
rtk python3 scripts/check_execution_ownership.py
rtk python3 scripts/check_git_forbidden_patterns.py
rtk git diff --check
```

### Results

- Formatting and clippy passed; clippy reported no issues with `-D warnings`.
- Protocol tests: 92 passed. Project-catalog core tests: 11 passed. Core daemon tests: 28 passed. Server tests: 25 passed.
- Workspace check completed successfully. The capped all-features workspace suite completed successfully; the main `codegg` library suite reported 3,834 passed and 0 failed, followed by passing integration, LSP, security, TUI, native-crate, and doc-test suites.
- Static guards all passed: core boundary, daemon cwd, project catalog invariants (7/7), project-agent PWD inference, discovery invariants (5/5), execution ownership, and Git forbidden patterns.
- The shell startup emitted a pre-existing missing `/Users/davidbowman/.local/bin/env` notice; it did not affect command outcomes.

## 5. Invariant review

- Paths remain compatibility locators; durable identity is typed project/workspace IDs.
- Catalog list/get/health paths do not activate services or start external processes.
- Archive is logical and preserves workspace/session/catalog history.
- Remote/placeholder locators remain data and are not executable server inputs.
- Server requests cannot silently select another project or use a process-global cwd.
- Existing wire fields and session behavior remain additive-compatible; unsupported or ambiguous scope returns actionable errors.

## 6. Failure and recovery review

- Invalid, archived, missing, mismatched, and ambiguous context maps to stable project-context errors without fallback.
- Registration and archive/restore use existing catalog transaction/idempotency behavior.
- Restart hydration and lazy activation are covered by project activation and storage migration tests.
- Existing activation coalescing and lease release behavior remains covered by the workspace/project activation suites.
- DTO/list bounds cap protocol responses; event payloads carry bounded identity and health summaries.
- Cancellation and scheduler ownership were not moved by this milestone; the execution-ownership guard passed.

## 7. Migration and compatibility review

No destructive schema migration was introduced. Existing catalog and workspace-binding tables remain authoritative. New protocol fields and capability flags are additive and serde-defaulted. Legacy server directory requests resolve only when a unique active context exists; otherwise they return `project_context_required`, `ambiguous_project_context`, or a typed context error. Rollback is the normal Git revert of `d1e5b70` before downstream clients adopt the new capability.

## 8. Security review

Project/workspace/session context is validated before storage or filesystem work. File paths are canonicalized and bounded under the resolved workspace root. Remote locators are inert. Secret-bearing provider operations remain denied over the remote socket. Core-boundary, cwd, path-traversal, Git-secret, SSRF, and all-features security suites passed. No credentials are added to project DTOs, events, or server state.

## 9. Documentation and operations

Updated:

- `architecture/project_catalog.md`
- `architecture/protocol.md`
- `architecture/server.md`
- `plans/subsystems/project-catalog-roadmap.md`
- `plans/registry.md`
- downstream TUI and Session Projections dependency plans

Operational guidance is now explicit about project IDs, workspace IDs, unique locator compatibility, restart hydration, and actionable context errors.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | The existing generic event replay filter remains session/global-shaped; project catalog event payloads carry explicit project/workspace IDs, but a separate project selector is deferred. | Future frontend projection consumers must filter project-scoped catalog events before applying them to project state. | Multi-Project TUI and Session Projections must consume the identity-bearing payloads and establish project-aware subscription/reducer filtering. |

No medium, high, or critical finding remains. The low item is a declared downstream integration boundary, not a server identity fallback or authorization bypass.

## 11. Roadmap disposition

Milestone closed and the next hard dependency may proceed. Multi-Project TUI 001 is now `ready`; Session Projections 001 remains `blocked` solely on closure of Multi-Project TUI 001. No other future plan became dependency-ready in this review.

## 12. Registry updates

- Project Catalog roadmap and implementation plan now point to this closed record.
- Project Catalog 004 moved from active/dependency-ready to recently closed at implementation commit `d1e5b70`.
- Multi-Project TUI 001 was removed from the blocked table and registered as dependency-ready with status `ready`.
- Session Projections 001 remains in the blocked table with its blocker narrowed to Multi-Project TUI 001.
