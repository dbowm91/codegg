# Agent Context Discovery Surface M001 — MCP Resource Projection and User-Controlled Prompt Activation

Status: ready for handoff

Repository baseline: 2f64c9e16f9ee96ea4d47ed897201f4c24d4606f

Source roadmap:

- plans/subsystems/agent-context-discovery-surface-roadmap.md#m001--mcp-resource-discoveryread-and-user-controlled-prompt-activation

Long-term requirements:

- plans/000-long-term-specification.md#4-architectural-principles
- plans/000-long-term-specification.md#24-protocol-and-storage-requirements
- plans/000-long-term-specification.md#26-reliability-and-recovery
- plans/000-long-term-specification.md#27-security-requirements
- plans/000-long-term-specification.md#28-observability
- plans/000-long-term-specification.md#29-system-invariants
- plans/003-planning-process.md

Applicable ADRs:

- None. Preserve McpService, ToolRegistry/ResolvedToolSurface, ToolBroker, runtime-asset/plugin activation, and existing command/context ownership.

Primary class: capability

Dependencies:

- closed coding-agent tool-surface corrective work;
- closed Runtime Assets M006 plugin MCP contribution work;
- current McpService prompt/resource APIs.

## 1. Objective

Make MCP resources useful to the model through a bounded, deferred, policy-filtered search/read surface, and make MCP prompts discoverable/invokable through an explicit user/host action.

Do not flatten MCP prompts and resources into unrestricted raw tools. Preserve MCP control semantics:

- tools are model-callable subject to CodeGG authority;
- resources are external context/data that the model may discover/read when authorized;
- prompts are user-selected templates and MUST NOT be silently activated by the model.

## 2. Research basis

Current MCP specification reviewed on 2026-09-20:

- https://modelcontextprotocol.io/specification/2026-07-28/server/resources
- https://modelcontextprotocol.io/specification/2026-07-28/server/prompts
- https://modelcontextprotocol.io/specification/2026-07-28/server/tools

The implementation should follow the current protocol shape where CodeGG actually consumes it, but must not turn this milestone into a complete MCP rewrite. Protocol fields that are required to preserve resource/prompt identity, arguments, content type, current capability metadata, or cache/list freshness should be retained. Unrelated MCP capabilities remain outside scope.

## 3. Current implementation evidence

At baseline:

- src/mcp/mod.rs defines McpPrompt, PromptArgument, McpResource, McpResourceContent, and McpService.
- McpService already exposes list_prompts, get_prompt, list_resources, and read_resource.
- local/remote clients already own JSON-RPC transport, auth, modern/legacy negotiation, reconnect, and tool discovery.
- raw MCP tools are namespaced and filtered by McpExposurePolicy.
- plugin-owned MCP servers carry McpServerOrigin and are reconciled through the same McpService.
- tool_search searches the native ToolCatalog but does not describe resources/prompts.
- architecture/agent-tool-surface.md already provides progressive disclosure and parent ceilings.
- there is no model-facing resource search/read tool and no explicit user prompt activation surface with typed provenance.

## 4. Invariants that must not regress

- McpService is the only protocol/connection owner.
- Existing raw MCP tool visibility and native-wrapper behavior remain compatible.
- Resource reads never bypass connected-server/origin/exposure checks.
- Plugin MCP resources are visible only in workspaces where that plugin contribution is active.
- A model cannot activate an MCP prompt by calling a normal model tool.
- Remote prompt content never becomes a provider system message or privileged instruction merely because an MCP server supplied it.
- Resource/prompt results are bounded before entering model/UI context.
- Binary/blob content is never emitted as unbounded base64.
- Credentials, headers, OAuth tokens, process environment, plugin secrets, and internal endpoint diagnostics never appear in model-facing discovery.
- Resource handles cannot be forged to read arbitrary URIs outside the listed/validated resource set.
- No second MCP catalog or connection registry becomes authoritative.
- In-flight turns use one coherent resolved MCP/resource view.

## 5. Explicit non-goals

- autonomous model invocation of prompts;
- general MCP resource subscriptions;
- broad resource-template execution unless needed for exact compatibility and bounded safely;
- adding a browser engine;
- storing MCP resource bodies in persistent MemoryStore;
- exposing arbitrary local files merely because an MCP URI resembles a file URI;
- changing MCP server installation/configuration;
- replacing raw MCP tool exposure policy;
- implementing every optional field or feature in the MCP specification.

## 6. Required production changes

### A. Audit and tighten prompt/resource protocol fidelity

Before adding model/UI surfaces, inspect the current local and remote client implementations for:

- prompts/list and prompts/get response shape;
- resources/list and resources/read response shape;
- multiple returned content entries;
- text versus blob content;
- prompt argument metadata;
- server capability flags and list-change/cache metadata currently retained or dropped.

Make the smallest additive DTO changes required to preserve the information the new projection needs.

