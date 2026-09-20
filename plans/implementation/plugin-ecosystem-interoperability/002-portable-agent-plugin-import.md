# Plugin Ecosystem Interoperability M002 — Agent Plugins 1.0 and Bounded Foreign-Package Import

Status: implemented

Repository baseline: 2f64c9e16f9ee96ea4d47ed897201f4c24d4606f

Source roadmap:

- plans/subsystems/plugin-ecosystem-interoperability-roadmap.md#m002--agent-plugins-10-and-bounded-foreign-package-import

Long-term requirements:

- plans/000-long-term-specification.md#12-repository-asset-and-harness-interoperability
- plans/000-long-term-specification.md#26-reliability-and-recovery
- plans/000-long-term-specification.md#27-security-requirements
- plans/000-long-term-specification.md#29-system-invariants
- plans/002-long-term-roadmap.md#phase-1--runtime-asset-registry-interoperability-and-refresh-correctness
- plans/003-planning-process.md

Applicable ADRs:

- None. Translate foreign package declarations into existing PluginRegistry, ProjectAssetSnapshotBuilder, and McpService owners.

Primary class: capability / interoperability

Dependencies:

- closed Runtime Assets M005/M006;
- existing portable SKILL.md parser and MCP contribution bridge.

## 1. Objective

Allow CodeGG to install and activate portable Agent Plugins 1.0 packages directly from a local directory, while retaining CodeGG-native manifest.toml compatibility.

Also add a deliberately bounded Claude-compatible passive import seam for package assets CodeGG already understands safely. Unsupported client-specific commands/hooks/LSP/UI must be diagnosed and ignored, not executed by guesswork.

## 2. Normative external format

Agent Plugins 1.0 is the portable conformance target:

- https://agent-plugins.org/specification
- canonical manifest schema: https://agent-plugins.org/schemas/1.0.0/plugin.schema.json
- canonical MCP schema selected by the matching 1.0.0 specification.

Important requirements to preserve:

- root plugin.json is required;
- $schema selects the supported specification version;
- v1 standardizes exactly skills and MCP servers;
- skills are fixed under skills/<name>/SKILL.md;
- MCP configuration is root mcp.json;
- unknown top-level manifest fields are reported/ignored per the specification's non-fatal rule rather than assigned semantics;
- unimplemented extension namespaces are ignored without interpreting them;
- plugin-relative paths remain contained in plugin root;
- component-local failures do not invalidate unrelated valid component types;
- stdio subprocesses receive PLUGIN_ROOT and PLUGIN_DATA;
- clients map portable MCP configuration into native MCP configuration rather than making it a second MCP implementation.

## 3. Current implementation evidence

At baseline:

- CodeGG install_from_path requires manifest.toml.
- install_from_url is specialized around WASM/archive downloads rather than general portable packages.
- PluginManifest supports executable capabilities plus passive PluginContributions.
- PluginContributions already models skills, agents, instructions, and MCP server declarations.
- ProjectAssetSnapshotBuilder already admits plugin skills/agents/instructions with provenance/precedence.
- McpService already reconciles plugin-owned MCP servers and supports local/remote transports.
- portable SKILL.md parser/resource bounds already exist.
- plugin install already validates local sources, archive traversal, symlink/hard-link behavior, and per-name install locking.
- CodeGG already discovers Claude/OpenCode skill roots, but that is filesystem skill interoperability, not plugin-package interoperability.

## 4. Invariants that must not regress

- Agent Plugins parsing/discovery executes no package scripts.
- valid skills and valid MCP components can load independently when another component is invalid, matching portable failure boundaries.
- portable package metadata cannot grant CodeGG tool/agent permissions.
- ProjectAssetSnapshotBuilder remains asset owner.
- McpService remains MCP owner.
- CodeGG native manifest.toml remains supported.
- package source format and provenance remain inspectable.
- package containment is checked lexically and canonically; symlink escape is rejected.
- PLUGIN_ROOT points only at the canonical installed package root.
- PLUGIN_DATA points at a separate CodeGG-managed writable data root, never at arbitrary package paths.
- secrets remain environment/credential mediated; imported mcp.json values do not become logs.
- unsupported foreign commands/hooks are inert.
- no remote schema fetch is required to validate a known Agent Plugins version; supported schemas/rules are implemented locally.
- installation does not imply activation.

## 5. Explicit non-goals

- claiming portable conformance for commands/hooks/agents/LSP/UI;
- automatically executing Claude hooks or commands;
- JavaScript/TypeScript runtime support;
- automatic plugin update;
- remote catalogs (M004);
- dependency solving;
- general Git repository cloning;
- copying foreign packages into CodeGG-native manifest format;
- writing into .claude/.grok/.opencode plugin roots.

## 6. Required production changes

### A. Add a package-format detector/parser

Introduce one canonical package loader that can classify a local plugin directory before installation.

Recommended precedence when multiple recognized manifests exist:

1. explicit caller-selected format if the UI/API supplies one;
2. CodeGG manifest.toml for backward compatibility;
3. root Agent Plugins plugin.json with recognized $schema;
4. bounded Claude-compatible fallback at .claude-plugin/plugin.json.

Ambiguous multiple formats should produce a diagnostic showing the selected precedence. Do not merge two root manifests opportunistically.

Represent the result as a canonical loaded-package descriptor with:

- format enum;
- package name/version/description/source metadata;
- passive contributions;
- executable CodeGG capabilities only for native CodeGG format;
- diagnostics;
- unsupported-component inventory;
- content/source digest inputs.

Do not manufacture executable PluginCapability values for portable components.

### B. Implement Agent Plugins 1.0 manifest validation

Add typed structs or a locally vendored/compiled schema validation path for the exact supported 1.0.0 core fields.

Requirements:

- require exact recognized $schema;
- validate name and metadata types/bounds;
- report/ignore unknown top-level fields as specified;
- ignore unimplemented extensions namespace members without interpreting their contents;
- reject fatal schema violations;
- preserve metadata such as author/homepage/repository/license/keywords for inspection only;
- bound JSON file size before parsing.

Do not fetch schemas at runtime.

### C. Discover fixed skills/ component

For an Agent Plugins package:

- inspect only immediate child directories under skills/;
- require exact SKILL.md regular file;
- use existing portable Agent Skills parser;
- preserve scripts/references/assets inventory through existing bounded skill resource model;
- skip invalid skill while retaining other skills/MCP;
- reject symlink/path escape;
- do not recursively treat arbitrary nested SKILL.md as additional skills.

Translate valid skills into the existing plugin passive contribution/source path consumed by ProjectAssetSnapshotBuilder.

### D. Parse and translate root mcp.json

Implement the Agent Plugins v1 mcp.json format locally.

At minimum support the transport variants CodeGG can map faithfully:

- stdio;
- streamable-http.

If the v1 schema includes a transport CodeGG cannot faithfully support, diagnose/skip only that server entry or MCP component according to the portable failure boundary.

For stdio:

- command resolution follows Agent Plugins rules;
- package-relative command/cwd containment is enforced;
- default cwd is plugin root where required by the specification;
- inject PLUGIN_ROOT and PLUGIN_DATA into the subprocess environment;
- expand only the portable variables/fields defined by the specification;
- preserve env clearing/secret rules from McpService;
- PLUGIN_DATA is a CodeGG-managed per-plugin data directory with owner-only permissions where supported.

For streamable-http:

- translate URL/headers/timeouts into the existing remote McpService config boundary;
- existing SSRF/DNS/redirect/OAuth behavior remains authoritative;
- portable config cannot weaken CodeGG network policy.

Translate resulting entries into ResolvedPluginMcpServer/PluginContributions rather than opening connections in the package parser.

### E. Add explicit passive runtime representation if needed

A conformant Agent Plugins package may contain only skills/MCP and no executable CodeGG runtime.

Do not fake a callable builtin handler.

If PluginManifest/PluginInfo currently requires an executable PluginRuntimeSpec for all packages, introduce a truthful Passive/None representation or a separate canonical InstalledPluginPackage descriptor consumed by PluginRegistry/contribution resolution. Choose the smallest approach that:

- keeps executable PluginService dispatch typed;
- cannot accidentally invoke a passive package;
- preserves manifest.toml compatibility;
- survives restart.

### F. Bounded Claude-compatible passive import

Add compatibility only for declarative assets with an existing exact CodeGG parser.

Recognize .claude-plugin/plugin.json as a compatibility package if no higher-priority native/Agent-Plugins manifest was selected.

May import:

- skills using the existing portable skill parser;
- agent definitions only when they match an already supported explicit CodeGG/Claude-compatible parser;
- MCP declarations from the documented package MCP location/config shape when a deterministic adapter exists.

Must only detect/report, not execute:

- commands;
- hooks/scripts;
- LSP contributions;
- client UI;
- unknown extension/runtime fields.

Diagnostics should state unsupported component counts/names without reading/executing bodies unnecessarily.

Do not claim Agent Plugins conformance for this compatibility path.

### G. Unify local install path

Refactor install_from_path_into to:

1. validate/canonicalize source;
2. detect/parse supported package format;
3. validate package identity/contributions;
4. derive install destination from canonical package identity;
5. hold existing per-name install lock;
6. copy the complete validated package under canonical plugin root;
7. read back/rehydrate from installed destination;
8. register with source metadata and leave activation policy unchanged.

A source need not contain manifest.toml if another supported package manifest is valid.

Preserve atomic/rollback behavior on copy failure.

### H. Provenance and management UI

Plugin info/doctor should show:

- source format: codegg / agent-plugins-1.0 / claude-compat;
- schema version;
- install path/source path;
- passive component counts;
- supported/unsupported components;
- package digest if available;
- MCP server diagnostics;
- no secret/body dumps.

## 7. Protocol, storage, migration, and compatibility effects

Protocol:

- management DTOs may gain package-format/schema/provenance fields;
- no change to provider tool protocol.

Storage:

