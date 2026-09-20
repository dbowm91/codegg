# Plugin Ecosystem Interoperability M001 — First-Class Plugin-Contributed Agent Tools

Status: implemented

Repository baseline: 2f64c9e16f9ee96ea4d47ed897201f4c24d4606f

Source roadmap:

- plans/subsystems/plugin-ecosystem-interoperability-roadmap.md#m001--first-class-plugin-contributed-agent-tools

Long-term requirements:

- plans/000-long-term-specification.md#4-architectural-principles
- plans/000-long-term-specification.md#12-repository-asset-and-harness-interoperability
- plans/000-long-term-specification.md#27-security-requirements
- plans/000-long-term-specification.md#29-system-invariants
- plans/003-planning-process.md

Applicable ADRs:

- None. Preserve PluginService as executable plugin runtime and ToolRegistry/ResolvedToolSurface/ToolBroker as model-tool authority.

Primary class: capability and contract repair

Dependencies:

- closed Runtime Assets M005/M006;
- closed coding-agent tool-surface corrective work.

## 1. Objective

Make CodeGG's advertised custom_tools plugin feature real.

Allow an activated plugin to declare bounded model-callable tools whose schemas are discoverable through the existing progressive tool surface, while every invocation still passes through CodeGG's canonical tool broker, permission system, parent-agent capability ceiling, plugin activation/policy, and plugin runtime.

The milestone must not create a second plugin-tool executor.

## 2. Research basis

OpenCode's current custom-tool mechanism demonstrates the ergonomic value of small extension-defined tools alongside native read/write/bash primitives:

- https://opencode.ai/docs/custom-tools/

CodeGG should adopt the capability, not the JavaScript runtime. Existing Process and WASM plugin runtimes are sufficient.

Agent Plugins 1.0 deliberately does not standardize generic executable tools, so this is a CodeGG-native plugin capability, not something to place in the portable Agent Plugins core format:

- https://agent-plugins.org/specification

## 3. Current implementation evidence

At baseline:

- src/plugin/api.rs ApiVersion::current() advertises custom_tools.
- src/plugin/api.rs already defines plugin-facing tools::ToolDefinition, ToolInput, and ToolOutput DTOs.
- src/plugin/manifest.rs PluginCapability has Command, Hook, Panel, StatusWidget, and EventSubscription only.
- crates/codegg-protocol/src/plugin.rs mirrors those capability and invocation variants and has no Tool variant.
- PluginRegistry indexes commands/hooks/panels/status/events only.
- PluginService has workspace-pinned activation and policy-gated Process/WASM/Builtin invocation.
- ToolRegistry and ToolBroker are the canonical model tool registration/execution boundaries.
- ToolSearch/ResolvedToolSurface already support deferred discovery, model disables, plan filtering, parent ceilings, aliases, and category/risk metadata.
- ToolDefinition/ToolExecuteBefore/After plugin hooks exist, but hooks are not a safe substitute for registering executable tool authority.

This is therefore primarily a missing adapter and capability contract.

## 4. Invariants that must not regress

- PluginService owns plugin runtime execution.
- ToolBroker remains the only production model-tool execution boundary.
- A PluginCapability::Tool declaration is never called directly from AgentLoop/provider code.
- Plugin tool names cannot shadow native tools, raw MCP tools, or another plugin's tools.
- Plugin activation is pinned for an in-flight turn.
- A child cannot gain a plugin tool whose required authority exceeds its parent's ceiling.
- Plugin-authored risk/effect metadata is not trusted to reduce host permission requirements.
- Missing effect metadata defaults conservatively.
- Existing plugin commands/hooks/panels/status/events remain compatible.
- Existing manifest.toml files without Tool capabilities remain valid.
- custom_tools is removed from advertised API features if this milestone cannot make it truthful; do not retain a false feature flag.
- Heavy/durable work launched by a plugin tool must use existing scheduler/submission APIs when the plugin runtime contract exposes such work; no plugin-owned scheduler is added.

## 5. Explicit non-goals

- adding JavaScript/TypeScript plugin execution;
- turning slash commands into tools automatically;
- making every plugin hook a model tool;
- portable Agent Plugins tool standardization;
- plugin dependencies/lockfiles;
- automatic plugin installation;
- friendly unnamespaced aliases in the first milestone;
- trusting plugin declarations as proof that external side effects are read-only.

## 6. Required production changes

### A. Add a canonical plugin Tool capability

Extend both manifest and protocol capability models with an additive Tool variant.

Define a bounded PluginToolSpec containing at minimum:

- name;
- description;
- input JSON Schema;
- optional output JSON Schema;
- optional handler/entry selector if the runtime supports per-capability routing;
- optional human-readable effect/risk hint;
- optional output-size hint that can only tighten host maxima.

Validation:

- normalized tool name length/character constraints;
- JSON Schema must be an object schema acceptable to existing provider/tool validation;
- bounded schema/description size;
- reject empty/duplicate tool names inside one plugin;
- reject reserved namespace patterns.

