# Plugin Ecosystem Interoperability M004 — Extension Catalog, Discovery, and Host-Mediated Installation

Status: implemented

Repository baseline: 2f64c9e16f9ee96ea4d47ed897201f4c24d4606f

Source roadmap:

- plans/subsystems/plugin-ecosystem-interoperability-roadmap.md#m004--extension-catalog-discovery-and-host-mediated-install

Long-term requirements:

- plans/000-long-term-specification.md#12-repository-asset-and-harness-interoperability
- plans/000-long-term-specification.md#22-audit-architecture
- plans/000-long-term-specification.md#24-protocol-and-storage-requirements
- plans/000-long-term-specification.md#26-reliability-and-recovery
- plans/000-long-term-specification.md#27-security-requirements
- plans/000-long-term-specification.md#28-observability
- plans/000-long-term-specification.md#29-system-invariants
- plans/003-planning-process.md

Applicable ADRs:

- None for bounded catalog metadata + explicit user install through existing plugin ownership.
- Stop and register an ADR before introducing a CodeGG-operated remote trust root, signing PKI, automatic update authority, or model-autonomous installation.

Primary class: capability / supply-chain control

Hard dependency:

- M002 unified installer/parser for CodeGG and Agent Plugins packages.

Soft dependency:

- M001 plugin tools enrich catalog capability summaries but are not required for catalog transport.

## 1. Objective

Replace the current empty official/repository MarketplaceService tiers with a bounded provenance-aware extension catalog and a single user-mediated installation workflow.

Models may search an already-resolved catalog for capability discovery. Models may propose that an extension be installed. They MUST NOT download, install, enable, update, or connect extension code without an explicit host/user action.

## 2. Current implementation evidence

At baseline:

- src/plugin/marketplace.rs defines MarketplaceService and PluginTier.
- list_local_plugins/search_plugins inspect only installed local CodeGG manifest.toml plugins.
- list_official_plugins and list_repository_plugins return empty vectors.
- PluginManager already supports list/info/enable/disable/install_from_path/uninstall/doctor.
- install_from_path has canonical path validation and install locking.
- install_from_url exists but is specialized around WASM/archive downloads and is not the desired general package installer.
- activation state is durable/global/workspace-aware and is deliberately separate from installation.
- passive plugin contributions and MCP reconciliation already have source provenance.
- tool_search discovers callable tools but cannot answer "what extension could provide capability X?" when that extension is not installed.

## 3. Research basis

Current coding-agent ecosystems increasingly provide extension catalogs/marketplaces, but the safe transferable pattern is metadata discovery plus explicit user installation.

Relevant sources reviewed 2026-09-20:

- Agent Plugins package format: https://agent-plugins.org/specification
- Grok Build marketplace/compatibility UX: https://docs.x.ai/build/features/skills-plugins-marketplaces
- OpenAI/other current plugin ecosystems were considered as product patterns, but CodeGG must preserve its local daemon/plugin/scheduler ownership rather than copy account-level cloud installation semantics.

## 4. Invariants that must not regress

- catalog metadata is not execution authority;
- PluginManager/installer remain install authority;
- PluginActivationStore remains activation authority;
- ProjectAssetSnapshotBuilder/McpService/PluginService remain component owners after install;
- model calls cannot choose arbitrary package URLs;
- model calls cannot approve their own install request;
- install does not imply enable/activate;
- PluginTier is provenance/UI classification, not a trust bypass;
- package digests provide integrity only; a digest from an untrusted catalog is not authenticity;
- installed package identity is derived from the package manifest, not catalog display name;
- package is parsed/validated in staging before commit;
- catalog/package size and entry counts are bounded;
- remote fetches use hardened CodeGG HTTP policy;
- catalog outage never disables already installed plugins;
- no automatic updates in this milestone.

## 5. Explicit non-goals

- automatic background plugin updates;
- dependency resolution;
- arbitrary Git clone/install;
- npm/pip/cargo package manager integration;
- central CodeGG signing PKI;
- model-autonomous install/enable;
- marketplace ranking/recommendation based on telemetry;
- payment/licensing system;
- running catalog-provided scripts during discovery;
- interpreting unsupported package formats.

## 6. Catalog source model

Introduce a typed ExtensionCatalogSource rather than hard-coded tiers only.

Initial source classes:

### Bundled

A catalog snapshot shipped with CodeGG/repository assets. This is the safest way to populate a small official set before a remote signing/trust system exists.

### Repository

An optional project-owned catalog file under a fixed CodeGG project location, for example a bounded .codegg extension catalog. Repository catalogs are project input, not trusted code.

### Personal configured remote/local

User-configured HTTPS or local-file catalog sources in CodeGG config.

Do not let project config silently add arbitrary remote catalogs unless existing project-trust policy explicitly permits it. A new remote source should require an explicit user/managed configuration action.

PluginTier may remain as a compatibility/display projection, but source provenance is the real data model.

## 7. Catalog schema

Define a versioned bounded internal catalog format.

Each entry should carry enough metadata for discovery without installing:

