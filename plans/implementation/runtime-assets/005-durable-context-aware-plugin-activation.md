# Runtime Assets Milestone 005 — Durable Context-Aware Plugin Activation

Status: implemented

Repository baseline: `85c22de98d8282dd33c044a40908cfb77ed76c6a`

Source roadmap:

- `plans/subsystems/runtime-assets-plugin-contributions-addendum.md#6-dependency-graph`

Long-term requirements:

- `plans/000-long-term-specification.md#12-repository-asset-and-harness-interoperability`
- `plans/001-terminology-and-domain-model.md`
- `plans/002-long-term-roadmap.md`

Applicable ADRs:

- None. Preserve existing plugin runtime and immutable runtime-asset ownership.

Primary class: infrastructure

## 1. Objective

Separate plugin installation from activation and make activation durable and explicit for the singleton multi-project daemon, including project/workspace-scoped resolution and immutable per-turn activation input suitable for later passive asset/MCP contribution integration.

## 2. Why this milestone is ready

- the canonical plugin registry, loader, manager, runtimes, policy, and permission model already exist;
- explicit project/workspace execution identity already exists;
- runtime assets already use explicit context and immutable generations;
- no passive contribution support is required to close activation semantics themselves.

This milestone is intentionally prerequisite-only: M006 must not add plugin-provided agents/skills/MCP until activation scope and restart behavior are trustworthy.

## 3. Current implementation evidence

At baseline:

- `PluginManager::enable` / `disable` update only `PluginRegistry::set_enabled` in the live process.
- plugin docs/TUI explicitly state enable/disable is runtime-only until persistence exists.
- installed plugins have source metadata and canonical installation directories.
- `PluginService` may be shared into agent/shell runtime paths while workspace execution identity is provided independently.
- runtime-asset construction already pins explicit workspace context and supports refresh generations.

A process-global mutable `enabled: bool` is insufficient once plugin state can influence project-scoped agents/skills/MCP composition.

## 4. Invariants that must not regress

- Installing a plugin does not implicitly grant project activation beyond the documented default policy.
- Activation survives daemon restart.
- Project/workspace overrides cannot leak into unrelated workspaces.
- Existing plugin executable invocation still passes through `PluginService`, policy, trust, and permissions.
- Activation state is configuration/selection, not execution authority.
- Concurrent turns observe an immutable resolved activation set; activation changes affect later turns/generations only.
- Builtin plugins required for core provider/auth compatibility retain explicitly documented behavior and are not accidentally disabled by migration of third-party activation state.
- Unknown/stale activation records fail safely and produce diagnostics rather than loading arbitrary plugin paths.

## 5. Scope

### In scope

- durable activation store/model separate from installed plugin metadata;
- global/default activation plus project/workspace override resolution;
- explicit precedence/inheritance rules for activation;
- immutable `ResolvedPluginActivationSet` or equivalent resolved view keyed to explicit workspace/project identity;
- integration of resolved activation into daemon/turn construction without making frontends authoritative;
- enable/disable commands updated to persist at a documented scope;
- restart, concurrent-project, uninstall, and stale-record behavior;
- bounded activation diagnostics and operator visibility;
- migration of current runtime-only semantics.

### Explicitly out of scope

- passive plugin agents/skills/MCP contributions (M006);
- new plugin runtime types;
- remote plugin marketplace/package dependency resolution;
- automatic updates;
- hook taxonomy expansion;
- scheduler/job integration for plugins;
- browser-specific integration.

## 6. Required production changes

### Core/domain and storage

Introduce a small durable activation record keyed by canonical plugin identity and scope. Prefer the existing SQLite/config ownership appropriate to daemon runtime state; do not write mutable activation back into installed plugin manifests.

A representative record includes:

- plugin ID;
- scope kind (`global` / project or workspace override);
- stable project/workspace identity where applicable;
- enabled/disabled state;
- updated timestamp/revision;
- optional installed-version/digest observation for diagnostics, not as an alternate plugin identity.

Define explicit inheritance. A simple acceptable rule is:

1. builtin/core plugin policy is resolved separately;
2. project/workspace override wins when present;
3. otherwise durable global/default activation applies;
4. otherwise migration/default policy applies.

Use stable project/workspace IDs, not canonical paths, as persisted activation identity.

### Plugin registry/service integration

Do not turn the live `PluginRegistry.enabled` flag into the durable source of truth. Either:

- resolve active plugin visibility at invocation/listing time from an immutable activation view; or
- construct a context-bound filtered view over the installed registry.

Avoid cloning/re-registering independent plugin registries per workspace if that creates divergent installation/runtime state.

The plugin service remains the executable dispatch owner. A plugin disabled for the current context must not have commands/hooks invoked for that context.

### Runtime/context integration

Carry the resolved activation set alongside existing explicit turn/runtime context. It should be immutable for the turn, analogous to runtime asset pinning.

If plugin activation changes while a turn is active:

- the active turn keeps its pinned activation/runtime behavior;
- later turns resolve the new state;
- M006 may later use the same transition to trigger asset/MCP refresh.

Do not consult process current directory to infer activation scope.

### Management commands and operator surface

Update enable/disable management to make scope explicit enough to avoid accidental global changes. Preserve a reasonable default for existing commands, but report exactly what scope was changed.

`list`/`info` should distinguish at least:

- installed;
- effective active/inactive for the current context;
- source of activation decision (global/default/project override/builtin policy);
- stale/missing installation diagnostics.

### Uninstall and stale state

Uninstall must not leave an activation record that later activates a different plugin merely because a name/path is reused. Either remove matching activation records transactionally or bind records to canonical plugin ID plus safe install identity and diagnose stale entries.

Reinstall behavior must be documented and tested.

