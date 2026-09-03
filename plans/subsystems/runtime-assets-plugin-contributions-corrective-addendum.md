# Runtime Assets — Plugin Contributions Corrective Addendum

Status: active — M007 active

Repository baseline reviewed: `4dd1220cf0f297d1e3d6206a1e2b39d2152fd8ce`

Parent roadmap/addendum:

- `plans/subsystems/runtime-assets-roadmap.md`
- `plans/subsystems/runtime-assets-plugin-contributions-addendum.md`

Historical milestones preserved:

- M005 closed: `plans/closure/runtime-assets/005-status.md`
- M006 closed: `plans/closure/runtime-assets/006-status.md`

Long-term references:

- `plans/000-long-term-specification.md#12-repository-asset-and-harness-interoperability`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`

## 1. Corrective purpose

The plugin contribution architecture landed in the intended non-overlapping form: passive assets are separate from executable plugin capabilities, runtime assets flow through `ProjectAssetSnapshotBuilder`, MCP flows through `McpService`, and activation is durable/context-aware. A post-closure review found one narrow compatibility defect in the MCP transport translation path.

This addendum preserves M005/M006 as historical closure records and adds one corrective milestone. It does not reopen plugin architecture or introduce a new runtime.

## 2. Discovered corrective finding

Plugin MCP contribution validation accepts `local`, `stdio`, `remote`, and `http`. Runtime reconciliation forwards that value to `McpService::connect_from_config()`, which accepts only `local` and `remote`.

As a result, a validated `stdio` or `http` plugin manifest can fail only when the runtime connection path is reached.

The existing M006 test suite did not catch this because its `stdio` fixture exercises a configured-server collision branch that returns before connection, while its `http` fixture is intentionally rejected for embedded credential data.

## 3. Invariants

- `PluginService` continues to own executable plugin behavior.
- plugin-contributed MCP declarations remain passive data until materialized through `McpService`.
- no `PluginMcpService`, duplicate client, or second server registry is introduced.
- configured MCP servers retain precedence and cannot be overwritten by plugins.
- plugin server names/origin remain namespaced and inspectable.
- `stdio` is only an alias for the existing local transport; `http` is only an alias for the existing remote transport.
- transport alias handling cannot weaken MCP security, exposure, OAuth, DNS/redirect, or permission checks.
- M005 activation semantics and runtime-asset contribution behavior remain unchanged.

## 4. Corrective milestone

### M007 — Plugin MCP transport alias corrective pass

Status: active

Plan:

- `plans/implementation/runtime-assets/007-plugin-mcp-transport-alias-corrective-pass.md`

Class: corrective capability / compatibility

Dependencies:

- hard: none beyond historical M005/M006 implementation on `main`;
- interface: existing plugin contribution schema and `McpService` connection/reconciliation APIs.

Exit conditions:

- one canonical translation maps `local|stdio` to the local MCP path and `remote|http` to the remote path;
- validation and runtime translation share the same accepted transport contract;
- successful tests actually reach the alias translation/reconciliation path rather than only collision/negative branches;
- canonical `local`/`remote` behavior, configured collisions, origin metadata, disable/removal, and workspace isolation remain intact;
- no duplicate MCP/plugin runtime is introduced;
- focused tests, Clippy, and `scripts/verify.sh quick` pass;
- closure record is written at `plans/closure/runtime-assets/007-status.md`.

## 5. Why the corrective milestone is independently ready

The defect is a translation mismatch between already-stable schemas and services. It requires no new dependency, storage migration, scheduler change, marketplace work, or architecture decision.

The repository already has deterministic plugin contribution fixtures and MCP origin/reconciliation tests. M007 only needs to make the accepted validation vocabulary true at runtime and add the missing positive-path coverage.

## 6. Verification posture

Use table-driven transport validation/canonicalization tests, local/fake MCP production-seam coverage, existing plugin contribution and MCP suites, Clippy, and `scripts/verify.sh quick`.

Do not add external network test dependencies, remote marketplace tests, or new CI lanes.

## 7. Deferred work remains deferred

M007 does not add:

- plugin dependency resolution;
- remote catalogs or automatic updates;
- new MCP transports;
- browser integration;
- broad hook expansion;
- plugin schedulers/jobs;
- arbitrary plugin effect rollback.

## 8. Closure disposition

Until M007 has an accepted closure record, M006 remains valid historical evidence for the contribution architecture but the plugin MCP alias compatibility claim is not strictly closed. M007 is the sole active corrective owner for this defect.
