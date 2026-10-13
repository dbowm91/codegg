# Agent Tool Runtime Reconciliation M002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/agent-tool-runtime-reconciliation/002-external-tool-discovery-and-generation.md`

Source subsystem roadmap:

- `plans/subsystems/agent-tool-runtime-reconciliation-roadmap.md#6-dependencies-and-milestones`

Repository baseline reviewed: `8535f6d7c2e68f5d134ba0c73bd146ca8bc0bd03`

Hard dependency: M001 accepted closure `plans/closure/agent-tool-runtime-reconciliation/001-status.md` (verified before code change; plan baseline identical so no rebaseline drift).

Implementation commits or pull requests:

- Pending commit on `planning/agent-tool-runtime-reconciliation` — M002 discovery/generation (same push as M001; files below; SHA to be recorded at commit time)
  - `src/agent/tool_descriptor_snapshot.rs` (new) — `ToolDescriptorSnapshot`, `snapshot_mcp_identities`, `plugin_activation_revision`, `confine_hook_definitions`, 3 focused tests
  - `src/agent/mod.rs` — `tool_descriptor_snapshot` module
  - `src/agent/coordinator.rs` — `ToolDefCache` extended to 13 fields (agent, plugin activation, snapshot fingerprint)
  - `src/agent/request_preparation.rs` — single-origin snapshot build, extended cache identity, hook confinement in both paths, generation telemetry
  - `tests/agent_tool_m002_discovery.rs` (new) — 12 integration tests (late registration, exact schema, ranking/bounds, cost, hide/disable, plan-mode/collision, drift/reconnect, concurrent refresh, snapshot, hook matrix, path independence, helpers, policy origin)
  - `scripts/check_tool_descriptor_snapshot.py` (new) — M002 static guard
  - `architecture/agent-tool-surface.md`, `architecture/mcp.md`, `architecture/tool.md`, `architecture/plugin.md` — discovery/generation/hook story reconciled

## 1. Executive finding

M002 is complete as a capability boundary. Every registered and authorized third-party MCP tool is discoverable by `tool_search` with its correct schema (keyword compact + exact `name`/`detail: "schema"`), whether immediate or deferred, while forbidden/unavailable/stale tools never leak into discovery or invocation. Connection and tool-list refresh produce a coherent per-turn generation: the immutable `ToolDescriptorSnapshot` pins MCP generation, visible revision, server origins, permitted names, policy revision, plugin activation, agent identity, hidden set, and surface fingerprint; its input fingerprint keys `tool_def_cache`, so any of those changes invalidates cached definitions. In-flight turns keep their pinned adapters and fail closed on mismatch; refresh affects future turns only (turn-boundary rebuild, no polling). Late plugin schema hooks are confined in both cache-hit and uncached paths: additions, renames, invalid schemas and parameter widening are rejected; canonical parameters/deferral are preserved and only bounded description rewordings survive.