- stable catalog entry id;
- package name;
- version;
- description;
- author/homepage/repository/license if known;
- package format: codegg or agent-plugins-1.0;
- package artifact URL or local contained path;
- SHA-256 digest when remote artifact is specified;
- optional archive media/format;
- component summary: skills, MCP, CodeGG tool capabilities, agents, commands/hooks if native;
- prerequisites/runtime notes;
- security-relevant declared requirements as informational metadata;
- catalog source/provenance.

Bounds:

- catalog bytes;
- entries per source;
- strings/keywords/components per entry;
- package URL/path length;
- number of versions retained per logical extension.

Reject malformed catalogs as one source failure; preserve other sources and installed plugins.

## 8. Required production changes

### A. Replace MarketplaceService stubs with resolved catalog service

Refactor MarketplaceService into a read/discovery layer that can:

- load bundled source;
- load repository source if trusted/enabled;
- load configured personal sources;
- validate/bound each catalog;
- merge entries deterministically;
- report source collisions/shadowing;
- distinguish installed/available/update-available metadata without auto-updating.

Do not make catalog service own PluginRegistry.

Cache remote catalog metadata only if needed for startup responsiveness. Any cache must have source URL, fetch time, validator/digest, and bounded retention. Stale cache state is shown honestly.

### B. Harden remote catalog fetching

Use CodeGG's ordinary hardened HTTP client/Eggfetch path.

Requirements:

- HTTPS for remote catalogs unless explicit local-development override;
- DNS/redirect policy consistent with other untrusted remote fetches;
- response byte limit before parse;
- content-type handling;
- timeout/cancellation;
- ETag/Last-Modified optional optimization;
- no credentials in catalog URLs/logs.

A catalog URL is host configuration, never model input.

### C. General remote package staging/install

After M002 provides a unified local package parser/installer, add one remote artifact staging path.

Preferred initial artifact formats:

- tar.gz with existing hardened extraction extended for general package roots;
- optionally one additional format only if already supported safely.

Flow:

1. user selects catalog entry/version;
2. host resolves exact catalog snapshot entry;
3. download to CodeGG-owned staging with compressed byte cap;
4. verify SHA-256 when supplied/required;
5. extract with file-count and expanded-byte caps, rejecting links/traversal/special files;
6. identify one package root;
7. run M002 package parser/validation;
8. ensure package manifest identity/version match the selected entry or show explicit mismatch and abort;
9. atomically install through canonical install lock/commit path;
10. register installed plugin;
11. leave inactive unless user separately selected enable.

Do not reuse the old URL basename as package identity.

### D. Add model-facing extension_search

Register a Deferred ReadOnly native tool that searches the already-resolved catalog snapshot.

Input:

- bounded semantic/keyword query;
- optional component filter from a closed enum (skill, mcp, tool, browser, etc. only if data model supports it);
- small limit.

No URL/source argument.

Output:

- catalog entry id;
- name/version/description;
- component summary;
- source provenance/tier;
- installed state;
- prerequisites/security notes;
- whether an explicit user install is required.

The tool MUST NOT fetch arbitrary catalogs or install anything.

Use a typed ExtensionDiscover capability if normal NetworkResearch is semantically misleading; do not imply code-execution authority.

### E. Add host-mediated install proposal

The model may need to surface an extension need without instructing the user to manually copy an ID.

Add one of the following, preferring reuse of existing Question/approval infrastructure:

1. a deferred extension_install_request tool that accepts only catalog entry id + version and creates a host-owned install proposal/question; or
2. a structured result from extension_search that the TUI can convert into an explicit install action when the user selects it.

Whichever is chosen:

- the model cannot pass URL/path/digest overrides;
- at most one bounded pending request per turn/session unless existing question policy says otherwise;
- user sees source, version, components, prerequisite/network/code-execution warning, and activation state;
- approval causes the host/PluginManager to execute the install, not the model tool;
- denial/cancellation is final for that request;
- install does not enable automatically unless the UI offers a separate explicit choice.

Do not create an approval-shaped path that the model can self-answer.

### F. TUI/CLI marketplace UX

Use the existing plugin/extensions modal/management style.

Provide:

- source selector/provenance;
- search;
- installed/available state;
- package format;
- version;
- component summary;
- install;
- enable/disable separately;
- doctor/info;
- refresh catalog.

CLI/headless install should require explicit entry id/source and should support noninteractive confirmation flags only when the user explicitly requested them; default safe behavior must not hang hidden automation.

### G. Refresh runtime assets after explicit enable/install changes

Reuse existing durable activation and asset refresh semantics.

Install alone does not need to inject assets if the plugin remains inactive.

After explicit enable:

- resolve activation;
- reconcile passive assets/MCP through existing owners;
- publish a new immutable runtime-asset generation for subsequent turns;
- keep in-flight turn pinning.

### H. Audit and provenance

Record structural audit fields where audit infrastructure exists:

- catalog source id;
- entry id/version;
- artifact digest;
- install actor/principal;
- install outcome;
- activation change separately.

Do not log package bodies, secrets, or catalog auth headers.

## 9. Supply-chain and trust policy

A catalog label such as Official MUST NOT by itself reduce plugin permissions or bypass user confirmation.

