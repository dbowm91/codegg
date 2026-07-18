# Runtime Assets Milestone 003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/runtime-assets/003-refresh-lifecycle-operator-surface.md`

Source subsystem roadmap:

- `plans/subsystems/runtime-assets-roadmap.md#milestone-3--refresh-lifecycle-and-operator-surface`

Repository baseline reviewed: `5974976` (Runtime Assets Milestones 001–002
closed; Project Catalog Milestone 002 implemented)

Implementation commit:

- `972c286` — `feat(runtime-assets): add refresh lifecycle and operator surface`.
Adds the daemon-owned coordinator, immutable publication and turn capture,
session/lifecycle gates, refresh protocol and TUI operator surface, additive
schema-v30 metadata, restart hydration, bounded reports, and regression tests.

## 1. Executive finding

Milestone 003 is closed. Runtime assets now refresh through one
project/workspace-scoped daemon coordinator. Candidate snapshots are built
outside the publication lock, validated, and atomically published as
monotonic generations. Invalid, cancelled, and failed candidates retain the
last valid publication. Same-scope requests coalesce, while different scopes
remain isolated.

Session create/load/attach/import/template paths and the final `TurnSubmit`
gate use the coordinator before capturing an immutable `Arc<ProjectAssetSnapshot>`
for a turn. Manual protocol requests and `/reload`, `/skills-refresh`, and
`/agents-refresh` aliases use that same coordinator. A public
`refresh_project_activation` daemon seam is available for Project Catalog
Milestone 003's activation path; catalog activation policy remains outside
this milestone.

The repository-wide capped run passed 3,817 tests. Five unrelated environment
failures remain separately identified in §4: two fake-Eggpool socket binds are
denied by the execution environment and three daemon-socket tests cannot find
their test socket. Milestone-specific tests, guards, migration compilation,
and focused protocol/TUI evidence are green, so these failures do not block
closure of this asset milestone.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| Per-project/workspace coordinator with immutable publication and monotonic generations | `src/agent/asset_refresh.rs`; `publishes_generation_and_pins_previous_snapshot`; `different_scopes_are_isolated` | pass | Publication state is `Arc`-based and generation increments only on successful publication. |
| Candidate build occurs outside publication lock | Coordinator implementation and `same_scope_requests_coalesce_to_one_publication` | pass | The builder runs before the write lock; one same-scope builder call yields one publication. |
| Same-scope coalescing and cross-project isolation | `same_scope_requests_coalesce_to_one_publication`; `different_scopes_are_isolated` | pass | Coalesced waiter receives the published generation; independent scopes each publish generation 1. |
| Invalid, cancelled, and failed refresh retain the last valid generation | `invalid_context_retains_previous_generation`; `cancelled_refresh_retains_the_last_valid_generation`; retained error path in coordinator | pass | Cancellation is checked both before build and before publication. |
| Session/project lifecycle refresh before a new turn | `CoreDaemon` session create/load/attach/import/template paths, public `refresh_project_activation`, and final `TurnSubmit` refresh gate | pass | Existing session seams are wired; Project Catalog activation policy consumes the explicit activation seam. |
| Manual refresh and `/reload` use common report semantics | `CoreRequest::AssetRefresh`, `CoreEvent::AssetRefreshCompleted`, `TuiCommand::RefreshAssets`, `/reload` aliases, and protocol test | pass | No second refresh implementation exists in the TUI. |
| Bounded added/removed/changed/shadowed/invalid/retained diagnostics | `RefreshReport`, bounded diff helpers, DTO capability limit, and protocol boundedness test | pass | Reports contain names/digests/diagnostics only; entries and diagnostics have separate caps. |
| Restart reconstruction from durable generation/fingerprint metadata | schema-v30 `runtime_asset_refresh`, daemon hydration, `restored_metadata_prevents_generation_reuse` | pass | Bodies remain reconstructible from explicit context; metadata prevents generation reuse. |
| In-flight turns retain their captured snapshot | `TurnRunInput.asset_snapshot`, `DefaultTurnRuntime`, and `turn_submit_uses_injected_runtime` | pass | Later refreshes cannot mutate an already captured turn snapshot. |
| Protocol capability and compatibility surface | Additive DTOs/requests/responses/events and `ASSET_REFRESH_CAPABILITY` | pass | Existing clients retain their paths; new behavior is explicitly advertised. |
| No refresh path executes scripts or starts external subsystems | Coordinator calls only the bounded snapshot builder; focused asset/adversarial tests and static guards | pass | Refresh does not invoke tools, providers, LSP, builds, or bundled scripts. |
| Architecture and operator documentation | `architecture/{agent,overview,skills,storage}.md`, `AGENTS.md` | pass | Storage layout and guard documentation were updated with the implementation. |

## 3. Production implementation evidence

### Coordinator and snapshot ownership

