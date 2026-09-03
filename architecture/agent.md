# Agent Module Architecture

## Purpose

The `agent` module (`src/agent/`) is Codegg's core orchestration engine. It
manages the execution cycle between LLM providers and tools, handling
streaming, tool dispatch, permissions, context compaction, background
subagents, model routing, and specialized runtimes (research, security
review). It also owns agent resolution, prompt compilation, and runtime
asset management.

## Where It Lives

| File | Role |
|------|------|
| `src/agent/mod.rs` | `Agent`, `AgentMode`, `AgentRuntimeKind`, builtin agents, resolution, safety envelope |
| `src/agent/loop.rs` | `AgentLoop` — main execution cycle, ~75 fields, streaming, tool dispatch |
| `src/agent/loop.rs:76` | `ToolDefCache` tuple type for cached tool definitions |
| `src/agent/loop.rs:403` | `AgentLoop` struct definition |
| `src/agent/tool_batch.rs` | Typed permission/MCP/broker batch boundary for tool calls |
| `src/agent/context_runtime.rs` | `ContextPolicyRuntimeState` — ephemeral context-policy backoff |
| `src/agent/provider_turn.rs` | `ProviderTurnAdapter` — provider retry and stream normalization |
| `src/agent/processor.rs` | `EventProcessor` — accumulates `ChatEvent` stream into messages |
| `src/agent/compaction.rs` | `ContextTracker`, compaction strategies, hybrid/programmatic engine |
| `src/agent/worker.rs` | `SubAgentPool`, `SubAgentSpawner`, `SubAgentReport`, descendant admission |
| `src/agent/router.rs` | `ModelRouter` — automatic model selection by task complexity |
| `src/agent/policy.rs` | `ExecutionPolicy`, `ToolExposureMode`, profile-aware defaults |
| `src/agent/tool_surface.rs` | `ResolvedToolSurface` — immutable, deterministic tool authority |
| `src/agent/context_frame.rs` | `ContextFrame`, `ContextLedgerState` — post-compaction context snapshot |
| `src/agent/registry.rs` | `AgentRegistry` — source-provenance-tracked agent resolution |
| `src/agent/asset_context.rs` | `AssetContext` — explicit workspace/project identity |
| `src/agent/asset_snapshot.rs` | `ProjectAssetSnapshot` — immutable runtime asset view |
| `src/agent/asset_snapshot_builder.rs` | `ProjectAssetSnapshotBuilder` |
| `src/agent/asset_refresh.rs` | `AssetRefreshCoordinator` — single-flight publication per scope |
| `src/agent/instructions.rs` | `ProjectInstructionResolver` — bounded instruction fragments |
| `src/agent/prompt.rs` | `PromptCompiler` — sole production system-prompt assembly |
| `src/agent/turn_runtime.rs` | `TurnRunInput`, `DefaultTurnRuntime` — daemon turn submission |
| `src/agent/specialized_runtime.rs` | Host-owned finalization for security-review and research runtimes |
| `src/agent/progress_recovery.rs` | `AutonomyState`, `RecoveryController` — bounded structured recovery |
| `src/agent/mention.rs` | `@mention` parsing and agent filtering |
| `src/agent/team.rs` | `Team`, `TeamMessage`, `AgentRole` — file-based multi-agent coordination |
| `src/agent/teams.rs` | `TeamManager`, `SharedTaskList`, team tools |
| `src/agent/builtins/generated.rs` | Auto-generated built-in agent definitions (do not edit) |

## How It Works

### Execution Lifecycle

