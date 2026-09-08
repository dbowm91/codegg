# Post-Audit Maintainability and Surface Milestone 005 — Runtime-Service Context and Mutable-Global Cleanup

Status: closed

Closure record: `plans/closure/post-audit-maintainability-surface/005-status.md`

Repository baseline: `15632a0483a8c4b9d573ff2ce43297b29be8f42a`

Source roadmap:

- `plans/subsystems/post-audit-maintainability-surface-roadmap.md#7-milestones`

Dependencies:

- hard: M002 — final model/tool construction and disclosure contract (satisfied: closed by `plans/closure/post-audit-maintainability-surface/002-status.md`; disclosure contract is `src/tool/disclosure.rs` + `ToolRegistry::with_options` registration ownership);
- interface: M003 — final agent construction/module seams (satisfied: closed by `plans/closure/post-audit-maintainability-surface/003-status.md`; seams are `AgentLoop::new` + setters, `definition`/`file_agents` public paths, `ToolRegistry::with_options`).

Long-term requirements:

- `plans/000-long-term-specification.md#42-explicit-ownership`
- `plans/000-long-term-specification.md#45-locality-by-default`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/000-long-term-specification.md#7-current-foundation-and-required-evolution`
- `plans/002-long-term-roadmap.md#phase-19--operational-hardening-and-scale-closure`

Applicable ADRs:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md` remains unchanged.

Primary class: infrastructure / invariant

## 1. Objective

Replace mutable process-wide installation of runtime services/configuration with explicit runtime-owned references where current globals prevent independent composition, force test serialization/reset, or blur daemon/session ownership. Start with search/MCP state, then migrate only closely related mutable service/config globals proven by an inventory to have the same problem.

The result should allow independently constructed runtime contexts to coexist without overwriting one another while retaining simple immutable globals/caches where they are genuinely process-wide.

## 2. Why this milestone is blocked

`ToolRegistry` construction/disclosure is being rationalized by M002, and agent construction/module seams are being clarified by M003. M005 should consume those stable seams instead of threading services through code that is simultaneously moving.

Once those dependencies are satisfied, the primary defect is already concrete. At the audit baseline, `src/search_backend/state.rs` contains mutable process-global slots:

```rust
static MCP_SERVICE: StdRwLock<Option<Arc<RwLock<McpService>>>> = StdRwLock::new(None);
static SEARCH_CONFIG: StdRwLock<Option<SearchConfig>> = StdRwLock::new(None);
```

Production startup installs these after `ToolRegistry::with_defaults()` has already constructed web/search wrappers; tests overwrite them and call `reset_for_tests()`. The source comments explicitly describe the global as a workaround for registry construction order.

This is therefore a bounded ownership/lifecycle problem, not a speculative dependency-injection project.

## 3. Current implementation evidence

The implementer must inspect:

- `src/search_backend/state.rs`, bootstrap, eggsearch/legacy backend modules;
- `src/tool/websearch.rs`, `webfetch.rs`, `repo_search.rs`, research/evidence wrappers and their service lookup paths;
- `ToolRegistryOptions`, `ToolRegistry::with_options`, session-specific registry construction, and post-bootstrap wiring;
- MCP service construction/ownership in daemon/core startup;
- model/profile tool-definition construction after M002;
- tests that use `reset_for_tests`, cross-process/shared locks, environment locks, or forced serial execution because of search/MCP mutable globals;
- other mutable `static`, `Lazy<Mutex/RwLock<...>>`, install/reset APIs, or singleton registries in runtime code.

The inventory must distinguish:

```text
true immutable/process singleton
  examples: constant tables, immutable default metadata, read-only compiled catalog

process-wide resource by product contract
  examples: a user-scoped daemon singleton where one instance is intentionally authoritative

mutable runtime service/config workaround
  examples: install/reset slot whose value depends on constructed runtime/config and is overwritten in tests
