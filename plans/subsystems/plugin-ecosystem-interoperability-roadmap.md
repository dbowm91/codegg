# Plugin Ecosystem and Harness Interoperability Roadmap

Status: closed; M001-M004 closed

Repository baseline reviewed: 2f64c9e16f9ee96ea4d47ed897201f4c24d4606f

Planning branch: agent/extension-interoperability-planning-2026-09

Canonical long-term references:

- plans/000-long-term-specification.md#4-architectural-principles
- plans/000-long-term-specification.md#12-repository-asset-and-harness-interoperability
- plans/000-long-term-specification.md#24-protocol-and-storage-requirements
- plans/000-long-term-specification.md#26-reliability-and-recovery
- plans/000-long-term-specification.md#27-security-requirements
- plans/000-long-term-specification.md#28-observability
- plans/000-long-term-specification.md#29-system-invariants
- plans/002-long-term-roadmap.md#phase-1--runtime-asset-registry-interoperability-and-refresh-correctness
- plans/003-planning-process.md

Related closed work:

- plans/subsystems/runtime-assets-plugin-contributions-addendum.md
- plans/closure/runtime-assets/005-status.md
- plans/closure/runtime-assets/006-status.md
- plans/subsystems/coding-agent-tool-surface-corrective-roadmap.md
- plans/closure/coding-agent-tool-surface-corrective/007-status.md
- architecture/plugin.md
- architecture/skills.md
- architecture/mcp.md
- architecture/agent-tool-surface.md

No ADR is required to begin this roadmap. Existing ownership already selects PluginService for executable plugin behavior, ProjectAssetSnapshotBuilder for skills/agents/instructions, McpService for MCP, ToolRegistry/ToolBroker for agent tools, and the scheduler for heavy execution. Stop and register an ADR if implementation would create another extension runtime, marketplace execution authority, tool broker, asset registry, or MCP client.

## 1. Purpose and ownership boundary

CodeGG already has a substantial plugin runtime, portable skill discovery, passive plugin contributions, MCP integration, workspace-scoped plugin activation, and progressive tool disclosure. The remaining ecosystem gap is that these capabilities are not yet packaged and exposed as ergonomically as contemporary coding harnesses.

This roadmap closes four concrete gaps:

1. make the advertised custom_tools plugin API truthful by adding a first-class plugin-contributed agent tool capability;
2. ingest the portable Agent Plugins 1.0 package format and a bounded Claude-compatible passive subset without another runtime;
3. ship browser-testing integration as a reusable skill/MCP package rather than a native browser engine;
4. turn the existing local MarketplaceService stub into a provenance-aware, host-mediated extension catalog/install workflow.

The target ownership split is:

~~~text
Plugin package parser / installer
  -> canonical PluginManifest + PluginContributions + provenance

Executable plugin capability
  -> PluginService runtime and PluginPolicy
  -> PluginToolAdapter implements normal Tool
  -> ToolRegistry / ToolBroker / ResolvedToolSurface own model execution authority

Skills / agents / instructions
  -> ProjectAssetSnapshotBuilder

MCP declarations
  -> McpService

Catalog
  -> metadata/discovery only
  -> install requires explicit host/user action
~~~

## 2. External research basis

Research was refreshed on 2026-09-20.

Primary sources:

- Agent Plugins 1.0:
  - https://agent-plugins.org/specification
- OpenCode custom tools:
  - https://opencode.ai/docs/custom-tools/
- Grok Build skills/plugins/marketplaces and Claude compatibility:
  - https://docs.x.ai/build/features/skills-plugins-marketplaces
  - https://docs.x.ai/build/features/mcp-servers
- Playwright coding-agent interfaces:
  - https://github.com/microsoft/playwright-cli
  - https://github.com/microsoft/playwright-mcp
- Claude-compatible plugin package conventions were reviewed as an ecosystem input, but portable conformance in this roadmap is defined by Agent Plugins 1.0, not by undocumented assumptions.

Key conclusions:

- Agent Plugins 1.0 intentionally standardizes only skills and MCP servers. It requires root plugin.json, fixed skills/ and mcp.json locations, strict path containment, component-local failure, and support for PLUGIN_ROOT/PLUGIN_DATA in stdio MCP configuration.
- OpenCode demonstrates the value of a lightweight custom-tool contribution path; CodeGG currently advertises custom_tools in its API feature list without a corresponding PluginCapability::Tool.
- Grok demonstrates the value of broad compatibility import and a unified extension browser, but automatic execution of foreign hook/command formats would exceed a safe portable baseline.
- Microsoft now recommends Playwright CLI+Skills for token-efficient coding-agent workflows and Playwright MCP for persistent state/introspection. CodeGG should support both modes through packaging, not embed a browser engine.

## 3. Current-state findings

### Plugin runtime