```
TurnSubmit (daemon)
  → DefaultTurnRuntime builds TurnRunInput
    → AgentLoop constructed per turn
      → PromptCompiler::compile() assembles system prompt
      → ResolvedToolSurface built (native plan/model filtered)
      → ExecutionPolicy derived from ResolvedModelProfile
    → AgentLoop::run()
      1. Pre-execution hooks (SessionStart)
      2. ModelRouter::apply_auto_routing()
      3. Tool definitions built + exposure filter applied
      4. ContextTracker initialized
      Main Loop:
      5. Check limits (turns, tokens, timeout, steering)
      6. Pre-turn hooks (AgentStart)
      7. compact_if_needed() — overflow detection → pruning → hybrid/legacy compaction
      8. History hardening (fix orphan tool messages)
      9. ProviderTurnAdapter::stream_with_retry()
      10. EventProcessor accumulates streaming events
      11. Text repair (if adapter grammar configured)
       12. ToolBatchExecutor: permission → affected-path extraction → pre-state capture → parallel/serialized execution → post-state capture → checkpoint persist → results
      13. RecoveryController: progress/stall/fingerprint tracking
      14. Plan mode detection
      15. Post-turn hooks (AgentEnd)
      Repeat until no tool calls
      Post-loop:
      16. Goal accounting, continuation decision
      17. Drain follow-up prompts
      18. SessionEnd hooks
```

### Agent Resolution (5-layer priority)

1. Compiled built-in agents (from `assets/agents/*.toml` → generated.rs)
2. Global user files: `~/.config/codegg/agents/*.toml|*.md`
3. Project files: `.codegg/agents/*.toml|*.md` (relative to workspace root)
4. Config `agent` map overrides
5. Config `mode` compatibility overrides

Overlay behavior: file-based agents merge by default; `replace = true` for
full replacement; `disable = true` removes an agent. TOML files support
`extends = "<base>"` for inheritance. Config layers merge on top of existing
agents. The safety envelope (`apply_safety_envelope`) bounds permissions by
the most restrictive across agent, session, config, and hard-deny.

### Canonical Prompt Compilation

`PromptCompiler` is the sole production entry point for system prompts.
It consumes a resolved agent, model profile, capability surface, skills,
and an immutable `ProjectAssetSnapshot`. Each block has a typed kind, cache
class, and content hash. The compiler emits a versioned fingerprint used
by `ContextPlan` for context identity. There is no production
post-compaction system-string mutation.

### Runtime Asset Refresh

`AssetRefreshCoordinator` owns one publication stream per
`(project_id, workspace_id)`. It accepts an explicit `AssetContext`,
builds a candidate outside the publication lock, and assigns a generation
on publish. Failures retain the previous valid snapshot.
`TurnRunInput::asset_snapshot` pins the published `Arc` for the whole
turn. Refresh swaps affect subsequent turns only.

### Durable Edit Checkpoints

`ToolBatchExecutor` (`src/agent/tool_batch.rs`) is the canonical
mutation boundary for the native file-edit surface. For each batch
containing supported mutators (`write`, `edit`, `replace`, `multiedit`,
`apply_patch` update/create/delete/move), it derives the complete
bounded affected path set from accepted structured arguments via
`crates/codegg-core/src/snapshot/affected_paths.rs`, captures
`FileState::Absent` / `Present { hash, content }` pre-state before
execution, executes tools (serializing overlapping paths within the
batch to `effective_max = 1` so pre/post ordering is deterministic),
captures post-state for the same path set after execution, and
persists an `EditCheckpoint` with explicit
`workspace_id`/`session_id`/`turn_id`/`batch_seq` provenance via
`EditCheckpointManager`. A foreign workspace `FileChanged` event cannot
contaminate another turn's checkpoint because durability no longer
drains the unscoped global event stream. `FileChanged` remains an
observational UI signal (`src/tui/file_diff.rs`, `projection`).

Checkpoints reuse `SnapshotOptions` bounds and `is_safe_relative_path`
validation; oversized/binary/symlink cases or malformed move args mark
the batch non-restorable rather than storing a partial checkpoint.
Non-restorable tools (bash, plugins/MCP, git) never produce a
checkpoint. Daemon restart rehydrates checkpoints from SQLite;
no broadcast receiver state is required.

### Durable run control

`codegg_core::agent_run_control` owns the ordered, bounded mailbox and the
stable-boundary journal. `src/agent/run_control.rs` is the daemon bridge: it
authorizes the exact originating turn for a top-level run or the direct parent
run for a child, persists a control, then feeds a live
run's existing follow-up, steering, and cancellation channels. Live channels
are an optimization; queued/delivered records are replayed when a run
reattaches after disconnect or restart.

