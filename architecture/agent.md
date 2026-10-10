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
| `src/agent/mod.rs` | Module declaration/re-export/composition surface; legacy `resolve_agents()` CLI boundary |
| `src/agent/definition.rs` | `Agent` (18 fields), `AgentMode`, `AgentRuntimeKind` (6 variants), model aliases, execution profiles, config-layer resolution |
| `src/agent/file_agents.rs` | Global/project agent-file loading (markdown/TOML), overlay flags, permission specs, lookup helpers |
| `src/agent/loop.rs` | `AgentLoop` — struct definition, turn identity, live controls, constructors, and high-level `run`/`run_inner` sequencing |
| `src/agent/tool_inspect.rs` | Pure tool-call inspection/classification (paths, bash/test/git/MCP), timeouts, model-flag gating |
| `src/agent/loop_output.rs` | Bounded `AgentLoopTerminalOutput` collector and local-path redaction |
| `src/agent/request_preparation.rs` | Per-turn request preparation: policy, routing, research hints, context frames, tool definitions |
| `src/agent/semantic_router.rs` | Bounded selector execution and exact virtual-model route resolution |
| `src/agent/turn_completion.rs` | Terminal publication, goal accounting/continuation, limit checks, run-boundary journaling |
| `src/agent/habit_observation.rs` | Host-owned habit observation adapter (allowlisted structural metadata only) |
| `src/agent/snapshot_capture.rs` | Snapshot capture, file-change draining, security-review trigger dispatch |
| `src/agent/follow_up.rs` | Notification injection and non-blocking follow-up drain |
| `src/agent/coordinator.rs` | `AgentLoopServices` construction boundary and typed `TurnLifecycle` phases |
| `src/agent/context_runtime.rs` | Turn-lifecycle compaction (`compact_if_needed`) and pack-observation phase (context-owned) |
| `src/agent/tool_batch.rs` | Typed permission/MCP/broker batch boundary for tool calls |
| `src/context/policy.rs` | `ContextPolicyRuntimeState` — ephemeral context-policy backoff |
| `src/agent/provider_turn.rs` | `ProviderTurnAdapter` — attempt-safe provider retry and stream normalization |
| `src/agent/processor.rs` | `EventProcessor` — accumulates `ChatEvent` stream into messages |
| `src/context/compaction.rs` | Canonical `ContextTracker`, budget policy, compaction engine, typed results |
| `src/agent/compaction.rs` | Compatibility re-export for the canonical context compaction owner |
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
      Admission → ContextPreparation
      Main loop (repeated per bounded turn):
        ContextPreparation
          → ProviderInvocation (ProviderTurnAdapter)
          → ToolExecution (ToolBatchExecutor, when calls exist)
          → Recovery (AutonomyState / RecoveryController)
        → Completion when the provider has no further calls, gated by the
          WorkPlan arbiter (`src/work_plan_arbiter.rs`): actionable work
          injects one bounded continuation prompt and continues within the
          existing turn/tool/time/token limits; `InFlight` polls the live
          handle; other assessments end the turn without marking complete
      Completion: WorkPlan turn-end close (ordinary plans only, host-passed
        and budget-intact), projection, goal accounting, follow-ups,
        goal continuation, SessionEnd hooks
```

### Semantic model routing

An optional `virtual:<name>` model is resolved by `SemanticRouter` after agent
configuration and before concrete provider invocation. Codegg translates its
`model_routers` config into the shared `eggpool-model-routing` compiler, then
executes the bounded selector through the normal `Provider` abstraction.
Selector output is accepted only when it is an exact compiled route ID;
invalid output, selector failure, or timeout uses the configured default.
Caller cancellation propagates without starting default work. Concrete models
and explicit concrete agent overrides bypass semantic routing.

The execution boundary has two independent decisions:

```text
Codegg selected provider connection / per-turn provider object
    |
    +-- semantic policy chooses one compatible concrete model
    |
    +-- request runs through the already-selected connection
            |
            +-- EggPool connection: EggPool selects the upstream
                account/provider behind its endpoint
