# Post-Audit Maintainability and Surface Milestone 003 — Closure Status

Status: closed

Source implementation plan:

- `plans/implementation/post-audit-maintainability-surface/003-agent-runtime-physical-decomposition.md`

Source subsystem roadmap:

- `plans/subsystems/post-audit-maintainability-surface-roadmap.md#7-milestones`

Repository baseline reviewed: `bd398a23` (pre-change HEAD; M001/M002 closed)

Implementation commits:

- `467c288e` — feat(agent): decompose agent-runtime monoliths into responsibility modules (maintainability M003)

## 1. Executive finding

M003 is complete. `src/agent/loop.rs` fell from 4965 to 1877 lines
(-62%) and `src/agent/mod.rs` from 3802 to 73 lines: the root is now a
declaration/re-export/composition surface only. Nine narrowly named
modules carry the moved production code with their focused tests, and
`compact_if_needed` plus the pack-observation phase joined the existing
canonical owner (`context_runtime.rs`), following the already established
`impl AgentLoop`-across-files precedent (`tool_batch.rs`,
`context_runtime.rs`). No new coordinator, state machine, provider path,
tool executor, scheduler, trait hierarchy, or workflow abstraction was
introduced. `AgentLoop` remains the canonical per-turn orchestrator with
`run`/`run_inner` sequencing intact; daemon/scheduler/broker authority,
turn-asset immutability, cancellation, compaction, delegation, goal,
persistence, and projection semantics are unchanged. M005's interface
dependency on M003 is now satisfied (its M002 hard dependency was already
closed), so M005 is unblocked to ready in this same closure.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Responsibility map before movement, no duplicate owners | Section 3 map; `compact_if_needed` extended `context_runtime` instead of a new module; helpers grouped by existing tool/context boundaries | pass |
| `loop.rs`/`mod.rs` materially less concentrated with coherent owners | loop.rs 4965→1877; mod.rs 3802→73; 9 new modules each with one describable responsibility (section 3) | pass |
| `AgentLoop` sequencing remains recognizable and canonical | `new`, setters, `run`, `run_inner`, `run_with_prompt` stay in `loop.rs`; struct fields untouched | pass |
| No new coordinator/state machine/executor/abstraction | No new trait; no new service struct; `AgentLoopServices`/`TurnLifecycle` untouched; moves are `impl AgentLoop` relocations + free-function moves | pass |
| Cancellation/compaction/delegation/goal/persistence/projection tests green | agent:: 316, agent_loop_harness 40, compaction 65, permission 32, tool_registry 54, tool_structured_execution 12, tool_execution 9, tool_surface_minimization 13, tool:: 534 | pass |
| Dependency direction no worse, no pass-through webs | New modules import only leaf helpers (`tool_inspect`, `loop_output`, `context_runtime` types, `coordinator::TurnPhase`); no sibling lifecycle imports; no external importers of new modules | pass |
| Public compatibility preserved | `crate::agent::{Agent, …}`, `crate::agent::r#loop::AgentLoop`, `AgentLoopTerminalOutput`, `resolve_agents*`, file-loader paths all resolve; only change is the `pub(crate)` `parse_mode` re-export (sole consumer `registry.rs` imports from `definition` directly) | pass |
| Docs reflect final ownership; no size gate added | `architecture/agent.md` module table rewritten; no CI gate added | pass |

## 3. Production implementation evidence

### 3.1 Responsibility map (WP-A) and extraction units

| Cluster | Previous home | New owner (one sentence) |
|---|---|---|
| Tool-call inspection/classification, timeouts, model-flag gating, `ToolPermissionOutcome` | `loop.rs` free fns + `filter/compute` | `agent/tool_inspect.rs` — pure narrow-input helpers, no execution authority |
| Terminal output collector + local-path redaction | `loop.rs` top | `agent/loop_output.rs` — bounded public-text collection and redaction |
| Request preparation (policy, auto-routing, research hint, context frame, todos, tool definitions) | `impl AgentLoop` | `agent/request_preparation.rs` — per-turn model-request assembly |
| Completion/goal/limits/run-boundary | `impl AgentLoop` | `agent/turn_completion.rs` — terminal publication and budget accounting |
| Habit observation adapter | `impl AgentLoop` | `agent/habit_observation.rs` — allowlisted structural metadata only |
| Snapshot capture + security-review trigger | `impl AgentLoop` | `agent/snapshot_capture.rs` — snapshot draining and scheduler-dispatched review |
| Notification injection + follow-up drain | `impl AgentLoop` | `agent/follow_up.rs` — safe-boundary queues |
| Turn-lifecycle compaction | `impl AgentLoop` | `agent/context_runtime.rs` (existing owner extended, not a new module) |
| Agent definition/resolution/profile | `mod.rs` 50–734 | `agent/definition.rs` — nominal types, safety envelope, config merging |
| File-based loading (md/TOML, overlay, specs) | `mod.rs` 736–1371 | `agent/file_agents.rs` — agent-file parsing and lookup |
| Orchestrator remainder | — | `agent/loop.rs` — struct, `new`/setters, `run`/`run_inner`, `run_with_prompt` |
| Composition surface | — | `agent/mod.rs` — module declarations, `pub use` re-exports, legacy `resolve_agents()` cwd boundary |

