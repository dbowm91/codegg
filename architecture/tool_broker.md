# Tool Broker

Status: implemented (M011 ownership closure)

## Purpose

The Tool Broker is the single canonical execution boundary for all
production tool calls — both direct (agent loop) and programmatic
(Tool Programs). It enforces an ordered policy pipeline and returns
typed results.

## Design principles

- **Additive and backward-compatible**: legacy tools that do not
  supply a `ToolContract` receive conservative defaults.
- **Single entry point**: all production tool calls pass through the
  broker. Direct `Tool::execute` calls outside the broker are a
  migration artifact, not a supported production path.
- **Typed results**: the broker returns `ToolValue` with display
  output, optional structured value, artifacts, provenance, and
  terminal status.
- **No ownership of the registry**: the broker holds a pre-built
  `ToolContractCatalog` and configuration. The `ToolRegistry` is
  passed to execution methods by the caller.

## Architecture

```text
AgentLoop / Tool Program
        |
        v
    ToolBroker
        |-- lookup_contract (catalog)
        |-- check_caller_policy
        |-- validate_pre_execution (input schema, bounds, authority)
        |-- deadline/cancellation gate
        |-- execute (via ToolRegistry reference)
        |-- normalize_result -> ToolValue
        `-- return BrokerResult
```

## Key types

| Type | Location | Purpose |
|------|----------|---------|
| `ToolContract` | `src/tool/contract.rs` | Metadata describing how a tool may be called |
| `ToolCallerPolicy` | `src/tool/contract.rs` | Who may invoke the tool |
| `ToolEffectClass` | `src/tool/contract.rs` | Side-effect classification for cache/retry |
| `ToolValue` | `src/tool/contract.rs` | Typed result with display, artifacts, status |
| `ToolBroker` | `src/tool/broker.rs` | Execution pipeline |
| `BrokerInvocationContext` | `src/tool/broker.rs` | Rich caller context |
| `BrokerAuthority` | `src/tool/broker.rs` | Structured verified/unverified authority proof |
| `BrokerResult` | `src/tool/broker.rs` | Typed result with contract and timing |
| `ToolContractCatalog` | `src/tool/contract.rs` | Pre-built contract lookup |

## Pipeline steps

1. **Lookup**: resolve contract from pre-built catalog
2. **Caller policy**: check `ToolCallerPolicy` against `ToolCaller`
3. **Input validation**: schema and size bounds
4. **Authority/permission**: reject `Unverified`; require a structured authority proof
5. **Deadline/cancellation**: nested timeout plus scheduler cancellation propagation
6. **Route selection**: inline native or scheduler-owned (future)
7. **Execution**: `Tool::execute_structured` via registry
8. **Output validation**: schema and format checks
9. **Artifact registration**: large body handles
10. **Terminal result**: `ToolValue` with status and provenance; large output receives a bounded `ctx://` handle and digest

## Legacy compatibility

Tools that do not override `Tool::contract()` receive:

- `ToolCallerPolicy::DirectOnly`
- `ToolEffectClass::NonIdempotent`
- `IdempotencyClass::NonIdempotent`
- No cache, no retry
- String output schema

This ensures existing tools work without modification.

## Migration status

The broker is the single execution boundary for all production tool
calls. `AgentLoop` routes through `tool_broker.execute()` for every
tool invocation. Direct `Tool::execute` calls outside the broker are
blocked by `scripts/check_tool_broker_boundary.py`.

## M011 production correctness

The broker is an enforced crash boundary for both direct and programmatic
calls. Inputs are checked against the bounded JSON-Schema subset in the tool
contract, working directories must be valid directories, and calls select on
the owning cancellation token and effective timeout. Program calls carry a
`BrokerAuthority::Verified` proof derived from the durable Tool Program
authority digest; there is no boolean authorization bypass. The scheduler
persists heartbeats and supplies the outer job deadline, while Tool Program
call reservations/completions/checkpoints are written atomically by
`ToolProgramLedger` before the interpreter advances.

## Related

- `architecture/tool.md` — Tool trait and registry
- `plans/implementation/tool-programs/002-tool-contracts-and-canonical-broker.md`