If a bundled catalog is shipped, it can be considered product-curated metadata but installed code still receives normal PluginPolicy/permission treatment.

If later work wants cryptographic publisher trust/signatures:

- define a separate ADR/plan for key ownership, revocation, signature format, update policy, and UI semantics.

Do not imply that SHA-256 from the same compromised catalog authenticates the publisher.

## 10. Protocol, storage, migration, and compatibility effects

Protocol:

- additive catalog list/search/install-proposal DTOs if daemon/frontends need them;
- no provider protocol change.

Storage:

- optional bounded catalog cache;
- downloaded staging is temporary;
- installed packages remain canonical plugin files;
- activation store unchanged.

Migration:

- existing local plugins are surfaced as installed entries even if absent from a catalog;
- existing PluginTier fields may become compatibility projections.

Compatibility:

- PluginManager local path install remains;
- existing installed plugins continue to work offline;
- no catalog is required for personal-local operation.

## 11. Ordered work packages

### WP1 — Catalog schema/source resolver

- typed source config;
- bundled/repository/personal loaders;
- deterministic merge/provenance;
- bounds/diagnostics;
- local-only fixtures.

### WP2 — Remote fetch and unified package staging

- hardened catalog fetch;
- package download/digest/extraction caps;
- M002 parser integration;
- identity/version verification;
- install lock/atomic commit.

### WP3 — Model extension discovery + install proposal

- extension_search deferred tool;
- closed filters/output;
- host-mediated install request/selection;
- no model URL/install authority.

### WP4 — TUI/CLI management, refresh, audit, docs

- marketplace browsing;
- install vs enable separation;
- catalog refresh;
- activation/runtime asset reconciliation;
- closure evidence.

## 12. Failure, restart, and contention semantics

- one catalog source failure does not erase other sources;
- remote outage may use clearly marked cache or return unavailable, but installed plugins remain;
- package download/cancel leaves staging removable and no partial install;
- digest/identity mismatch aborts before commit;
- concurrent same-package install uses existing per-name lock;
- restart reconstructs installed plugins from disk and catalog cache independently;
- stale catalog entry cannot replace an installed version without explicit update action;
- activation remains pinned for in-flight turns.

## 13. Required tests

Catalog:

- bounded parse;
- malformed source isolation;
- deterministic duplicate resolution;
- bundled/repository/personal provenance;
- remote cache stale/fresh behavior if cache implemented.

Remote fetch:

- HTTPS policy;
- redirect/DNS policy;
- response byte cap;
- timeout/cancel;
- tar traversal/link/special-file rejection;
- compressed and expanded size caps;
- digest mismatch;
- manifest identity/version mismatch;
- atomic install/read-back.

Model authority:

- extension_search cannot accept URL;
- search only sees resolved catalog;
- install request accepts entry id/version only;
- model cannot approve request;
- denial/cancel leaves filesystem/activation unchanged;
- install leaves plugin inactive by default.

Compatibility/restart:

- local unlisted plugin remains visible/usable;
- offline startup with installed plugins;
- restart catalog/install state;
- enable publishes new asset generation; in-flight turn remains pinned.

## 14. Required verification commands

~~~text
cargo test -p codegg plugin
cargo test --test plugin_contributions
cargo test -p codegg tool_surface
scripts/verify.sh quick
~~~

Add hermetic fake-HTTP catalog/package tests; no public network in CI.

## 15. Documentation updates

- architecture/plugin.md
- docs/PLUGINS.md
- architecture/agent-tool-surface.md for extension_search
- configuration docs for catalog sources
- security/troubleshooting docs
- plans/closure/plugin-ecosystem-interoperability/004-status.md

## 16. Acceptance criteria

- MarketplaceService has real bounded catalog sources instead of empty official/repository stubs;
- CodeGG and Agent Plugins packages can be installed from a selected catalog entry through one canonical installer;
- package digest/identity/version are checked before atomic commit;
- extension_search is model-visible but read-only/deferred;
- model cannot choose arbitrary URLs or perform installation;
- install requires explicit user/host action and remains separate from enable;
- catalog provenance is visible;
- installed plugins work offline and do not depend on catalog availability;
- no automatic update/trust bypass exists;
- focused tests and scripts/verify.sh quick pass.

## 17. Stop conditions

Stop and register a new ADR/plan if:

- a remote official catalog requires CodeGG-managed signing keys/PKI;
- product requirements change to automatic updates;
- package dependency resolution is required;
- catalog installation must execute arbitrary setup scripts;
- model-autonomous install/enable becomes a requested behavior;
- supporting a catalog source requires bypassing hardened HTTP/installer policy.

## 18. Closure evidence required

- implementation commit(s);
- catalog schema/source fixtures;
- malformed-source isolation evidence;
- remote fetch/redirect/size/digest tests;
- archive traversal/expansion tests;
- manifest identity/version verification evidence;
- model search/no-install-authority tests;
- explicit user approval/denial tests;
- install-vs-enable separation evidence;
- offline/restart evidence;
- exact verification commands/outcomes;
- explicit confirmation that no automatic update or remote trust root was introduced.