```

`apply_semantic_routing` validates the resolved `provider/model` reference
against the existing turn provider and applies only the model suffix. It never
looks up or clones a route-provider as a replacement, and no target failure
re-enters semantic selection after concrete dispatch begins. Direct
cross-provider routes fail closed before dispatch; an EggPool connection is
the intentional aggregation exception. This protects the durable
`ProviderConnectionId`, revision, credential, and lifecycle decision. Selector
prompts and raw session identifiers are not persisted or logged; the bounded
EggPool `/models` probe remains provider discovery, not semantic routing.

Codegg retains `sticky` and `affinity_ttl_s` in the shared policy and
fingerprint, but does not implement EggPool's process-owned async affinity
cache. Codegg evaluates the selector per virtual-model turn, so those fields
do not provide Codegg-side route pinning or provider failover.

`TurnLifecycle` is an in-memory sequencing aid only. Durable run identity,
completion, recovery, and scheduler admission remain owned by the run-control,
RunStore, and scheduler services; the lifecycle enum never becomes a second
durable state machine.

### Coordinator/service ownership (M004)

`AgentLoop` owns only the current agent/turn counters, workspace and run
identity, cancellation/steering/question channels, bounded context ledger,
and transient habit observation. `AgentLoopServices` owns the handles needed
to invoke canonical services and is initialized once by the compatibility
constructor (daemon callers use the typed factory).

| Phase or concern | Canonical owner | Coordinator responsibility |
|---|---|---|
| Context preparation, compaction, cache policy | `crate::context` and `context_runtime` adapters | Sequence preparation and apply typed context plans |
| Provider invocation and retry | `ProviderTurnAdapter` / provider implementation | Supply the request and consume normalized events |
| Tool authorization and execution | `ToolBroker`, `ToolRegistry`, `ToolBatchExecutor` | Order the batch and route outcomes |
| Progress and recovery | `RecoveryController` / convergence services | Feed bounded observations and apply typed recovery decisions |
| Runs, checkpoints, goals, artifacts | RunStore/run-control, checkpoint, goal, artifact services | Trigger persistence at lifecycle boundaries |
| Projection and completion | context projection, event bus, lifecycle hooks | Publish final bounded events and terminal output |

No coordinator field reconstructs context-token policy, subprocess lifecycle,
Git safety/provenance, provider transport, tool authorization, scheduler
admission, or durable completion authority.

### Provider-turn attempt safety (execution-reliability M001)

Each logical turn runs at most 3 attempts under one UUID-scoped attempt
chain. Every attempt publishes `ProviderAttemptStarted`; failures publish
`ProviderAttemptFailed` (secret-safe `error_class`, `visible_output`,
`will_retry`); abandonment after visible output publishes
`ProviderAttemptSuperseded` instead of replaying.

- Only `Transient` taxonomy failures retry, and only before any
  externally visible output (`TextDelta`/`ReasoningDelta`/`ToolCallStarted`).
- After visible output the turn stops with a typed
  `interrupted after visible output (attempt <id>, class <class>)` stream
  error. The partial buffer is discarded, so an abandoned tool-call start
  is never executed and two generations never merge.
- Backoff is bounded exponential (1s/2s/4s, 30s cap) with full jitter;
  a capped server `Retry-After` hint raises the floor. Sleeps and the
  inter-event loop observe the turn `cancel_rx`; cancellation returns a
  `provider turn cancelled` error with no further attempt.
- The same provider object and request are reused; session-selected
  provider/model never changes here. Attempt diagnostics log only
  provider/model logical IDs plus error class — never credentials or URLs.
- No `CoreEvent`/protocol change was needed: the new bus events map to
  `None` in `map_app_event_to_core_event` (diagnostic-only, older clients
  unaffected).

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

Prompt-block precedence for overlapping work state (M002 §6.9, M004 §6.5):
current turn user input outranks an active goal revision newer than the
checkpoint, which outranks the installed continuation projection
(`PromptBlockKind::ContinuationState`, source
`continuation:installed-checkpoint`, `required` so a future active
packer cannot silently omit resume state), which outranks stale
checkpoint semantic next steps. `GoalContext`
(`goal:active-checkpoint`) remains the pre-first-compaction projection;
once an installed checkpoint exists the compiler must prefer the single
`ContinuationState` projection and must not emit a contradictory
duplicate objective/progress projection. M004 activates durable rollover:
`AgentLoop::compact_if_needed` sequences prepare/verify/replace/install
(A-J) and `inject_installed_continuation_for_turn` restores the latest
installed projection at turn start with newer-goal merge.

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

### Every tool-path await is bounded

The per-tool `tokio::time::timeout` wraps only `exec_fut`, which is
constructed *after* a tool has acquired its execution permit. Any await that
happens before that — the semaphore acquire, the workspace repository
checkpoint lease — therefore runs outside every timeout. Because the batch
joins all futures with `join_all`, a single unbounded await stalls the entire
batch: no tool is reported as failing, no error is returned, and the TUI spins
on one tool call indefinitely. Two guards close that class:

- `effective_max` is clamped to at least 1 before `Semaphore::new`. Config
  validation rejects `server.max_parallel_tools == 0`, but a profile-supplied
  value reaches `effective_max` through `ExecutionPolicy::from_profile`
  without that guard, and a zero-permit semaphore is permanently unsatisfiable.
- The permit acquire and the repository lease acquire are both wrapped in
  `tokio::time::timeout` using the same `tool_timeout()` (default 120s). A
  permit that never arrives becomes `ToolError::Timeout` with a `tracing::warn!`;
  a lease that never arrives drops checkpoint authority, which the surrounding
  code already treats as the recoverable "batch non-restorable" degradation.

`tool_timeout()` was previously bound to `_timeout_secs` and discarded, so the
intended telemetry never fired; it now drives both bounds.

### Tool-call timeouts

One effective duration is resolved per call, in `AgentLoop::effective_tool_timeout`
(`src/agent/tool_batch.rs`), and threaded through
`ToolExecutionContext::timeout_ms` so the batch wrapper and the tool itself
cannot disagree:

```text
model "timeout" / "timeout_ms" argument  (clamped to [1s, 3600s])
        |
        +-- absent --> per-tool ToolTimeoutConfig default
        |               (server.tool_timeout_seconds overrides)
