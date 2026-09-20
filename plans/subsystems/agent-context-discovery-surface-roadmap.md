# Agent Context and Discovery Surface Roadmap

Status: active; M001 and M002 ready for handoff

Repository baseline reviewed: 2f64c9e16f9ee96ea4d47ed897201f4c24d4606f

Planning branch: agent/extension-interoperability-planning-2026-09

Canonical long-term references:

- plans/000-long-term-specification.md#4-architectural-principles
- plans/000-long-term-specification.md#7-current-foundation-and-required-evolution
- plans/000-long-term-specification.md#16-durable-multilevel-agent-run-hierarchy
- plans/000-long-term-specification.md#24-protocol-and-storage-requirements
- plans/000-long-term-specification.md#26-reliability-and-recovery
- plans/000-long-term-specification.md#27-security-requirements
- plans/000-long-term-specification.md#28-observability
- plans/000-long-term-specification.md#29-system-invariants
- plans/003-planning-process.md

Related closed work:

- plans/subsystems/coding-agent-tool-surface-corrective-roadmap.md
- plans/closure/coding-agent-tool-surface-corrective/007-status.md
- plans/subsystems/context-continuity-compaction-roadmap.md
- plans/closure/context-continuity-compaction/004-status.md
- plans/implementation/context-continuity-compaction/003-bounded-exact-context-recovery-references.md
- plans/subsystems/runtime-assets-plugin-contributions-addendum.md
- plans/closure/runtime-assets/006-status.md
- architecture/agent-tool-surface.md
- architecture/mcp.md
- architecture/memory.md

No ADR is required to begin this roadmap. It adds bounded projections over existing canonical owners. Stop and register an ADR if implementation would move MCP lifecycle out of McpService, durable memory ownership out of codegg-core, expose generic transcript/event stores as model tools, or redefine the public session/history contract.

## 1. Purpose and ownership boundary

CodeGG now has a broad native coding surface, progressive tool disclosure, a capable MCP client, persistent memory, durable context artifacts, and a canonical ToolBroker. The remaining gap is not another large primitive-tool set. It is a coherent way for an agent to discover and read external MCP context and previously curated CodeGG memory without front-loading schemas or introducing parallel stores.

The target ownership split is:

~~~text
MCP servers
  -> McpService owns transport, auth, discovery, resources/prompts and structured protocol data
  -> bounded CodeGG projection owns model/user presentation
  -> ToolRegistry / ResolvedToolSurface owns model advertisement
  -> ToolBroker / permission system owns invocation authority

Persistent memory
  -> codegg-core MemoryStore owns durable user/project memory
  -> daemon CoreRequest operations own authorized access
  -> bounded model adapters expose search/read only
  -> ResolvedToolSurface parent ceilings prevent child authority widening
~~~

MCP prompts and resources are deliberately not treated as identical capabilities. MCP defines prompts as user-controlled interaction primitives and resources as application/context-controlled data. CodeGG should preserve that distinction: resource discovery/read may be available to an authorized model, while prompt invocation remains host/user controlled.

## 2. External research basis

Research was refreshed on 2026-09-20 before this roadmap was written.

Primary inputs:

- Model Context Protocol 2026-07-28 specification:
  - https://modelcontextprotocol.io/specification/2026-07-28/server/resources
  - https://modelcontextprotocol.io/specification/2026-07-28/server/prompts
  - https://modelcontextprotocol.io/specification/2026-07-28/server/tools
- Agent Plugins 1.0 specification for the growing MCP packaging/interoperability baseline:
  - https://agent-plugins.org/specification
- xAI Grok Build current extension/MCP behavior:
  - https://docs.x.ai/build/features/mcp-servers
  - https://docs.x.ai/build/features/skills-plugins-marketplaces
- Current coding-agent memory patterns, including explicit search/get rather than unconditional full-memory injection, were compared against CodeGG's existing MemoryStore and context-continuity architecture.

Relevant conclusions:

1. MCP tools, prompts, and resources have distinct control semantics. A compatibility layer should not flatten them into one unrestricted model authority.
2. Progressive discovery is preferable to eagerly advertising every external schema or resource.
3. Persistent memory retrieval is useful when it is scoped and explicit, but semantic search over raw transcripts substantially expands privacy, prompt-injection, and retention surfaces.
4. CodeGG already has the durable owners needed for both capabilities. New databases or external memory services are unnecessary for the first implementation.

