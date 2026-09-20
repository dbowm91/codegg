# Agent Context Discovery Surface M002 — Bounded Persistent-Memory Retrieval

Status: ready for handoff

Repository baseline: 2f64c9e16f9ee96ea4d47ed897201f4c24d4606f

Source roadmap:

- plans/subsystems/agent-context-discovery-surface-roadmap.md#m002--bounded-model-facing-persistent-memory-searchread

Long-term requirements:

- plans/000-long-term-specification.md#4-architectural-principles
- plans/000-long-term-specification.md#24-protocol-and-storage-requirements
- plans/000-long-term-specification.md#26-reliability-and-recovery
- plans/000-long-term-specification.md#27-security-requirements
- plans/000-long-term-specification.md#29-system-invariants
- plans/003-planning-process.md

Related closed work:

- plans/subsystems/context-continuity-compaction-roadmap.md
- plans/implementation/context-continuity-compaction/003-bounded-exact-context-recovery-references.md
- plans/closure/context-continuity-compaction/004-status.md
- architecture/memory.md
- architecture/context-ledger.md

Applicable ADRs:

- None. This milestone exposes existing curated MemoryStore data through normal daemon/tool authority. It does not add a new history service or change compaction ownership.

Primary class: capability

Dependencies:

- existing codegg-core MemoryStore;
- existing daemon memory operations;
- closed tool-surface progressive disclosure/parent ceiling work.

## 1. Objective

Add model-facing deferred memory_search and memory_get capabilities over CodeGG's existing persistent curated memory.

The model should be able to retrieve a relevant user preference or current-project convention when the bounded startup summary is insufficient, without receiving access to raw transcripts, event stores, continuation checkpoints, arbitrary context artifacts, other projects, or a new vector index.

## 2. Why the previously deferred area is now narrow enough

The closed context-continuity roadmap intentionally deferred:

- semantic history search across an entire session;
- vector retrieval over prior windows;
- cross-session continuation/shared team memory.

That remains deferred.

The current repository already has a distinct curated MemoryStore whose purpose is session-to-session learning. It supports:

- durable user and project namespaces;
- get(id);
- list(namespace);
- case-insensitive search(query);
- startup summary injection;
- daemon CoreRequest::MemorySearch;
- TUI /memory-search and /memory-list.

This milestone exposes only that existing curated store. It does not search conversation history. The new evidence therefore does not reopen the compaction roadmap's transcript/history non-goals.

## 3. Current implementation evidence

At baseline:

- crates/codegg-core/src/memory/mod.rs owns MemoryStore and Memory.
- Memory fields include id, namespace, title, content, URI, timestamps, access_count, importance, and superseded_by.
- MemoryStore persists under the CodeGG config/memory tree with advisory locking/atomic save behavior.
- MemoryStore.search currently searches persistent memory content.
- architecture/memory.md documents user/preferences and project namespaces.
- daemon CoreRequest::MemorySearch calls the configured memory store.
- TUI memory search caps display but the daemon operation is not a model authority boundary.
- no ToolRegistry memory_search or memory_get tool exists.
- AgentCapabilitySet has no explicit MemoryRead capability.

## 4. Invariants that must not regress

- MemoryStore remains the durable memory owner.
- Model tools call a daemon/core memory service seam; they do not open MEMORY.md files directly.
- Search/read is limited to the current authorized user's preference namespace and current authorized project's memory namespace.
- Tool input cannot supply an arbitrary namespace or filesystem path.
- exact get revalidates namespace scope and current authority.
- a child agent cannot gain memory access not granted by its parent.
- memory content is untrusted persisted data, not system instruction.
- memory_search returns compact metadata/snippets, not every full body.
- memory_get returns one exact record with hard output bounds.
- no memory write/delete/remember/consolidate action is model-callable in this milestone.
- superseded/deleted memory behavior remains consistent with MemoryStore.
- no MessageStore/EventStore/session transcript/continuation-checkpoint search is introduced.
- no vector/embedding dependency is added.
- personal-local behavior remains available without team configuration.

## 5. Explicit non-goals

- semantic embeddings;
- vector databases;
- fuzzy model-generated memory ranking beyond existing deterministic search/importance;
- transcript search;
- context artifact search;
- cross-project memory;
- team/shared memory;
- model-facing memory writes/deletes;
- automatic skill promotion changes;
- changing pattern detection/consolidation;
- replacing startup memory summary injection.

## 6. Required production changes

### A. Introduce an authoritative scoped memory read service

Do not let model tools call MemoryStore.search globally and filter after the fact.

Add or extend a core/daemon memory read seam with explicit host-derived scope. The service should accept a typed scope derived from current execution identity, conceptually:

- user preferences;
- current project;
- both allowed scopes.

The model cannot provide raw namespace strings.

Preferred additive operations:

- search_scoped(query, scope, limit);
- get_scoped(id, scope).

If the daemon protocol is the correct existing boundary, add bounded additive CoreRequest/CoreResponse forms or extend the current operation with host-derived context. Do not create a second in-process shortcut for the agent path.

### B. Resolve project memory namespace from authoritative context

The repository has historical path-derived memory namespace usage in some TUI paths, while the long-term architecture requires stable project identity.

Do not silently rewrite the entire memory namespace system in this milestone.

Add one named resolver used by the new agent path and, where practical, by existing TUI/startup memory paths. It should:

1. prefer stable ProjectId/project identity from ExecutionContext/session/project binding where production provides it;
2. retain a documented read-only compatibility fallback for the current workspace/path-derived namespace so existing memories do not disappear;
3. never let tool arguments choose project identity;
4. avoid writing duplicate memories to both namespaces;
5. record diagnostics when legacy fallback is used.

If clean stable-project migration requires a broader storage migration, stop and register a separate migration plan rather than embedding it here.

### C. Add explicit MemoryRead capability

Extend AgentCapabilitySet/Capability with MemoryRead or an equivalent narrowly typed authority.

Map memory_search and memory_get to MemoryRead.

Requirements:

- parent ceiling intersection removes both tools when MemoryRead is absent;
- specialist/custom agent permission definitions can deny the tools;
- plan/model disabled-tool filters remain effective;
- no filesystem/network authority is implied by MemoryRead.

Do not overload FilesystemRead; durable personal/project memory is a distinct authority domain.

### D. Add deferred memory_search

Register a deferred, read-only model tool:

Input:

- query: non-empty bounded string;
- optional limit that can only tighten a small host maximum;
- optional scope selector limited to host-admitted enum values such as current_project, user, both.

The tool MUST NOT accept namespace, path, project ID, user ID, session ID, or memory root.

Output per result:

- exact memory id;
- source scope kind (user/current_project);
- title where present;
- bounded content snippet;
- importance;
- updated timestamp;
- optional URI only if already safe/model-visible under existing memory policy;
- supersession status only as needed;
- truncation metadata.

Sort deterministically using existing search semantics plus stable tie-breakers. Do not add opaque model ranking.

Default maximum should be small, for example 8-10.

### E. Add deferred memory_get

Register a separate exact-read tool:

Input:

- memory id only.

The host resolves allowed scopes from current execution context and searches only those namespaces.

Output:

- id;
- source scope;
- title;
- bounded full content;
- importance/timestamps;
- URI if safe;
- provenance/trust framing.

Reject:

- unknown ID;
- ID found only in a different project;
- superseded/deleted entries if existing MemoryStore API considers them non-effective;
- forged namespace/path data (not accepted by schema).

Use a hard content cap even if current Memory entries are usually small.

### F. Progressive disclosure and role defaults

Both tools should be Deferred for ordinary coding.

tool_search descriptions should make the distinction from context_read explicit:

- context_read: exact recovery of retained same-session evidence handles;
- memory_search/get: curated cross-session persistent user/current-project memory.

Do not advertise memory tools in the minimal palette by default. Consider Curated inclusion only if evidence shows tool_search discovery is insufficient; default plan is deferred.

Plan mode may allow memory_search/get if it already permits read-only context operations and MemoryRead authority is present. Add explicit tests either way; do not rely on unknown-tool defaults.

### G. Trust framing and prompt-injection resistance

Persistent memory may contain stale or malicious text.

Structured output must frame memory as remembered data, not policy. The model-facing description should state that retrieved memory:

- may be outdated;
- may be superseded by the current user's instructions/project state;
- never overrides system, project, or current user instructions.

Do not interpolate full memory bodies into tool descriptions/system prompts.

### H. Audit and privacy

Use existing audit seams for model tool invocation without logging memory body/query content if current privacy policy treats it as retained user content. Structural metadata may include:

- operation search/get;
- scope kind;
- result count;
- memory-id digest or bounded identifier as allowed;
- success/denial/truncation.

Never log the memory content as audit metadata.

## 7. Protocol, storage, migration, and compatibility effects

Protocol:

- additive scoped memory request/response DTOs may be required;
- legacy TUI CoreRequest::MemorySearch remains compatible or is internally redirected to the scoped service with its existing UI semantics.

Storage:

- no new store;
- no schema change expected unless stable ProjectId migration is separately approved.

Migration:

- no broad migration in this milestone;
- legacy project-namespace read fallback may be necessary.

Compatibility:

- /memory*, consolidation, startup summary, and skill-promotion behavior must remain unchanged;
- personal-local use remains functional;
- memory disabled/unavailable state omits or typed-fails model tools honestly.