```

Only the third class is presumptively in scope.

## 4. Invariants that must not regress

- The daemon remains the canonical owner of daemon-scoped services.
- Tool construction remains through the accepted registry/options path; M005 does not create a service locator beside it.
- Search backend execution remains owned by the accepted search/eggsearch boundary.
- MCP service lifetime remains explicit and shared safely where one daemon owns it.
- A turn/session cannot observe another independently constructed runtime's search configuration merely because both exist in one process/test binary.
- Runtime service references are immutable after construction unless the owning subsystem already defines a versioned refresh mechanism.
- No secret/provider configuration is copied into model-facing metadata.
- Test isolation improves without introducing one giant test-global mutex.
- Immutable caches/constants need not be removed merely because they are `static`.
- Runtime service context must not become a generic bag of `Any` or untyped string-keyed services.

## 5. Scope

### In scope

- Inventory mutable service/config globals and install/reset APIs with runtime-dependent values.
- Introduce the smallest typed runtime-service context/options seam needed by current tools.
- Make search/MCP dependencies explicit in production tool construction.
- Remove normal-production reliance on `search_backend::state::install_mcp_service`, `install_search_config`, and `reset_for_tests` style state.
- Update search/research/web tool constructors to hold explicit immutable/shared references as appropriate.
- Reduce/remove tests serialized solely because they mutate those globals.
- Migrate other mutable runtime service slots only when the inventory proves the same ownership defect and the migration fits one coherent pass.
- Update architecture/testing docs.

### Explicitly out of scope

- Eliminating every global/static.
- Replacing the user-scoped daemon singleton.
- A framework-level DI container, `AnyMap`, service locator, macro-generated wiring, or new crate.
- Provider HTTP-client unification.
- Persistent search indexing.
- Dynamic hot-replacement of search/MCP services during an in-flight turn unless an accepted existing contract already requires it.
- Rewriting MCP transport/runtime.
- Configuration schema redesign.
- General test parallelization as a goal independent of ownership cleanup.

## 6. Required production changes

### Core/domain

Prefer extending existing typed construction inputs. `ToolRegistryOptions` already carries runtime-specific dependencies such as scheduler submission, run store, workspace root, asset snapshot, and LSP service. Search/MCP runtime dependencies should follow the same style or be grouped into a narrowly typed `SearchRuntimeServices`/`RuntimeServices` member if direct fields become unwieldy.

A good shape has these properties:

- constructed after config/MCP bootstrap or supports two-stage construction without global mutation;
- cloned/shared by `Arc` only for actual shared services;
- configuration values are owned/immutable snapshots where cheap;
- tools retain only the dependencies they need;
- production tools never query a mutable process-wide slot at execution time.

If registry construction currently must occur before MCP bootstrap, change the construction order or inject a stable explicit handle whose internal initialization has one daemon owner. Do not recreate the same global as an `Arc<RwLock<Option<_>>>` hidden inside a generic context unless the lifecycle truly requires delayed initialization and is scoped to one runtime instance.

### Storage and migrations

No production migration expected.

### Protocol and DTOs

No protocol change expected. Runtime service context is internal and must not be serialized wholesale.

### Runtime and concurrency

Explicitly define:

- daemon construction order for config → MCP/search bootstrap → tool registry/session runtime;
- ownership of mutable MCP connection state versus immutable search configuration;
- whether session registries share one daemon MCP service or receive narrower handles;
- teardown behavior when daemon/runtime drops;
- lock granularity for MCP itself versus service lookup.

The desired reduction is removal of global lookup locks, not replacement with more per-call locks.

### Frontend or operator surface

No behavior change expected. Diagnostics reading search backend state must receive it through daemon/runtime state rather than global lookup.

### Security and authorization

Tool capability/policy filtering remains authoritative. An explicit service reference must not allow a tool to bypass broker/permission policy simply because it has direct access to MCP.

Do not store credentials or unrestricted MCP service handles in broadly serializable/debug-printable contexts.

### Documentation and static guards

Update `architecture/search_backend.md`, `architecture/tool.md`, daemon/service construction docs, and test notes. A narrow source guard against reintroducing production calls to removed `install_*` APIs may be appropriate if the APIs are deleted entirely; otherwise tests are preferable.

## 7. Ordered work packages

### Work package A — Mutable-global inventory and scope lock

Intent: keep the milestone evidence-driven.

Required actions:

1. Search production/test source for mutable static/Lazy slots, install/set/reset/swap APIs, and test-global locks.
2. Record owner, value type, mutation timing, production callers, test callers, and whether independent runtime instances should be possible.
3. Classify each as immutable/singleton-by-contract/mutable-runtime-workaround.
4. Lock scope to search/MCP plus only directly analogous high-value slots that fit the milestone.

Acceptance evidence:

- inventory and explicit out-of-scope globals;
- no blanket “remove all globals” objective.

### Work package B — Define explicit search/MCP runtime services

Intent: establish typed construction ownership.

Required changes:

- select/extend the existing `ToolRegistryOptions`/session-runtime construction seam;
- represent resolved `SearchConfig` explicitly;
- represent shared daemon-owned MCP/search execution service explicitly;
- define constructor order and optional/disabled backend semantics.

Acceptance evidence:

- two independent service contexts can be created in one process with different search configurations;
- neither writes process-global runtime state.

### Work package C — Migrate production consumers

Intent: eliminate execution-time global lookup.

Required changes:

- websearch/webfetch/repo/research/evidence tools consume injected services/config;
- agent model flags/capability gates consume their runtime/profile context rather than global `search_config()`;
- diagnostics and bootstrap callers use explicit daemon/runtime state;
- delete or production-gate obsolete install/get APIs when all production callers move.

Acceptance evidence:

- `rg` shows no normal production execution path calls removed global search state APIs;
- search enabled/disabled/eggsearch/builtin behavior remains covered.

### Work package D — Remove test mutation/serialization debt

Intent: prove composition benefit.

Required changes:

- rewrite tests that call `reset_for_tests()` or serialize solely around search/MCP state to construct isolated service contexts;
- retain serialization only for genuinely process-global external resources such as environment variables or fixed ports where unavoidable;
- add an isolation test constructing two differently configured registries/runtimes concurrently or interleaved.

Acceptance evidence:

- mutable-global reset usage removed for migrated services;
- cross-context isolation test passes.

### Work package E — Optional analogous global cleanup and docs

Intent: migrate only clearly identical ownership defects discovered in WP-A.

Required changes:

- at most a small number of directly analogous runtime service slots;
- otherwise record them for later rather than expanding scope;
- update architecture/testing docs.

Acceptance evidence:

- closure inventory clearly distinguishes migrated, retained-by-contract, and deferred globals.

## 8. Failure, cancellation, restart, and contention semantics

Construction failure must fail the affected runtime/service explicitly; it must not leave a partially installed process-global value visible to unrelated contexts.

If eggsearch/MCP bootstrap fails and configuration requires it, preserve existing actionable failure semantics. If search is disabled, construct an explicit disabled/no-service state rather than relying on an absent global meaning multiple things.

Dropping one independent runtime context must not invalidate another. Shared daemon MCP service teardown follows daemon ownership and must not be triggered by dropping an individual tool/registry clone.

Runtime restart reconstructs services from config/bootstrap; no global stale value may survive to be observed by the new instance in-process tests.

Concurrent tool calls may share the same daemon MCP service using its existing synchronization. They should no longer contend on an extra global `StdRwLock` merely to retrieve the pointer/config.

## 9. Compatibility and migration

No user config syntax change is required. Existing search backend choices (`Eggsearch`, `Builtin`, `Disabled`) and their defaults retain semantics.

Internal constructors may gain required service/context parameters. Preserve public/downstream constructors only if intentionally supported; otherwise workspace call sites should migrate in one pass.

M001 compatibility decisions remain authoritative for literal tool names. M005 does not resurrect removed aliases.

## 10. Required tests

### Focused unit tests

- runtime service context construction for enabled/disabled/builtin/eggsearch configurations;
- tool constructor behavior with/without required services;
- no default/global fallback when explicit runtime says disabled.

### Integration tests

- two registries/runtimes with different search configs coexist without cross-talk;
- eggsearch-backed search tool uses its injected MCP service;
- disabled context remains disabled even if another context has search enabled;
- research/evidence wrappers retain structured result/provenance behavior.

### Restart and recovery tests

- destroy/reconstruct a runtime context in one process and prove no previous config/service is observed.

### Contention and cancellation tests

- concurrent search calls share only the intended daemon service locks;
- no new deadlock from construction/service handles;
- existing MCP call cancellation/timeouts preserved.

### Security and negative tests

- unavailable/unauthorized tools cannot use an injected service to bypass normal tool policy;
- debug/diagnostic rendering of runtime services contains no secret configuration;
- failed bootstrap does not fall back to an unintended backend.

### Migration and compatibility tests

- existing search config files parse identically;
- M001/M002 canonical/deferred tool behavior unaffected except for explicit service wiring.

## 11. Required verification commands

After M002/M003 dependencies are satisfied, select current test names. Expected commands include:

```bash
cargo test -p codegg search_backend
cargo test -p codegg tool::websearch
cargo test -p codegg tool::webfetch
cargo test -p codegg tool::research
cargo test -p codegg mcp

