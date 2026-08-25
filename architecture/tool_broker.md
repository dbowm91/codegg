# Tool Broker

Status: implemented (M011 ownership closure, M012 authority correction,
M019 strict closure, M020 corrective disposition — all closed)

## Purpose

The Tool Broker is the single canonical execution boundary for all
production tool calls — both direct (agent loop) and programmatic
(Tool Programs). It enforces an ordered policy pipeline and returns
typed results. All production tool calls pass through the broker;
direct `Tool::execute` calls outside the broker are blocked by
`scripts/check_tool_broker_boundary.py`.

## Where It Lives

| File | Purpose |
|------|---------|
| `src/tool/broker.rs` | Execution pipeline, `ToolBroker`, `BrokerInvocationContext`, `BrokerAuthority`, `BrokerResult`, `BrokerError` |
| `src/tool/contract.rs` | `ToolContract`, `ToolCallerPolicy`, `ToolEffectClass`, `ToolTerminalStatus`, `ToolValue`, `ToolContractCatalog`, `ToolCaller` |

## Design Principles

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

## How It Works

```text
AgentLoop / Tool Program
        |
        v
    ToolBroker
        |-- lookup_contract (catalog)
        |-- check_caller_policy
        |-- verify_grant_scope (authority)
        |-- validate_pre_execution (input schema, bounds, deadline)
        |-- execute (via ToolRegistry::execute_structured)
        |-- normalize_result -> ToolValue
        |-- validate_output (schema, bounds)
        |-- register_artifacts (large bodies)
        `-- return BrokerResult
```

## Key Types & APIs

| Type | Location | Purpose |
|------|----------|---------|
| `ToolContract` | `src/tool/contract.rs:183` | Metadata: caller policy, effect class, schemas, retry/cache/projection policy |
| `ToolCallerPolicy` | `src/tool/contract.rs:28` | `DirectOnly` / `DirectOrProgrammatic` / `ProgrammaticOnly` |
| `ToolEffectClass` | `src/tool/contract.rs:48` | `ReadOnly` / `ReadValidate` / `SafeRepeat` / `IdempotentMutating` / `NonIdempotent` / `ProcessExec` |
| `ToolTerminalStatus` | `src/tool/contract.rs:302` | `Success` / `Error` / `Denied` / `Cancelled` / `TimedOut` / `InfrastructureError` |
| `ToolValue` | `src/tool/contract.rs:328` | Typed result: display, value, artifacts, provenance, status, truncated |
| `ToolContractCatalog` | `src/tool/contract.rs:452` | Pre-built HashMap of tool contracts |
| `ToolBroker` | `src/tool/broker.rs:435` | Execution pipeline: catalog, config, optional artifact store |
| `ToolBrokerConfig` | `src/tool/broker.rs:40` | `default_timeout_ms`, `max_input_bytes`, `max_output_display_bytes`, `max_output_bytes` |
| `BrokerInvocationContext` | `src/tool/broker.rs:72` | Rich caller context (caller, cwd, session/workspace/agent/turn/job/attempt IDs, authority, cancellation, deadline, principal, path policy, allowed tools, policy revision) |
| `BrokerAuthority` | `src/tool/broker.rs:115` | `Unverified` / `Verified { grant: ToolAuthorityGrant }` |
| `BrokerResult` | `src/tool/broker.rs:386` | Typed result with contract, invocation_id, elapsed_ms |
| `BrokerError` | `src/tool/broker.rs:946` | `NotFound` / `NoContract` / `CallerDenied` / `InputTooLarge` / `Execution` / `AuthorityError` |
| `ToolCaller` | `src/tool/contract.rs:283` | `Agent` / `Program { program_id }` / `Subagent { parent_agent_id }` / `Api { client_id }` / `Internal` |

## Pipeline Steps (broker.rs:5-16)

1. **Lookup**: resolve contract from pre-built catalog
2. **Caller policy**: check `ToolCallerPolicy` against `ToolCaller`
3. **Input validation**: schema and size bounds
4. **Authority/permission**: reject `Unverified`; verify grant scope
5. **Deadline/cancellation**: nested timeout plus scheduler cancellation propagation
6. **Route selection**: inline native or scheduler-owned (future)
7. **Execution**: `Tool::execute_structured` via registry with cancellation token
8. **Output validation**: schema and format checks
9. **Artifact registration**: large body handles
10. **Terminal result**: `ToolValue` with status and provenance; large output receives a bounded `ctx://` handle and digest

## Legacy Compatibility

Tools that do not override `Tool::contract()` receive `ToolContract::legacy()`:

- `ToolCallerPolicy::DirectOnly`
- `ToolEffectClass::NonIdempotent`
- `IdempotencyClass::NonIdempotent`
- No cache, no retry
- `output_schema: None`

This ensures existing tools work without modification.

## Configuration Surface

`ToolBrokerConfig` defaults:

| Parameter | Default | Description |
|-----------|---------|-------------|
| `default_timeout_ms` | 120,000 | Per-call timeout when none specified |
| `max_input_bytes` | 10 MB | Maximum input payload |
| `max_output_display_bytes` | 256 KB | Threshold for artifact spillover |
| `max_output_bytes` | 10 MB | Hard output limit (truncation beyond this) |

## Invariants & Gotchas

1. **Broker does not own the registry**: `ToolRegistry` is passed to
   `execute()` — the broker only holds the contract catalog snapshot.
2. **Unverified authority is rejected**: the broker rejects calls
   with `BrokerAuthority::Unverified` in `validate_pre_execution`.
   Programmatic callers always carry a `BrokerAuthority::Verified`.
3. **Grant scope verification**: `verify_grant_scope()` checks
   validity, integrity, workspace, caller class, effect class,
   session binding, permission mode, principal, path policy,
   manifest, contract snapshot, and policy revision.
4. **Programmatic failure mapping**: `into_programmatic_outcome()`
   maps terminal statuses — only `Success` becomes a `CompletedCall`.
5. **Workspace artifacts**: `with_artifact_store()` attaches the
   canonical artifact store for large output spillover.

## Testing

```bash
cargo test -p codegg --lib tool::broker
cargo test -p codegg --lib tool::contract
```

## Related Docs

- `architecture/tool.md` — Tool trait and registry
- `architecture/tool_programs.md` — Tool Program domain, storage, call ledger
- `architecture/tool_program_language.md` — Restricted-Python language spec