No new ADR was required: catalog, surface, broker, and plugin ownership are unchanged. No database, global registry, separate MCP search engine, remote install flow, or durable grant migration was introduced.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| A — Canonical session descriptor projection via `register → Catalog`; `ToolSearchTool` indexes exact schema + category/effect; preserve `expose_in_definitions`/`defer_loading`/`has_functional_backend`; catalog vs turn membership distinguished; no DB/global/separate engine | M001 adapters in live shared catalog (retained); `tool_search` exact-schema test for `mcp__db__query` (category/risk/disclosure + parameters); `expose_in_definitions` filter preserved in snapshot permitted set + surface; `has_functional_backend` (`task` spawner) unchanged | pass | `session_late_registration_is_discoverable_with_exact_schema` 12-test suite. |
| A — Curated/Minimal expose MCP as deferred or compact metadata, not invisible; Full + no-deferral providers functional; aliases validated | `project_initial_tool_palette` (existing) marks non-palette MCP deferred in Curated/Minimal; `apply_tool_exposure_filter` runs on registry definitions including MCP; generic-provider fallback (`supports_defer_loading false` → all immediate) unchanged; surface alias collisions error (`AliasCollision`/`AmbiguousReverseAlias`) | pass | Palette unit tests retained; `plan_mode_hides_mcp_and_collisions_are_rejected` covers plan-hide + alias error. |
| B — Immutable snapshot (origin/identity, generation, schema fp, permitted names, policy rev, surface fp); lifecycle on add/remove/reconnect/`list_changed`/plugin activation/managed hiding/config refresh/cache invalidation; turn-boundary refresh, no polling; cache invalidated on all inputs; mid-turn disappearance fails closed; no long write lock across `tools/list` | `ToolDescriptorSnapshot` + `fingerprint` (excludes surface fp); `snapshot_mcp_identities` (`try_read`, empty on contention); `register_mcp_adapters` turn-boundary rebuild (turn/worker/`AgentLoop::new`); cache key extended (agent, plugin activation, snapshot fp); `check_pinned_locked` fail-closed; no `tools/list` fetch holds catalog locks | pass | Tests: `snapshot_fingerprint_covers_all_generation_owners`, `schema_change_and_reconnect_are_generation_correct`, `concurrent_refresh_does_not_rebind_in_flight`. |
| C — `tool_search` keyword + exact-schema (`database search` → compact, `mcp__db__query` schema); canonical/wire name + host risk, never grants; bounded 10 + determinism; disabled/denied respected | `session_late_...` (keyword compact no `parameters`, exact has `parameters` + `risk`/`category`); `rank_ordering_is_deterministic_and_bounded` (sorted + ≤10); `hidden_and_disabled_tools_are_absent` (hidden server absent from catalog; allow-list miss → `no_results`) | pass | `MAX_SEARCH_RESULTS=10` enforced in tool, not just catalog. |
| D — Hook confinement: reject additions/renames/invalid/collisions/widening; allowed removal/rewording only after revalidation; cache keys include hook/activation revs; no quiet `defer_loading=None`; parameters constrained to description edits; metadata not a grant | `confine_hook_definitions` (both paths); canonical parameters/deferral preserved; `defer_loading: None` absent from hook windows (guard); activation rev in cache key; hook output never cached (re-dispatched per build) | pass | Tests: `hook_confinement_matrix`, `hook_confinement_is_path_independent` + snapshot unit tests. Guard `check_tool_descriptor_snapshot.py` enforces. |
| E — Docs/observability: update surface/mcp/tool/plugin/disclosure docs + guards; generation/omission telemetry without credentials/PII | Four architecture docs updated; two guards (`check_mcp_broker_adapter.py` retained, `check_tool_descriptor_snapshot.py` new); `tracing::debug!` snapshot (generation, visible rev, permitted/advertised/omitted counts, policy, activation, agent, surface fp) | pass | No endpoint/token/args/output echo in telemetry. |
| Work package 1 — Inventory (constructors, sources, hook callers, generations, policy owners; design note/test matrix) | M001 inventory reused + snapshot builder documents owners; matrix in this §2 + `agent_tool_m002_discovery` 12 tests covering native/plugin/raw-MCP/hidden-managed | pass | Native/plugin/raw-MCP/hidden covered across adapter, snapshot, and discovery suites. |
| Work package 2 — Canonical registration/discovery + exact schema in Full/Curated/Minimal + fallback | Single-origin retained (no second append); exact-schema test; palette deferral preserved; fallback path unchanged (all-immediate when `supports_defer_loading` false) | pass | Fallback proven by unchanged code path + existing provider capability tests (`verify.sh quick` provider guards green). No new fallback test with live provider (see §10). |
| Work package 3 — Generation/failure semantics (immutable revision per turn, next-turn refresh, coherent cache; add/remove/rename/schema-change/reconnect fixtures) | Snapshot + extended cache; `schema_change_and_reconnect...` (generation change, in-flight fail, next-turn rebuild); `concurrent_refresh...` (no rebind) | pass | Rename covered as addition-rejection (canonical map) + ambiguous-name rejection at adapter. `tools/list_changed` notification has no production caller (mock-only); turn-boundary rebuild is the bounded refresh per plan §5B. |
| Work package 4 — Hook confinement (validate against registry/surface; preserve deferral/schema identity; injection/collision/widening negatives + compliant smoke) | `confine_hook_definitions` both paths + guard; negatives in `hook_confinement_matrix`; compliant smoke = no-plugin passthrough (definitions unchanged) + subset removal test | pass | Live plugin hook smoke with a real third-party plugin not run (see §10); deterministic unit/integration hook matrix is the closure evidence. |
| Work package 5 — Docs/qualification (guards, regression matrix, closure) | Guards + this record + `verify.sh quick` | pass |  |
| Acceptance — every allowed external tool discoverable with exact params under tested profiles; hidden/unauthorized absent; discovery points to callable broker adapter; refresh/cache generation-correct; hooks cannot advertise unregistered/denied; 40-tool cost measured | Above + `forty_tool_fixture_reports_compact_cost_saving`: full 40-tool schemas 5750 bytes vs compact top-10 1780 bytes (~69% smaller); broker-mediated call test retained from M001 | pass | Cost is for minimal `{"type":"object"}` schemas; real MCP schemas with larger properties would show larger absolute savings. |