Do not let the plugin directly set ToolCategory, Capability, Core/Deferred disclosure, or permission bypass.

### B. Add PluginCapabilityInvocation::Tool

Extend codegg-protocol PluginCapabilityInvocation with:

- Tool { name }

Add PluginService::invoke_tool or a shared generalized invocation path that:

1. resolves the plugin/tool from the pinned activation view;
2. rechecks plugin enabled/active state;
3. builds PluginInvocation with session/turn/project/model/agent context where already available;
4. applies PluginPolicy;
5. dispatches to Process/WASM/Builtin runtime as appropriate;
6. validates/bounds response;
7. applies UiEffect policy before emitting any side effects;
8. returns structured tool data/diagnostics.

Do not route through invoke_command by pretending a tool is a command; share lower-level runtime dispatch if useful, but keep typed capability identity.

Builtin runtime behavior should remain explicit. If builtin handlers do not support tools, return a typed unsupported result rather than a success placeholder.

### C. Index plugin tools in PluginRegistry

Add a PluginToolRegistration index with:

- plugin id;
- local tool name;
- canonical model-facing name;
- description/schema;
- handler;
- source/trust metadata required by the adapter.

Canonical name policy:

- use a deterministic namespaced identity such as plugin__<plugin-id>__<tool-name>;
- normalize safely and collision-test;
- no first-milestone implicit unnamespaced alias.

Registration should fail the owning capability/plugin transactionally on canonical collisions or invalid schemas. Do not partially register half the tools from one malformed executable capability set unless existing plugin capability failure semantics explicitly support partial registration and diagnostics.

Unregister/disable must remove future-turn tool contributions without mutating an in-flight pinned registry/surface.

### D. Introduce a PluginToolAdapter implementing Tool

Create one normal Tool implementation per active plugin tool descriptor.

The adapter:

- exposes canonical namespaced name;
- returns bounded description/input schema;
- is Deferred by default;
- executes by calling the pinned/workspace PluginService invocation seam;
- returns StructuredToolResult with plugin provenance and bounded output;
- never calls ProcessRuntime/WasmRuntime directly;
- cannot access PluginRegistry global mutable activation at call time if the turn has a pinned activation view.

ToolCategory/effect policy must be host-owned and conservative.

Recommended first-milestone policy:

- default every executable third-party plugin tool to Mutating;
- permit a narrower category only when CodeGG can prove it from host-enforced runtime/permission constraints, not solely from a plugin-authored string;
- record the plugin's declared hint separately for diagnostics/UI.

Do not weaken prompts merely to match a plugin's read_only label.

### E. Wire adapters into session/turn ToolRegistry

The session tool registry is built synchronously around native options, while plugin activation resolution is async/contextual. Do not introduce process-global mutable tool registration.

Preferred seam:

1. resolve/pin PluginService for the workspace before final session/turn registry construction;
2. produce an immutable Vec<PluginToolDescriptor>;
3. pass those descriptors plus the pinned PluginService/dispatcher into the session tool-registry factory or register adapters in one explicit post-native construction step before ResolvedToolSurface is built;
4. update the shared live ToolCatalog so tool_search sees them;
5. freeze the set for that turn.

If the current construction order makes this impossible without a second registry, stop and refactor the factory seam narrowly first.

### F. Capability ceilings and permissions

Plugin tools must map to typed authority.

Because third-party effects cannot safely be inferred from schemas, add a conservative PluginExecute capability if needed, rather than mapping every plugin tool to FilesystemWrite.

The effective callable condition should include:

- parent AgentCapabilitySet permits PluginExecute;
- agent/tool rules do not deny canonical name;
- plugin is active in current workspace;
- PluginPolicy permits invocation;
- ordinary permission/approval mode admits the ToolCategory/risk;
- sandbox/runtime constraints admit the plugin runtime.

If PluginExecute is added, update serialization/tests/docs wherever AgentCapabilitySet is projected. Child agents inherit/narrow it exactly like Delegate/MemoryRead/etc.

### G. Progressive disclosure

All plugin tools are Deferred by default.

tool_search broad results should include:

- canonical name;
- plugin display/name provenance;
- purpose;
- category/risk;
- disclosure;
- schema_available.

Exact schema expansion should return only the admitted current-turn schema.

Do not place arbitrary plugin tools in CORE_PALETTE/CURATED_PALETTE/MINIMAL_PALETTE automatically.

An operator/project policy may later promote trusted tools, but that is outside this milestone.

### H. Output and UiEffect handling

PluginResponse data should become the structured tool result.

Bound:

- serialized structured data;
- text projection;
- diagnostics count/length;
- UiEffects count/payload via existing policy.

A tool response cannot use UiEffects to evade tool permission—for example, opening a dialog must still satisfy PluginUiPolicy. Effects should be emitted only after successful policy filtering and should remain frontend-neutral.

## 7. Protocol, storage, migration, and compatibility effects

Protocol:

