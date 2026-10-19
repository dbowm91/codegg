---
name: agent
description: Agent loop orchestration, tool dispatch, compaction, routing, delegated runs in codegg
version: 1.0.0
tags:
  - agent
  - loop
  - orchestration
  - runs
---

# Agent Loop Guide

Operational guide for changing `src/agent/`. The full contract lives in
`architecture/agent.md`; this skill covers the ownership boundaries and
invariants that are easy to violate.

## Ownership Map

| Layer | Location | Role |
|-------|----------|------|
| Turn entry | `src/agent/loop.rs`, `turn_runtime.rs`, `coordinator.rs` | `AgentLoop::run`/`run_inner`, `DefaultTurnRuntime.run_turn(TurnRunInput)`, `AgentLoopServices` + typed `TurnLifecycle` phases |
| Request prep | `src/agent/request_preparation.rs`, `tool_surface.rs`, `prompt.rs` | Per-turn policy, `ResolvedToolSurface` (immutable tool authority), `PromptCompiler` (sole system-prompt assembly) |
| Provider turn | `src/agent/provider_turn.rs`, `processor.rs`, `semantic_router.rs`, `router.rs` | `ProviderTurnAdapter` retry/stream normalization, `EventProcessor`, bounded `virtual:<name>` selector, `ModelRouter` |
| Tool dispatch | `src/agent/tool_batch.rs`, `tool_inspect.rs` | Typed permission/MCP/broker batch boundary; pure tool-call inspection (paths, bash/test/git/MCP, timeouts, model-flag gating) |
| Compaction | `src/context/compaction.rs` (owner), `src/agent/compaction.rs` (compat re-export), `context_runtime.rs`, `context_frame.rs` | Canonical `ContextTracker`/budget engine; turn-lifecycle `compact_if_needed`; post-compaction `ContextFrame` snapshot |
| Delegated runs | `src/agent/worker.rs`, `run_control.rs`, `run_integration.rs`, `convergence.rs` | `SubAgentPool`/`SubAgentSpawner`, run control (owner/ancestor lineage, `wait` bounded long-poll), convergence tracking |
| Assets | `src/agent/asset_snapshot*.rs`, `asset_context.rs`, `asset_refresh.rs`, `instructions.rs`, `bootstrap.rs`, `definition.rs`, `registry.rs`, `file_agents.rs` | Immutable `ProjectAssetSnapshot`, explicit `AssetContext`, single-flight `AssetRefreshCoordinator`, bounded repository-init drafts, instruction fragments, agent resolution |
| Built-ins | `assets/agents/*.toml` + `assets/prompts/` → `src/agent/builtins/generated.rs` | Generated built-in agents; never edit `generated.rs` directly. Derive the current set from the assets/generator rather than pinning a count here. |

## Hard Rules

Repository initialization drafts are deterministic, read-only project evidence.
The `/init` TUI owns preview and explicit approval, while the daemon owns the
fixed-path AGENTS.md publication and subsequent project-scoped refresh. Never
make the analyzer write files, run discovered commands, or read global personal
instructions into the candidate. See `architecture/agent.md`.

1. **`ToolBroker` is the only production tool-call boundary.** Never call
   executors directly from the loop; heavy work goes
   `JobSubmissionService` → `JobScheduler`.
   A top-level model session gets its trusted scratch prompt block from the
   canonical `SessionId`; tool/process authorization is resolved from broker
   context and never inferred from prompt text or a supplied path.
   Durable delegated runs use a private `AgentRunId` root. Shared roots are
   granted only by their direct owner through an opaque `ScratchId` stored in
   the accepted run snapshot; the worker revalidates grants before exposing
   `CODEGG_SHARED_SCRATCH_<ALIAS>`. Keep standard temporary variables private.
   A structured write rejected under a known host temp root can return an
   advisory path under private scratch; preserve the original path and all
   explicit permission decisions. Reads and shell commands are not redirected.
2. **Never edit `src/agent/builtins/generated.rs`.** Edit
   `assets/agents/*.toml` or `assets/prompts/` then run
   `python3 scripts/generate_builtin_agents.py` (`--check` in CI).
3. **Compaction owner is `src/context/compaction.rs`.** `agent::compaction`
   is a compat re-export; `eggcontext` is the tokenizer primitive only.
4. **Semantic routing never migrates connections.** A `virtual:<name>` route
   resolves to a concrete model compatible with the already-selected
   per-turn provider connection. Concrete models bypass it. `sticky` /
   `affinity_ttl_s` are shared policy fields only; Codegg keeps no local
   affinity cache. Selector failure uses the compatible default route;
   caller cancellation propagates.
