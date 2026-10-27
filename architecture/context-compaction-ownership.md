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

## Cross-turn provider history replacement (C001)

The frontend continues to own and display its full transcript. `TurnRuntime`
computes a versioned, domain-separated SHA-256 prefix identity from the exact
frontend message DTOs before provider conversion. At compaction, `rollover`
compares the source and replacement sequences, records the covered message
count and identity in the bounded checkpoint only when the dropped prefix and
retained suffix are unambiguous, and keeps the existing `source_history_digest`
diagnostic-only. On the next turn, `AgentLoop` drops only an exact incoming
prefix match, retains every suffix row, and injects the checkpoint projection
before the current user prompt. Unsupported versions, missing identities,
ambiguous peer/control rows, conversion mismatches, and invalid counts retain
the submitted history and emit a bounded diagnostic. A compactor may retain a
contiguous opening anchor in its immediate provider request; the checkpoint
frame still supersedes the covered prefix on the next turn, provided the exact
retained suffix is unambiguous. Any non-contiguous retained row in that prefix
invalidates the identity and fails closed. Durable session history and the
public `TurnSubmit` contract are unchanged.

## Fresh context epochs — M004 handoff integration (consumer, not owner)

`src/context/epoch.rs` is a consumer/path of the canonical owner above, not
a second compaction engine. It owns no token-accounting algorithm, no
transcript store, and no history rewrite:

- policy is `codegg-core::work_plan::epoch_policy::decide_epoch`
  (deterministic host-state function; default disabled/conservative);
- reconstruction is `build_fresh_epoch_messages` from canonical
  system/objective/Goal/WorkPlan/Todo/continuation/steering/handles only;
- validation reuses `rollover::validate_replacement_messages` and
  `compaction::{validate_message_invariants, count_continuation_frames}`;
- persistence reuses `rollover::prepare_candidate` /
  `AgentLoop::finish_prepared_install` (Prepared → Installed + atomic
  `ContextCompacted` marker) via `AgentLoop::try_start_fresh_epoch`;
- durable session history is never deleted; only the
  in-memory/provider-visible sequence is replaced after verification.

Checkpoints carry bounded WorkPlan provenance
(`codegg-core::work_plan::checkpoint`, max 5 actionable + 5 blocked
summaries, never the full plan); pre-M004 checkpoints without `work_plan`
remain readable and render safely. Rollover revalidation compares
WorkPlan ID/revision alongside goal/plan/todo/parent and aborts/rebuilds on
drift. Restarts load only `Installed` rows. The `context_epoch:started`
bus event carries IDs/revisions/counts/reasons only. Normal compaction
remains the canonical fallback/default path; unsupported profiles
(default/fast/local/tool-fragile) stay on it.

## Source Verification

First verified 2026-10-06 against `src/context/`, `src/agent/compaction.rs`, and
`crates/codegg-core/src/session/`. Re-verified 2026-10-23 against tree
`12999178a` (static grep and file reads only; no build or test run).

Re-checked on 2026-10-23:
- `CompactionStatus` (`src/context/compaction.rs:832`) still has exactly the 7
  variants listed above: `Ready`, `CompactionRequired`, `Compacted`,
  `InsufficientCapacity`, `ProviderFailure`, `InvalidHistoryOrBudget`,
  `Cancelled`.
- The `ContextCompactionRequest` → `ContextCompactionResult` boundary is real
  (`compaction.rs:845`, `:889`), with `compact_context` at `:950` and
  `compact_with_policy` at `:2821`.
- `src/agent/compaction.rs` is a **7-line pure re-export**
  (`pub use crate::context::compaction::*;`) with **zero** `pub fn` /
  `pub async fn` definitions — it contains no implementation, as documented.
- All named modules exist: `src/context/{compaction,rollover,epoch,plan,packer,
  policy,volatile_tail,continuation}.rs`.
- The durable store claims are exact:
  `CONTINUATION_CHECKPOINT_MAX_PAYLOAD_BYTES = 128 * 1024`
  (`crates/codegg-core/src/session/continuation.rs:51`) and the
  `continuation_checkpoint` table is created by `migrate_v57`
  (`crates/codegg-core/src/session/schema.rs:2568`, dispatched at `:354`),
  matching the doc's `continuation_checkpoint` (v57) reference.
- The M004 handoff symbols all exist: `assemble_continuation_snapshot`
  (`src/context/continuation.rs:167`), `validate_replacement_messages`
  (`src/context/rollover.rs:306`), `count_continuation_frames`
  (`src/context/compaction.rs:2496`), `build_fresh_epoch_messages`
  (`src/context/epoch.rs:264`), `decide_epoch`
  (`crates/codegg-core/src/work_plan/epoch_policy.rs:215`), and the four
  `AgentLoop` adapter methods in `src/agent/context_runtime.rs`
  (`inject_installed_continuation_for_turn` at `:773`, `compact_if_needed` at
  `:827`, `finish_prepared_install` at `:1640`, `try_start_fresh_epoch` at
  `:1752`).
- `codegg-core::work_plan::checkpoint` bounds both the actionable and the
  blocked summary lists at `MAX_CHECKPOINT_WORK_ITEMS = 5`
  (`crates/codegg-core/src/work_plan/checkpoint.rs:26`), matching the
  "max 5 actionable + 5 blocked" claim.
- `ContextCompacted` is the durable commit-marker event
  (`crates/codegg-core/src/session/continuation.rs:354-366`), and
  `context_epoch:started` is `AppEvent::ContextEpochStarted`
  (`crates/codegg-core/src/bus/events.rs:167`, named at `:510`).

Corrected on 2026-10-23:
- `migrate_v57` was cited at `crates/codegg-core/src/session/schema.rs:2491`
  with its dispatch at `:297`; both had drifted. The function is at `:2568` and
  the `57 => migrate_v57(...)` dispatch arm is at `:354`.
- `ContextCompactionRequest` was cited at `compaction.rs:844`; it is at `:845`.

Re-checked on 2026-10-26 against the C001 implementation and focused tests:
`TurnRuntime` hashes the ingress DTO prefixes before conversion; `rollover`
computes the replaced prefix identity from the exact source/replacement pair;
the bounded checkpoint stores the version/count/hash; and `AgentLoop` trims
only an exact match before injecting the installed projection ahead of the
current user. `source_history_digest` remains diagnostic, the transcript and
`TurnSubmit` contract are unchanged, and ambiguous cases retain history. The
new two-run AgentLoop regression and the existing M004, epoch-handoff, and
long-horizon continuity suites passed locally.