## 3. Current-state findings

### MCP

At the reviewed baseline:

- McpService already supports local stdio and remote HTTP clients, OAuth, reconnect, structured tool results, server origin, and plugin-contributed MCP servers.
- McpService exposes list_prompts/get_prompt and list_resources/read_resource.
- MCP tools can be added to the model surface with namespaced identities and an exposure policy.
- MCP prompts and resources have no comparable resolved agent/user discovery projection.
- McpPrompt, McpResource, and McpResourceContent are currently thin compatibility DTOs; the implementation should verify their fidelity against the current protocol before exposing them more broadly.
- tool_search searches CodeGG ToolCatalog entries only. It does not search MCP resources or prompts.

### Persistent memory

At the reviewed baseline:

- codegg-core MemoryStore persists user and project memories and already implements get, list, and case-insensitive search.
- daemon protocol already has memory operations, including CoreRequest::MemorySearch.
- the TUI already exposes /memory-search, /memory-list, /memory-remember, and /memory-forget.
- session startup injects a bounded memory summary.
- there is no model-facing memory_search/memory_get pair.
- the closed context-continuity roadmap intentionally deferred semantic transcript/history search, vector retrieval across prior windows, and shared cross-session/team memory.
- context_read already supports exact same-session durable artifact recovery and must remain distinct from persistent curated memory.

The new evidence justifies exposing the existing curated MemoryStore to an authorized model. It does not justify reopening raw session-history search.

## 4. Invariants

The following remain mandatory:

- McpService remains the only MCP connection/auth/protocol owner.
- MemoryStore remains the persistent memory owner.
- ToolRegistry/ResolvedToolSurface decide model advertisement; ToolBroker and permission checks remain execution authority.
- A child agent cannot gain MCP-resource or memory-read authority absent from its parent ceiling.
- MCP resource text and persistent memory are untrusted data, never system-level instructions merely because they were persisted or supplied by a trusted transport.
- MCP prompt invocation is user/host controlled; model discovery cannot silently activate a server prompt.
- No generic MessageStore, EventStore, continuation-checkpoint, or transcript query tool is introduced.
- No vector database or embedding dependency is introduced in this roadmap.
- External binary/blob resource content is not dumped as unbounded base64 into model context.
- All search/list/read results have explicit item/byte limits and truncation diagnostics.
- Existing raw-MCP-tool exposure and native-wrapper policy remains compatible.
- Existing memory slash commands and startup summary behavior remain compatible.
- Runtime errors do not silently widen scope or fall back to unrelated namespaces/servers.

## 5. Non-goals

This roadmap does not add:

- browser automation;
- first-class plugin executable tools;
- plugin marketplace/catalog installation;
- semantic search over session transcripts;
- vector embeddings for memories;
- cross-project memory reads;
- team-shared memory;
- MCP server implementation changes unrelated to prompt/resource fidelity;
- MCP resource subscriptions unless required by a narrowly proven compatibility defect;
- automatic MCP prompt execution;
- automatic persistence of MCP resources into CodeGG memory.

## 6. Dependency graph

~~~text
closed coding-agent tool-surface work
            |
            +----> M001 MCP prompt/resource projection
            |
closed memory + daemon operations
            |
            +----> M002 bounded model memory retrieval

M001 and M002 may execute in parallel.
~~~

Hard dependencies for both milestones are already closed.

## 7. Milestones

### M001 — MCP resource discovery/read and user-controlled prompt activation

Plan:

- plans/implementation/agent-context-discovery-surface/001-mcp-resource-and-prompt-projection.md

Status: ready for handoff.

Establish a bounded projection over existing MCP resource/prompt APIs. The model gets deferred, policy-filtered resource search/read capability. MCP prompts gain user-facing discovery/activation through the existing command/question/context path, not autonomous model execution.

Exit condition: a connected MCP server can expose resources that an authorized agent discovers and reads through bounded namespaced handles, while prompts can be listed and explicitly invoked by the user with typed/provenanced content; neither path bypasses McpService, ToolBroker, or workspace/plugin activation policy.