5. **Turns carry immutable `Arc<ExecutionContext>`.** Never use
   `std::env::current_dir()` in workspace-bound turn code; thread the
   context (guard: `scripts/check_daemon_cwd_usage.py`).
6. **`PermissionRegistry`/`QuestionRegistry` are sync.** Register the
   responder BEFORE publishing the Pending event.
7. **Run control remains the only sibling control authority.** The legacy
   `src/agent/team.rs` inbox is removed; the `task` tool's board/claim/release/
   handoff-request actions expose bounded same-group coordination facts and
   advisory intent only. They cannot message, cancel, interrupt, or reassign
   another run. Derive the action set from `src/tool/task.rs`.
   Scheduled child runs inherit the approval mode captured from the authorized
   parent TaskTool invocation; historical payloads default to Interactive.
   This only controls Ask resolution. Child agent tool/path rules and the
   narrowed sandbox remain independent limits.
8. **Peer hints stay untrusted and push-only.** Deliver only allowlisted,
   bounded host observations at provider boundaries; recheck after provider
   completion and refuse a returned tool batch when new peer facts or a gap
   must be reviewed first. Never promote peer text or raw tool output to
   system/developer instructions, and never wake an idle session. On a gap,
   direct the model to re-read the canonical board. This does not serialize
   filesystem writers.
   `spawn_many` reserves and persists its group before children enter the
   model; children must observe their exact durable membership before their
   first provider call. Parent `task get` and authorized `status`/`wait`
   return the bounded stored Git result, including commit and changed-path
   evidence.
9. **Every progress-recovery incident keys on the whole `action_key()`**, and
   ladder state belongs to one incident (`src/agent/progress_recovery.rs`).
   Two tool-name-only arms got this wrong and each killed ordinary turns:
   `ShortCycle` fired on the same tool with *different* arguments, and
   `EquivalentError` fired on the same tool with the same error class — so a
   model bisecting a build failure stalled for work it never repeated. A
   successful read-only call the turn has never made is `NewEvidence`; a
   successful mutating or process-class call is **not** progress on its own,
   because `state_changed` needs a drained file-change bus event. A
   `ProgressSignal` variant with no producer is a claim, not a hook.
10. **Bound comparative delegated work explicitly.** `task.spawn` and each
    `task.spawn_many` request accept an optional host-enforced
    `max_tool_calls` from 1 to 128. The limit is persisted with the durable
    run and applied by the child loop. Set equal limits across experiment
    arms; omission preserves the configured child default. Daemon-created child
  registries inherit the daemon `JobSubmissionService` for heavy tools; keep
  authenticated origin attribution separate from the broker grant principal.

## Orientation Facts

- **A turn is one `AgentLoop` instance.** `DefaultTurnRuntime::run_turn`
  builds a fresh loop per turn (`src/agent/turn_runtime.rs:725`), so
  per-loop state — `AgentLoopState`, `ContextCacheStats`, `context_ledger`,
  `ExecutionLimits` — is per-turn and never spans turns.
- **Turn phases are typed and in-memory only.** `TurnPhase`
  (`src/agent/coordinator.rs:110`) has six variants — `Admission`,
  `ContextPreparation`, `ProviderInvocation`, `ToolExecution`, `Recovery`,
  `Completion`. `TurnLifecycle` is a sequencing aid; durable state belongs to
  run-control/RunStore/scheduler.
- **Provider retry is bounded before visible output.** `MAX_ATTEMPTS = 3`
  (`provider_turn.rs:64`) under one UUID-scoped attempt chain; backoff is
  1s/2s/4s capped at 30s. After a `TextDelta`/`ReasoningDelta`/
  `ToolCallStarted` the turn aborts rather than replaying, so an abandoned
  tool-call start never executes.
- **Group ops are bounded.** `MAX_GROUP_MEMBERS = 16` and
  `MAX_GROUP_WAIT_MS = 30_000`
  (`crates/codegg-core/src/agent_run_group.rs:20-22`); `wait`/`wait_group`
  clamp `timeout_ms` to 30,000 and a timeout means *still running*.

## Static Guards

```bash
python3 scripts/generate_builtin_agents.py --check  # agent asset staleness + schema
python3 scripts/check_daemon_cwd_usage.py           # no current_dir in daemon turn code
python3 scripts/check_scheduler_bypass.py           # heavy work via JobSubmissionService
```

`check_daemon_cwd_usage.py` guards `src/core/**` plus the specific turn-code
files `src/agent/turn_runtime.rs` and `src/agent/worker.rs` (not all of
`src/agent/`), with tool `default()` constructors allowlisted.

## Testing