- PluginCapability currently includes Command, Hook, Panel, StatusWidget, and EventSubscription only.
- codegg-protocol mirrors that set in PluginCapability and PluginCapabilityInvocation.
- src/plugin/api.rs advertises custom_tools as a current API feature and already defines ToolDefinition, ToolInput, and ToolOutput DTOs.
- PluginService has workspace-pinned activation and policy-gated Process/WASM/Builtin runtime dispatch.
- PluginRegistry indexes commands/hooks/panels/status/events but not agent tools.
- ToolRegistry/ToolBroker already provide the correct canonical execution boundary for model tool calls.

### Passive contributions and skills

- Runtime Assets M006 already allows plugins to package skills, CodeGG-compatible agents, bounded instructions, and MCP declarations.
- CodeGG discovers portable SKILL.md from CodeGG, .agents, OpenCode, Claude, and plugin sources.
- Foreign harness skill roots are read-only by default.
- McpService already reconciles plugin-owned MCP servers with namespaced origins.

### Installation and catalog

- install_from_path requires CodeGG manifest.toml.
- install_from_url is oriented around WASM/archive installation and is not a general portable-plugin package importer.
- MarketplaceService lists/searches only locally installed CodeGG plugins.
- list_official_plugins and list_repository_plugins are empty stubs.
- plugin activation state already records version/install-path staleness and is suitable for preserving explicit re-activation after updates.

## 4. Invariants

- PluginService remains the only executable plugin runtime owner.
- Every model-invoked plugin tool goes through ToolRegistry/ResolvedToolSurface/ToolBroker before PluginService dispatch.
- Plugin manifest metadata cannot grant authority beyond project/session/parent/tool/plugin policy intersection.
- ProjectAssetSnapshotBuilder remains the only runtime-asset composition owner.
- McpService remains the MCP lifecycle owner.
- Scheduler ownership of heavy/durable execution is not bypassed by plugin tools.
- Portable-package discovery never executes scripts merely because they exist in a package.
- Unsupported foreign components are diagnosed and ignored, never guessed into CodeGG semantics.
- Agent Plugins core conformance is version-selected from its $schema and keeps component-local failure boundaries.
- Existing manifest.toml packages remain backward compatible.
- Installation and activation remain separate; install never implies model authority.
- Remote catalog metadata is untrusted and never becomes executable authority.
- Model-originated extension requests cannot silently install or enable code.

## 5. Non-goals

This roadmap does not:

- create a JavaScript/TypeScript runtime merely to mimic OpenCode;
- execute Claude/Grok/ZCode hook scripts automatically;
- claim portable support for commands, agents, hooks, LSP servers, or UI where Agent Plugins 1.0 does not standardize them;
- add a native Chromium/browser engine;
- add automatic plugin updates;
- add dependency resolution/lockfiles;
- make marketplace data authoritative over installed package manifests;
- let a model approve its own plugin installation;
- permit plugin tools to bypass CodeGG's permission/sandbox/scheduler paths.

## 6. Dependency graph

~~~text
closed runtime-assets M005/M006 + closed tool-surface work
        |                         |
        |                         +--> M001 first-class plugin tools
        |
        +--> M002 Agent Plugins 1.0 / passive compatibility import
                                  |
                                  +--> M003 Playwright browser integration bundle
                                  |
                                  +--> M004 extension catalog + host-mediated install

M001 and M002 may execute in parallel.
M003 has a hard dependency on M002 so the bundled package exercises the portable path.
M004 has a hard dependency on M002 and a soft dependency on M001.
~~~

## 7. Milestones

### M001 — First-class plugin-contributed agent tools

Plan:

- plans/implementation/plugin-ecosystem-interoperability/001-first-class-plugin-tools.md

Status: closed.

Add a Tool capability to plugin manifests/protocol, index it in PluginRegistry, adapt it into the canonical ToolRegistry, and dispatch through PluginService only after ordinary broker/permission/surface checks. Make the existing custom_tools API feature truthful.

Exit condition: an activated process/WASM plugin can declare a bounded JSON-schema tool that is discoverable through CodeGG progressive disclosure and callable only through normal CodeGG authority.

### M002 — Agent Plugins 1.0 and bounded foreign-package import

Plan:

- plans/implementation/plugin-ecosystem-interoperability/002-portable-agent-plugin-import.md

Status: closed.

Add a package parser/installer that recognizes Agent Plugins 1.0 plugin.json, skills/, and mcp.json and translates them into existing CodeGG passive contributions. Add a bounded Claude-compatible passive import seam for skills/agents/MCP where formats are already safely understood; detect but do not execute unsupported commands/hooks.

Exit condition: a conformant Agent Plugins 1.0 directory can be installed and activated without rewriting it into manifest.toml, with path containment, component-local diagnostics, PLUGIN_ROOT/PLUGIN_DATA handling, and no duplicate runtime.

### M003 — Playwright browser-testing integration bundle

Plan:

- plans/implementation/plugin-ecosystem-interoperability/003-playwright-browser-integration-bundle.md

Status: closed.

