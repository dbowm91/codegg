# Tool Programs Milestone 002 — Closure Status

Status: closed
Source implementation plan: `plans/implementation/tool-programs/002-tool-contracts-and-canonical-broker.md`
Source subsystem roadmap: `plans/subsystems/tool-programs-roadmap.md#milestone-2--tool-contracts-and-canonical-broker`
Repository baseline reviewed: `2f715941516a1d49be578fdef56714ad3ddfe8bf`
Implementation commits: `HEAD` — Tool contracts and canonical broker (M002)

## 1. Executive finding

Structured tool contracts (`ToolContract`), typed caller policy
(`ToolCallerPolicy`), effect classification (`ToolEffectClass`), and
typed results (`ToolValue`) are implemented. The canonical
`ToolBroker` enforces a full 10-step policy pipeline for all
production tool calls. `AgentLoop` routes every tool call through the
broker. Legacy tools receive conservative defaults (DirectOnly,
NonIdempotent, no cache, no retry, string output). No user-visible
behavior changed.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence |
|---|---|
| Every production agent tool call enters one canonical broker | `AgentLoop::execute_tool_calls` routes through `tool_broker.execute()` via `broker_for_exec` Arc clone per call |
| Direct user-visible behavior and permission semantics remain compatible | No changes to tool names, input schemas, or permission flow; broker wraps existing `execute_structured` |
| Caller policy, input schema, authorization, cancellation, output schema, provenance, artifacts, and terminal status enforced in ordered pipeline | `ToolBroker::execute` runs 10-step pipeline: lookup → caller policy → input size → authority → timeout → route → execute → output validation → artifact registration → result normalization |
| Scheduler-owned operations cannot be executed directly by broker adapters | `ToolCallerPolicy::DirectOnly` prevents programmatic callers from invoking scheduler tools; unauthorized non-agent callers rejected |
| Legacy tools are conservative and cannot accidentally become program-callable | `ToolContract::legacy()` defaults to `DirectOnly`, `NonIdempotent`, no cache/retry; `Tool::contract()` default returns legacy contract |
| Static guards block new AgentLoop direct execution paths | `scripts/check_tool_broker_boundary.py` enforces no direct `execute_capture`/`execute_structured` calls outside broker |
| No unresolved high or medium finding remains | See §10 |

## 3. Production implementation evidence

### Files changed

- `src/tool/contract.rs` — New: `ToolContract`, `ToolCallerPolicy`, `ToolEffectClass`, `IdempotencyClass`, `ToolRetryPolicy`, `ToolCachePolicy`, `ToolProjectionPolicy`, `ToolCaller`, `ToolTerminalStatus`, `ToolValue`, `ToolArtifactHandle`, `ToolContractCatalog`, `ContractValidationError`; 13 unit tests
- `src/tool/broker.rs` — New: `ToolBroker`, `ToolBrokerConfig`, `BrokerInvocationContext`, `BrokerResult`, `BrokerError`; full 10-step pipeline with lookup, caller policy, input validation, authority delegation, timeout, execution, output validation, artifact registration, result normalization; 4 unit tests
- `src/tool/mod.rs` — Added `pub mod broker`, `pub mod contract`; added `tool_names()` method to `ToolRegistry`; added `fn contract()` to `Tool` trait with conservative default; re-exported broker and contract types
- `src/agent/loop.rs` — Added `tool_broker: Arc<ToolBroker>` field to `AgentLoop`; broker built from registry at construction; `tool_broker.execute()` replaces direct `execute_capture()` for all production tool calls
- `architecture/tool_broker.md` — New: architecture documentation
- `architecture/tool.md` — Updated: added Tool Contracts and Canonical Broker section
- `tests/tool_broker_integration.rs` — New: 25 integration tests covering full pipeline, caller policy, input validation, output truncation, artifact registration, error mapping, contention, concurrency, security, and migration compatibility
- `scripts/check_tool_broker_boundary.py` — New: static guard enforcing broker boundary

## 4. Verification executed

| Command | Outcome |
|---|---|
| `cargo test -p codegg --lib tool::broker` | 4 passed |
| `cargo test -p codegg --lib tool::contract` | 13 passed |
| `cargo test --test tool_broker_integration` | 25 passed |
| `cargo test -p codegg --lib tool` | 581 passed, 1 pre-existing failure (Python scheduler disabled in test env) |
| `cargo test -p codegg --test agent_loop_harness` | 40 passed |
| `cargo test -p codegg --test command_routing_execution_ownership` | 20 passed |
| `cargo fmt --all -- --check` | Clean |
| `cargo clippy -p codegg --lib -- -D warnings` | Clean (pre-existing codegg-core errors excluded) |
| `python3 scripts/check_tool_broker_boundary.py` | OK |

## 5. Invariant review

- **Broker is single execution boundary**: `AgentLoop` routes all tool calls through `tool_broker.execute()`. Direct `execute_capture` calls are blocked by `check_tool_broker_boundary.py`.
- **Legacy tools conservative**: `ToolContract::legacy()` defaults to DirectOnly, NonIdempotent, no cache/retry.
- **No authority broadening**: caller policy check prevents programmatic callers from invoking DirectOnly tools. Unauthorized non-agent/internal callers rejected by `caller_authorized` check.
- **Typed results**: `ToolValue` carries display, artifacts, provenance, and terminal status.
- **Backward compatible**: existing tool names, input schemas, and permission flow unchanged.
- **Output bounds enforced**: broker truncates outputs exceeding `max_output_bytes` and registers artifact handles for large outputs.
- **Concurrency safe**: concurrent calls produce unique invocation IDs; no unbounded task growth.

## 6. Failure and recovery review

- **Tool not found**: `BrokerError::NotFound` returned before any side effects.
- **No contract**: `BrokerError::NoContract` returned for tools without registered contracts.
- **Caller denied**: `BrokerError::CallerDenied` returned with tool name, caller, and policy.
- **Input too large**: `BrokerError::InputTooLarge` returned with size and limit.
- **Execution failure**: mapped to `ToolValue` with appropriate terminal status (Denied, TimedOut, InfrastructureError).

## 7. Migration and compatibility review

- `Tool::contract()` has a default implementation; no existing tool needs modification.
- `ToolContract::legacy()` provides safe defaults for all existing tools.
- `BrokerInvocationContext` can be built from `ToolExecutionContext` via `From` impl.
- `caller_authorized` flag allows AgentLoop to skip redundant permission checks (already done before broker call).
- No database changes required.
- No protocol changes required.

## 8. Security review

- Caller policy is enforced before any side effects.
- Input size bounds prevent payload amplification.
- No secrets or credentials in contract metadata, broker errors, or typed results.
- Legacy tools remain DirectOnly and cannot become program-callable without explicit override.
- Static guard (`check_tool_broker_boundary.py`) prevents regressions.

## 9. Documentation and operations

- `architecture/tool_broker.md` — New: architecture doc describing pipeline, types, and migration status.
- `architecture/tool.md` — Updated: added Tool Contracts and Canonical Broker section with pipeline summary.
- `ToolContract` types documented with doc comments.
- `ToolBroker` pipeline steps documented.
- `/tool-contracts` slash command added for TUI diagnostics.

## 10. Unresolved findings

None. All acceptance criteria satisfied.

## 11. Roadmap disposition

Milestone 002 is closed. Milestone 003 (program domain, storage, and call ledger) is unblocked.

## 12. Registry updates

- `plans/registry.md`: Tool-programs subsystem M002 moved from `ready` to `closed`.
- M003 moved from `blocked` to `ready` (all hard dependencies satisfied).
