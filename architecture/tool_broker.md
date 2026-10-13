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
| `ToolContract` | `src/tool/contract.rs:184` | Metadata: caller policy, effect class, schemas, retry/cache/projection policy |
| `ToolCallerPolicy` | `src/tool/contract.rs:28` | `DirectOnly` / `DirectOrProgrammatic` / `ProgrammaticOnly` |
| `ToolEffectClass` | `src/tool/contract.rs:48` | `ReadOnly` / `ReadValidate` / `SafeRepeat` / `IdempotentMutating` / `NonIdempotent` / `ProcessExec` |
| `ToolTerminalStatus` | `src/tool/contract.rs:302` | `Success` / `Error` / `Denied` / `Cancelled` / `TimedOut` / `InfrastructureError` / `UncertainSideEffect` |
| `ToolValue` | `src/tool/contract.rs:333` | Typed result: display, value, artifacts, provenance, status, truncated |
| `ToolContractCatalog` | `src/tool/contract.rs:471` | Pre-built HashMap of tool contracts |
| `ToolBroker` | `src/tool/broker.rs:495` | Execution pipeline: catalog, config, optional artifact store |
| `ToolBrokerConfig` | `src/tool/broker.rs:40` | `default_timeout_ms`, `max_input_bytes`, `max_output_display_bytes`, `max_output_bytes` |
| `BrokerInvocationContext` | `src/tool/broker.rs:72` | Rich caller context with 21 fields, including `allowed_tools` and `parent_tool_authority` (the inherited child ceiling checked for every child invocation and forwarded to `task` for further narrowing) |
| `BrokerAuthority` | `src/tool/broker.rs:163` | `Unverified` / `Verified { grant: ToolAuthorityGrant }` |
| `BrokerResult` | `src/tool/broker.rs:440` | Typed result with contract, invocation_id, elapsed_ms |
| `BrokerError` | `src/tool/broker.rs:1281` | `NotFound` / `NoContract` / `CallerDenied` / `InputTooLarge` / `Execution` / `AuthorityError` |
| `ToolCaller` | `src/tool/contract.rs:283` | `Agent` / `Program { program_id }` / `Subagent { parent_agent_id }` / `Api { client_id }` / `Internal` |

## Pipeline Steps (`src/tool/broker.rs:7-16`)

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
   session binding, permission mode, principal, and path policy
   (9 dimensions). Manifest, contract snapshot, and policy revision
   are verified conditionally for programmatic callers.
   **Descendant ceiling (M003)**: agent child loops bind `allowed_tools`
   to the validated parent eligible set, and the broker independently
   validates `parent_tool_authority` on every child call. It checks
   integrity, expiry, worker-bound child session/workspace, exact
   canonical name, and the pinned per-name maximum effect against the
   current broker contract before execution. Missing effect entries fail
   closed, including on older serialized payloads. The same snapshot reaches
   `task` for re-narrowing.
   Its SHA-256 is an unkeyed integrity checksum; authenticity comes from
   host-owned AgentLoop/TaskTool or scheduler payload carriage, not a
   cryptographic sender signature. The independent grant effect check,
   sandbox, and tool-specific approvals remain in force.
   `allowed_effect_class` is a **ceiling**, not an exact-match label:
   the check is `tool_class.severity() <= grant_ceiling.severity()`,
   ordered `read_only < read_validate < safe_repeat < idempotent_mutating
   < non_idempotent < process_exec` (`ToolEffectClass::severity`,
   `src/tool/contract.rs`). `process_exec` outranks every mutation class
   so process execution never rides in on a mutation-classed grant, and
   an empty or unrecognized ceiling fails closed. `"any"` is unrestricted.
4. **Programmatic failure mapping**: `into_programmatic_outcome()`
   maps terminal statuses — only `Success` becomes a `CompletedCall`.
5. **Program-capable caller policies**: `resolve_manifest` admits
   `DirectOrProgrammatic` and `ProgrammaticOnly` (rejecting
   `DirectOnly`); `resolve_contract_snapshot`
   (`src/tool/tool_program_context.rs`) admits the same two policies
   with a read-side effect class. The hidden M003 `git_read`/`lsp_read`
   adapters are `ProgrammaticOnly`: broker-callable by programs,
   denied to `Agent` callers, and invisible to model disclosure.
5. **Workspace artifacts**: `with_artifact_store()` attaches the
   canonical artifact store for large output spillover.
5b. **MCP broker mediation (M001 agent-tool-runtime-reconciliation)**:
   raw `mcp__*` model calls execute through session-pinned
   `McpToolAdapter` (`src/tool/mcp_adapter.rs`) registered before this
   broker's catalog snapshot. The retired `tool_batch.rs` direct
   `McpService::call_tool` fork is gone; adapter calls use the context
   deadline/cancellation token (no independent 60 s timer), preserve
   `structuredContent` and `isError`-as-`Error`, enforce pinned-schema
   staleness before transport, and keep `DirectOnly`/`NonIdempotent`/
   no-retry conservatism. Guard: `scripts/check_mcp_broker_adapter.py`.
   `ToolBatchExecutor` issues exactly one broker dispatch per accepted model
   tool call. It does not retry based on `ToolError::is_retryable()` because
   that error class cannot distinguish a pre-dispatch failure from an
   ambiguous post-dispatch timeout. Automatic replay is available only through
   `execute_with_retry` when its caller supplies a bounded `RetryContext` and
   the frozen contract admits the effect/acknowledgement combination. With no
   qualified chain, `execute()` remains single-attempt. A timeout from a
   non-idempotent MCP call therefore cannot cause another `tools/call` from
   the batch layer.
6. **Trusted audit preservation (M001)**: `BrokerInvocationContext`
   carries `execution_audit: Option<TrustedExecutionAuditContext>`
   threaded only from daemon-owned admission. `execute_with_retry`
   preserves it into `ToolExecutionContext` via `apply_execution_audit`
   without synthesizing from `principal_ref`, tool input, model output,
   or grant strings. See `architecture/audit.md` "Trusted execution
   audit seam".

## Testing

```bash
cargo test -p codegg --lib tool::broker
cargo test -p codegg --lib tool::contract
```

## Related Docs

- `architecture/tool.md` — Tool trait and registry
- `architecture/tool_programs.md` — Tool Program domain, storage, call ledger
- `architecture/tool_program_language.md` — Restricted-Python language spec

## Source Verification

Verified 2026-10-06 against source. Corrected: `ToolValue` `:328` → `:333`,
`ToolContractCatalog` `:452` → `:471`, `ToolBroker` `broker.rs:435` → `:495`,
`BrokerAuthority` `:115` → `:163`, `BrokerResult` `:386` → `:440`,
`BrokerError` `:951` → `:1281`, pipeline steps `broker.rs:5-16` → `:7-16`;
`ToolTerminalStatus` 6 → 7 variants (`UncertainSideEffect` was missing);
`BrokerInvocationContext` now lists all 20 fields instead of a subset.
Verified accurate: `ToolContract` (11 fields, `contract.rs:184`),
`ToolCallerPolicy` (`:28`), `ToolEffectClass` (`:48`), `ToolCaller` (`:283`),
`ToolValue` 6 fields, `ToolBroker` 3 fields, `BrokerAuthority` 2 variants,
`BrokerResult` 4 fields, `BrokerError` 6 variants, `ToolBrokerConfig` (4 fields,
`broker.rs:40`) and all four of its defaults (120,000 ms / 10 MB / 256 KB /
10 MB), the 10-step pipeline list, the 5-item legacy contract default set,
and `scripts/check_tool_broker_boundary.py`.