Deliberately retained in the orchestrator: `run_inner` (~1000 lines of
high-level turn sequencing — sequencing, not a reusable responsibility),
the constructor/setter surface, and the `destructive_*` tests (they test
`crate::tool::destructive`, so moving them into `tool/` would cross the
milestone boundary). `resolve_agents()` stays in `mod.rs` because the
`check_project_agent_pwd_inference` guard explicitly owns that cwd-reading
boundary at that path.

### 3.2 Before/after sizes (descriptive only)

`loop.rs` 4965→1877, `mod.rs` 3802→73, `context_runtime.rs` 591→1332.
New: `definition.rs` 2274 ( Agent core + 73 tests), `file_agents.rs`
1518 (32 tests), `request_preparation.rs` 748, `tool_inspect.rs` 540,
`follow_up.rs` 360, `turn_completion.rs` 315, `snapshot_capture.rs` 233,
`habit_observation.rs` 152, `loop_output.rs` 144.

### 3.3 No-bypass evidence

- `rg` for new `impl Provider`, `ToolBroker`, `JobSubmissionService`,
  or `Command::new` in the new modules: only the moved
  scheduler-submission call in `snapshot_capture.rs` (pre-existing path).
- `check_tool_broker_boundary.py` green; tool execution still enters only
  via `ToolBatchExecutor`/broker.
- `merge_agent_config`/`agent_from_config` widened `fn`→`pub(super)`
  (same `agent` tree only); moved `impl AgentLoop` methods widened
  private→`pub(super)` for cross-file sequencing calls. No `pub` widening
  except none. No `codegg-core` move.

## 4. Verification executed (commands + results; local unless noted)

- `cargo check -p codegg --all-targets` — clean, zero warnings.
- `cargo test -p codegg --lib agent::` — 316 passed, 0 failed (moved
  definition/file_agents/inspect/output/prep/completion/context tests).
- `cargo test -p codegg --lib tool::` — 534 passed (M002 count preserved).
- `cargo test -p codegg --lib permission::` — 32 passed.
- `cargo test --test agent_loop_harness` — 40 passed (turn/tool/delegation paths).
- `cargo test --test compaction` — 65 passed (context lifecycle).
- `cargo test --test tool_registry/tool_structured_execution/tool_execution` — 54/12/9 passed.
- `cargo test --test tool_surface_minimization` — 13 passed (M002 disclosure intact).
- `cargo fmt --all -- --check` — clean (via `cargo fmt --all`).
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — clean.
- `scripts/verify.sh quick` — passed (generated-agents check, core
  boundary, sandbox contract, execution ownership, locked workspace check).
- Static guards: `check_execution_ownership.py`,
  `bash scripts/check-core-boundary.sh`, `check_scheduler_bypass.py`,
  `check_daemon_cwd_usage.py`, `check_project_agent_pwd_inference.py`,
  `check_tool_broker_boundary.py` — all passed.
- No hosted `CI / verify` run: behavior-preserving source reorganization
  with no daemon, scheduler, protocol, or release change; local quick +
  all-feature Clippy is the proportionate posture per the roadmap (same
  basis as M001/M002).

## 5. Invariant review

- `AgentLoop` canonical; `AgentLoopServices`/`TurnLifecycle` untouched.
- Scheduler admission authoritative: `maybe_spawn_security_review` moved
  verbatim (scheduler `submit` + standalone-compat `spawner().send` with
  its `scheduler-audit: standalone-compat` annotation intact).
- Broker/contract/policy governed: `tool_batch.rs` imports resolve through
  `loop.rs` re-exports; no direct execution path added.
- Turn asset pin, compaction ownership, child-authority ceiling,
  cancellation propagation, worktree/run/session attribution, projection
  ordering, model adaptation/tool surface: all untouched code paths, only
  relocated.