The same bridge keeps a bounded live-turn map keyed by the exact
(session_id, turn_id) supplied by TurnRunInput. A top-level child completion
uses that endpoint when the originating root turn is still active; it never
guesses from session-only or current-UI state. Nested completion continues to
use the direct parent run handle. Turn-owned group completion follows the
persisted owner discriminator, and member-terminal reconciliation publishes
the bounded group projection through the same bus path. Durable notification
claims prevent replay or concurrent reconciliation from duplicating terminal
follow-ups.

The `task` tool exposes `spawn`, `status` (`get` remains the legacy alias),
`message`, `interrupt`, `wait`, and `cancel`. `message` is ordinary bounded
model input. `interrupt` sets the loop steering flag and delivers at the next
safe boundary; it does not claim to preempt a side effect already executing.
`wait` is capped at 30 seconds and timeout means `still running`, never run
failure. The journal records lifecycle, control, safe-boundary, completion,
and recovery milestones only; token streams, hidden reasoning, credentials,
and complete tool output remain outside it.

Durable orchestration ownership is explicit: a root turn owns its accepted
top-level fan-out, while a delegated run owns only its direct descendants.
`AgentRunRecord.depth` is persisted and validated transactionally against the
parent, so scheduler/worker hops cannot reset descendant depth. Nested TaskTool
instances are built from the current run and store-derived task context; a
parent's parent is never used as the nested owner.

TaskTool structured execution preserves the accepted model tool-call identity
from `ToolExecutionContext.invocation_key`. Explicit input idempotency keys
override it; direct legacy calls receive a fresh bounded compatibility key.
Delegation, group, and mailbox acceptance therefore distinguish identical
intentional calls while retrying one accepted call idempotently. Durable task
rows retain a bounded request fingerprint so reusing one call identity for a
different spawn request fails closed.

The invocation key is a bounded digest of the execution owner (root turn or
durable run), provider-turn occurrence, provider call ID, and accepted call
ordinal. Delegation identity is derived from that resolved call identity only;
the durable task row's bounded request fingerprint independently rejects
reusing one accepted call identity for a different spawn request. Thus
identical requests from different accepted calls remain distinct while an
internal retry of one accepted call remains idempotent.

## Key Types & APIs

### Agent (`src/agent/mod.rs:100`)

```rust
pub struct Agent {
    pub name: String,
    pub role: Option<String>,
    pub description: String,
    pub mode: AgentMode,
    pub mode_name: Option<String>,
    pub model: Option<String>,
    pub fallback_model: Option<String>,
    pub variant: Option<String>,
    pub temperature: Option<f64>,
    pub top_p: Option<f64>,
    pub color: Option<String>,
    pub steps: Option<usize>,
    pub system_prompt: Option<String>,
    pub permissions: HashMap<String, String>,
    pub hidden: bool,
    pub thinking_budget: Option<usize>,
    pub reasoning_effort: Option<String>,
    pub runtime_kind: Option<AgentRuntimeKind>,
}
```

### AgentRuntimeKind (`src/agent/mod.rs:51`)

```rust
pub enum AgentRuntimeKind {
    Standard,        // Default
    SecurityReview,  // Defensive scanning
    Research,        // Multi-hop research
    Compaction,      // Context compaction
    Title,           // Title generation
    Summary,         // Summary generation
}
```

### AgentLoop (`src/agent/loop.rs:403`)

~57 fields. Key groups:

- **Turn identity**: `session_id`, `agents`, `state`, `limits`
- **Provider**: `provider: Box<dyn Provider>`, `tool_def_cache`, `base_request_tools`
- **Context**: `context_tracker`, `execution_policy`, `context_ledger`, `context_plan_cache_key`
- **Tool execution**: `tool_registry`, `tool_broker`, `permission_checker`, `mcp_service`
- **Recovery**: `progress_recovery: RecoveryController`, `recovery_parallel_limit`
- **State**: `steering`, `follow_up_tx/rx`, `question_tx/rx`, `pending_steer`, `cancel_rx`
- **Assets**: `runtime_asset_pin`, `projection_config`, `artifact_store`
- **Goals**: `goal_store`, `goal_wall_clock`
- **Subagents**: `subagent_pool`, `submission`
- **Workspace**: `workspace_root` (immutable, captured at construction)

