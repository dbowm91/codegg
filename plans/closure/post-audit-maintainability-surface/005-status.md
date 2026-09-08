# Post-Audit Maintainability and Surface Milestone 005 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/post-audit-maintainability-surface/005-runtime-service-context-global-state-cleanup.md`

Source subsystem roadmap:

- `plans/subsystems/post-audit-maintainability-surface-roadmap.md#7-milestones`

Repository baseline reviewed: `b73f0e0e` (pre-change HEAD; M001–M004 closed)

Implementation commits:

- (this closure) feat(search): explicit runtime-service context, retire search/MCP mutable globals (maintainability M005)

## 1. Executive finding

M005 is complete. Production search/MCP tool execution no longer depends
on the mutable process-global install/get slots in
`src/search_backend/state.rs`. The new explicit runtime-owned
`SearchRuntimeContext` (`src/search_backend/context.rs`) carries an owned
immutable `SearchConfig` snapshot plus the shared daemon-owned
`McpService` handle; it is threaded through the existing
`ToolRegistryOptions` seam (`search_runtime` field, following the
established scheduler/run-store/workspace-root style — no DI framework,
no `AnyMap`, no service locator, no new crate) into all eleven
search/evidence wrapper tools, the deep-research eggsearch adapter, and
the agent-loop capability gates. Two independently constructed runtime
contexts with different search configurations coexist in one process
without cross-talk (proven by the new
`tests/search_runtime_isolation.rs`, 9 tests, lock-free including a
`tokio::join!` concurrency case). Test serialization debt that existed
solely to mutate the search/MCP globals is removed from all execution-
path suites (28 + 11 + 4 + 9 tests migrated, locks deleted); the
cross-process flock remains only for the bootstrap compat tests that
genuinely assert the retained legacy install slots. Search backend
semantics (eggsearch/builtin/disabled, fallback, trust framing, output
caps), structured provenance, permissions, and MCP cancellation/timeout
behavior are unchanged. No downstream plan was blocked on M005; the
roadmap completion definition (§11) is now fully met (M001–M005 closed).

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| M002/M003 dependency contracts satisfied | M002 closed (`002-status.md`: disclosure contract is `src/tool/disclosure.rs` + `with_options` ownership); M003 closed (`003-status.md`: `AgentLoop::new` + setters). M005 consumes both seams unchanged (§3.6) | pass |
| Production search/MCP execution independent of global install/get slots | `rg 'search_backend::state::'` in `src`/`tests` outside `src/search_backend/`: zero hits. `rg 'install_mcp_service\|install_search_config\|reset_for_tests'` in `src` outside `src/search_backend/`: zero hits | pass |
| Typed runtime-service context, no generic bag | `SearchRuntimeContext { config: SearchConfig, mcp: Option<Arc<RwLock<McpService>>> }` — two concrete fields, no `Any`/string keys; `ToolRegistryOptions::search_runtime: Option<SearchRuntimeContext>` | pass |
| Two independent contexts coexist without cross-talk | `tests/search_runtime_isolation.rs`: interleaved + concurrent + registry-level independence tests, all lock-free | pass |
| Tests no longer serialized solely for migrated globals | `fake_eggsearch_mcp`, `search_backend_arg_mapping`, `search_backend_legacy`, `search_backend_eggsearch`, `search_backend::{mod,eggsearch}` unit tests migrated; `lock()`/`reset_for_tests`/flock usage deleted there | pass |
| Backend semantics/provenance/permissions/cancellation intact | Focused suites green (§4); `ensure_tool_available` WouldBlock path preserved and unit-tested via explicit handle; `call_structured_tool_with_service` keeps timeout + framing/cap behavior byte-identical | pass |
| Immutable/singleton state retained where justified | Inventory §3.1: regex/`OnceLock` caches, `DEFAULT_REGISTRY`, daemon singleton, env/file locks retained with reasons; only the proven workaround class migrated | pass |
| No DI framework/locator/new runtime owner | No new crate; no `AnyMap`; construction order fixed (bootstrap → registry) instead of hidden delayed-init handle; `AgentLoop::new` signature unchanged | pass |
| No secret/provider config in model-facing metadata | `SearchRuntimeContext` is never serialized; manual `Debug` redacts `[search.eggsearch.env]` values (keys only, unit-tested); provenance shapes unchanged | pass |
| Docs updated | `architecture/search_backend.md` (context vs legacy sections), `architecture/tool.md` (options/registry/constructor rows), `architecture/mcp.md` (startup ownership), `architecture/exec.md` (bootstrap ordering), `test_support.rs` posture note | pass |