- add PluginCapability::Tool and PluginCapabilityInvocation::Tool;
- bump plugin protocol/API version only if existing compatibility policy requires it; otherwise additive v1 decoding must be verified;
- ToolDefinition API fields should converge with the canonical spec rather than remain unused duplicates.

Storage:

- no new durable store;
- plugin manifests/activation remain authoritative.

Migration:

- none for existing plugins;
- old manifests decode unchanged.

Compatibility:

- existing commands/hooks and passive contributions unchanged;
- plugins without Tool capability see no behavior change;
- older plugin runtimes receiving unknown Tool capability must fail with explicit version/capability diagnostics rather than misdispatch.

## 8. Ordered work packages

### WP1 — Manifest/protocol/registry contract

- define Tool spec/invocation;
- validation;
- registry index and collision policy;
- truthful ApiVersion feature reporting.

Exit: plugin can register a tool descriptor but it is not yet model-callable.

### WP2 — PluginService typed invocation

- generalized runtime dispatch;
- pinned activation;
- PluginPolicy and UiEffect enforcement;
- output bounds/structured result conversion.

Exit: direct service test can invoke a registered Tool capability under policy.

### WP3 — ToolRegistry/Broker integration

- immutable descriptors into session registry;
- PluginToolAdapter;
- deferred ToolCatalog/tool_search visibility;
- PluginExecute parent ceiling/category/permission mapping.

Exit: model-style ToolBroker call reaches PluginService only after all authority checks.

### WP4 — Removal/reload/restart and docs

- disable/uninstall/future-turn disappearance;
- restart registry reconstruction;
- diagnostics/doctor;
- architecture/docs and closure.

## 9. Failure, cancellation, restart, and contention

- runtime timeout/cancellation uses existing Process/WASM timeout/cancellation behavior;
- failed plugin invocation returns a typed tool failure, not a malformed success;
- disable/uninstall racing with an in-flight turn does not invalidate the turn's pinned adapter/service; later turns omit it;
- duplicate registration is deterministic and fails before exposure;
- daemon restart reloads installed plugins/activation then reconstructs descriptors before turns;
- catalog/tool_search never returns an adapter without a callable pinned backend.

## 10. Required tests

Manifest/protocol:

- Tool capability round trip;
- invalid schema/name rejected;
- duplicate local tool rejected;
- old manifest compatibility.

Registry:

- canonical namespacing;
- native/MCP/plugin collision impossible;
- unregister removes indices;
- two plugins same local tool remain distinct.

Authority:

- tool not active in workspace omitted;
- parent without PluginExecute cannot call;
- explicit agent deny wins;
- model disabled_tools wins;
- permission mode/category still applies;
- plugin read_only hint cannot lower conservative category absent host proof.

Execution:

- Process tool invocation;
- WASM tool invocation when feature enabled;
- structured response + output bounds;
- UiEffect policy;
- timeout/error;
- restart reconstruction.

Discovery:

- deferred by default;
- tool_search compact result and exact schema;
- not added to core palettes.

## 11. Required verification commands

~~~text
cargo test -p codegg plugin
cargo test -p codegg tool_surface
cargo test --test plugin_contributions
scripts/verify.sh quick
~~~

Use final targeted selectors after implementation.

## 12. Documentation updates

- architecture/plugin.md
- architecture/tool.md
- architecture/tool_broker.md
- architecture/agent-tool-surface.md
- docs/PLUGINS.md
- plugin SDK examples/docs
- plans/closure/plugin-ecosystem-interoperability/001-status.md

## 13. Acceptance criteria

- custom_tools is truthful;
- Tool capability exists in manifest/protocol/registry;
- namespaced plugin tools are deferred and discoverable;
- invocation always enters ToolBroker before PluginService;
- PluginService/PluginPolicy/runtime remain canonical execution owner;
- parent ceiling has a typed plugin execution authority;
- untrusted plugin metadata cannot reduce host risk category;
- disable/uninstall/restart semantics are correct;
- existing plugins remain compatible;
- no second plugin runtime or tool broker is introduced;
- focused tests and scripts/verify.sh quick pass.

## 14. Stop conditions

Stop and report if:

- session tool construction cannot accept immutable plugin descriptors without process-global mutation or a second registry;
- correct effect enforcement requires trusting plugin-authored read-only labels;
- Process/WASM runtime cannot receive typed Tool invocation without a breaking protocol redesign;
- plugin tool execution would bypass ToolBroker/ApprovalRouter/sandbox;
- adding PluginExecute requires a non-additive agent authority redesign;
- the existing custom_tools API is used externally in a materially incompatible undocumented form.

## 15. Closure evidence required

- implementation commit(s);
- API/manifest compatibility matrix;
- namespacing/collision evidence;
- ToolBroker-only call-path evidence;
- PluginExecute parent-ceiling tests;
- conservative category/risk evidence;
- workspace activation pinning/race evidence;
- Process/WASM invocation tests;
- restart/disable/uninstall evidence;
- exact verification commands/outcomes;
- explicit confirmation that no duplicate tool/plugin runtime was introduced.