`src/agent/asset_refresh.rs` owns `AssetScope`, refresh reasons/outcomes,
bounded reports/status, per-scope single-flight locks, cancellation checks,
last-valid retention, generation assignment, snapshot diffs, and restored
metadata. `src/agent/asset_snapshot_builder.rs` adapts the existing bounded
`ProjectAssetSnapshotBuilder` without adding a second discovery path.

`TurnRunInput` carries an optional immutable snapshot. The daemon refreshes at
the final turn boundary and passes the published snapshot into
`DefaultTurnRuntime`, so active turns retain the generation they captured.

### Lifecycle and operator surface

The daemon owns refresh state and routes all manual requests through
`AssetRefreshCoordinator`. Session create/open/attach/rebind/import/template
seams refresh before returning or before turn construction. Project Catalog
receives `refresh_project_activation(project_id, workspace_id)` as an
explicit daemon seam; its activation leases and health aggregation are the
next catalog milestone.

The protocol adds bounded scope, request, report, status, and capability DTOs,
plus `AssetRefreshCompleted`. The TUI command layer adds `/reload` and the
focused aliases, dispatches a native refresh request, and renders only the
bounded summary.

### Storage and restart

Schema migration v30 adds `runtime_asset_refresh` with project/workspace
scope, generation, fingerprint, last-success timestamp, bounded diagnostics,
and update time. Snapshot bodies are not stored. Daemon hydration restores
generation/fingerprint metadata and tolerates an older database without the
new table so restart remains actionable during compatibility transitions.

## 4. Verification executed

### Commands run

```bash
rtk cargo fmt --all -- --check
rtk cargo check --workspace --all-targets --all-features
rtk cargo test --test asset_snapshot
rtk cargo test -p codegg --lib asset_refresh
rtk cargo test -p codegg --lib agent::
rtk cargo test -p codegg --lib agent:: -- --test-threads=1
rtk cargo test -p codegg --lib tui::commands::agents
rtk cargo test -p codegg-protocol core::tests::asset_refresh_protocol_is_bounded_and_defaults_manual_reason
rtk cargo test --workspace --all-features -- --test-threads=14
rtk python3 scripts/check_project_agent_pwd_inference.py
rtk python3 scripts/check_daemon_cwd_usage.py
rtk python3 scripts/check_execution_ownership.py
rtk bash scripts/check-core-boundary.sh
rtk git diff --check
```

### Results

- `cargo fmt --all -- --check` — exit 0.
- `cargo check --workspace --all-targets --all-features` — exit 0.
- `cargo test --test asset_snapshot` — 7 passed.
- `cargo test -p codegg --lib asset_refresh` — 6 passed after the final
  coordinator regression additions.
- `cargo test -p codegg --lib agent::` — 289 passed in the earlier default
  run before the final three coordinator tests; the post-change deterministic
  run with `--test-threads=1` passed 292 tests. A later default-parallel
  rerun became silent and was interrupted after the serial run passed; this
  is recorded as a runner-stability note, not hidden as a pass.
- `cargo test -p codegg --lib tui::commands::agents` — 10 passed.
- Focused asset-refresh protocol test — 1 passed.
- Capped workspace run (`CARGO_BUILD_JOBS=1` and `--test-threads=14`) —
  3,817 passed and 5 failed in unrelated environment-bound tests:
  `core::eggpool::tests::cancellation_compensates_operation_owned_credential`
  and `core::eggpool::tests::successful_provision_persists_redacted_connection_and_catalog`
  fail to bind their fake Eggpool with `Operation not permitted`; three
  `core::transport::daemon_socket` integration tests fail to connect because
  the expected socket path does not exist. The run completed in 34.68s;
  there was no refresh-related failure.
- `check_project_agent_pwd_inference.py` — passed.
- `check_daemon_cwd_usage.py` — passed.
- `check_execution_ownership.py` — passed.
- `check-core-boundary.sh` — passed.
- `git diff --check` — passed.

## 5. Invariant review

- **Explicit scope:** daemon refresh requests require project/workspace
  identity resolved through `ProjectContextResolver`; the coordinator
  rejects synthetic/unbound project IDs and inaccessible roots.
- **Transactional publication:** candidate construction is separate from
  publication; only a successful, non-cancelled build obtains a new
  generation and swaps the `Arc`.
- **Last-valid retention:** invalid context, cancellation, and builder errors
  return bounded diagnostics and preserve the prior immutable publication.
- **No in-flight mutation:** turns receive an `Arc<ProjectAssetSnapshot>`;
  later generation swaps cannot alter that object.
- **Deterministic precedence and isolation:** the existing snapshot builder
  and asset registry retain source ordering and path bounds; coordinator
  scope state is keyed by both project and workspace IDs.
- **Bounded operator output:** reports truncate names and diagnostics and do
  not include asset bodies, absolute paths, or secret-bearing content.
- **No execution side effects:** refresh is a bounded read/build operation;
  it does not route through the scheduler, tool registry, provider, LSP,
  build, or script execution surfaces.

## 6. Failure and recovery review

