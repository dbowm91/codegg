# Agent Context Discovery Surface M002 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/agent-context-discovery-surface/002-bounded-persistent-memory-retrieval.md`

Source subsystem roadmap:

- `plans/subsystems/agent-context-discovery-surface-roadmap.md`

Repository baseline reviewed: `2f64c9e16f9ee96ea4d47ed897201f4c24d4606f`

Implementation commit:

- `c320117` — Implement context discovery and plugin interoperability foundations

## 1. Executive finding

M002 is complete. Existing curated `MemoryStore` data is now available through
deferred, read-only `memory_search` and `memory_get` tools. The model supplies
only query/limit/scope or an exact memory ID; namespaces, paths, project IDs,
and user IDs are host-derived. Search and exact reads exclude superseded records,
apply deterministic ordering, and return bounded trust-framed projections.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Authoritative scoped memory seam | `MemoryReadScope`, `search_scoped`, and `get_scoped` in `codegg-core` | pass |
| Current/legacy project compatibility | `MemoryReadScope::for_project` derives stable and legacy namespaces | pass |
| Explicit capability ceiling | `Capability::MemoryRead`, capability intersection, discovery mapping | pass |
| Deferred model tools | `src/tool/memory.rs` and factory registration | pass |
| No writes/history/vector retrieval | only search/get registration; no new index/store or write tool | pass |
| Exact scope revalidation | `get_scoped` checks namespace before access increment | pass |

## 3. Production implementation evidence

`MemoryStore` remains the durable owner. `MemorySearchTool` maps a host-derived
project identity to `MemoryReadScope`, admits only the `user`,
`current_project`, or `both` enum, and caps result count/snippet bytes.
`MemoryGetTool` accepts only an exact ID and revalidates that ID against the
same host-derived scope before returning bounded content.

The turn path prefers the stable project binding and falls back to the
authoritative workspace root for compatibility with existing path-derived
memories. The fallback is internal to the host context and cannot be supplied
by model input.

## 4. Verification executed

```text
rtk cargo test -p codegg-core memory::tests --locked                 # 9 passed
rtk scripts/verify.sh quick                                          # passed
```

The focused tests cover namespace derivation, legacy compatibility, foreign
project exclusion, and superseded-record exclusion. Quick verification also
passed all repository boundary and workspace checks.

## 5. Invariant review

- MemoryStore remains the only persistence/search owner.
- Model input cannot select a namespace, filesystem path, project identity, or
  user identity.
- Parent capability intersection removes MemoryRead and both memory tools when
  the parent does not grant the capability.
- Memory text is evidence with provenance, never system instruction.
- No model-facing write, delete, consolidate, transcript, or checkpoint path
  was added.

## 6. Failure and recovery review

Missing stores and unknown/foreign IDs fail through the normal tool error path;
there is no direct-file fallback. Concurrent persistence continues to use the
existing MemoryStore lock/atomic-save behavior. Daemon restart reconstructs
the same store and scope resolver; no in-memory authority was introduced.

## 7. Migration and compatibility review

No schema migration or embedding dependency was added. Stable project namespace
resolution is additive and retains the legacy path-derived namespace as a
read-only compatibility candidate. Existing startup summary injection and TUI
memory operations remain unchanged.

## 8. Security review

Scope is host-derived, exact reads recheck scope, superseded records are hidden,
and output is bounded. The tool does not expose raw memory roots, arbitrary
paths, session transcripts, continuation state, or hidden project data.

## 9. Documentation and operations

The implementation follows the existing `architecture/memory.md` and context
ledger ownership model. Tool results include scope/provenance framing and
bounded/truncation metadata; sensitive memory bodies are not logged.

## 10. Unresolved findings

None that prevent strict closure. Stable project identity migration beyond the
new read path remains a separate future storage decision, as required by the
plan's stop condition.

## 11. Roadmap disposition

M002 is closed. The agent context and discovery surface roadmap is now closed.
No downstream plan was blocked by this workstream; the separately gated plugin
interoperability milestones remain governed by their own M002 dependency.

## 12. Registry updates

The implementation plan is marked `implemented`, the roadmap is marked `closed`,
and the registry records M002 closed in the same status-change commit as this
closure record.