- create per-plugin PLUGIN_DATA root for portable stdio MCP packages;
- retain installed package files as source of truth with activation state separate;
- no new database required unless existing activation metadata needs a format field.

Migration:

- existing installed manifest.toml packages continue to load unchanged;
- no conversion of existing packages required.

Compatibility:

- current CodeGG plugin SDK/runtime unaffected;
- existing plugin passive contributions continue through same owners;
- Agent Plugins packages become additive;
- Claude-compatible import is best-effort bounded, never silent execution.

## 8. Ordered work packages

### WP1 — Format detector and Agent Plugins manifest parser

- package-format enum/provenance;
- plugin.json 1.0 validation;
- fixed-location discovery and component-local diagnostics;
- hermetic fixtures from the normative spec examples.

Exit: valid/invalid portable package inventories can be produced without installation/execution.

### WP2 — Skills and MCP translation

- skills/ integration through existing parser;
- mcp.json translation;
- PLUGIN_ROOT/PLUGIN_DATA handling;
- passive-package representation.

Exit: local fixture package contributes skill/MCP through existing owners.

### WP3 — Unified local installer and restart

- install_from_path accepts CodeGG + Agent Plugins + bounded Claude compat;
- installed destination revalidation;
- restart load/provenance;
- activation isolation.

### WP4 — Claude passive compatibility, management UX, docs

- compatibility detection;
- unsupported component diagnostics;
- info/doctor;
- docs/conformance matrix.

## 9. Failure, restart, and contention semantics

- fatal root plugin.json error rejects the portable package;
- invalid skills skip only those skills;
- invalid mcp.json disables MCP component but valid skills remain;
- invalid individual MCP server follows spec/local adapter failure boundary without corrupting valid unrelated components;
- install copy failure rolls back staging/destination as today;
- concurrent same-identity install uses existing lock;
- restart reparses installed package deterministically and resolves durable activation;
- missing PLUGIN_DATA can be recreated safely; deleting package data must not alter immutable plugin root.

## 10. Required tests

Agent Plugins conformance fixtures:

- minimal plugin.json;
- full metadata;
- unknown top-level field non-fatal diagnostic;
- fatal wrong-type field;
- unsupported $schema;
- extensions ignored;
- skills missing is valid;
- invalid skill skipped;
- mcp.json missing is valid;
- invalid mcp.json leaves skills;
- version mismatch plugin.json/mcp.json behavior;
- stdio and streamable-http parsing;
- unsupported transport diagnostic;
- PLUGIN_ROOT/PLUGIN_DATA expansion;
- path/symlink escape rejection.

Install/restart:

- install native manifest.toml unchanged;
- install Agent Plugins directory;
- duplicate name lock;
- read-back from destination;
- restart registry/contributions;
- activation remains separate;
- uninstall removes package while preserving unrelated plugin state.

Claude compatibility:

- skills/MCP imported where supported;
- commands/hooks detected but never executed/registered;
- higher-priority native/Agent Plugins manifest wins deterministically.

## 11. Required verification commands

~~~text
cargo test -p codegg plugin
cargo test --test plugin_contributions
cargo test --test skills_registry
cargo test -p codegg mcp
scripts/verify.sh quick
~~~

## 12. Documentation updates

- architecture/plugin.md
- architecture/skills.md
- architecture/mcp.md
- docs/PLUGINS.md
- plugin install/user docs
- explicit Agent Plugins 1.0 conformance matrix
- plans/closure/plugin-ecosystem-interoperability/002-status.md

## 13. Acceptance criteria

- local Agent Plugins 1.0 package installs without manifest.toml;
- plugin.json/mcp.json use locally supported versioned validation;
- skills and MCP flow through existing owners;
- component-local failure semantics match the portable spec;
- PLUGIN_ROOT/PLUGIN_DATA behavior is correct and contained;
- passive package has no fake executable runtime;
- native CodeGG plugins remain compatible;
- bounded Claude passive import never executes unsupported hooks/commands;
- install and activation remain separate;
- restart reconstructs package/provenance/contributions;
- focused tests and scripts/verify.sh quick pass.

## 14. Stop conditions

Stop and report if:

- Agent Plugins conformance requires runtime schema download;
- portable stdio path semantics cannot be mapped without weakening install/process containment;
- PLUGIN_DATA requires broad filesystem authority outside a CodeGG-owned root;
- correct passive-package support requires pretending it has an executable runtime;
- Claude compatibility requires executing undocumented script semantics;
- existing native plugin install identity would be broken.

## 15. Closure evidence required

- implementation commit(s);
- Agent Plugins 1.0 conformance checklist/evidence matrix;
- native-plugin compatibility tests;
- component-local failure tests;
- path/symlink containment evidence;
- PLUGIN_ROOT/PLUGIN_DATA tests;
- MCP transport translation tests;
- restart/activation isolation evidence;
- Claude unsupported-component no-execution evidence;
- exact verification commands/outcomes;
- explicit confirmation that no new MCP/asset/plugin runtime owner was introduced.
