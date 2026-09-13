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
| Built-ins | `assets/agents/*.toml` + `assets/prompts/` → `src/agent/builtins/generated.rs` | 10 compiled agents; never edit `generated.rs` directly |

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
   `src/agent/team.rs` inbox is removed; `task` tool ops (`spawn`,
   `status`/`get`, `message`, `interrupt`, `wait`, `cancel`,
   `spawn_many`) address typed durable run IDs with bounded payloads.

## Static Guards

```bash
python3 scripts/generate_builtin_agents.py --check  # agent asset staleness + schema
python3 scripts/check_daemon_cwd_usage.py           # no current_dir in daemon turn code
python3 scripts/check_scheduler_bypass.py           # heavy work via JobSubmissionService
```

## Testing

```bash
cargo test -p codegg agent::
cargo test --test agent_convergence
cargo test --test agent_loop_harness
cargo test --test tool_program_scenarios  # programmatic tool path through the loop
```

## See Also

- `architecture/agent.md` — authoritative contract
- `.skills/context/SKILL.md` — packer observation, tool-palette policy, volatile-tail
- `.skills/jobs/SKILL.md` + `.skills/scheduler/SKILL.md` — durable runs and admission
- `.skills/core/SKILL.md` — turn submission transport
