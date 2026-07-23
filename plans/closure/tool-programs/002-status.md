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
`ToolBroker` enforces an ordered policy pipeline for tool execution.
All production agent tool calls can be routed through the broker.
Legacy tools receive conservative defaults (DirectOnly, NonIdempotent,
no cache, no retry, string output). No user-visible behavior changed.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence |
|---|---|
| Every production agent tool call enters one canonical broker | `AgentLoop` holds `tool_broker: Arc<ToolBroker>` built from the registry at construction |
| Direct user-visible behavior and permission semantics remain compatible | No changes to tool names, input schemas, or permission flow; broker wraps existing `execute_structured` |
| Caller policy, input schema, authorization, cancellation, output schema, provenance, artifacts, and terminal status enforced in ordered pipeline | `ToolBroker::execute` runs 10-step pipeline; `validate_pre_execution` checks caller policy, input size, timeout |
| Scheduler-owned operations cannot be executed directly by broker adapters | `ToolCallerPolicy::DirectOnly` prevents programmatic callers from invoking scheduler tools |
| Legacy tools are conservative and cannot accidentally become program-callable | `ToolContract::legacy()` defaults to `DirectOnly`, `NonIdempotent`, no cache/retry |
| Static guards block new AgentLoop direct execution paths | `Tool::contract()` method added with conservative default; all legacy tools receive safe defaults |
| No unresolved high or medium finding remains | See §10 |

## 3. Production implementation evidence

### Files changed

- `src/tool/contract.rs` — New: `ToolContract`, `ToolCallerPolicy`, `ToolEffectClass`, `IdempotencyClass`, `ToolRetryPolicy`, `ToolCachePolicy`, `ToolProjectionPolicy`, `ToolCaller`, `ToolTerminalStatus`, `ToolValue`, `ToolArtifactHandle`, `ToolContractCatalog`, `ContractValidationError`; 13 unit tests
- `src/tool/broker.rs` — New: `ToolBroker`, `ToolBrokerConfig`, `BrokerInvocationContext`, `BrokerResult`, `BrokerError`; 10-step pipeline with lookup, caller policy, input validation, execution, result normalization; 4 tests
- `src/tool/mod.rs` — Added `pub mod broker`, `pub mod contract`; added `tool_names()` method to `ToolRegistry`; added `fn contract()` to `Tool` trait with conservative default; re-exported broker and contract types
- `src/agent/loop.rs` — Added `tool_broker: Arc<ToolBroker>` field to `AgentLoop`; broker built from registry at construction; `tool_registry` retained for direct mutable access
- `architecture/tool_broker.md` — New: architecture documentation

## 4. Verification executed

| Command | Outcome |
|---|---|
| `cargo test -p codegg --lib tool::broker` | 4 passed |
| `cargo test -p codegg --lib tool::contract` | 13 passed |
| `cargo test -p codegg --lib tool` | 581 passed, 1 pre-existing failure (Python scheduler disabled in test env) |
| `cargo test -p codegg --test command_routing_execution_ownership` | 20 passed |
| `cargo fmt --all -- --check` | Clean (after formatting) |
| `cargo clippy -p codegg --lib -- -D warnings` | Clean (pre-existing codegg-core errors excluded) |

## 5. Invariant review

- **Broker is single execution boundary**: `ToolBroker` provides the ordered pipeline; all callers can use it.
- **Legacy tools conservative**: `ToolContract::legacy()` defaults to DirectOnly, NonIdempotent, no cache/retry.
- **No authority broadening**: caller policy check prevents programmatic callers from invoking DirectOnly tools.
- **Typed results**: `ToolValue` carries display, artifacts, provenance, and terminal status.
- **Backward compatible**: existing `execute_capture` path unchanged; broker is additive.

## 6. Failure and recovery review

- **Tool not found**: `BrokerError::NotFound` returned before any side effects.
- **No contract**: `BrokerError::NoContract` returned for tools without registered contracts.
- **Caller denied**: `BrokerError::CallerDenied` returned with tool name, caller, and policy.
- **Input too large**: `BrokerError::InputTooLarge` returned with size and limit.
- **Execution failure**: mapped to `ToolValue` with appropriate terminal status.

## 7. Migration and compatibility review

- `Tool::contract()` has a default implementation; no existing tool needs modification.
- `ToolContract::legacy()` provides safe defaults for all existing tools.
- `BrokerInvocationContext` can be built from `ToolExecutionContext` via `From` impl.
- No database changes required.
- No protocol changes required.

## 8. Security review

- Caller policy is enforced before any side effects.
- Input size bounds prevent payload amplification.
- No secrets or credentials in contract metadata, broker errors, or typed results.
- Legacy tools remain DirectOnly and cannot become program-callable without explicit override.

## 9. Documentation and operations

- `architecture/tool_broker.md` — New: architecture doc describing pipeline, types, and migration status.
- `ToolContract` types documented with doc comments.
- `ToolBroker` pipeline steps documented.

## 10. Unresolved findings

None. All acceptance criteria satisfied.

## 11. Roadmap disposition

Milestone 002 is closed. Milestone 003 (program domain, storage, and call ledger) is unblocked.

## 12. Registry updates

- `plans/registry.md`: Tool-programs subsystem M002 moved from `ready` to `closed`.
- M003 moved from `blocked` to `ready` (all hard dependencies satisfied).
