# Agent Tool Runtime Reconciliation M002 — External Tool Discovery and Generation

Status: sequenced; blocked until M001 closure

Repository baseline: `8535f6d7c2e68f5d134ba0c73bd146ca8bc0bd03`

Source roadmap: `plans/subsystems/agent-tool-runtime-reconciliation-roadmap.md#6-dependencies-and-milestones`

Long-term requirements: `plans/000-long-term-specification.md#46-progressive-disclosure`, `#47-correctness-before-transparent-magic`, `#27-security-requirements`, `#29-system-invariants`; `plans/003-planning-process.md`.

Applicable ADRs: current tool surface and MCP ownership; none new if those owners remain unchanged.

Primary class: capability

## 1. Objective

Ensure every registered and authorized third-party MCP tool is discoverable by `tool_search` with its **correct schema**, whether immediate or deferred, while forbidden/unavailable/stale tools do not leak into discovery or invocation. Make connection and tool-list refresh produce coherent per-turn descriptor/surface generations. Prevent late model-facing schema hooks from advertising names that cannot execute or names excluded by policy.

## 2. Why this is sequenced

M001 creates the single registered MCP adapter and broker dispatch boundary and is a **hard** dependency. Existing live `ToolCatalog`, `tool_search` and `ResolvedToolSurface` are closed foundations; this milestone must compose rather than replace them. It may be written/reviewed now but implementation must verify M001 closure and rebaseline before changing code.

## 3. Current evidence and target seam

- `src/agent/request_preparation.rs` currently appends `McpService::list_filtered_tools()` directly to provider definitions, then consults `ToolCatalog::is_deferred`, while `ToolCatalog::register()` indexes only registered `Tool` instances. This can advertise names not represented in the actual discovery catalog.
- `src/tool/tool_search.rs` ranks `ToolCatalog` descriptors and resolves exact-name schema, filtered by a list of available names; availability alone cannot populate absent metadata.
- `src/tool/catalog.rs` is shared/live, which repaired earlier native late-registration omissions. External descriptors must enter the same catalog, not another search index.
- `src/agent/request_preparation.rs` applies plugin `dispatch_tool_definition()` after surface resolution in both cached and uncached paths; replacement `tools` arrays are not shown to be revalidated through all denial/capability/registry/collision checks.
- `src/mcp/mod.rs` retains namespaced server identity, origin, connection status, per-server descriptor set and schema. Existing `mcp_tool_surface_revision` hashes model-facing definitions but can change between registry installation and request build unless generation ownership is pinned.
- The existing engine already tracks immediate/deferred separation and schema cache; preserve provider alias handling and role-specific palettes.

## 4. Invariants

Registration, discovery, advertisement, and invocation derive from **one** immutable per-turn catalog membership plus policy projection. `tool_search` never reveals tools hidden by user, model, plan mode, child ceiling, plugin activation or backend absence; schema lookup fails safely for unknown names. An untrusted remote tool definition or plugin tool-definition hook cannot create invocation authority. Native, plugin and MCP tool names cannot collide; `mcp__` cannot impersonate another scope.

No turn continues to advertise a tool whose pinned adapter cannot execute; no remote descriptor refresh silently changes an in-flight turn's schema or effects. Fresh turns may see refreshed tools, but stale tool-call IDs/names are rejected. Payloads retain bounded sizes, no credentials in descriptions, schema or caches.

## 5. Scope

### A — One canonical session descriptor projection

With the M001 adapters installed before broker construction, remove redundant external append paths and integrate tool metadata through the existing `ToolRegistry::register()` -> shared `ToolCatalog` route. `ToolSearchTool` indexes those descriptors with exact schema and category/effect; preserve native `Tool::expose_in_definitions()`, `defer_loading()` and scoped `has_functional_backend()` semantics. Distinguish catalog membership from eligible-in-this-turn membership in diagnostics and tests. Do not add a database, global registry or separate MCP search engine.

Expose third-party MCP tools in Curated/Minimal as deferred or through compact `tool_search` metadata, not as permanently invisible; Full mode and no-native-deferral providers must remain functional. Validate model-name/wire aliases against collisions before publishing.

### B — Snapshot/refresh generation

Create explicit immutable session/turn `ToolDescriptorSnapshot` (or reuse existing snapshot owner) carrying server origin/identity, connection or descriptor generation, schema fingerprint, permitted tool names, policy revision, and resolved surface fingerprint. No raw credentials inside hash/log. Define lifecycle on server add/remove/reconnect, `tools/list_changed`, plugin MCP contribution activation/deactivation, managed eggsearch hiding, runtime config refresh and cache invalidation. If protocol library cannot deliver a notification, support bounded refresh at **turn boundary**; do not invent continuous polling for closure.

Ensure cached `tool_def_cache` is invalidated when any of these change, including plugin-hook generation and active agent authority. A remote server may disappear mid-turn; execution uses the pinned handle/descriptor and fails closed on mismatched generation rather than calling a newer tool by the same name. Do not hold long-lived catalog write locks while fetching a remote `tools/list`.

### C — `tool_search` schema and policy parity