Package a CodeGG-supported browser-testing integration that prefers Playwright CLI + skill instructions for high-throughput coding tasks and offers Playwright MCP as an optional persistent/introspective mode. Do not add a native browser engine.

Exit condition: a user can install/activate one bounded package and an authorized CodeGG agent can run browser tests through ordinary shell/tool policy or opt into MCP browser state, with documented network/profile/security behavior.

### M004 — Extension catalog, discovery, and host-mediated install

Plan:

- plans/implementation/plugin-ecosystem-interoperability/004-extension-catalog-and-install.md

Status: closed.

Replace the empty official/repository marketplace tiers with a provenance-aware catalog abstraction and unified installer for supported CodeGG/Agent-Plugins packages. Add read-only model extension discovery, but keep install/enable as explicit host/user actions.

Exit condition: CodeGG can browse/search configured extension catalogs, verify metadata/package identity, install a selected supported package through one canonical installer, and refresh activation/assets without allowing autonomous model installation.

## 8. Compatibility policy

### CodeGG native packages

manifest.toml remains fully supported and retains current semantics.

### Agent Plugins 1.0

CodeGG should target conformance for:

- root plugin.json selected by recognized $schema;
- fixed skills/ discovery;
- root mcp.json;
- at least stdio and streamable-http where CodeGG transport semantics can map faithfully;
- PLUGIN_ROOT and PLUGIN_DATA variable expansion and containment;
- component-local failure semantics;
- unimplemented extensions ignored without assigning semantics.

### Claude/Grok/ZCode compatibility

Treat these as compatibility adapters, not a new portable standard.

Initial safe support is limited to declarative/passive assets CodeGG already understands: skills, compatible agent definitions where the parser is explicit, and MCP declarations. Commands/hooks/LSP/client-specific UI are detected and reported as unsupported unless a later plan defines an exact typed translation. No foreign script executes during package discovery.

## 9. Security and supply-chain posture

Every package/catalog flow must retain:

- source URL/path and install provenance;
- normalized package identity/version;
- content digest for installed package;
- manifest/schema version;
- activation scope;
- diagnostics for unsupported components;
- symlink/path traversal rejection;
- archive entry containment;
- bounded file counts/sizes;
- no secret-bearing catalog fields in logs.

Remote catalogs and downloads must use the existing hardened HTTP client, explicit redirect policy appropriate to package retrieval, maximum download/archive expansion bounds, and deterministic staging/atomic commit. A catalog cannot override an installed package identity without an explicit user update action.

Plugin tools must default conservatively when effect/risk metadata is missing. A plugin-authored declaration is a request for capability, not a trust assertion.

## 10. Failure, restart, and contention

- failed portable component parsing does not corrupt unrelated valid component types;
- failed plugin-tool registration leaves the plugin diagnosed and prevents ambiguous tool exposure;
- concurrent installs of one identity retain the existing install-lock/atomic-commit invariant;
- activation changes affect future pinned turns, not in-flight turns;
- daemon restart rebuilds installed package registry, activation, runtime assets, and MCP contribution state deterministically;
- catalog unavailability does not disable already installed plugins;
- browser integration process/session cleanup follows existing shell/MCP/process ownership.

## 11. Observability

Provide bounded inspect/doctor information for:

- package format and source;
- portable schema version;
- supported/unsupported component counts;
- plugin-tool names/categories/disclosure without schema/body dumps;
- install digest/version and activation source;
- catalog source and freshness;
- Playwright integration mode selected (CLI or MCP), without browser profile secrets.

## 12. Verification posture

Each milestone defines focused tests. Broad verification remains:

~~~text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
~~~

Do not add live marketplace/network/browser CI. Use fixture packages, fake HTTP/catalog servers, fake MCP servers, and local deterministic browser-integration contract tests. A manual optional smoke may be documented separately.

## 13. User-visible completion criteria

The roadmap is complete when:

- custom_tools is a truthful, working plugin capability;
- plugin tools obey the same model disclosure, parent ceiling, permission, audit, and execution contracts as native tools;
- CodeGG installs conformant Agent Plugins 1.0 packages directly;
- unsupported foreign components are transparent rather than silently executed;
- browser testing is available as an extension without increasing the core binary/browser footprint;
- extension discovery has provenance and useful metadata;
- installation/activation remain explicit user actions;
- existing CodeGG plugins and skill/MCP behavior remain compatible;
- no second plugin runtime, tool broker, asset registry, MCP client, or scheduler exists.

## 14. Risks and deferred work

Primary risks are supply-chain compromise, foreign-format semantic drift, duplicate tool identity, tool effect misclassification, automatic-install authority creep, and dependency-download unpredictability.

Deferred:

- automatic updates;
- package dependency solving/lockfiles;
- signed central marketplace trust root unless separately designed;
- generalized Claude hook/command execution;
- LSP plugin portability;
- UI/App portability;
- browser credential/profile synchronization;
- model-autonomous installation or enablement.