```

Precedence and bounds live in `resolve_tool_timeout`. A malformed, zero, or
negative value falls back to the default instead of inventing a duration. The
ceiling matches the bound config validation already enforces on
`server.tool_timeout_seconds`, and is restated in the `bash` tool schema so the
model can see it rather than discover it by having its call killed.

Two properties this exists to protect:

- **A legitimate long run is not cut off.** The model-requested timeout used to
  be honoured only by the child process (`bash.rs` read `input["timeout"]`)
  while the batch wrapper applied `get_tool_timeout(name)` unconditionally, so
  a build asking for 1800s was still killed at the 120s default. The agent had
  no way to run a full build.
- **A timeout does not discard what it collected.** `bash` returned only the
  command line on `TimedOut`, throwing away the drained pipes. Those bytes are
  the only evidence of *where* a run went long, and are now returned as the
  tool result via `partial_timeout_report` (tail-bounded, so the tail shows
  where the command stopped).

Three deadlines wrap one tool call and **must stay strictly ordered**:

| Deadline | Bound | Role |
|---|---|---|
| `T` | the tool itself | primary bound; produces the detailed `ToolError::Timeout` with partial output |
| `T + 5s` | broker watchdog (`BROKER_WATCHDOG_GRACE_MS`) | backstop for a tool whose own teardown wedges |
| `T + 10s` | batch wrapper (`2 × TOOL_TIMEOUT_GRACE`) | last-resort backstop |

The broker re-arms a `tokio::time::timeout` from `BrokerContext::timeout_ms`
around the whole invocation, so that field must be strictly greater than the
tool's own deadline. It previously carried `exec_ctx.timeout_ms` unchanged, and
with no `deadline` the broker resolved it to exactly `T` — arming a timer
microseconds before the tool's own. The watchdog therefore *always* fired
first, converting `partial_timeout_report()` into a bare "broker invocation
timed out". That silently disabled `TOOL_TIMEOUT_GRACE`, `timeout_notice()`,
`tail_bounded()` and `TIMEOUT_OUTPUT_BYTES` on the bash path: a simple
`bash ls … | grep …` reported as an opaque timeout carrying no output and no
indication of where it stopped. `broker_watchdog_timeout_ms` owns the ordering
and is covered by `broker_watchdog_ordering_tests`.

`AppEvent::ToolResult` is published per *batch*, not per tool, so a slow call
freezes every sibling row in the chat area on "waiting for command/tool
output..." until the slowest tool in the batch returns. The live activity line
(`architecture/tui.md`) is what signals the agent is still working.

The model-facing timeout message names the ceiling when a request was clamped,
and otherwise says how to recover — re-run with a larger `timeout` if the work
is legitimately slow, or diagnose the cause if it should have been fast. This
follows OpenCode's model-visible per-call timeout; like OpenCode and Codex,
CodeGG has no total turn deadline (see below).

Note there is still no *total turn deadline*: `ExecutionLimits.timeout`
(default 600s) is only consulted after a turn completes, and the batch itself
is awaited without an outer wrapper. Neither OpenCode nor Codex CLI imposes one
either — Codex instead yields a `session_id` at `yield_time_ms` and lets the
model poll. A hard turn deadline is deliberately not added: it kills exactly the
long builds this section exists to protect. Closing that gap, if wanted, should
adopt yield-and-resume rather than a wall-clock cut-off.

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

The `task` tool action set is the schema enum in `src/tool/task.rs:799`:
`spawn`, `converge`, `convergence_status`, `convergence_decide`,
`convergence_cancel`, `spawn_many`, `create_group`, `status` (`get` is the
legacy alias), `message`, `interrupt`, `wait`, `cancel`, `status_group`,
`wait_group`, `cancel_group`. `message` is ordinary bounded model input.
`interrupt` sets the loop steering flag and delivers at the next safe
boundary; it does not claim to preempt a side effect already executing.
`wait` and `wait_group` clamp `timeout_ms` to 30,000 and a timeout means
`still running`, never run failure. The journal records lifecycle, control,
safe-boundary, completion, and recovery milestones only; token streams, hidden
reasoning, credentials, and complete tool output remain outside it.

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

### Agent (`src/agent/definition.rs:78`)

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

### AgentRuntimeKind (`src/agent/definition.rs:30`)

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

### AgentLoop (`src/agent/loop.rs:86`)

The loop has 34 direct fields. Service and policy handles are grouped in
`AgentLoopServices` so the coordinator cannot accidentally initialize one
canonical concern independently of the others.

Full field list: `services`, `lifecycle`, `state`, `limits`, `steering`,
`follow_up_tx`, `follow_up_rx`, `question_tx`, `question_rx`, `plugin_service`,
`session_id`, `turn_id`, `workspace_id`, `workspace_locks`,
`workspace_service_lease`, `checkpoint_batch_seq`, `recent_findings`,
`reviewer_denial_counts`, `original_user_prompt`, `current_user_prompt`,
`subagent_pool`, `submission`, `workspace_root`, `max_tool_calls`,
`goal_wall_clock`, `cancel_rx`, `steer_rx`, `pending_steer`, `local_paths`,
`context_ledger`, `run_id`, `habit_project_namespace`, `habit_actions`,
`habit_had_failure`.

- **Coordinator state**: `state`, `limits`, `lifecycle`, `pending_steer`, and live control channels
- **Identity**: `session_id`, `turn_id`, `run_id`, and immutable `workspace_root`
- **Service boundary**: `services: AgentLoopServices` (provider, context, tool, recovery, persistence, and projection handles)
- **Compatibility/observation**: `context_ledger`, `recent_findings`, and bounded habit observation
- **Delegation**: `subagent_pool`, `submission`

### AgentLoopState (`src/agent/loop.rs:56`)

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

### ExecutionLimits (`src/agent/loop.rs:70`)

```rust
pub struct ExecutionLimits {
    pub max_turns: usize,      // Default: 100
    pub max_tokens: usize,     // Default: 1,000,000
    pub timeout: Duration,     // Default: 600 seconds
}
```

### ResolvedAgentExecutionProfile (`src/agent/definition.rs` — re-exported via `mod.rs`)

Fully resolved execution profile for subagent tasks. Bundles agent,
runtime kind, resolved model, and effective permissions.
`resolve()` applies model inheritance: agent.model → fallback_model →
parent model → config model → emergency default.

### EMERGENCY_DEFAULT_MODEL (`src/agent/definition.rs:379`)

```rust
pub const EMERGENCY_DEFAULT_MODEL: &str = "openai/gpt-4o";
pub const EMERGENCY_DEFAULT_WORKHORSE_MODEL: &str = "openai/gpt-4o-mini";
```

`EMERGENCY_DEFAULT_WORKHORSE_MODEL` is defined at
`src/agent/definition.rs:382`.

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

### Model Aliases (`src/agent/definition.rs:374`)

```rust
pub const MODEL_ALIAS_FRONTIER: &str = "tier.frontier";
pub const MODEL_ALIAS_WORKHORSE: &str = "tier.workhorse";
```

### SubAgentRequest (`src/agent/worker.rs:82`)

```rust
pub struct SubAgentRequest {
    pub task_id: u64,
    pub run_id: Option<codegg_core::identity::AgentRunId>,
    pub prompt: String,
    pub agent: String,
    pub parent_id: Option<String>,
    pub parent_run_id: Option<codegg_core::identity::AgentRunId>,
    pub denied_tools: Vec<String>,
    pub allowed_paths: Vec<String>,
    pub description: String,
    pub depth: usize,
    pub max_tool_calls: Option<usize>,
    pub parent_model: Option<String>,
    pub workspace_root: Option<PathBuf>,
    pub workspace_locks: Option<Arc<codegg_core::workspace_services::WorkspaceLockTable>>,
    pub parent_sandbox_profile: Option<codegg_core::approval::SandboxProfile>,
    pub sandbox_profile: Option<codegg_core::approval::SandboxProfile>,
}
```

`run_id`/`parent_run_id` carry typed durable run identity;
`workspace_locks` is shared with the owning turn when the child runs in the same
workspace; `parent_sandbox_profile` is the parent-turn ceiling the child is
narrowed against and can never broaden, and a `FullHost` request under a
constrained parent fails closed.

### SubAgentReport (`src/agent/worker.rs:31`)

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
and disabled tools. It has 13 fields; see
`src/agent/policy.rs:12` for the full listing.

### ToolExposureMode (`src/agent/policy.rs:5`)

```rust
pub enum ToolExposureMode {
    Full,                  // All tools (default for unknown models)
    Curated,               // Frontier/reviewer: core + specialized
    MinimalWithDiscovery,  // Fast/fragile/local: core + tool_search
}
```

Coding-oriented profiles preserve a complete native loop: inspect/search,
edit/create, controlled command execution, supervised test verification, and
context-artifact recovery when registered. Goal and WorkPlan tools are
promoted from the policy-allowed deferred universe only when their host-bound
session state is active; WorkOrder remains distinct from live task delegation.

### Capability & AgentCapabilitySet (`src/agent/tool_surface.rs:15`)

15 capability kinds: `FilesystemRead`, `FilesystemWrite`, `ShellReadonly`,
`ShellMutating`, `GitRead`, `GitWrite`, `NetworkResearch`, `Delegate`,
`ManageTodos`, `ManageGoals`, `ManageWorkOrders`, `Terminal`, `Image`,
`MemoryRead`, `PluginExecute`. The capability set is
monotonic and supports intersection for parent ceiling enforcement.

### AgentRegistry (`src/agent/registry.rs:377`)

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

### Convergence coordinator (`src/agent/convergence.rs`)

The convergence vertical slice is application-owned and composes the durable
`ConvergenceStore` with `TaskTool`, `AgentRunStore`, and the existing event bus.
`task` actions `converge`, `convergence_status`, `convergence_decide`, and
`convergence_cancel` create one producer, wait for its authoritative terminal
result, then submit one fresh `verifier` run. The coordinator advances only
after durable state is persisted; terminal notifications are wake-ups and
reconciliation remains safe to repeat.

The verifier receives a bounded `VerifierEvidencePacket` assembled from
`AgentRunResult`, never producer transcript or hidden reasoning. Its host-side
denied-tool ceiling is intersected into the child request, so project agent
overrides cannot grant mutation, shell, delegation, permission, or goal
completion authority. A marked `Pass | Revise | Inconclusive` result is persisted
as semantic evidence only. The exact turn/run owner must explicitly choose
`accept`, `stop`, `escalate`, `repair`, or `replan`. Repair requires a terminal
successful clean result with complete same-repository worktree provenance and
seeds a new scheduler-owned run/worktree from its immutable result commit.
Replan uses the recorded original base or an owner-selected validated
last-clean result base. Both decisions advance one bounded cycle, remain
restart-safe through idempotency, and receive a fresh verifier. Accepting does
not merge Git or complete a goal.

M003 convergence defaults to two cycles, with a hard maximum of four and a
producer width ceiling of three (the current strategy is deliberately the
single-producer strategy). Creation-time wall-clock checks and repeated
result/verdict fingerprints exhaust rather than permit retry loops. Unknown
models remain `SoloPreferred`; `auto_convergence` is false by default and only
a configured `ConvergenceCapable` root model receives optional guidance. The
projection exposes remaining budget, selected terminal result, and finding
count; the result remains an explicit integration handoff and host goal
verification remains authoritative.

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

## Source Verification

Verified 2026-10-06 against source. Corrected: `AgentLoop` field count
32 → 34 (all 34 names now listed, `src/agent/loop.rs:86`);
`SubAgentRequest` listing 11 → 16 fields (`src/agent/worker.rs:82`);
`Capability` 12 → 15 kinds (`src/agent/tool_surface.rs:15`); stale refs
`tool_surface.rs:14` → `:15`, `policy.rs:4` → `:5`,
`registry.rs:374` → `:377`, `ToolExposureMode`. Verified accurate:
`Agent` (18 fields, `definition.rs:78`), `AgentRuntimeKind` (6 variants,
`:30`), `AgentLoopState` (10 fields, `loop.rs:56`), `ExecutionLimits`
(3 fields, `loop.rs:70`), `SubAgentReport` (6 fields, `worker.rs:31`),
`ModelRouter` (`router.rs:22`), `ExecutionPolicy` (13 fields,
`policy.rs:12`), `EMERGENCY_DEFAULT_MODEL` (`:379`), `MODEL_ALIAS_*`
(`:374`), referenced test files.

Second pass (2026-10-06) against all of `src/agent/*.rs`,
`src/tool/task.rs`, `src/agent/builtins/generated.rs`, `assets/agents/*.toml`,
and the three referenced guard scripts:
- **`task` tool action set was stale.** It listed six ops; the schema enum at
  `src/tool/task.rs:799` has sixteen, and the four `convergence_*` plus four
  `*_group` ops were undocumented at the durable-run-control section even
  though the bounded-run-groups section described them. Rewrote the list from
  the enum and added the `wait_group` 30s clamp (`task.rs:1101`).
- "Where It Lives" listed `src/agent/loop.rs` twice (two rows); merged into
  one and annotated the verified `Agent` / `AgentRuntimeKind` shapes.
- Verified accurate: every `src/agent/*` path in the inventory table;
  `AgentLoop::run`/`run_inner` (`loop.rs:766`/`:776`);
  `TurnRunInput`/`DefaultTurnRuntime` (`turn_runtime.rs:84`/`:193`) and
  per-turn construction (`:725`); `AgentLoopServices`/`TurnLifecycle`
  (`coordinator.rs:44`/`:122`) with 6 `TurnPhase` variants (`:110`);
  `ResolvedToolSurface` (`tool_surface.rs:147`), `PromptCompiler`
  (`prompt.rs:338`), `ProviderTurnAdapter` (`provider_turn.rs:42`),
  `EventProcessor` (`processor.rs:5`), `SemanticRouter`
  (`semantic_router.rs:92`), `ModelRouter` (`router.rs:22`);
  `MAX_ATTEMPTS` 3 and the 1s/2s/4s / 30s-cap backoff
  (`provider_turn.rs:64`/`:66`/`:532-534`); `ExecutionLimits` defaults 100 /
  1,000,000 / 600s (`loop.rs:79-81`); 5-layer agent resolution and the
  `~/.config/codegg/agents` + `.codegg/agents` paths
  (`definition.rs:484-516`); `ExecutionLimits` header at `:70`; the
  compatibility re-export at `src/agent/compaction.rs:6`;
  `compact_if_needed` (`context_runtime.rs:827`); `ContextFrame`
  (`context_frame.rs:131`); `SubAgentPool`/`SubAgentSpawner`
  (`worker.rs:284`/`:752`) and the bounded `RunControlService::wait`
  (`run_control.rs:318`); `ProjectAssetSnapshot`/`AssetContext`/
  `AssetRefreshCoordinator` (`asset_snapshot.rs:34`, `asset_context.rs:82`,
  `asset_refresh.rs:130`); `MAX_GROUP_MEMBERS` 16 and
  `MAX_GROUP_WAIT_MS` 30,000 (`crates/codegg-core/src/agent_run_group.rs:20-22`);
  the four `TaskPolicy` wire names; `TaskStatePolicy` variants
  `SoloPreferred`/`ConvergenceCapable` (`schema.rs:105`/`:107`);
  `apply_semantic_routing` (`request_preparation.rs:691`) and its
  single call site (`loop.rs:846`); the absence of `src/agent/team.rs`;
  `Arc<ExecutionContext>` on `AgentLoopBuildInput`
  (`agent_loop_factory.rs:27`); the 10 built-in agents in `generated.rs`;
  and all three guard scripts, including that
  `check_daemon_cwd_usage.py` guards `core/**` plus `agent/turn_runtime.rs`
  and `agent/worker.rs` specifically.
