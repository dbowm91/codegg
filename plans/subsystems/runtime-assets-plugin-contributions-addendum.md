# Runtime Assets — Plugin Declarative Contributions Addendum

Status: closing

Repository baseline reviewed: `85c22de98d8282dd33c044a40908cfb77ed76c6a`

Source subsystem roadmaps:

- `plans/subsystems/runtime-assets-roadmap.md`
- existing plugin architecture under `architecture/plugin.md`
- existing MCP architecture under `architecture/mcp.md`

Long-term references:

- `plans/000-long-term-specification.md#12-repository-asset-and-harness-interoperability`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`
- `plans/003-planning-process.md`

Applicable ADRs:

- None required for the scoped work. Existing plugin runtime, runtime-asset snapshot, MCP service, daemon, and scheduler ownership remain authoritative.

## 1. Purpose and ownership boundary

This addendum adds one packaging/composition capability without creating a second extension framework: an installed CodeGG plugin may declaratively contribute passive runtime assets and MCP server declarations that are consumed by the existing canonical owners.

The governing rule is:

> `PluginService` owns executable plugin behavior; `ProjectAssetSnapshotBuilder` owns agents/skills/instructions; `McpService` owns MCP connections; the scheduler owns execution. Plugin packaging may declare inputs to those owners but must not become another runtime, registry, or scheduler.

This work owns:

- durable separation between plugin installation and activation;
- project/workspace-aware activation suitable for the singleton multi-project daemon;
- immutable resolved plugin activation input for a turn/workspace generation;
- passive plugin contribution descriptors for agents, skills, optional instructions, and MCP servers;
- provenance, namespacing, precedence, refresh, disable/uninstall, and restart behavior for those contributions.

## 2. Invariants

- Plugin executable capabilities continue to run only through the existing plugin runtime/policy/permission system.
- Passive plugin assets never execute merely because they were discovered.
- Plugin-contributed agents and skills are resolved through the existing runtime-asset builder/registries and obey immutable in-flight snapshot pinning.
- Plugin-contributed MCP servers are connected/disconnected through `McpService`; no plugin-specific MCP client is introduced.
- Plugin activation is explicit and durable; process restart does not silently re-enable a disabled contribution set.
- Repository/workspace activation cannot leak plugin-contributed agents, skills, or MCP servers into unrelated projects.
- Installation state and activation state are separate concepts.
- Project-native/runtime-native assets retain deterministic precedence over plugin-provided defaults unless an explicit documented precedence rule says otherwise.
- Plugin identifiers namespace internally contributed asset/MCP identities so two plugins cannot silently collide.
- Disabling/uninstalling a plugin affects subsequent asset generations/turns but does not mutate an already pinned in-flight `ProjectAssetSnapshot`.
- Plugin absence or failure must not weaken core authorization or goal/snapshot correctness.

## 3. Explicit non-goals

This addendum does not:

- replace the plugin system with skills or vice versa;
- add another plugin runtime, plugin registry, MCP client, scheduler, or job type;
- add generic remote marketplace/package dependency resolution;
- implement browser automation or browser-specific security framing;
- expand the generic hook taxonomy merely to mimic another harness;
- treat passive `Skill` or `Agent` declarations as executable `PluginCapability` variants;
- allow plugin metadata to bypass normal agent/tool/provider permissions;
- add automatic plugin updates;
- add opportunistic/idle scheduling.

## 4. Current-state evidence

At the reviewed baseline:

- `PluginManifest` already defines executable/interactive contributions including commands, hooks, panels, status widgets, and event subscriptions, with Builtin/Process/WASM runtimes and permission declarations.
- plugin enable/disable currently toggles only the live registry and is explicitly runtime-only; re-registration restores original state.
- `PluginService` is passed into turn/runtime paths while workspace identity and `ProjectAssetSnapshot` are separately explicit.
- `ProjectAssetSnapshotBuilder` already owns deterministic workspace-scoped agents, skills, and instructions.
- skill discovery already models source kinds and deterministic precedence, but has no plugin source kind/descriptors.
- `McpService` already owns local/remote MCP connections, discovery, OAuth, reconnect, resources/prompts, structured tool calls, and exposure policy.
- `MarketplaceService` already exists as a local plugin catalog abstraction; official/repository catalogs are empty and are unrelated to this integration.

The missing work is therefore activation/composition plumbing, not another extension runtime.

## 5. Target architecture

### 5.1 Installation versus activation

Installed plugins remain in the canonical plugin catalog/registry. A separate durable activation store records whether a plugin is active at supported scopes, initially at least:

- global/default;
- project/workspace override.

Resolve activation for an explicit workspace into an immutable `ResolvedPluginActivationSet` (name illustrative) containing plugin IDs, versions/digests, trust/runtime metadata needed for provenance, and declared passive contributions.

Turn/runtime construction pins the resulting asset generation just as it does today. Active turns do not observe activation changes mid-turn.

### 5.2 Passive contribution contract

Keep executable capabilities and passive contributions distinct in the manifest. A representative shape is:

```text
PluginManifest
  runtime
  permissions
  capabilities
    command
    hook
    panel
    status
    event_subscription
  contributions
    skills
    agents
    instructions   # optional, bounded
    mcp_servers