Test keyword and exact-schema query: `{"query":"database search"}` yields compact candidate descriptors and `{"name":"mcp__db__query","detail":"schema"}` resolves the actual JSON Schema. The result must identify canonical/wire name and trusted **host-assigned** risk category, never grant permissions. Preserve `tool_search` bounded result limit and ranking determinism for both native and external entries. Do not let newly registered tools silently bypass disabled/denied model settings.

### D — Definition-hook confinement

When plugins return altered tool definitions after initial surface construction, reject additions outside authorized registry names, changed names, invalid schema, collisions, or permission-expanding modifications. An allowed removal/rewording (if contract permits) may survive only after revalidation against the canonical schema, alias map and resolved surface, and cache keys must include deterministic hook/activation revisions. Do not quietly set `defer_loading=None` for every transformed schema if it changes visibility semantics. If there is no safe use case for altering parameters, constrain to bounded description edits. Plugin-reported metadata is not an effect-class or capability grant.

### E — Documentation/observability

Update `architecture/agent-tool-surface.md`, `architecture/mcp.md`, `architecture/tool.md`, `architecture/plugin.md`, disclosure documentation and registry test guards. Instrument generation, omission reason and counts in bounded telemetry; do not log remote credentials, complete context or PII.

## 6. Ordered work packages

1. **Inventory** all constructor/registry paths, MCP sources and definition-hook callers, mapping their generations and policy owners. Acceptance: explicit design note/test matrix covering native, plugin, raw MCP and hidden managed-backend tools.
2. **Canonical registration/discovery**: use M001 adapters in ToolCatalog and implement exact `tool_search` schema resolution. Acceptance: two-stage discover-and-call in Full/Curated/Minimal, with provider fallback mode accounted for.
3. **Generation/failure semantics**: capture one immutable revision per active turn, generation-safe refresh at next turn and coherent tool-def cache. Acceptance: deterministic add/remove/rename/schema-change/reconnect fixtures.
4. **Definition-hook confinement**: validate hook replacements against caller-eligible registry/surface; preserve deferral and schema identity. Acceptance: negative injection/collision/privilege-widening tests and compliant plugin smoke test.
5. **Docs and qualification**: update guards, full regression matrix and closure record.

## 7. Failure, cancellation, restart and contention

In-progress calls execute only if their pinned registry adapter and descriptor/connection generation match. Unexpected disappearance, server restart or schema mismatch returns a typed unavailable/stale error and **does not** rebind to a replacement. Server `tools/list` failure retains old in-flight snapshot but prevents new-turn advertisement if the backend cannot be qualified; no stale fallback that silently grants access. Simultaneous refreshes are coalesced or scoped to future turns. Cancellation interrupts refresh or a call without modifying existing turn authority. On daemon restart, rebuild registry/catalog from fresh activation/MCP connection; never persist unreviewed tool grants.

## 8. Compatibility, migrations, and non-goals

No user-facing MCP tool rename, persistent migration, broad change to plugin manifest or public provider DTO. Preserve explicit raw MCP opt-in and managed eggsearch duplicate avoidance. Models already receiving full allowed tool arrays remain compatible; actual provider-native deferral is M004. No remote install flow, durable catalog database, semantic search service, tool-call execution redesign, or unauthorized tool inheritance. No re-opening of closed ToolCatalog predecessor milestones absent a concrete regression.

## 9. Required verification

Tests: session-late MCP registration; exact schema; rank ordering and max 10 candidates; raw-hide false/true; managed-server duplicates; disabled/name collision; plan-mode and model-profile filtering; plugin definition-hook injection/shape mutations; schema change with same name; tools/list_changed; concurrent refresh vs execution; server reconnect; provider alias collisions; restart rebuild. Test hooks and cache-hit/cache-miss paths separately.

Commands, after verifying current target names:

```bash
cargo fmt --all -- --check
cargo test --workspace tool_search
cargo test --workspace mcp
cargo test --workspace tool_surface
cargo test --workspace request_preparation
cargo test --workspace plugin
cargo clippy --workspace --all-targets -- -D warnings
bash scripts/verify.sh quick
```

Static guard should ensure no `mcp__` definition can be provider-advertised without an eligible registered adapter and that `dispatch_tool_definition` output cannot bypass surface validation. Document any platform-specific live remote test not run.

## 10. Acceptance, stop and closure

M002 closes when every allowed external tool is discoverable with exact parameters under tested profiles; hidden or unauthorized tools are absent; a valid discovery result points to a callable broker-backed adapter; schema/connection refresh and cache invalidation are generation-correct; and plugin schema hooks cannot advertise unregistered or denied tools. Provide measurable prompt/descriptor cost before and after for a representative 40-tool MCP fixture.

Stop for unresolved ownership of refresh notification, need for unconditional eager external calls in every turn, changes to provider protocol outside adapters, unreviewed effect metadata as authority, or need to migrate durable tool grants. Escalate to ADR if necessary.

Closure: `plans/closure/agent-tool-runtime-reconciliation/002-status.md` with implementation commits, per-condition tests and actual outcomes, explicit provider/OS/network limitations, security and contention evidence, and closure recommendation. M003/M004 unblock only after acceptance.