## 8. Ordered work packages

### WP1 — Scoped memory service and namespace resolver

- extract one host-authoritative read seam;
- implement scoped search/get with item/content limits;
- resolve user/current-project namespaces from runtime context;
- add legacy namespace fallback diagnostics if required.

Exit: daemon/core tests prove cross-project IDs cannot be retrieved.

### WP2 — MemoryRead capability and tools

- add capability;
- register memory_search/memory_get;
- set deferred disclosure;
- wire session tool registry with current execution/memory service;
- ensure parent ceiling/model/agent denies.

Exit: model can search and exact-read current scopes through ToolBroker only.

### WP3 — Trust projection, audit, and compatibility

- bounded snippets/full body;
- source/provenance fields;
- trust framing;
- existing TUI/startup behavior regression coverage;
- static guard against direct filesystem MemoryStore access from tool adapters if useful.

### WP4 — Qualification and docs

- forced cross-project/child-agent/legacy-namespace tests;
- architecture docs;
- closure record.

## 9. Failure, cancellation, restart, and concurrency semantics

Memory operations are finite reads and should honor turn cancellation before expensive projection/serialization where possible.

MemoryStore locking remains authoritative. A concurrent consolidation/remember/delete sees either the prior or next valid persisted/in-memory state; model reads never parse partially written files.

Daemon restart reloads MemoryStore through its normal initialization. The new tools have no independent cache that can outlive the authoritative store.

If memory service is unavailable, omit the tools when construction can know that; otherwise return a typed unavailable error. Do not fall back to reading config files.

## 10. Required tests

Core/service tests:

- scoped search user only;
- scoped search current project only;
- both scope union with deterministic ordering;
- exact get in scope;
- exact get different project denied/not found;
- query/result/item/content bounds;
- legacy project namespace fallback where applicable.

Tool-surface tests:

- memory tools deferred;
- tool_search discovers them;
- MemoryRead parent ceiling removes them;
- explicit agent deny removes them;
- model disabled_tools removes them;
- minimal/curated behavior matches intended policy.

Integration tests:

- create two project memories with overlapping terms; active project returns only its project memory plus allowed user memory;
- child without MemoryRead cannot search/get even if parent store exists;
- restart reloads and retrieval still works;
- current user instruction conflicts with memory text: projection labels memory as non-authoritative data (assert prompt/tool framing rather than model behavior).

Negative tests:

- schema has no namespace/path/project-id argument;
- unknown/foreign memory id cannot be read;
- no MessageStore/EventStore/context-artifact call from the memory tool implementation;
- no vector/embedding dependency added.

## 11. Required verification commands

~~~text
cargo test -p codegg-core -- memory
cargo test -p codegg memory
cargo test -p codegg tool_surface
scripts/verify.sh quick
~~~

Use final focused selectors after implementation.

## 12. Documentation updates

- architecture/memory.md
- architecture/agent-tool-surface.md
- architecture/tool.md
- architecture/context-ledger.md only to clarify distinction if needed
- docs/TROUBLESHOOTING.md or user command docs if memory availability diagnostics change
- plans/closure/agent-context-discovery-surface/002-status.md

## 13. Acceptance criteria

- memory_search and memory_get exist as deferred read-only tools;
- tools use daemon/core scoped memory authority, not filesystem reads;
- only user and current-project curated memory are eligible;
- cross-project exact reads fail closed;
- parent-agent capability ceiling includes MemoryRead;
- search and get outputs are bounded/trust-framed;
- no model memory mutation path is introduced;
- context_read remains exact same-session artifact recovery;
- no transcript search, vector DB, or new durable store is introduced;
- existing TUI/startup memory behavior remains compatible;
- focused tests and scripts/verify.sh quick pass.

## 14. Stop conditions

Stop and report if:

- current project identity cannot be resolved without path-derived durable identity and a safe compatibility seam cannot be defined;
- adding MemoryRead requires redesigning all capability serialization/permission contracts beyond an additive enum change;
- scoped daemon access cannot be implemented without exposing arbitrary namespaces;
- existing MemoryStore format cannot safely distinguish current-project/user scope;
- correct behavior would require transcript/vector search;
- implementation would make retrieved memory authoritative instructions.

## 15. Closure evidence required

- implementation commit(s);
- namespace-resolution/source-of-authority evidence;
- two-project isolation tests;
- parent-ceiling/agent-deny/model-disable tests;
- exact-ID scope revalidation test;
- item/content bound evidence;
- restart evidence;
- compatibility evidence for existing memory commands/summary;
- dependency diff showing no vector/embedding store addition;
- exact verification commands/outcomes.