### AgentLoopState (`src/agent/loop.rs`)

```rust
pub struct AgentLoopState {
    pub current_agent: String,
    pub turn_count: usize,
    pub total_tokens: usize,
    pub start_time: Instant,
    pub plan_mode: bool,
    pub plan_topic: Option<String>,
    pub tool_call_count: usize,
    pub unaccounted_tool_calls: usize,
    pub unaccounted_input_tokens: i64,
    pub unaccounted_output_tokens: i64,
}
```

### ExecutionLimits (`src/agent/loop.rs`)

```rust
pub struct ExecutionLimits {
    pub max_turns: usize,      // Default: 100
    pub max_tokens: usize,     // Default: 1,000,000
    pub timeout: Duration,     // Default: 600 seconds
}
```

### ToolDefCache (`src/agent/loop.rs:76`)

```rust
type ToolDefCache = (
    Option<String>,    // model
    bool,              // plan_mode
    bool,              // lsp_enabled
    String,            // mcp_surface_digest
    u64,               // permission_version
    bool,              // has_functional_task_spawner
    Vec<ToolDefinition>, // base definitions
    Vec<ToolDefinition>, // deferred definitions
);
```

### ResolvedAgentExecutionProfile (`src/agent/mod.rs:436`)

Fully resolved execution profile for subagent tasks. Bundles agent,
runtime kind, resolved model, and effective permissions.
`resolve()` applies model inheritance: agent.model → fallback_model →
parent model → config model → emergency default.

### EMERGENCY_DEFAULT_MODEL (`src/agent/mod.rs:402`)

```rust
pub const EMERGENCY_DEFAULT_MODEL: &str = "openai/gpt-4o";
pub const EMERGENCY_DEFAULT_WORKHORSE_MODEL: &str = "openai/gpt-4o-mini";
```

### Durable delegated runs

Daemon-owned `task spawn` calls create an `AgentTaskRecord` and an
`AgentRunRecord` in `codegg_core::agent_run` before submitting a
`JobPayload::SubagentRun`. The typed task/run IDs are the durable ownership
identities; the scheduler's `JobId`/`AttemptId` remain the queue and attempt
authority, and the numeric `TaskStore` ID is only a compatibility alias.

The run store persists bounded provenance (session/turn, project/repository/
workspace, agent/model, authority digest, budget, lineage, job/attempt links)
and validates lifecycle transitions. Duplicate call-derived delegation keys
resolve the original task/run only when the bounded request fingerprint also
matches. Completion, cancellation, submission failure, and startup
recovery use first-terminal-wins semantics; scheduler-owned cancellation and
generation recovery reconcile the durable run rather than relying only on a
live `CancellationToken`.

Mutation-capable durable children are automatically allocated a
`WorktreeLease` during `Preparing`, before the child loop is built. The
request's filesystem, terminal, Bash, Git, and commit tools are rooted at the
leased worktree; read-only children reuse the parent root and retain no write
or Git authority. Child Git policy permits only local staging/commit, while
push, remote/configuration, reset/clean, history integration, and recovery
remain separately controlled.

Completion persists a bounded `codegg_core::agent_run_result::AgentRunResult`
with Git-derived base/result commits, changed paths, repository state,
findings, validation/artifact slots, retryability, and recovery guidance.
The transcript is explanatory only. `AgentRunIntegrationService` validates
the recorded base and a clean, unchanged parent before dispatching an
explicit typed merge, cherry-pick, or rebase; no child completion mutates the
parent automatically.

### Durable convergence foundation

`codegg_core::agent_convergence` owns the bounded, host-side lifecycle for a
produce/verify convergence request. `ConvergenceRecord` persists the exact
delegated objective and acceptance criteria, their SHA-256 digests, the
turn/run owner, a hard maximum of four cycles, and a revision-checked state
machine. `ConvergenceCycleRecord` stores only references to existing producer
groups/runs and verifier runs; it never copies transcripts, hidden reasoning,
tool arguments, credentials, or complete `AgentRunResult` values.