# prove old mutable global production references are gone
rg 'install_mcp_service|install_search_config|reset_for_tests|search_backend::state::' src tests

cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
```

The `rg` result may legitimately contain historical/test compatibility during intermediate work; closure must explain any surviving live references.

## 12. Documentation updates

- `architecture/search_backend.md` — explicit runtime service ownership and construction.
- `architecture/tool.md` — registry/options service injection.
- Daemon/startup architecture docs — bootstrap ordering.
- Test documentation if shared locks/serialization assumptions change.
- Remove comments claiming global state is required once it is no longer true.

## 13. Acceptance criteria

- M002 and M003 dependency contracts are satisfied.
- Production search/MCP tool execution no longer depends on mutable process-global install/get slots.
- Two independent runtime contexts with different search configuration can coexist without cross-talk.
- Tests no longer require reset/serialization solely to mutate migrated service globals.
- Search backend semantics, structured provenance, permissions, and cancellation remain intact.
- Immutable/process-singleton state is retained where justified rather than mechanically eliminated.
- No DI framework, service-locator crate, generic untyped context, or new runtime owner is introduced.

## 14. Stop conditions

Stop and report when:

- M002 is not closed or M003 construction interfaces are still unstable;
- migration requires changing canonical daemon/MCP/search ownership;
- the only proposed design is a new generic DI/service-locator framework;
- a mutable global is actually required by a documented one-process singleton contract and independent instances are nonsensical;
- runtime config hot-reload semantics would need to be invented;
- public protocol/config migration becomes necessary;
- concurrent implementation materially changes `ToolRegistryOptions` or daemon bootstrap and cannot be safely rebased.

## 15. Closure evidence required

- implementation commits/PRs;
- mutable-global inventory with migrated/retained/deferred disposition;
- explicit runtime-service construction/ownership diagram or concise narrative;
- source-search evidence for removed production global APIs;
- cross-context isolation test;
- restart/reconstruction test;
- search/MCP/research focused tests and outcomes;
- contention/cancellation/security evidence where touched;
- formatting/lint/quick verification outcomes;
- list of test-global locks/reset helpers removed or retained and why;
- unresolved globals with explicit reason not to migrate.

## 16. Handoff notes

The search global exists because of historical construction order, not because search is conceptually process-global. Fix the construction ownership rather than wrapping the same mutable slot in a new type.

At the same time, do not treat every singleton as debt. CodeGG deliberately has a user-scoped daemon owner. The target is explicit runtime-dependent service ownership, not ideology about `static` usage.