## 3. Production implementation evidence

What changed beyond M001 (which already established the single registry origin):

- New owner `src/agent/tool_descriptor_snapshot.rs`: snapshot struct + fingerprint (stable hash, no secrets), `snapshot_mcp_identities` (bounded `try_read`, empty on contention so definition building never blocks), `plugin_activation_revision` (sorted ids), and `confine_hook_definitions` (hook → authorized surface projection).
- `ToolDefCache` is now 13 fields: the original 10 plus acting agent name, plugin activation revision, and snapshot input fingerprint. All three are checked on cache-hit and stored on miss. The resolved surface fingerprint is attached to the snapshot for telemetry only, never for identity.
- Both `build_tool_definitions` paths confine hook output. Cache-hit confines the cached immediate set then re-appends deferred (retained against both canonical sets so deferred-only smuggling fails). Uncached confines the full immediate+deferred result. When the hook returns no `tools` array, the canonical result passes through unchanged. Hook-supplied `parameters` never become authority; `defer_loading` is never cleared.
- Telemetry: one bounded `tracing::debug!` per uncached build with generation, visible revision, permitted/advertised/omitted counts, policy, activation, agent, and surface fingerprint. No credentials, endpoints, args, or full schemas.
- Guards: `check_tool_descriptor_snapshot.py` (no second origin, hook confinement in both paths, no hook `defer_loading: None` window, snapshot tokens present, 13-field cache, single-origin derivation) plus retained `check_mcp_broker_adapter.py`.

What did not change (explicit non-goals honored):

- No remote install flow, durable catalog DB, semantic search service, execution redesign, unauthorized inheritance, provider protocol change, or reopening of closed ToolCatalog predecessors.
- No user-facing MCP rename, persistent migration, plugin manifest change, or public provider DTO change. Raw opt-in and managed-eggsearch duplicate avoidance preserved (`search_backend_eggsearch` still 9/9).
- No polling: refresh is per-turn registry rebuild. `handle_tool_list_changed` remains available for future notification wiring but has no production caller (mock-only); this is recorded, not hidden.

## 4. Verification executed

### Commands run

```bash
cargo fmt --all -- --check
cargo test -p codegg --lib -- agent::tool_descriptor_snapshot tool::mcp_adapter
cargo test -p codegg --lib -- agent::request_preparation agent::tool_batch tool::broker tool::tool_search
cargo test -p codegg --test agent_tool_m002_discovery
cargo test -p codegg --test agent_loop_harness
cargo test -p codegg --test search_backend_eggsearch
cargo test -p codegg --test fake_eggsearch_mcp
cargo test -p codegg --test mcp
cargo clippy --workspace --all-targets -- -D warnings
python3 scripts/check_mcp_broker_adapter.py
python3 scripts/check_tool_descriptor_snapshot.py
bash scripts/verify.sh quick
```

### Results

- `cargo fmt --all -- --check`: exit 0.
- New snapshot lib tests: 3/3 pass; adapter lib tests retained 11/11 pass (14/14 in the combined filter).
- Focused lib sweep (request_preparation, tool_batch, broker, tool_search): all pass (56/56 previously; rerun green after M002 edits — no new failures).
- `agent_tool_m002_discovery`: 12/12 pass (late registration, exact schema, ranking/bounds, 40-tool cost, hide/disable, plan/collision, drift/reconnect, concurrent refresh, snapshot fp, hook matrix, path independence, helpers, policy origin).
- 40-tool cost: `full=5750 bytes, compact-top10=1780 bytes` (~69% smaller; minimal schemas — real schemas save more in absolute terms).
- `agent_loop_harness`: 52/52 pass (rerun after M002; hooks/order preserved).
- `search_backend_eggsearch`: 9/9 pass; `fake_eggsearch_mcp`: 28/28 pass; `mcp`: 26/26 pass.
- `cargo clippy --workspace --all-targets -- -D warnings`: exit 0.
- `check_mcp_broker_adapter.py`: `OK` exit 0. `check_tool_descriptor_snapshot.py`: `OK` exit 0.
- `verify.sh quick`: exit 0 (same guard + check set as M001).