## 3. Production implementation evidence

### 3.1 Mutable-global inventory (WP-A) with disposition

Classification per plan §3 (`immutable` / `singleton-by-contract` /
`mutable-runtime-workaround`); only the third class is in scope.

Migrated:

| Slot | Defect proven | New owner |
|---|---|---|
| `search_backend::state::{MCP_SERVICE, SEARCH_CONFIG}` + `install_*/reset_for_tests` | Runtime-dependent values overwritten per test/turn; construction-order workaround documented in source comments | `SearchRuntimeContext` (owned config snapshot + shared daemon MCP handle); globals retained as deprecated bootstrap-reuse/legacy-wrapper compat (see §7) |
| `search_backend::test_support::{SHARED_TEST_LOCK, acquire_cross_process_lock}` execution-path usage | Serialization existed solely to guard the slots above | Deleted from all execution suites; retained for bootstrap compat tests only |

Retained by contract or cache semantics (explicitly out of scope,
recorded here per plan §7-E):

| Slot | Reason not migrated |
|---|---|
| `tool::DEFAULT_REGISTRY: Lazy<ToolRegistry>` | Immutable singleton constructed once, never mutated; now holds an isolated default search context |
| Regex/`Lazy` tables (bash policy, destructive, search providers, TUI syntax/theme) | Immutable compiled constants, not runtime-dependent |
| `codegg-core` `DEFINITIONS`, redactor `OnceLock`s, provider resolution cache | Immutable catalogs / memoization caches, not service identity |
| `codegg-core` bus `PERMISSION_REGISTRY`/`QUESTION_REGISTRY` | Sync registries by product contract (registration-before-publish invariant; AGENTS.md documents sync nature) |
| `security/sandbox.rs` `CANONICAL_PATHS_CACHE`, `plugin` `WASM_CACHE` | Content caches, not runtime-dependent service identity |
| `test_runner` `INDEX_LOCK`, `tui` `DIFF_SEMAPHORE` | Genuinely process-global external resources (file index, render semaphore) |
| `auth`/`providers` `ENV_LOCK`s, `tests/common/pool` `SHARED_POOL` | Process-global env vars / documented test fixture with cleanup contract — independent of runtime composition |
| Daemon singleton (`daemon.lock`) | Explicit non-goal: user-scoped daemon remains authoritative |
| `egglsp` `self_ref: OnceLock`, eggpool `RefreshCell` | Per-instance/per-key memoization, not process-global service slots |

No other mutable service/config slot met the migration bar, so WP-E
added no further migrations — the milestone stays one coherent pass.

### 3.2 Explicit service seam (WP-B)

- `src/search_backend/context.rs` (new): `SearchRuntimeContext` with
  `new/from_config/disabled/with_mcp/with_mcp_opt` constructors,
  `config()/mcp()/has_mcp_service()/backend()/server_name()` accessors,
  secret-redacting `Debug`, and `install_as_global_compat` (the only
  sanctioned writer besides bootstrap).
- `src/search_backend/mod.rs`: all 9 `dispatch_*`, all 10
  `dispatch_*_structured`, and all 9 `provenance_for_*` functions are now
  `SearchRuntimeContext` methods (single canonical path). The free
  functions remain as `legacy_*` macro-generated thin wrappers over a
  global snapshot for backward-compatible callers.
- `src/search_backend/eggsearch.rs`: the single choke point
  `call_structured_tool_with_service(Option<&McpHandle>, …)` plus
  explicit-service `ensure_tool_available_with_service` /
  `call_provider_status_with_service`; all 18 adapter functions take the
  explicit handle. The dead global choke wrapper was deleted.