Preferred result shapes:

- prompt descriptor: server, name, description, bounded arguments;
- prompt get result: typed/bounded content blocks or messages with source role metadata retained as data;
- resource descriptor: server, URI identity, name/title where available, description, MIME type, bounded metadata;
- resource read result: one or more bounded content entries with URI/MIME/projection kind.

Do not map remote prompt roles directly into provider System or Assistant messages.

### B. Add a turn/workspace-scoped resource projection catalog

Do not make tool_search itself own MCP transport state.

Add a small projection owned by the turn/session construction path that derives resource descriptors from the already-resolved McpService view. The projection may cache descriptors for the turn, but McpService remains authoritative.

Each projected resource needs:

- server identity;
- server origin class (configured or plugin, without secrets);
- canonical resource URI internally;
- compact display metadata;
- an opaque or integrity-bound model handle;
- optional catalog generation/fingerprint;
- truncation/freshness diagnostics.

The model handle MUST NOT trust arbitrary user/model-supplied URI text. Exact reads should resolve a previously admitted handle or a validated listed-resource identity. If current MCP semantics require dynamic resource templates, stop and split a separate bounded template plan unless a safe exact matcher is trivial.

### C. Add deferred model tools for resource search/read

Add two native wrappers, names subject to final code review but semantically equivalent to:

- mcp_resource_search
- mcp_resource_read

mcp_resource_search:

- input: bounded query, optional server selector, optional limit capped by host;
- output: compact descriptors only;
- no resource bodies;
- default result limit should be small (for example 8-10);
- report total/truncated count honestly;
- filter to servers/resources visible in the current workspace/turn policy.

mcp_resource_read:

- input: exact handle only;
- optional bounded text byte/character request that can only tighten host maxima;
- output: provenance + bounded text content or artifact/metadata projection for binary data;
- no arbitrary URI/server pair accepted from the model.

Both tools:

- are Deferred by default;
- remain discoverable through normal tool_search;
- use ReadOnly ToolCategory;
- map to existing NetworkResearch/external-read authority rather than creating shell/filesystem authority;
- pass parent ceilings/model disables/plan-mode policy explicitly;
- execute through ToolBroker like every other native wrapper.

Plan mode may expose resource search/read only if the plan-mode policy already permits network research and tests establish that no mutation path is reachable.

### D. Bound text and binary resources

Define explicit host maxima for:

- descriptors per search;
- descriptor string lengths;
- content entries per read;
- total text bytes/characters returned;
- per-entry metadata size.

For text resources, truncate at valid UTF-8 boundaries and include truncation metadata.

For binary/blob resources, do not return raw unbounded base64. Preferred order:

1. if an existing artifact/run-store path can safely store the decoded or raw payload under the current session/workspace, return a bounded artifact handle and metadata;
2. otherwise return metadata explaining that binary content is available but not projected.

Do not add a new blob store only for this milestone.

### E. Add explicit user prompt discovery and activation

Add a user-facing MCP prompt operation through the existing command/TUI authority. The exact command shape may integrate with existing /mcp UX, for example:

- /mcp prompts [server]
- /mcp prompt <server> <name>
- an interactive argument form when required arguments are present.

Requirements:

- list only currently connected/visible servers;
- show bounded name/description/required arguments;
- prompt invocation occurs only because the user explicitly selected it;
- required arguments are validated before get_prompt;
- returned prompt content is wrapped as externally supplied prompt context with server/name provenance;
- remote System/Assistant role labels are preserved only as source metadata and MUST NOT directly create privileged provider messages;
- injection should use the ordinary next-turn input/context compiler so permission, audit, compaction, and transcript behavior remain coherent;
- preview/confirmation MAY be used for large or multi-message prompts, but do not add confirmation to trivial explicit invocation unless required by existing UX patterns.

The model may be told in normal context that the user selected an MCP prompt. It does not receive a model tool that can choose and activate prompts autonomously.

### F. Freshness and catalog identity

Extend MCP catalog identity only as needed so resource/prompt descriptor changes can invalidate the resource projection where appropriate. Avoid forcing full prompt/resource body fetches merely to compute a catalog fingerprint.

If current MCP capability metadata provides list-change notifications or cache TTLs already available in the transport, consume them consistently. Otherwise refresh descriptors at bounded host-owned points such as:

- server connect/reconnect;
- explicit MCP refresh;
- turn/session construction when stale.

Do not add a persistent subscription engine solely for this milestone.

### G. Permission, trust framing, and output

Resource outputs must identify content as untrusted external data. Tool descriptions and prompt compilation should make clear that instructions inside resource bodies do not supersede system/project/user authority.

Use existing structured tool result paths so provenance and truncation survive projection without dumping internal client data.

## 7. Protocol, storage, migration, and compatibility effects

Protocol:

- additive internal/public DTO changes may be required for prompt/resource fidelity;
- if CoreRequest/CoreResponse additions are needed for TUI prompt activation, keep them version-compatible and bounded;
- no removal/rename of existing MCP tool protocol.

Storage:

- no new authoritative storage required;
- artifact store reuse for binary resources is optional only if already appropriate;
- no resource/prompt bodies persisted by default.

Migration:

- none expected.

Compatibility:

- existing MCP configs, raw tools, plugin MCP contributions, OAuth stores, and native wrappers remain unchanged;
- servers that implement tools but not resources/prompts remain valid;
- unsupported resource/prompt features degrade with diagnostics rather than disabling tool use.

## 8. Ordered work packages

### WP1 — Protocol fidelity and projection DTOs

- audit local/remote prompt/resource parsing;
- add typed/bounded response models needed by projection;
- add protocol fixtures for multiple text/blob entries, required prompt arguments, malformed responses, and absent capabilities.

Exit: McpService can faithfully return bounded typed descriptors/content without model-facing changes.

### WP2 — Resource projection and model tools

- add turn-scoped resource catalog/handle resolver;
- register deferred resource search/read wrappers;
- thread current McpService/policy/workspace/plugin activation view;
- integrate parent ceilings and disclosure.

Exit: fake MCP resources are searchable/readable only through admitted handles.

### WP3 — Prompt UX

- add prompt listing/selection;
- add required-argument entry/validation;
- compile returned content as explicit external prompt context;
- ensure no server role becomes provider-system authority.

Exit: a user can intentionally invoke a fake/local MCP prompt end-to-end.

### WP4 — Freshness, observability, and docs

- catalog invalidation at existing lifecycle points;
- bounded diagnostics/doctor information;
- architecture/docs updates;
- static guards if needed to prevent direct prompt-role injection.

## 9. Required tests

Focused unit tests:

- prompt/resource protocol parsing;
- resource handle cannot be forged;
- search limit/truncation;
- text truncation on UTF-8 boundary;
- blob path does not dump base64;
- hidden/unavailable server omitted;
- plugin-origin server visibility respects workspace activation;
- parent ceiling can remove resource tools.

Integration tests:

- fake MCP server: resources list -> search -> exact read;
- two resources with same display name remain unambiguous;
- reconnect/catalog refresh replaces stale descriptor set without widening;
- user invokes prompt with required args and receives bounded context;
- model cannot invoke prompt through ToolBroker because no model-callable prompt tool exists;
- malicious prompt content cannot become provider System message;
- existing raw MCP tool fixture remains unchanged.

Negative/security tests:

- arbitrary URI read rejected;
- disabled plugin MCP server resource inaccessible;
- oversized descriptor/body bounded;
- malformed blob/text response fails typed;
- secrets/headers absent from result/log projections.

## 10. Required verification commands

~~~text
cargo test -p codegg mcp
cargo test -p codegg tool::mcp_resource
cargo test --test mcp_reconnect
cargo test --test plugin_contributions
scripts/verify.sh quick
~~~

Use actual final selectors after implementation. No public-network tests are required.

## 11. Documentation updates

- architecture/mcp.md
- architecture/agent-tool-surface.md
- architecture/tool.md
- architecture/command.md or MCP user docs for prompt invocation
- docs/MCP.md if present
- plans/closure/agent-context-discovery-surface/001-status.md

## 12. Acceptance criteria

- resource search/read is model-callable only through bounded deferred native wrappers;
- reads accept only admitted exact handles/validated identities;
- all model-visible content is bounded and provenance-tagged;
- binary resource content is not dumped unbounded;
- plugin/configured MCP origin and workspace activation are respected;
- prompt listing/invocation is user-controlled;
- prompt server role data never becomes privileged provider messages directly;
- existing raw MCP tools and wrappers remain compatible;
- focused tests and scripts/verify.sh quick pass;
- no duplicate MCP client/catalog authority is introduced.

## 13. Stop conditions

Stop and report if:

- correct implementation requires allowing the model to autonomously invoke MCP prompts;
- dynamic resource templates require arbitrary URI execution that cannot be safely bounded in this milestone;
- binary projection requires a new artifact store rather than reusing an existing owner;
- current MCP protocol parsing is materially incompatible with 2026-07-28 beyond prompt/resource scope;
- workspace/plugin activation cannot be determined without introducing process-global state;
- implementation requires bypassing ToolBroker or McpService.

## 14. Closure evidence required

- implementation commit(s);
- prompt/resource protocol fixture matrix;
- resource-handle forgery/URI-boundary evidence;
- server-origin/workspace isolation evidence;
- parent-ceiling/disclosure evidence;
- prompt role/authority regression test;
- byte/item bound tests;
- raw MCP tool compatibility test;
- exact verification commands/outcomes;
- explicit confirmation that no prompt/resource body persistence or second MCP owner was added.