```bash
cargo test -p codegg agent::
cargo test --test agent_convergence
cargo test --test agent_loop_harness
cargo test --test tool_program_scenarios  # programmatic tool path through the loop
```

## See Also

Daemon turns derive delegated-task repository identity from the durable
session workspace binding. See `architecture/agent.md` for the boundary and
`src/core/daemon_turns.rs` for the source path.

Turn-owned group lifecycle operations are resumable by a later top-level turn
in the same durable session. The creator turn remains provenance; child runs
and other sessions cannot take over the group.

Scheduled subagents apply the approval mode serialized from the accepted parent
tool invocation. Never derive it from prompt or peer text; see
`architecture/permission.md` for the independent sandbox and tool authority
ceilings.

- `architecture/agent.md` — authoritative contract
- `.opencode/skills/context/SKILL.md` — packer observation, tool-palette policy, volatile-tail
- `.opencode/skills/jobs/SKILL.md` + `.opencode/skills/scheduler/SKILL.md` — durable runs and admission
- `.opencode/skills/core/SKILL.md` — turn submission transport

## Source verification

Verified 2026-10-06 against `src/agent/mod.rs`, `src/agent/loop.rs`,
`src/agent/turn_runtime.rs`, `src/agent/coordinator.rs`,
`src/agent/{request_preparation,tool_surface,prompt,provider_turn,processor,semantic_router,router}.rs`,
`src/agent/{tool_batch,tool_inspect}.rs`,
`src/agent/{compaction,context_runtime,context_frame}.rs`,
`src/agent/{worker,run_control,run_integration,convergence}.rs`,
`src/agent/{asset_snapshot,asset_snapshot_builder,asset_context,asset_refresh,instructions,definition,registry,file_agents}.rs`,
`src/context/compaction.rs`, `src/agent/task_tool_runtime.rs`,
`src/tool/task.rs`, `src/tool/broker.rs`, `assets/agents/*.toml`,
`assets/prompts/`, and `scripts/generate_builtin_agents.py`,
`scripts/check_daemon_cwd_usage.py`, `scripts/check_scheduler_bypass.py`.
Corrected the hard rule that pinned the `task` tool action set to seven
ops; the schema enum in `src/tool/task.rs:799` has sixteen
(`spawn`, `converge`, `convergence_status`, `convergence_decide`,
`convergence_cancel`, `spawn_many`, `create_group`, `status`, `get`,
`message`, `interrupt`, `wait`, `cancel`, `status_group`, `wait_group`,
`cancel_group`), so the list was replaced with a pointer to the source.
Confirmed accurate as written: every `src/agent/*` path in the ownership
map, `AgentLoop::run`/`run_inner`, `DefaultTurnRuntime::run_turn`,
`AgentLoopServices`/`TurnLifecycle`, `ResolvedToolSurface`,
`PromptCompiler`, `ProviderTurnAdapter`, `EventProcessor`,
`SemanticRouter`, `ModelRouter`, `ToolBatchExecutor`,
`ToolTimeoutConfig`, the compat re-export in `src/agent/compaction.rs`,
`compact_if_needed` in `src/agent/context_runtime.rs`, `ContextFrame`,
`SubAgentPool`/`SubAgentSpawner`, the bounded `RunControlService::wait`,
`ProjectAssetSnapshot`, `AssetContext`, `AssetRefreshCoordinator`, the
absence of `src/agent/team.rs`, the sync registry entry points, and all
three guard scripts plus all four `cargo test` targets. Claims without a
traceable source were removed rather than guessed.

For bounded comparative runs, durable children accept
`max_model_tokens`, a cumulative provider-reported input-plus-output ceiling.
`spawn_many` can instead take one `aggregate_model_tokens` ceiling; CodeGG
divides it across the requests and rejects mixed per-child caps. Sum each
child run's bounded `provider_usage` progress milestones for actual usage and
record coordinator-session usage separately. Use `cost_available` to
distinguish an estimated zero from an unknown price. The response that
crosses a cap is retained as actual usage and classified as over budget.

Second pass (2026-10-06): re-verified the same surface and added the
`## Orientation Facts` section (per-turn `AgentLoop` lifetime, 6
`TurnPhase` variants, `MAX_ATTEMPTS`/backoff bounds, group caps), each
anchored to source. Narrowed the `check_daemon_cwd_usage.py` guard
description to the actual `PROTECTED_GLOBS` list. The sync-registry claim
was re-checked against `crates/codegg-core/src/bus/mod.rs:104-305`, where
`register`/`respond`/`answer_question` are plain `fn` (not `async fn`). No stale claims
were found in this file's existing content.