### 3.3 Production consumer migration (WP-C)

- 11 wrapper tools (`websearch`, `webfetch`, `repo_search`,
  `repo_fetch`, `repo_map`, `security_search`, `research_search`,
  `batch_fetch`, `evidence_bundle`, `codesearch`, plus `research` via
  the service chain below) hold `search_runtime` with
  `with_search_runtime()` builders; `Default` is an isolated
  disconnected context, never a global read.
- `ToolRegistryOptions::search_runtime` + `ToolRegistry::search_runtime()`;
  `with_options` wires every wrapper; `with_config` /
  `with_session_config_defaults` / `build_session_tool_registry` derive
  isolated config contexts; new `with_config_and_search_runtime` serves
  bootstrapped startup paths (exec, single-shot run).
- Deep research: `EggsearchSource::with_search_runtime` (explicit with
  global fallback for legacy direct construction),
  `ResearchCoordinator::with_search_runtime` (name-matched adapter swap),
  `ResearchService::with_search_runtime` (consuming builder),
  `ResearchTool::with_search_runtime` (`Arc::try_unwrap` with warn-and-
  fallback when shared).
- Agent loop: `AgentLoopServices::search_runtime` derived in
  `AgentLoop::new` from its existing `Config` + MCP inputs (**no
  signature change**, M003 seam preserved);
  `resolve_native_backend`, `compute_model_flags` (now takes an explicit
  backend), and the MCP exposure-policy assembly read it.
- Bootstrap: new `bootstrap_search_runtime()` returns
  `(SearchRuntimeContext, BootstrapReport)`; turn runtime and subagent
  spawn bootstrap **before** registry construction (order fix); exec and
  single-shot run use `with_config_and_search_runtime`. The shared MCP
  transport is still one daemon-owned connection reused across entry
  points (no per-turn eggsearch process spawn).
