# Runtime Assets Milestone 006 — Plugin Declarative Asset and MCP Contributions

Status: blocked

Repository baseline: `85c22de98d8282dd33c044a40908cfb77ed76c6a`

Source roadmap:

- `plans/subsystems/runtime-assets-plugin-contributions-addendum.md#6-dependency-graph`

Long-term requirements:

- `plans/000-long-term-specification.md#12-repository-asset-and-harness-interoperability`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`

Applicable ADRs:

- None. Preserve the current plugin runtime, runtime-asset snapshot builder, MCP service, and scheduler ownership boundaries.

Primary class: capability

Hard dependency:

- `plans/implementation/runtime-assets/005-durable-context-aware-plugin-activation.md` must be closed with a stable immutable resolved activation interface.

## 1. Objective

Allow an activated CodeGG plugin to package passive agents/skills/optional instructions and MCP server declarations while routing each contribution through its existing canonical owner, with explicit provenance, deterministic namespacing/precedence, workspace isolation, refresh semantics, and no duplicate extension runtime.

## 2. Why this milestone is blocked

Passive plugin contributions affect runtime behavior beyond an executable plugin command/hook. They therefore require durable project-aware activation first. Until M005 closes, loading contributed assets/MCP servers would inherit the current process-global runtime-only enable/disable ambiguity and could leak across projects after restart or concurrent use.

## 3. Current implementation evidence

At baseline:

- `PluginManifest` has `runtime`, `permissions`, executable/interactive `capabilities`, and legacy hooks/config.
- `PluginCapability` covers command, hook, panel, status widget, and event subscription.
- `ProjectAssetSnapshotBuilder` is the single side-effect-bearing constructor for project agents, skills, and instructions and produces immutable pinned snapshots.
- skill `SourceKind` covers CodeGG, Agents, OpenCode, and Claude project/global locations but no plugin provenance.
- `McpService` already owns local/remote connection lifecycle, discovery, structured calls, OAuth, reconnect, resources/prompts, and exposure policy.
- plugin installation already validates/copies a plugin directory into a canonical install root.

Therefore contributions should be descriptors consumed by those owners, not new executable `PluginCapability` variants or new services.

## 4. Invariants that must not regress

- Passive contribution discovery executes no bundled scripts.
- Agent/skill/instruction contributions are resolved only through the existing runtime-asset pipeline.
- MCP contributions are connected only through `McpService`.
- `PluginService` remains the only owner of executable plugin capabilities.
- A plugin's runtime trust class does not automatically grant agent/tool/MCP authority.
- All contribution paths remain inside the validated installed plugin root and pass normal size/symlink/path constraints.
- Internal contributed identities are namespaced by canonical plugin identity where collisions are possible.
- Precedence is deterministic, documented, and inspectable; project-native definitions are not silently shadowed by plugin defaults.
- Disabling/uninstalling a plugin changes future resolved generations but not already pinned turns.
- Two workspaces with different activation sets cannot see one another's contributions.
- Legacy manifests without contributions remain valid.

## 5. Scope

### In scope

- additive plugin manifest `contributions` section or equivalent passive descriptor model;
- bounded skill contribution roots/files;
- bounded CodeGG-compatible agent definitions;
- optional bounded instruction fragments if they can reuse existing instruction provenance safely;
- MCP stdio/http declarations compatible with existing `McpService` configuration types;
- plugin origin/provenance metadata for runtime assets and MCP servers;
- deterministic canonical internal names/namespacing and collision behavior;
- integration with M005 resolved activation state;
- runtime-asset refresh on activation/install/uninstall/contribution change where existing lifecycle allows;
- MCP connect/disconnect/reconcile for active plugin contributions;
- two-project isolation, restart, disable/uninstall, malformed manifest, and collision tests.

### Explicitly out of scope

- adding `Skill` or `Agent` variants to executable `PluginCapability` merely for packaging;
- plugin-defined schedulers/jobs/workflows;
- new MCP transport/client implementation;
- plugin dependency resolution or lockfiles;
- remote marketplace catalogs or automatic updates;
- broad hook taxonomy expansion;
- browser engine integration;
- auto-execution of scripts/resources found beside skills/agents.

## 6. Required production changes

### Manifest and validation

Extend `PluginManifest` with a backward-compatible passive contribution section. Keep it structurally separate from executable capabilities.

Descriptors should reference paths relative to the installed plugin root or contain bounded declarative configuration. Validate at plugin load/install or contribution resolution time:

- no absolute/path-escape references;
- no symlink escapes;
- bounded file counts and sizes using existing runtime-asset limits where applicable;
- valid agent/skill formats through existing parsers;
- valid MCP transport/config fields through existing MCP config validation;
- no secret values embedded where existing MCP/plugin config expects credential references/environment mediation.

Malformed contributions should diagnose/disable that contribution or plugin according to a documented transactional rule; they must not lead to partial arbitrary execution.

### Runtime asset contribution bridge

Introduce an explicit plugin asset source descriptor consumed by `ProjectAssetSnapshotBuilder` for the current `AssetContext` and M005 activation set.

Do not make `ProjectAssetSnapshotBuilder` query global plugin mutable state directly. The builder should receive an immutable resolved contribution input or source provider appropriate to the explicit context.

Extend provenance/source modeling to retain at least:

- plugin ID;
- plugin version/digest where available;
- contribution kind/path;
- precedence rank/source class;
- validation/shadowing diagnostics.

Define deterministic precedence. Default preferred policy:

- explicit project-native CodeGG assets retain precedence over plugin defaults;
- plugin contributions are namespaced internally;
- an optional friendly alias is exposed only when collision-free and the existing asset model supports aliases without ambiguity.

Do not automatically reinterpret arbitrary third-party agent formats; use CodeGG-compatible definitions only unless an existing explicit adapter owns the conversion.

### MCP contribution bridge

Translate active plugin MCP declarations into normal `McpService` server definitions. Add origin metadata sufficient to distinguish configured servers from plugin-contributed servers and to reconcile only the owning plugin's servers.

Use deterministic canonical server names such as `plugin:<plugin-id>:<declared-name>` internally. Do not allow a plugin to replace an unrelated configured MCP server by declaring the same short name.

Connection, OAuth, DNS/redirect security, reconnect, exposure policy, prompt/resource behavior, and tool calls remain `McpService` responsibilities.

Activation/install/uninstall transitions should reconcile contributed servers:

- newly active contribution -> connect/prepare through normal MCP service lifecycle;
- disabled/uninstalled contribution -> disconnect/remove only that origin's servers;
- unrelated configured/plugin servers remain untouched;
- active turn behavior follows the existing MCP/tool-surface consistency rules and must not mutate pinned runtime assets mid-turn.

### Refresh and generation integration

When the effective passive contribution set for a workspace changes, trigger or enqueue the existing runtime-asset refresh mechanism. Preserve transactional publication and last-valid snapshot behavior.

If plugin files change on disk outside managed installation/update flow, existing refresh semantics may detect the change; do not add another mandatory watcher system.

### Plugin runtime and permissions

Do not route passive asset parsing through process/WASM plugin execution. Conversely, plugin executable commands/hooks remain in `PluginService`; they do not become asset-builder callbacks.

Agent definitions contributed by plugins must still receive ordinary CodeGG agent/tool authority; no manifest field may grant an agent more authority than the normal parent/session policy permits.

MCP tools receive ordinary MCP exposure/permission treatment. Plugin activation is not tool approval.

### Management/operator surface

Extend plugin `info`/doctor output with bounded contribution inventory and validation status. Avoid dumping skill bodies, agent prompts, secrets, or MCP credentials.

Provide enough origin information in runtime-asset/MCP diagnostics to answer “which plugin contributed this?” and “why is it shadowed/disabled?”.

## 7. Ordered work packages

### Work package A — Passive contribution schema and validation

Intent: define packaging without changing runtime ownership.

Required changes:

- add manifest contribution descriptors;
- implement relative-path and bounded validation;
- keep legacy manifests compatible;
- produce typed contribution inventory/diagnostics.

Acceptance evidence:

- legacy manifest decode unchanged;
- malicious path/symlink escape fixtures rejected;
- discovery performs no executable plugin invocation;
- malformed one-plugin contribution cannot corrupt unrelated installed plugins.

### Work package B — Runtime asset source integration

Intent: let activated plugins provide passive assets through the existing snapshot builder.

Required changes:

- define plugin source/provenance input;
- feed M005 resolved activated contributions into `ProjectAssetSnapshotBuilder`;
- resolve agent/skill/instruction contributions with deterministic precedence/shadowing;
- trigger normal refresh/generation transitions on contribution activation changes.

Acceptance evidence:

- contributed skill/agent appears in a later snapshot for an active workspace;
- same plugin remains absent in a workspace where disabled;
- project-native collision follows documented precedence;
- active turn retains old pinned snapshot across disable/change.

### Work package C — MCP service contribution integration

Intent: package MCP servers without a new client/runtime.

Required changes:

- validate/translate contribution declarations into normal MCP server config;
- namespace server identity by plugin;
- add origin-aware connect/disconnect/reconcile;
- preserve all existing MCP transport/security/exposure behavior.

Acceptance evidence:

- fake/local MCP plugin server connects through `McpService` and exposes expected tools under normal policy;
- disabling/uninstalling plugin disconnects only its servers;
- collision with configured or another plugin server cannot overwrite the other origin;
- restart reconstructs active plugin servers from install + M005 activation state.

### Work package D — Management, documentation, and closure

Intent: make composition inspectable and explicitly non-overlapping.

Required changes:

- expose bounded contribution/provenance data in plugin info/doctor and asset/MCP diagnostics;
- update plugin/runtime-assets/MCP architecture docs;
- add static/type-level ownership checks only where they prevent duplicate runtimes.

Acceptance evidence:

- docs state the four canonical owners and passive nature of contributions;
- code contains no `PluginMcpService`, second asset registry, or executable skill-loading path.

## 8. Failure, cancellation, restart, and contention semantics

Contribution resolution is transactional per runtime-asset refresh: an invalid candidate does not replace the last valid asset generation. Decide and document whether one malformed contribution invalidates only that contribution or the owning plugin's full passive bundle; never publish a partially parsed asset with missing provenance.

MCP connection failure should use existing per-server error/reconnect semantics and must not invalidate unrelated runtime assets. A plugin can remain installed/activated while one contributed MCP server is unhealthy; diagnostics must make the degraded state visible.

Activation/uninstall races use M005 durable state as authority. An in-flight turn keeps pinned asset/activation state; MCP tool-surface behavior must follow existing turn/tool catalog consistency rather than hot-swapping arbitrary calls mid-execution.

Daemon restart reconstructs passive contributions from installed plugin manifests plus M005 activation state. No hidden in-memory marketplace/cache state is authoritative.

## 9. Compatibility and migration

`contributions` is additive and optional. Existing plugin manifests and executable capabilities continue to work unchanged.

Do not reinterpret existing legacy `config` keys as contributions automatically.

Existing runtime asset precedence must remain stable for native/foreign harness sources. Insert plugin source precedence deliberately and test the full ordering so a new enum rank cannot accidentally invert prior project/global behavior.

Existing MCP configuration remains authoritative for explicitly configured servers; plugin servers use namespaced identities and origin metadata.

## 10. Required tests

### Focused unit tests

- manifest contribution serialization/deserialization;
- path/symlink/count/size validation;
- source precedence and namespacing;
- MCP origin/name collision resolution.

### Integration tests

- activated plugin contributes agent and skill to one workspace only;
- disable triggers later asset generation without mutating active pin;
- uninstall removes later contributions;
- project-native asset collision precedence;
- plugin MCP fake stdio/http server through normal `McpService`;
- unrelated configured/plugin MCP servers survive disable/uninstall.

### Restart and recovery tests

- install + active state -> restart -> same contributed assets/MCP;
- disabled state -> restart -> no contributions;
- stale missing plugin activation yields diagnostics, no ghost assets/server.

### Contention tests

- simultaneous refresh for two workspaces with different activation sets;
- activation change concurrent with turn start yields one complete old/new generation, never mixed contributions;
- MCP reconciliation for one plugin does not disconnect another.

### Security and negative tests

- passive contribution discovery never invokes process/WASM runtime;
- path/symlink escape rejected;
- plugin agent cannot gain extra tool authority from contribution metadata;
- MCP contribution does not bypass exposure/permission/DNS/redirect policies;
- secrets are not emitted in diagnostics.

## 11. Required verification commands

```bash
cargo test plugin
cargo test runtime_assets
cargo test mcp
scripts/verify.sh quick
```

Use actual focused selectors after implementation. No new CI lanes, marketplace network tests, or broad matrices are required.

## 12. Documentation updates

- `architecture/plugin.md`
- `docs/PLUGINS.md`
- `architecture/mcp.md`
- runtime-assets architecture/skills docs as applicable
- closure record: `plans/closure/runtime-assets/006-status.md`

## 13. Acceptance criteria

- M005 is strictly closed first.
- Plugins may declare passive bounded agent/skill/optional-instruction/MCP contributions without creating new executable capability categories for those assets.
- Runtime assets flow through `ProjectAssetSnapshotBuilder`; MCP flows through `McpService`; executable behavior remains in `PluginService`; execution remains scheduler-owned.
- Contribution provenance and namespacing are deterministic and inspectable.
- Two workspaces with different activation cannot leak contributions.
- Disable/uninstall affects later generations/connections and preserves pinned in-flight turn semantics.
- Legacy plugins/configured MCP/native assets remain compatible.
- Focused tests and `scripts/verify.sh quick` pass.

## 14. Stop conditions

Stop and report when:

- M005 is not strictly closed;
- implementation requires a second plugin runtime/registry or MCP client;
- passive asset discovery requires executing plugin code;
- correct source precedence would change established native/foreign harness precedence without explicit roadmap/ADR review;
- MCP contribution config needs credential storage semantics inconsistent with existing `McpService`/plugin permission policy;
- project/workspace isolation cannot be expressed through existing explicit asset/runtime context.

## 15. Closure evidence required

- M005 closure dependency reference;
- implementation commit(s);
- manifest backward-compatibility evidence;
- source precedence/provenance matrix;
- two-workspace activation isolation tests;
- immutable in-flight snapshot evidence;
- MCP origin/collision/disable/uninstall/restart evidence;
- security evidence showing no passive-discovery execution or authority bypass;
- exact verification commands/outcomes;
- explicit confirmation that no duplicate plugin/MCP/asset runtime was introduced.

## 16. Handoff notes

The desired feature is packaging, not architecture duplication. Whenever there is a choice between adding plugin-owned execution machinery and translating a contribution into an existing owner, use the existing owner or stop if its contract is insufficient.
