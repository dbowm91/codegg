# Context and Compaction Ownership

## Canonical owner

`src/context/compaction.rs` is the single CodeGG production owner for
model-aware context accounting and compaction. It owns effective capacity
calculation, reserved-output handling, trigger decisions, pruning and
selection, hybrid/programmatic compaction, invariant validation, fallback
classification, and the bounded typed result returned to the agent runtime.
`eggcontext` remains the dependency-free tokenizer primitive used by that
owner.

`AgentLoop` supplies turn sequencing, plugin hooks, provider selection, the
owning `ProviderRequestContext`, and post-compaction frame/todo/event
orchestration. It does not select a compaction strategy or independently
calculate the context budget.

## Production-path inventory

| Path | Owner | Caller | Provider interaction | Persistence | Mode |
|---|---|---|---|---|---|
| Context capacity and compaction trigger | `context::compaction::needs_context_compaction` and `compact_context` | `AgentLoop::compact_if_needed` (in `agent::context_runtime`) | None for the decision; provider is passed only to the canonical engine | None; caller retains history | synchronous decision, asynchronous execution |
| Legacy truncate/summarize/drop behavior | `context::compaction` | `compact_context` | Optional summarizer call with the owning session context | None | asynchronous when a provider is used |
| Hybrid/programmatic evidence and invariant fallback | `context::compaction::compact_with_policy` | `compact_context` | Optional semantic checkpoint with the owning session context | No transcript rewrite or compaction metadata store | asynchronous |
| Context plan and provider message projection | `context::plan::ContextPlan` | AgentLoop before provider calls | No direct provider call | None | synchronous |
| Cache-aware packing diagnostics | `context::packer`, `effective_cost`, `cache_stats` | `agent::context_runtime` | Reads normalized usage only | In-memory, session-local | synchronous |
| Tool-palette policy state | `context::policy::ContextPolicyRuntimeState` and policy functions | `agent::context_runtime` / AgentLoop adapter | No direct provider call | Ephemeral turn state | synchronous |
| Volatile-tail policy | `context::volatile_tail` | `agent::context_runtime` | No direct provider call | Existing artifact handles remain authoritative | synchronous mutation only in explicit compact mode |
| Token primitive | `eggcontext` | `context::compaction` and projection helpers | None | None | synchronous |
| Historical API path | `agent::compaction` re-export | Existing integrations/tests | Delegates entirely to `context::compaction` | None | compatibility adapter only |
| Durable continuation checkpoints | `codegg-core::session::continuation::ContinuationCheckpointStore` | Later M004 rollover sequencing (M001 lands the store only) | None; the store never calls a provider/model | `continuation_checkpoint` (v57) + atomic `ContextCompacted` commit marker | durable epoch foundation, no model-visible behavior change |
| Authoritative continuation snapshot | `context::continuation::assemble_continuation_snapshot` (pure) + `agent::context_runtime` adapter (storage lookups) | `AgentLoop::compact_if_needed` before the canonical engine | None; assembler is sync/deterministic, provider only in semantic enrichment | In-memory M002 candidate convertible to an M001 payload; M004 owns install sequencing | one typed snapshot per compaction attempt with explicit provenance/diagnostics |

## Before and after ownership map

Before this convergence, `AgentLoop` chose between a legacy path in
`agent::compaction` and a hybrid path in the same module, while the separate
`context` module owned an independent plan/packer and volatile-tail policy.
The two compaction branches duplicated trigger, budget, and fallback
orchestration.

After convergence, all production compaction requests go through the typed
`ContextCompactionRequest` → `ContextCompactionResult` boundary in
`context::compaction`. The result distinguishes `Ready`, `Compacted`,
`CompactionRequired`, `InsufficientCapacity`, `ProviderFailure`,
`InvalidHistoryOrBudget`, and `Cancelled`. The old module path is retained
only as a bounded source-compatible re-export; it contains no implementation.

`ContextFrame` and `ContextLedgerState` remain agent-owned compatibility
state because they describe CodeGG-specific evidence and post-compaction UI
instructions, not token policy. `ContextPlan` remains the provider-facing
chronology and cache-identity adapter. Neither is a second compaction owner.

## Provider context and cancellation

The canonical engine receives `ProviderRequestContext` from the owning turn
and passes it to every model-backed compaction request. A caller may provide a
`CancellationToken`; cancellation is checked before work and races the
provider-backed future so dropping the future aborts the in-flight operation
and returns the typed `Cancelled` result without replacing the input history.

Compaction policy state itself is not persisted separately. Durable
context-epoch state lives in `codegg-core` as continuation checkpoints
(`prepared -> installed | aborted`, explicit lineage, SHA-256 digest,
128 KiB bound) with an atomic `ContextCompacted` commit marker. Existing
session/history data is otherwise read as-is, hidden reasoning remains
private, checkpoint diagnostics carry IDs/digests/sizes only, and
recovery handles continue to point at the existing artifact store.
The historical `checkpoints` table and the goal Markdown journal are
not continuation storage and were not repurposed.

## Transactional rollover (M004)

`src/context/rollover.rs` owns steps C-F + D validation, `prepare_candidate`
/ `install_prepared` / `degraded_fallback`, source-revision capture/
revalidation, restart validation/rendering, and bounded diagnostics.
`AgentLoop::compact_if_needed` sequences A-J and owns the in-memory
replacement (H) between verification (F) and atomic install (I); it never
destroys history before a verified prepared checkpoint exists.
`AgentLoop::inject_installed_continuation_for_turn` loads the latest
installed checkpoint at turn start, merges a newer goal with M002
precedence, and injects exactly one bounded block. Production selection uses
the resolved default when `auto=true` and mode is omitted (deterministic
without a provider/model); legacy helpers are compat-only.