`ConvergenceStore` has in-memory and SQLite implementations. The store
enforces idempotent creation, first-valid reference/verdict/decision writes,
compare-and-set lifecycle transitions, bounded owner/recovery listings, and
terminal monotonicity. `assemble_verifier_evidence` is a pure assembler over
the durable spec and bounded authoritative run-result fields. Its semantic
`Pass` verdict is advisory and is never `GoalVerificationVerdict::Met`, a
permission approval, Git integration authorization, or goal-completion
authority.

`classify_reconciliation` is a pure restart classifier. It reports whether an
existing run/group can advance, needs execution resumption, failed/cancelled,
or needs attention; M001 does not schedule work. The internal
`ConvergenceSummary` is intentionally bounded for a later frontend-neutral
projection and leaves detailed specs and evidence to authorized on-demand
fetches.

`SubAgentPool` remains the child-runtime adapter and retains semantic
delegation limits such as depth, fan-out, and tool budgets. Scheduler-owned
requests do not acquire the pool's machine-capacity semaphore, so resource
contention queues at the global scheduler. Standalone compatibility paths may
continue to use the pool directly and do not claim daemon guarantees.

The session projection adds bounded summaries for the durable run tree, owned
worktrees, and run groups. Run depth is copied from `AgentRunRecord` by the
single projection adapter; callers never infer it from parent presence or
presentation nesting. Group summaries expose only the bounded turn/run owner
discriminator and optional session/turn identity. It is derived from the
authoritative stores and is safe to replay after reconnect or daemon restart; it is never a second
execution or control authority. The summary includes typed run/task identity,
status/control state, branch/base/result commit, validation, and
attention-required state while leaving prompts, mailboxes, transcripts,
reasoning, and full artifacts behind their existing durable handles.

The legacy numeric `TaskStore` alias, `Subagent*` events, and `task get` remain
read/control compatibility surfaces for older clients and standalone mode.
New daemon clients should prefer typed run IDs, `wait`/group joins, and push
notifications.

### Model Aliases (`src/agent/mod.rs:397`)

```rust
pub const MODEL_ALIAS_FRONTIER: &str = "tier.frontier";
pub const MODEL_ALIAS_WORKHORSE: &str = "tier.workhorse";
```

### SubAgentRequest (`src/agent/worker.rs:82`)

```rust
pub struct SubAgentRequest {
    pub task_id: u64,
    pub prompt: String,
    pub agent: String,
    pub parent_id: Option<String>,
    pub denied_tools: Vec<String>,
    pub allowed_paths: Vec<String>,
    pub description: String,
    pub depth: usize,
    pub max_tool_calls: Option<usize>,
    pub parent_model: Option<String>,
    pub workspace_root: Option<PathBuf>,
}
```

### SubAgentReport (`src/agent/worker.rs:30`)

```rust
pub struct SubAgentReport {
    pub summary: String,
    pub files_examined: Vec<String>,
    pub commands_run: Vec<String>,
    pub findings: Vec<SubAgentFinding>,
    pub next_steps: Vec<String>,
    pub confidence: Option<String>,
}
```

### ModelRouter (`src/agent/router.rs:22`)

Routes by task complexity (Simple/Medium/Complex) based on tool name and
prompt content keywords. Configured via `auto_route_models`,
`small_model`, `medium_model`, `model`.

### ExecutionPolicy (`src/agent/policy.rs:12`)

Per-turn configuration derived from `ResolvedModelProfile`. Controls
context window, compaction threshold, reserved output tokens, max
parallel tools, tool exposure mode (Full/Curated/MinimalWithDiscovery),
and disabled tools.

### ToolExposureMode (`src/agent/policy.rs:4`)

```rust
pub enum ToolExposureMode {
    Full,                  // All tools (default for unknown models)
    Curated,               // Frontier/reviewer: core + specialized
    MinimalWithDiscovery,  // Fast/fragile/local: core + tool_search
}
```

### Capability & AgentCapabilitySet (`src/agent/tool_surface.rs:14`)

12 capability kinds: `FilesystemRead`, `FilesystemWrite`, `ShellReadonly`,
`ShellMutating`, `GitRead`, `GitWrite`, `NetworkResearch`, `Delegate`,
`ManageTodos`, `ManageGoals`, `Terminal`, `Image`. The capability set is
monotonic and supports intersection for parent ceiling enforcement.

