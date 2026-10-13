# Agent Tool Runtime Post-Closure Corrective C002 — Closure Record

Status: closed

Plan: `plans/implementation/agent-tool-runtime-reconciliation-post-closure-corrective/002-descendant-effect-and-nested-task-corrective.md`

Implementation commits:

- `d213150c10875b5dfe3531066f664e27a86588ba` — descendant effect enforcement, session/workspace binding, nested task registration and permission integration, docs and static guard.
- `7ced77b075274f9f2937b3169c43253d99cd20dd` — broker rejection for sibling-session snapshot reuse.
- C001 predecessor: `8d13bfd6` implementation and `d458c4d8` accepted closure.

The C002 implementation head before this closure record was `7ced77b075274f9f2937b3169c43253d99cd20dd` on `planning/agent-tool-runtime-reconciliation`.

## 1. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Authorized child exposes a functional nested `task` before surface resolution | Worker registers a functional `TaskTool` with the validated parent snapshot before resolving the surface, then replaces it with the child-narrowed snapshot before constructing the loop. `worker_child_advertises_and_executes_authorized_nested_task` asserts `task` and `tool_search` are advertised. | Pass |
| Actual root→child→grandchild trajectory | The deterministic fake-provider test invokes `task`, checks the successful task result, observes the grandchild's provider request with the nested prompt, and waits for the shared task store to report the grandchild `Completed`. | Pass |
| Role permission is enforced at invocation | Child workers now seed their permission checker from `agent.permission_ruleset()` plus the requested workspace path envelope. The positive trajectory requires the child role's explicit `task: allow`; the denied nested-surface test still omits `task`. | Pass |
| Every child call carries inherited authority and broker enforces it | Non-task child calls forward the worker-bound snapshot. `ToolBroker::validate_pre_execution` checks session identity, integrity, expiry, exact workspace/name, and current contract effect before execution, in addition to the grant and manifest checks. | Pass |
| Exact per-tool effects reject contract drift without blocking an authorized ProcessExec ceiling | Minting reads each eligible name's current `ToolBroker` contract. A broker test rejects ReadOnly→ProcessExec drift before execution even with a ProcessExec grant; a paired broker fixture admits ProcessExec when the inherited per-name ceiling also permits it. Sandbox and command permission paths remain independent. | Pass |
| Child session, workspace, scope and legacy payloads fail closed | Worker binds the narrowed snapshot to the exact child session and workspace after parent containment or durable lineage checks. Wrong session/workspace, forged fingerprint, expiry and legacy payloads lacking the effect envelope are denied. Scheduler `SubagentRun` JSON round-trips; absent legacy authority remains `None`. | Pass |
| No authority is accepted from model arguments; checksum is not misrepresented | Model tool schema is unchanged. Docs state that SHA-256 is an unkeyed integrity checksum and describe host-owned AgentLoop/TaskTool or scheduler carriage as the trust boundary. | Pass |

## 2. Verification executed

All Rust commands used the installed Rust 1.89 arm64 toolchain and matching `RUSTC` on this macOS arm64 host.

- `cargo fmt --all -- --check` — passed through `scripts/verify.sh quick`.
- `cargo test -p codegg --test agent_tool_m003_descendant` — 21 passed.
- `cargo test -p codegg --test subagent` — 22 passed.
- `cargo test -p codegg --test tool_broker_integration` — 25 passed.
- `cargo test -p codegg --lib agent::parent_authority` — 10 passed.
- `cargo test -p codegg --lib agent::tool_surface` — 5 passed.
- `cargo test -p codegg --lib tool::broker::tests::broker_` — 7 passed.
- `cargo test -p codegg --test agent_loop_harness` — 54 passed.
- `python3 scripts/check_descendant_tool_ceiling.py` — passed.
- `python3 scripts/check_mcp_broker_adapter.py` — passed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — passed.
- `bash scripts/verify.sh quick` — passed, including workspace/all-target check.
- `git diff --check` — passed.

## 3. Compatibility and trust-boundary review

The model-visible `task` schema and tool names did not change. No storage migration or new grant store was added. The additive scheduler authority payload remains JSON; the new effect map and child-session field deserialize with fail-closed defaults, and integrity validation rejects an old checksum that does not cover the new fields. A payload without authority remains a distinct system/legacy case and does not become model-driven root authority.

The worker checks parent session identity and either workspace containment or durable run lineage before binding the child workspace. Broker checks then compare each call to the bound child session/workspace. The snapshot SHA-256 detects accidental mutation but is not keyed authentication. The assurance is that model arguments cannot populate the carrier and production values come from host AgentLoop/TaskTool or daemon scheduler state. A same-user actor able to rewrite daemon-owned SQLite state is outside this local daemon trust boundary; this change does not claim cryptographic protection against that actor or cross-process signature security.

`ProcessExec` stays governed by the existing grant, permission, approval and sandbox paths. The new effect map preserves an inherited process contract only when the parent surface's broker contract grants it; it does not widen sandbox containment.

## 4. Documentation and operational updates

Updated `architecture/agent-tool-surface.md`, `architecture/tool_broker.md`, `architecture/tool.md`, `architecture/mcp.md`, `architecture/permission.md`, `.opencode/skills/agent/SKILL.md`, and `.opencode/skills/tool-execution/SKILL.md`. Strengthened `scripts/check_descendant_tool_ceiling.py` to check broker enforcement, per-name effect pinning, child identity binding, permission-rule application, and functional task registration order.

## 5. Limitations

- C002's fake-provider trajectory qualifies local child/grandchild execution and typed task completion; hosted CI, desktop E2E, and current-main integration belong to C003 and have not been claimed here.
- The ProcessExec ceiling has broker-level positive/negative coverage and existing AgentLoop sandbox/approval regressions; this record does not claim a new live shell process run under every sandbox profile.
- The scheduler payload test covers typed serialization/reload and fail-closed validation, not killing and restarting a daemon between parent and child execution.

No unresolved C002 finding blocks the invariant repair. C003 owns the remaining exact-head and hosted qualification work.

## 6. Roadmap and registry disposition

C002 is closed at commits `d213150c` and `7ced77b0`. C003's hard dependencies C001 and C002 are both accepted, so C003 is now active and may begin sequentially. Registry review found no other registered plan newly unblocked by C002. Historical M001–M004 closure records remain unchanged.
