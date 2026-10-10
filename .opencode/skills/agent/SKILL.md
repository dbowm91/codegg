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
| Assets | `src/agent/asset_snapshot*.rs`, `asset_context.rs`, `asset_refresh.rs`, `instructions.rs`, `definition.rs`, `registry.rs`, `file_agents.rs` | Immutable `ProjectAssetSnapshot`, explicit `AssetContext`, single-flight `AssetRefreshCoordinator`, instruction fragments, agent resolution |
| Built-ins | `assets/agents/*.toml` + `assets/prompts/` → `src/agent/builtins/generated.rs` | Generated built-in agents; never edit `generated.rs` directly. Derive the current set from the assets/generator rather than pinning a count here. |

## Hard Rules

1. **`ToolBroker` is the only production tool-call boundary.** Never call
   executors directly from the loop; heavy work goes
   `JobSubmissionService` → `JobScheduler`.
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
7. **Run control is the only agent coordination authority.** The legacy
   `src/agent/team.rs` inbox is removed; `task` tool ops address typed
   durable run IDs with bounded payloads. Derive the current action set
   from the tool schema in `src/tool/task.rs` rather than pinning it here.

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

Second pass (2026-10-06): re-verified the same surface and added the
`## Orientation Facts` section (per-turn `AgentLoop` lifetime, 6
`TurnPhase` variants, `MAX_ATTEMPTS`/backoff bounds, group caps), each
anchored to source. Narrowed the `check_daemon_cwd_usage.py` guard
description to the actual `PROTECTED_GLOBS` list. The sync-registry claim
was re-checked against `crates/codegg-core/src/bus/mod.rs:104-305`, where
`register`/`respond`/`answer_question` are plain `fn` (not `async fn`). No stale claims
were found in this file's existing content.