- No public API renamed; `pub(crate) parse_mode` path consolidated to
  `definition` with its sole consumer updated.

## 6. Failure and recovery review

No new tasks, locks, stores, or recovery paths. The two `tokio::spawn`
sites in moved code (`maybe_spawn_security_review`,
`compact_if_needed` hook emission) moved verbatim with the same
lifetimes; no detached task was introduced to solve borrow placement
(all moved methods take `&self`/`&mut self` on the existing loop).
`maybe_continue_goal` still drains through the same bounded
`drain_follow_up`; goal-accounting deltas still reset only on success.
Lock ordering unchanged (no lock acquisition moved across `.await`).

## 7. Migration and compatibility review

No user migration. Internal paths added (`agent::definition::`,
`agent::file_agents::`, etc.); all documented `crate::agent::` paths
preserved via re-export. `AgentRegistry` import updated to the new
canonical paths. Durable names (tools, runs, sessions) untouched. The
`execution-ownership.toml` manifest gained one additive
`snapshot_capture.rs` site entry (guard-verified); no existing entry
changed semantics.

## 8. Security review

- Permission/safety-envelope code moved verbatim (`definition.rs`);
  `permission::` negative tests green (32 lib + harness).
- Security-review dispatch preserves scheduler-first/standalone-fallback
  order and authority intersection (no widening: `pub(super)` only).
- Session-import redaction, eggsentry classification, destructive-command
  policy untouched.
- Auth logging, credential handling, sandbox untouched.

## 9. Documentation and operations

- `architecture/agent.md`: module table rewritten for the ten
  responsibility owners; ownership-prose sections (lifecycle,
  coordinator, checkpoints, run control) unchanged and still accurate.
- `docs/execution-ownership.toml`: additive `snapshot_capture.rs` site.
- `AGENTS.md`: no edit — it does not name the old giant-file locations
  as source layout (only the TUI note, unaffected).
- No user README/operator change: no behavior or public-path change.

## 10. Unresolved findings

No critical/high/medium/low findings requiring a corrective pass. Known
remaining concentration (intentional, recorded per plan §15):

1. `definition.rs` (2274 lines, ~half tests) still couples the `Agent`
   nominal type with config-layer resolution. It stays because both halves
   share one reason to change (the agent definition contract); splitting
   type from resolution would create a pass-through pair.
2. `run_inner` remains ~1000 lines of high-level sequencing in `loop.rs`.
   It stays because it is orchestration, not a reusable responsibility.
3. `destructive_*` tests remain in `loop.rs` tests although they cover
   `crate::tool::destructive`; relocating them into `tool/` is M004-adjacent
   tidy-up, not agent-runtime ownership, and was left to avoid scope bleed.

## 11. Roadmap disposition

M003 meets all exit conditions in
`plans/subsystems/post-audit-maintainability-surface-roadmap.md#7-milestones`:
dominant orchestration responsibilities are in named modules with focused
tests; `AgentLoop` turn lifecycle reads without unrelated helper
families; no duplicate state machine or coordinator introduced; agent
runtime, cancellation, compaction, tool, delegation, and projection tests
green.

Dependency audit (per planning-skill unblock check): M005
(`005-runtime-service-context-global-state-cleanup.md`) lists a hard
dependency on M002 (closed) and an interface dependency on M003's final
agent/tool construction seams. Both are now satisfied: M002's disclosure
module is untouched by this refactor, and M003's final seams are
`ToolRegistry::with_options` (unchanged), `AgentLoop::new` + setters
(unchanged signatures), `definition`/`file_agents` public paths
(re-exported), and `tool_inspect` pure helpers (additive). No new
follow-up or corrective plan is required. M004 is independent and
unaffected.

Recommendation: closed; M005 moves from blocked to ready.

## 12. Registry updates

- `plans/registry.md`: M003 moved to closed with this closure record;
  subsystem row updated to `M001 closed, M002 closed, M003 closed`;
  M005 blocked-work entry removed and M005 registered as ready
  (both dependencies satisfied); dependency-ready table gains M005 and
  retains M004.
- `plans/subsystems/post-audit-maintainability-surface-roadmap.md`:
  M003 → closed with closure link; M005 → ready (unblocked by M002+M003).
- `plans/implementation/post-audit-maintainability-surface/003-agent-runtime-physical-decomposition.md`:
  status → closed.
- `plans/implementation/post-audit-maintainability-surface/005-runtime-service-context-global-state-cleanup.md`:
  status → ready for handoff (dependency audit: hard dep M002 closed,
  interface dep M003 closed; no other outstanding dependency).