### Security and authorization

Activation cannot bypass plugin trust/permission policy. A globally activated process plugin still cannot use disallowed hooks/permissions.

Project activation state must be mutable only through the same user/operator authority as existing plugin management; remote clients must not gain implicit broader authority because they know a plugin ID.

## 7. Ordered work packages

### Work package A — Durable activation model and migration

Intent: replace ephemeral enable/disable state with a restart-safe authority.

Required changes:

- add activation record/store;
- define scope and precedence;
- migrate/default existing installed plugin behavior;
- protect builtin compatibility semantics.

Acceptance evidence:

- enable/disable persists across service reconstruction;
- legacy installation with no activation record follows documented default;
- stale/unknown records produce diagnostics, not activation.

### Work package B — Context-aware resolution

Intent: make activation safe in a singleton multi-project daemon.

Required changes:

- resolve effective activation for explicit project/workspace context;
- add immutable resolved activation view;
- thread it through turn/plugin dispatch boundaries as needed;
- remove assumptions that one process-global enabled flag defines all project behavior.

Acceptance evidence:

- project A and B can have opposite activation for the same installed plugin concurrently;
- commands/hooks for A do not fire in B when B is disabled;
- active turns retain the activation set pinned at start.

### Work package C — Management and uninstall behavior

Intent: make durable/scoped semantics inspectable and safe.

Required changes:

- persist enable/disable from management commands;
- expose effective scope/source in list/info/doctor output;
- clean or safely invalidate activation state on uninstall/reinstall.

Acceptance evidence:

- command/TUI tests report scope accurately;
- uninstall cannot leave a record that activates an unrelated future installation;
- restart maintains expected effective state.

### Work package D — Documentation and compatibility closure

Intent: make the new distinction explicit for M006.

Required changes:

- update `architecture/plugin.md` and `docs/PLUGINS.md`;
- document installation versus activation and context inheritance;
- expose a stable internal interface M006 can consume without reading the activation database directly.

Acceptance evidence:

- M006 can depend only on the resolved activation interface;
- no passive contribution code lands in this milestone.

## 8. Failure, cancellation, restart, and contention semantics

Activation writes are transactional. If persistence fails, live effective state must not claim the requested durable change succeeded.

Concurrent activation writes for the same plugin/scope use revision or last-committed transaction semantics and produce one deterministic effective record.

Daemon restart resolves activation from durable state and installed plugin inventory. Missing installations yield diagnostics and inactive contributions/invocation rather than executing stale paths.

An activation change during a running turn does not mutate that turn's pinned view.

## 9. Compatibility and migration

Legacy manifests remain unchanged. No manifest contribution fields are required in M005.

Define a migration default that preserves current expected installed plugin behavior as closely as safely possible. If existing third-party installed plugins currently start enabled, a one-time/default global active state may preserve compatibility; builtin/core plugins may require explicit special handling. Document the exact policy in closure evidence.

Management command syntax should remain compatible where possible, adding scoped options rather than removing existing forms abruptly.

## 10. Required tests

### Focused unit tests

- activation scope/precedence resolution;
- durable store round trips;
- stale installation/version/digest diagnostics;
- builtin default policy.

### Integration tests

- two projects with opposite activation concurrently;
- hook/command dispatch respects context activation;
- active turn pin versus later activation change;
- uninstall/reinstall behavior.

### Restart and recovery tests

- enabled/disabled state survives daemon/service reconstruction;
- stale records remain inactive and diagnosed.

### Contention tests

- concurrent writes to same scope;
- independent project activation updates do not contend globally beyond required store serialization.

### Security and negative tests

- activation cannot bypass lifecycle/permission policy;
- a workspace cannot select another workspace's override via path spoofing;
- knowing a plugin ID is insufficient to invoke a disabled plugin.

## 11. Required verification commands

```bash
cargo test plugin
cargo test plugin_management
cargo test runtime_assets
scripts/verify.sh quick
```

Use focused actual selectors after implementation. Do not add new CI lanes.

## 12. Documentation updates

- `architecture/plugin.md`
- `docs/PLUGINS.md`
- runtime/turn architecture if activation pinning is documented there
- closure record: `plans/closure/runtime-assets/005-status.md`

## 13. Acceptance criteria

- Plugin enable/disable is durable rather than runtime-only.
- Installation and activation are distinct and documented.
- Effective activation is resolved from explicit project/workspace identity.
- Concurrent projects can safely use different activation states.
- In-flight turns keep an immutable activation view.
- Existing plugin runtime/policy/permission ownership remains unchanged.
- Uninstall/reinstall cannot accidentally revive stale activation for an unrelated installation.
- M006 has a stable resolved activation interface and remains otherwise unimplemented.
- Focused tests and `scripts/verify.sh quick` pass.

## 14. Stop conditions

Stop and report when:

- correct scoping would require per-workspace duplicate plugin runtimes/registries;
- activation persistence would be placed inside installed manifests;
- stable project/workspace identity is unavailable at an invocation boundary that needs contextual activation;
- preserving builtin/core behavior conflicts with the generic migration policy;
- implementation starts adding agent/skill/MCP contribution loading before M005 closure.

## 15. Closure evidence required

- implementation commit(s);
- activation schema/config migration and default-policy evidence;
- restart persistence tests;
- two-project isolation tests;
- command/hook dispatch context tests;
- uninstall/reinstall stale-state evidence;
- immutable in-flight activation evidence;
- exact verification commands/outcomes;
- unresolved findings and compatibility limitations.

## 16. Handoff notes

Do not solve project scoping by instantiating a complete independent plugin registry/service per workspace unless repository evidence demonstrates that is already the canonical design. The intended result is one installed catalog/runtime system with context-bound activation resolution.