### M002 — Bounded model-facing persistent-memory search/read

Plan:

- plans/implementation/agent-context-discovery-surface/002-bounded-persistent-memory-retrieval.md

Status: ready for handoff.

Expose the existing curated MemoryStore through model-facing deferred memory_search and memory_get operations. Search is limited to the user preference namespace and the current authorized project namespace, with compact snippets and exact-ID reads. It does not search raw messages, continuation checkpoints, context artifacts, or other projects.

Exit condition: an authorized agent can retrieve relevant persistent CodeGG memories on demand without receiving unrestricted history access, creating a new index/store, or bypassing daemon/project/parent authority.

## 8. Security and trust posture

MCP resource and memory content can contain stale, adversarial, or instruction-like text. Both milestones must mark provenance and source class in structured results and frame bodies as untrusted evidence. Tool descriptions should tell models to use the content as data rather than authority.

For MCP:

- preserve DNS/redirect/OAuth protections already owned by McpService;
- never expose credentials, request headers, OAuth tokens, plugin environment values, or server internals in discovery results;
- enforce server exposure/activation and workspace/plugin provenance;
- bound text; route binary content to an existing artifact/handle path or return metadata only.

For memory:

- derive project scope from authoritative execution/project context, not arbitrary tool arguments;
- exact get must re-check scope instead of trusting a prior search result;
- user-scope and project-scope results must be distinguishable;
- hidden/superseded/deleted entries follow existing MemoryStore semantics;
- tool calls cannot remember, mutate, delete, or supersede memory in this milestone.

## 9. Failure, cancellation, restart, and contention

Both surfaces are read-oriented.

MCP connection loss returns a typed unavailable/degraded result and uses existing reconnect behavior. A failed resource read does not switch servers or broaden URI scope. Resource catalogs may refresh at turn boundaries or explicit MCP refresh points; an in-flight turn must use one coherent server/exposure view.

Memory search/read failures return typed errors and never fall back to direct filesystem reads. Concurrent memory consolidation/writes use existing MemoryStore locking and atomic persistence. Search/read sees one valid store state; partial persistence is not surfaced.

Daemon restart reconstructs MCP servers and MemoryStore through existing owners. No new in-memory-only authority is introduced.

## 10. Observability

Add bounded diagnostics sufficient to answer:

- which MCP server/origin supplied a resource;
- whether resource metadata/content was truncated;
- which prompt was explicitly activated and by which user/host action, without logging its full body;
- memory search scope, result count, truncation, and exact-read outcome;
- policy/parent-ceiling omission reason for resource/memory tools.

Do not log resource bodies, memory bodies, MCP prompt bodies, secrets, or tool arguments containing user content.

## 11. Verification posture

Each milestone owns focused tests. Broad verification remains:

~~~text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
scripts/verify.sh quick
~~~

No new CI lane, external-network integration suite, vector benchmark, or live third-party MCP dependency is required. Use fake/local MCP fixtures and hermetic memory stores.

## 12. User-visible completion criteria

This roadmap is complete when:

- CodeGG can use MCP resources without requiring users to wrap them as ad-hoc MCP tools;
- MCP prompts are discoverable and explicitly user-invoked rather than silently model-triggered;
- model-facing MCP context remains progressively disclosed and bounded;
- an agent can search/read existing persistent user/project memories on demand;
- persistent-memory retrieval cannot escape current project/user scope or parent authority;
- context_read remains exact same-session artifact recovery, not a generic history search;
- no new memory database, vector index, MCP client, or duplicate tool execution authority exists.

## 13. Risks and deferred work

Primary risks are prompt injection through resource/memory bodies, accidental cross-project memory access, incorrect MCP prompt control semantics, large-resource context blowups, and duplicate discovery catalogs.

Deferred pending separate evidence:

- semantic/vector memory retrieval;
- raw transcript/history search;
- cross-session continuation search beyond curated MemoryStore;
- team/shared memory;
- MCP resource subscriptions and change streaming beyond compatibility needs;
- automatic promotion of MCP content into persistent memory;
- remote/coordinator replication of memory search.