- **Duplicate delivery/idempotency:** the per-scope mutex makes duplicate
  lifecycle/manual signals coalesce; only the lock owner can publish.
- **Cancellation race:** cancellation is checked before build and immediately
  before publication, so a cancelled candidate cannot replace the last valid
  snapshot.
- **Daemon restart:** schema-v30 metadata is hydrated into the coordinator;
  the next explicit-context refresh resumes at the next generation and
  rebuilds the body from current sources.
- **Partial metadata persistence:** persistence is additive and best effort;
  a failure emits a warning without changing the live publication or turn
  snapshot.
- **Stale generation:** publication generation is derived from the retained
  in-memory or restored maximum and is assigned only after validation.
- **Contention/resource release:** same-scope waiters release the lock after
  observing the owner result; separate scopes build concurrently. The
  coalescing test asserts one builder call.
- **Malformed/unauthorized input:** explicit context resolution, authoritative
  project-ID validation, root-directory validation, and existing asset
  registry/path/frontmatter bounds remain in force.
- **Bounded artifacts:** only generation/fingerprint/diff names/diagnostics
  are persisted or sent through protocol; asset bodies remain local.

## 7. Migration and compatibility review

- Migration v30 is additive and idempotent (`CREATE TABLE IF NOT EXISTS`,
  indexed update time); existing schema versions continue through the normal
  migration chain.
- `STORAGE_LAYOUT_VERSION` and architecture/agent storage references were
  updated from 29 to 30.
- Refresh protocol types are additive and capability-gated. Existing clients
  do not need to understand refresh events to use existing session flows.
- The legacy refresh-free path remains available; daemon-owned turn paths
  now perform the final refresh gate before constructing a new runtime.
- Snapshot bodies are intentionally not persisted, so a workspace whose
  sources become unavailable after restart reports a retained/invalid result
  rather than fabricating an old body.

## 8. Security review

- Project/workspace roots are resolved through typed context and must be
  accessible directories; no refresh path infers project identity from cwd.
- Existing source-root, symlink-containment, file-size, frontmatter, and
  instruction bounds are reused by the snapshot builder.
- Reports and durable metadata omit bodies and absolute paths, and diagnostics
  are bounded to prevent unbounded protocol/storage growth.
- Foreign harness directories remain read-only; refresh discovers assets but
  does not write into them or execute bundled scripts.
- Refresh never grants tools, providers, LSP, build, or shell authority.
- The static project-agent/CWD, daemon-CWD, execution-ownership, and
  codegg-core-boundary guards all pass.

## 9. Documentation and operations

Updated:

- `architecture/agent.md` — coordinator, generation, and immutable turn
  capture.
- `architecture/overview.md` — runtime-asset module map.
- `architecture/skills.md` — refresh lifecycle and safe operator behavior.
- `architecture/storage.md` and `AGENTS.md` — schema/layout v30.

Operator surface:

- native `AssetRefresh` request/status/capability operations;
- completion event `asset_refresh_completed`;
- TUI `/reload`, `/skills-refresh`, and `/agents-refresh` aliases;
- bounded generation/fingerprint/diff/diagnostic summaries.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Default-parallel `agent::` rerun was silent in this environment after the serial run passed. | Test-runner stability evidence is weaker than the deterministic serial result; no product failure was observed. | Re-run the default-parallel agent filter in CI or a normal process environment. |
| low | Five workspace tests require socket/process capabilities unavailable in this sandbox. | Full local workspace result is not all-green, but failures are unrelated to runtime assets and the run had 3,817 passes. | Re-run the named Eggpool and daemon-socket tests in the repository CI/host environment. |
| low | Project Catalog activation policy is not implemented by this milestone. | The daemon seam is ready, but activation leases/health remain future catalog scope. | Execute `plans/implementation/project-catalog/003-lazy-activation-and-health.md`. |
| low | File watchers and distributed manifests remain deferred. | Refresh correctness still relies on lifecycle/manual triggers as designed. | Address only in the later Runtime Assets M004/manifest handoff, not as a correctness shortcut. |

No medium, high, or critical finding remains in this milestone's implemented
scope.

## 11. Roadmap disposition

Milestone closed and next dependencies may proceed. Runtime Assets Milestone
004 is now dependency-ready with a handoff plan. Project Catalog Milestone
003 is also dependency-ready because the explicit daemon activation-refresh
interface is available. Multi-Project TUI M001 and Session Projections M001
remain blocked on Project Catalog M004 and the project-aware TUI foundation;
they are not unblocked by this closure.

## 12. Registry updates

The following planning updates are included with this closure:

- mark Runtime Assets M003 closed and link this record;
- register Runtime Assets M004 as ready;
- register Project Catalog M003 as ready and link the Runtime Assets M003
  activation interface;
- remove Runtime Assets M003 from the dependency-ready table and add it to
  recently closed;
- retain Multi-Project TUI M001 and Session Projections M001 as blocked;
- update both subsystem roadmaps and the active registry current-milestone
  pointers.