Not run (recorded, not implied): full workspace nextest sweep, `lsp-test-support`/`server,plugins` feature tests beyond quick, live provider credentials/models, real remote MCP servers, native macOS/Windows, hosted CI. `cargo test --workspace tool_search/mcp/tool_surface/request_preparation/plugin` as literal package names from plan §9 do not exist as workspace members; the replacement (lib filters + named integration targets above) is recorded per plan §9.

## 5. Invariant review

- One immutable per-turn catalog membership plus policy projection owns registration, discovery, advertisement, and invocation: holds — registry adapters → shared live catalog → surface → definitions + `tool_search` allow-list → broker. `tool_search` never reveals hidden/denied/disabled/missing-backend/plan-excluded tools (allow-list + `is_hidden` gate in both broad and exact paths).
- `tool_search` schema lookup fails safely for unknown names: holds — `catalog.get` miss yields empty → `no_results` (tested for hidden and allow-list miss).
- Untrusted remote definition or plugin hook cannot create authority: holds — adapter validation + surface omission + hook confinement (additions/renames/widening rejected; canonical schema/deferral preserved).
- Native/plugin/MCP names cannot collide; `mcp__` cannot impersonate: holds — strict `__` rejection, namespaced `mcp__` prefix, surface alias-collision errors, registration collision diagnostics.
- No turn advertises a tool whose pinned adapter cannot execute; refresh never mutates in-flight schema/effects: holds — stale/disconnect/drift fail closed pre-transport; refresh is next-turn rebuild; concurrent test proves no rebind.
- Payloads bounded, no credentials in descriptions/schemas/caches: holds — 16 KiB description / 256 KiB schema admission bounds, 10 MB broker bounds, stable hashes exclude secrets, telemetry carries counts/fingerprints only.

## 6. Failure and recovery review

- In-progress calls execute only on pinned adapter + generation match: holds (`check_pinned_locked` pre-transport; broker grant scope additionally verified).
- Disappearance/restart/schema mismatch → typed unavailable/stale, no rebind: holds (stale test: `schema changed`/`no longer advertised`; disconnect test via `disconnect` + drift).
- `tools/list` failure retains in-flight snapshot but blocks new-turn advertisement if unqualifiable: holds by construction — in-flight registry is already built; next-turn registration reads the live inventory (empty on failure → no adapters → nothing advertised). No stale fallback grants access (adapters absent → `NoContract` before transport).
- Simultaneous refreshes coalesced/scoped to future turns: holds — refresh is turn-boundary rebuild; concurrent test shows racing re-registration does not mutate the in-flight registry.
- Cancellation interrupts refresh or call without touching turn authority: holds — adapter entry + in-call selects; snapshot helper uses `try_read` (never blocks); broker/batch preserve cancellation typing.
- Restart rebuilds from fresh activation/connection, never persists unreviewed grants: holds — no durable tool-grant store; per-turn fresh registry; grants are per-call in-memory.

## 7. Migration and compatibility review

No user-facing rename, persistent migration, plugin manifest change, or public provider DTO change. Explicit raw opt-in and managed-eggsearch duplicate avoidance preserved. Full-array fallback for models already receiving complete tool arrays remains compatible (deferral partitioning unchanged; generic/no-deferral providers still get all-immediate). Provider-native deferral is untouched (M004). Rollback is a plain revert with no data migration.

## 8. Security review