```

Do not add `Skill` or `Agent` to `PluginCapability`; they have different authority semantics.

Contribution paths must remain inside the installed plugin root, pass the same symlink/path/size validation expected for equivalent native assets, and never execute scripts during discovery.

### 5.3 Runtime assets

`ProjectAssetSnapshotBuilder` consumes plugin contribution descriptors as another explicit source input. Plugin provenance should include plugin ID/version/digest and deterministic precedence.

Prefer canonical internal names such as `plugin:<plugin-id>:<asset-name>` where namespacing is required for identity. User-facing aliases may be friendlier only when collision-free and deterministic.

Activation changes trigger the existing refresh coordinator for affected workspaces. Publication remains transactional and in-flight pins remain immutable.

### 5.4 MCP

Plugin MCP declarations are validated into normal `McpService` server definitions with origin metadata and plugin namespacing. `McpService` remains responsible for transport/security/OAuth/exposure behavior.

Disabling/uninstalling a plugin disconnects/removes its contributed MCP servers for future/runtime-owned use without affecting unrelated configured servers.

## 6. Dependency graph

```text
M005 durable/context-aware plugin activation
        |
        v
M006 declarative runtime-asset + MCP contribution bridge
```

### M005 — Durable and workspace-aware plugin activation

Status: closed

Plan:

- `plans/implementation/runtime-assets/005-durable-context-aware-plugin-activation.md`

Class: infrastructure/invariant

Exit conditions:

- enable/disable survives restart;
- activation can differ between unrelated projects/workspaces without leakage;
- installation and activation are distinct durable concepts;
- turn/runtime paths consume one immutable resolved activation view;
- existing plugin executable capability behavior remains compatible.

### M006 — Plugin declarative runtime-asset and MCP contribution bridge

Status: closing

Plan:

- `plans/implementation/runtime-assets/006-plugin-declarative-asset-mcp-contributions.md`

Class: capability/infrastructure

Exit conditions:

- an activated plugin can package bounded skills/agents and MCP declarations;
- assets flow through `ProjectAssetSnapshotBuilder`, MCP through `McpService`;
- disabling/removing the plugin removes future contributions without affecting pinned in-flight turns;
- provenance, collisions, validation, and two-project isolation are explicit and tested;
- no duplicate plugin/MCP/asset runtime is introduced.

## 7. Security, restart, contention, and compatibility

Plugin trust/runtime class does not automatically confer asset/tool authority. Agent definitions contributed by plugins remain subject to normal agent resolution and parent authority; MCP tools remain subject to normal exposure and permission rules.

Activation updates should be transactional and scoped. Concurrent turns pin the prior or next complete generation, never a partially applied set.

Daemon restart reloads installed plugins and durable activation state, then reconstructs affected asset/MCP composition deterministically.

Legacy manifests with no `contributions` remain valid. Existing globally installed/enabled behavior needs a documented migration/default so users are not surprised by silent disablement or cross-project activation.

## 8. Verification posture

Use focused plugin registry/store tests, two-project activation isolation fixtures, runtime-asset source/precedence tests, fake/local MCP server fixtures, disable/uninstall/restart tests, and `scripts/verify.sh quick`. Do not add new CI lanes or remote marketplace tests.

## 9. Deferred work

- official/repository remote plugin catalogs;
- plugin dependency resolution or lockfiles;
- automatic updates;
- plugin-contributed executable schedulers/jobs;
- arbitrary hook expansion not tied to a concrete missing boundary;
- browser automation integration;
- plugin-contributed undo semantics for arbitrary effects.