- TUI `/tool-backends` no longer touches the global slot (reports
  config truth; live server list remains `/mcps`' job).

### 3.4 Construction/ownership narrative

`config → bootstrap_search_runtime → (SearchRuntimeContext + report) →
ToolRegistryOptions::search_runtime → per-tool clones + registry snapshot
→ AgentLoop::new derives loop-level context from same inputs`.
Mutable MCP connection state stays inside the daemon-owned `McpService`
(its own internal synchronization); immutable search configuration is
snapshotted per runtime. Teardown: dropping a context never tears down
the shared service (daemon-owned). Restart reconstructs from
config/bootstrap; no stale value can be observed. Per-call global
`StdRwLock` lookups are eliminated (tools hold the `Arc` directly).

## 4. Verification executed (commands + results; local unless noted)

- `cargo check --workspace --all-targets` — clean, zero warnings.
- `cargo fmt --all -- --check` — clean.
- `cargo clippy --workspace --all-targets` — clean (three
  `derivable_impl`/`field_reassign`/`type_complexity` findings fixed, not
  suppressed, except one justified `#[allow(clippy::type_complexity)]` on
  a test helper with a named `RecordedCalls` alias).
- `cargo clippy --workspace --all-targets --features server,plugins,lsp-test-support` — clean.
- `scripts/verify.sh quick` — passed (agents check, core boundary,
  sandbox contract, execution ownership, locked workspace check).
- `cargo test -p codegg --lib search_backend` — 82 passed.
- `cargo test -p codegg --lib tool::` — 546 passed.
- `cargo test -p codegg --lib research` — 123 passed; `mcp` — 8 passed;
  `websearch`/`webfetch` lib — 6 + 6 passed.
- `cargo test --test search_runtime_isolation` (new) — 9 passed:
  coexistence, concurrency (`tokio::join!`, lock-free), injected-service
  tool use, disabled-stays-disabled with live handle, reconstruction,
  unavailable-not-builtin, registry independence, per-backend provenance,
  no-silent-fallback on upstream failure.
- `cargo test --test fake_eggsearch_mcp` — 28 passed (fully migrated off
  globals/locks, incl. structured-value retention across all 9 wrappers
  and `EggsearchSource` with explicit runtime).
- `cargo test --test search_backend_arg_mapping` — 11 passed;
  `--test search_backend_legacy` — 4 passed;
  `--test search_backend_eggsearch` — 9 passed.
- `cargo test --test tool_registry` — 12 passed;
  `--test tool_surface_minimization` — 9 passed;
  `--test tool_structured_execution` — 13 passed;
  `--test agent_loop_harness` — 40 passed.
- `rg` evidence: zero production execution-path references to removed
  global APIs (§2 row 2).
- Static guards: `check-core-boundary.sh`, `check_daemon_cwd_usage.py`,
  `check_execution_ownership.py`, `check_scheduler_bypass.py`,
  `check_sandbox_contract.py` — all passed. No new guard added: the
  isolation test suite is the durable ownership enforcement (a source
  guard was considered per plan §6 but tests are preferable while the
  legacy wrappers intentionally still exist).
- No hosted `CI / verify` run: no daemon, scheduler, protocol, config-
  schema, or release change; local quick + feature-gated Clippy +
  focused suites is the proportionate posture per the roadmap (same
  basis as M001–M004).

## 5. Invariant review

- Daemon remains canonical owner of daemon-scoped services: the MCP
  transport is shared daemon-owned; only the config snapshot is
  per-runtime.
- Tool construction stays through `with_options`; no service locator
  beside it (one optional typed field, existing style).
- Search execution stays owned by the search/eggsearch boundary
  (adapter signatures widened, ownership unchanged).
- MCP lifetime explicit and safely shared; sessions receive the shared
  handle (a documented, plan-sanctioned option), never narrower
  per-session transports that would multiply server processes.
- No cross-runtime config observation (isolation tests).
- Context immutable after construction; no refresh mechanism invented.
- No secrets in model-facing metadata (redacting `Debug`, unchanged
  provenance; `summarize_provider_status` secret hygiene untouched).
- No giant test-global mutex introduced (locks deleted, not replaced).
- Immutable caches/constants retained (§3.1).
- No generic `Any`/string-keyed service bag (§2 row 3).

## 6. Failure and recovery review

- Bootstrap failure preserves existing actionable semantics
  (`eggsearch_unavailable` / missing-tool errors; unavailable-not-builtin
  without opt-in fallback, tested).
- Disabled constructs an explicit disabled state, never an absent global
  with multiple meanings (tested, including with a live handle present).
- Dropping one context cannot invalidate another (shared `Arc`; tested
  via reconstruction).
- Concurrent calls share only the daemon service's own locks; the extra
  global `StdRwLock` hop per call is gone.
- Existing MCP cancellation/timeout paths preserved verbatim in
  `call_structured_tool_with_service` (timeout mapping, WouldBlock
  deferral in `ensure_tool_available_with_service`).

## 7. Migration and compatibility review

- No user config syntax change; `Eggsearch`/`Builtin`/`Disabled`
  semantics and defaults unchanged; existing search config files parse
  identically (schema untouched).
- Internal constructors gained optional parameters only:
  `ToolRegistryOptions::search_runtime` (`Option`), tool
  `with_search_runtime()` builders, `with_config_and_search_runtime`
  (additive), research `with_search_runtime` builders.
  `AgentLoop::new` signature unchanged. `Tool` trait unchanged.
- Legacy global free functions (`dispatch_*`, `provenance_for_*`,
  `ensure_tool_available`, `call_provider_status`) retained as thin
  wrappers; M001 canonical/deferred tool behavior unaffected.
- Retained globals and why: `state` slots — daemon cross-entry-point
  connection reuse + legacy-wrapper snapshot source (writers: bootstrap
  and `install_as_global_compat` only); `test_support` locks — bootstrap
  compat tests asserting those slots; deep-research `None`-runtime path
  — backward compatibility for direct `EggsearchSource::new()` callers
  (production registry path always provides explicit).
- Future removal: once all external/legacy callers move off the free
  wrappers, `state.rs` + wrappers can be deleted; the isolation suite
  will catch any regression first. Deliberately not done here to keep
  this pass to one coherent ownership change.

## 8. Security review

- Capability/policy filtering authoritative and unchanged: explicit
  handles flow through the ordinary `ToolBroker`/permission path; a tool
  cannot reach MCP except via its registry-given context, and
  `execute_capture` provenance still records backend/implementation.
- Negative tests: disabled-with-live-handle errors without invoking MCP;
  unavailable/unauthorized paths return errors, never a fallback backend
  (isolation suite + `no_fallback_surfaces_eggsearch_error`).
- Diagnostics contain no secret configuration (`Debug` redaction test;
  `summarize_provider_status` untouched).
- Failed bootstrap does not fall back to an unintended backend
  (`eggsearch_without_service_is_unavailable_not_builtin`,
  `failing_mock_does_not_fall_back_without_opt_in`).

## 9. Documentation and operations

- `architecture/search_backend.md`: canonical context section vs legacy
  state section, bootstrap entry points, testing posture.
- `architecture/tool.md`: options field, registry accessor,
  constructor table, wiring narrative.
- `architecture/mcp.md`: startup ownership wording.
- `architecture/exec.md`: bootstrap ordering wording.
- `src/search_backend/test_support.rs`: retained-lock scope note.
- `state.rs`/`context.rs` module docs: ownership contract + deprecation
  guidance for future readers.
- No operator/procedure change: no behavior, protocol, or config change.

## 10. Unresolved findings

No critical/high/medium/low findings requiring a corrective pass. Notes
(intentional, recorded per plan §15):

1. `bootstrap_search_runtime` still installs the legacy slots as a side
   effect (connection-reuse role). Per-turn global search-config churn
   therefore still occurs, but no production reader observes it; full
   deletion awaits legacy-wrapper removal (§7).
2. Concurrent runtimes with different `server_name`s share one daemon
   MCP transport (pre-existing reentrant-bootstrap semantics, now
   documented in code and architecture docs rather than changed).
3. `ResearchTool::with_search_runtime` warns and keeps global fallback
   when its service `Arc` is shared elsewhere; all production registry
   paths hold unique ownership so the explicit path applies.
4. `coordinator.with_search_runtime` swaps the eggsearch adapter by its
   stable `"eggsearch"` name; a rename would need updating the swap
   predicate and its tests together.

## 11. Roadmap disposition

M005 meets all exit conditions in
`plans/subsystems/post-audit-maintainability-surface-roadmap.md#7-milestones`:
production search/web tools consume explicit runtime-owned references;
two independently constructed contexts coexist without cross-talk; test
serialization solely for the migrated globals is removed; immutable
caches remain global only where justified; no DI framework added.

Dependency audit (per planning-skill unblock check): M005 listed a hard
dependency on M002 (closed) and an interface dependency on M003 (closed);
both held. No registered plan lists M005 as a hard or interface
dependency — the registry's dependency-ready table contains only
provider-auth M010 (independent; untouched by this milestone), and the
post-audit execution-order batch (CI M010, provider-auth M010) is
independent per the registry's parallel-work rule. No blocked milestone
is unblocked by this closure and none is newly blocked. With M001–M005
all closed, the roadmap §11 completion definition is fully met
(canonical compatibility/tool surface, smaller default palette with
discoverability, decomposed agent/Bash modules, no mutable-global
search/MCP installation in normal production construction, no new
framework), so the subsystem roadmap itself returns to `closed`.

Recommendation: closed; subsystem roadmap closed.

## 12. Registry updates

- `plans/registry.md`: subsystem row → `closed` with M001–M005 closure
  links; M005 removed from the dependency-ready table (provider-auth
  M010 retained); closure control-point row added for M005.
- `plans/subsystems/post-audit-maintainability-surface-roadmap.md`:
  Status → closed; M005 row → closed with closure link.
- `plans/implementation/post-audit-maintainability-surface/005-runtime-service-context-global-state-cleanup.md`:
  status → closed.