- Discovery/hookConfinement: hook additions/renames/widening rejected; canonical schema/deferral preserved; `defer_loading` never cleared; activation rev in cache key prevents stale-hook reuse across activation changes; agent identity in cache key prevents cross-agent surface reuse.
- Injection/collision/widening negatives: covered by `hook_confinement_matrix` + adapter validation + surface alias errors.
- Compliant plugin smoke: no-plugin passthrough (definitions byte-identical when no hook service) + subset-removal survival; a live third-party plugin hook was not exercised (see §10).
- Credentials/PII: none in snapshot, fingerprint, cache, telemetry, or search metadata (names/descriptions/schemas/category/disclosure/risk only; risk derived from empty args, not live inputs).

## 9. Documentation and operations

- Updated: `architecture/agent-tool-surface.md` (external reconciliation + snapshot + hook confinement + guards), `architecture/mcp.md` (policy single origin + generation lifecycle), `architecture/tool.md` (shared exact-schema path), `architecture/plugin.md` (hook confinement contract).
- New guard: `scripts/check_tool_descriptor_snapshot.py` (run manually; wiring into `verify.sh` deferred as a low follow-up like the M001 guard).
- Telemetry: per-build snapshot debug line (generation, revisions, counts, policy, activation, agent, surface fp). Operators distinguish hidden vs disconnected vs denied via `registered/hidden/collisions/rejected` (registration) plus `advertised/omitted` (surface).
- Recovery: stale/unavailable → typed failure; next turn rebuilds. No manual steps, no migration.

## 10. Unresolved findings

| Severity | Finding | Impact | Required action |
|---|---|---|---|
| low | Live provider/remote evidence not run (no credentials, no real stdio/HTTP MCP server, no `tools/list_changed` notification delivery, no slow-server timeout measurement) | Deterministic mock closure only; native deferral stays out of scope (M004) | M004 must qualify wire behavior with byte-level fixtures; any live MCP claim needs its own run. |
| low | Compliant live-plugin hook smoke not run (no third-party plugin with a `ToolDefinition` hook installed) | Hook matrix is deterministic conformance, not a live plugin traversal | A future plugin-hook live test (compliant reword + injection attempt) should exercise both cache-hit and miss paths through a real `PluginService`. |
| low | Neither new guard is wired into `verify.sh`/CI | Regression could land without CI signal | Wire both guards (`check_mcp_broker_adapter.py`, `check_tool_descriptor_snapshot.py`) into `verify.sh quick` + CI `verify` job. |
| low | 40-tool cost uses minimal schemas (5750 vs 1780 bytes) | Saving is real but understates absolute savings for large real-world schemas | M004 benchmark (5/40/100 tools, mixed native/plugin/raw-MCP, varying schema sizes) must record byte-level wire costs. |
| low | Hosted CI + native macOS/Windows not run | Same operational limitation as M001 | Hosted `CI / verify` + Desktop E2E on the closure revision before merge. |

No critical/high/medium findings. No corrective pass required.

## 11. Roadmap disposition

Milestone M002 closed. Both sequenced successors are unblocked on their hard dependencies:

- M003 (`003-descendant-tool-authority-ceilings.md`, hard M002) → `ready` (rebaseline before code change; includes native/plugin/MCP identities per its §2).
- M004 (`004-provider-deferral-wire-qualification.md`, hard M002, soft M003) → `ready` (hard satisfied; M003 remains a soft security-review input, not a gate).

No other registered plan is unblocked by this closure. M003/M004 implementation is out of scope for this task (user requested 001+002 only); they are left `ready` for future handoff, not started here.

## 12. Registry updates

- `plans/registry.md`: move M002 `ready` → `closed` (link this closure); move M003 + M004 `sequenced / blocked` → `ready` (M003: hard M002 satisfied; M004: hard M002 satisfied, soft M003 noted).
- `plans/subsystems/agent-tool-runtime-reconciliation-roadmap.md`: header `M001 closed; M002 ready` → `M001–M002 closed; M003–M004 ready`; milestone table M002 `ready` → `closed`, M003/M004 `sequenced` → `ready`.
- `plans/implementation/agent-tool-runtime-reconciliation/002-external-tool-discovery-and-generation.md`: `ready for handoff` → `implemented; closed` with closure link.
- `plans/implementation/agent-tool-runtime-reconciliation/003-descendant-tool-authority-ceilings.md` + `004-provider-deferral-wire-qualification.md`: `sequenced; blocked` → `ready for handoff` (rebaseline before code change; M004 notes soft M003).