### AgentRegistry (`src/agent/registry.rs:374`)

Central registry separating declarative sources from resolved runtime
agents. API: `load_for_context()`, `get()`, `list()`, `list_visible()`,
`list_primary()`, `list_spawnable()`, `diagnostics()`, `source_stack()`.
Uses `BTreeMap` for deterministic iteration order.

## Configuration Surface

| Config Key | Effect |
|------------|--------|
| `model` | Default model for all agents |
| `small_model` | ModelRouter simple-tier model |
| `medium_model` | ModelRouter medium-tier model |
| `auto_route_models` | Enable ModelRouter (default: false) |
| `agent.<name>` | Per-agent overrides (model, prompt, permissions, etc.) |
| `mode.<name>` | Mode definitions applied to matching agents |
| `compaction.*` | Compaction settings (mode, policy, thresholds) |
| `server.max_parallel_tools` | Override max parallel tool executions |
| `[context_policy]` | Context budget compaction settings |
| `[context_packer]` | Cache-aware context packing settings |
| `[model_profile.<model>]` | Per-model profile overrides |
| `EMERGENCY_DEFAULT_MODEL` | Hardcoded fallback when no model configured |
| `CODEGG_ROUTING_DISABLE=1` | Kill switch for command routing |

## Invariants & Gotchas

### Bounded run groups

`codegg_core::agent_run_group::AgentRunGroupService` coordinates at most 16
already-accepted direct child runs. Members are admitted independently by the
single scheduler; a group is not a workflow engine or a second resource
authority. `all` collects every terminal member, `any_successful` completes on
the first successful member, `first_completed` uses member order as the
deterministic tie-break, and `detached` returns after durable acceptance while
the group remains observable until all members finish. Cancellation is an
explicit persisted option for the `any_successful` and `first_completed`
policies. `spawn_many`, `status_group`, `wait_group`, and `cancel_group` are
bounded additions to the existing task tool; single `spawn` remains the
normal path.

- **Singleton daemon**: Exactly one daemon per OS user. `AgentLoop` runs
  inside the daemon; it does not hold the daemon lock itself.
- **Workspace root is immutable**: `workspace_root` is captured at
  construction. Never derive from `std::env::current_dir()` mid-turn.
- **Sync registries**: `PermissionRegistry`, `QuestionRegistry` are
  synchronous (`fn`, not `async fn`). Register before publishing events.
- **Registration-before-publish**: When publishing `PermissionPending` or
  `QuestionPending`, register the responder first.
- **Tool call count is cumulative**: For hard limits. Goal accounting
  uses separate `unaccounted_*` deltas.
- **Recovery never expands tool surface**: Permission denial is typed
  separately from tool failure. No base-palette restoration on failure.
- **AgentLoop::run returns Vec<ChatEvent>**: Compatibility vector.
  `terminal_output()` exposes only bounded public text to finalizers.
  Reasoning deltas are never passed to specialized finalizers.
- **Agent files merge by default**: `replace = true` for full replacement.
  Markdown files are merge-only (no overlay flags).
- **TOML-only features**: `bash_permission`, `path_permission`, `replace`,
  `merge` keys only work in TOML format, not markdown.
- **`disable = true`** removes an agent from resolution (Info diagnostic).

## Testing

- Unit tests: `src/agent/mod.rs::tests`, `src/agent/registry.rs::tests`
- Integration: `tests/agent_loop_harness.rs` (extensive harness)
- Compaction: `tests/compaction.rs`
- Narrowest run:
  ```bash
  cargo test -p codegg --test compaction
  cargo test -p codegg --lib agent
  ```

## Related Docs

- [compaction.md](compaction.md) — context window compaction
- [model-adapters.md](model-adapters.md) — declarative model adapters
- [agent-tool-surface.md](agent-tool-surface.md) — resolved tool surface
- [provider.md](provider.md) — provider trait and registry
- [permission.md](permission.md) — permission system
- [goal.md](goal.md) — goal runtime for long-horizon work
- [scheduler.md](scheduler.md) — global admission scheduler
