# Agent Tool Runtime Reconciliation M001 — MCP Broker Transport Adapter

Status: implemented; closed (`plans/closure/agent-tool-runtime-reconciliation/001-status.md`)

Repository baseline: `8535f6d7c2e68f5d134ba0c73bd146ca8bc0bd03`

Source roadmap: `plans/subsystems/agent-tool-runtime-reconciliation-roadmap.md#6-dependencies-and-milestones`

Long-term requirements: `plans/000-long-term-specification.md#27-security-requirements`, `#29-system-invariants`, `#47-correctness-before-transparent-magic`, and `plans/003-planning-process.md`.

Applicable ADRs: existing broker/permission and MCP owners; none new. A new ADR is mandatory if a change reallocates transport authorization or broker ownership.

Primary class: invariant

## 1. Objective

Remove the direct `mcp__*` model-execution branch. Expose third-party MCP tools through conservative, session-pinned `Tool` adapters registered in the ordinary `ToolRegistry`, so each model call uses the same approval, hook, grant verification, broker schema/size/deadline/cancellation, typed-result/artifact and telemetry pipeline as native and plugin tools.

## 2. Readiness and baseline evidence

Hard dependencies are already closed: ToolBroker, MCP client, PluginToolAdapter and the existing direct-call permission mechanism.

- `src/agent/tool_batch.rs` separates `mcp__` model tool calls and invokes `McpService::call_tool()` directly with a fixed 60-second deadline and ad hoc lock retries. This branch is outside ordinary broker dispatch and the regular tool-execution hook pipeline; never assume the current MCP calls have **no** permission check — prior filtering does occur.
- `src/mcp/mod.rs::McpTool` provides server, name, description, bounded schema, optional outputSchema, annotations and metadata; `call_tool_structured()` retains `is_error` and `structured` while `call_tool()` flattens to text.
- `src/tool/mod.rs` already registers native/plugin tools through `ToolRegistry::register()`; `src/agent/loop.rs` builds `ToolBroker` from the registry when constructing the loop. Ensure any MCP adapters are installed **before** broker contract snapshot construction.
- `src/agent/request_preparation.rs` separately appends raw MCP definitions. That path must be reconciled so MCP names are neither advertised twice nor callable through old direct branch.
- `src/agent/tool_batch.rs` already normalizes native completion, event/hook and progress semantics. Reuse this path; preserve model-facing `mcp__server__tool` names.

## 3. Invariants

Every raw MCP call requires the existing permission/approval receipt and a broker-verified, scoped authority grant before a remote request. Remote MCP metadata is **never** trusted to downgrade authorization. Default effect class is conservative mutating/non-idempotent, `DirectOnly` caller policy, zero automatic retries. The adapter does not execute when its pinned service/server/descriptor is gone or replaced, or when the session's tool exposure policy excludes raw tools. A cancellation/timeout does not prove an external effect was rolled back.

Keep McpService solely responsible for HTTP/stdio, credential handling, DNS rebinding checks and protocol status. Do not create a second client. Raw managed eggsearch remains hidden unless explicitly opted in; native eggsearch wrappers continue to execute normally.

## 4. Scope and required production changes

### A — Adapter and registration

Add a bounded `McpToolAdapter` near `src/tool/mcp.rs` (or equivalent canonical location) implementing `Tool`: frozen validated canonical identity `mcp__<server>__<tool>`, description, input schema, conservative category/contract, `execute_structured` and no legacy side effects outside broker.

Session construction (`src/agent/turn_runtime.rs`; `src/agent/worker.rs`; all other `AgentLoop` constructors) must take the already bootstrapped `McpService` handle, a frozen generation/version and `McpExposurePolicy`, register adapters before constructing the broker, and publish one registry-defined callable tool each. Preserve session-scoped plugin activation, extension tools, and work-order registrations; no loss of existing tools.

Validate names/collisions/maximum schema bounds before admission. Reject malformed `mcp__` names, ambiguous separators, duplicate canonical names, oversized schemas, unreachable/disallowed servers. Never treat names alone as authority. Metadata must have bounded, sanitized disclosure; remote annotations remain descriptive/untrusted. Decide and test whether schema drift invalidates the pinned descriptor even when name is unchanged.

### B — Call and result semantics

Call `McpService::call_tool_structured()` only from the registered adapter after broker admission. Translate `McpToolCallResult` to a `ToolStructuredOutput` preserving structured JSON, `is_error` as a failed terminal status, text fallback, bounded provenance/server identity without credentials, and broker-created `ctx://` artifacts for large results. Do not mistakenly map a JSON-RPC transport success with MCP `isError=true` to a successful tool execution. Treat `outputSchema` as an optional consistency check against *untrusted* server output, with explicit behavior for malformed/missing/oversized output (reject or safe error; do not silently treat as verified).

Prefer the context's deadline/cancellation token over an independent 60-second timer. Handle stdio/remote lock contention without busy looping or holding registry locks across awaits; cooperative cancellation should release caller resources promptly. Execution failure/timeout returns a typed tool result, not a successful answer containing the literal string `Error:`.

### C — Single execution path

Remove the special `mcp_tool_calls` fork from `src/agent/tool_batch.rs` and route adapter calls into existing native batch pipeline, preserving max-parallel admission, progress recovery, hook order, auditable execution receipts, deterministic output ordering and display. Ensure no other raw `mcp__*` string dispatch path bypasses the broker. Retire the separately appended MCP schema definitions or make registration the single canonical origin, without changing public wire names or operator raw-visibility config.

### D — Guard and docs

Add a static guard detecting direct model-path `McpService::call_tool*` / `mcp__` execution routing outside the adapter. Amend `architecture/tool_broker.md`, `architecture/mcp.md`, `architecture/tool.md` and `architecture/agent-tool-surface.md` to state the accurate execution and visibility story. Do not modify closed predecessor records.

## 5. Ordered work packages and acceptance evidence

1. **Contract first**: write fake-server and adapter tests for identity validation, denied/hidden/disabled tools, schema checks, conservative contract and broker catalog presence. Evidence: deterministic unit fixtures.
2. **Broker adapter integration**: register before loop/broker creation, exercise consent-grant/scope, hook/event order, read-only and full-profile sandbox interactions. Evidence: one end-to-end model-call trajectory using a fake MCP service and canonical broker.
3. **Result fidelity**: structuredContent and text-only paths, `isError`, malformed result, 256KB+ artifact spillover and bounded log redaction. Evidence: typed status/asserted artifact retrieval, no credential leakage.
4. **Remove bypass**: delete split dispatch and its independent retries, add static guard and negative tests. Evidence: grep/static check, complete old and new focused suite.
5. **Lifecycle and documentation**: disconnect, cancellation, timeout, duplicate effect and concurrent tool batch tests; reconcile docs and provide closure matrix.

## 6. Failure, restart, contention and compatibility

No new durable store or protocol migration. `McpService::call_tool()` remains available for non-model compatibility consumers; it must never be reintroduced as model bypass. Existing names `mcp__<server>__<tool>`, raw MCP exposure configuration, provider aliases, plugin and eggsearch wrapper behavior stay stable. A disconnected/restarted server must not silently redirect a call to a different descriptor. A timed-out non-idempotent remote call is **uncertain side effect**, never blindly replayed. Existing cancellation of local subprocess/HTTP and broker cancellation cooperate; tests record limits if the server ignores cancellation.

## 7. Required tests and commands

Focused tests:

- Fake MCP server with two tools (read-hinted and write-capable), duplicate name conflict, input/output schema, structured content, `isError`, oversized return and transport failure.
- Approval deny/ask/allow cases and negative forged broker grants, raw exposure off and managed-eggsearch exclusion.
- Hook `PreToolExecute`/`PostToolExecute` and terminal statuses, cancellation before/during call, deadline exhaustion, concurrent batches with a slow server and no global lock held.
- Native/plugin/raw-MCP parity without changing existing Tool Programs caller restrictions.
- Test that zero extra remote calls occur on ambiguous timeout/errors and that model does not see an available schema for an unregistered tool.

Commands, adapted to actual test targets identified at implementation:

```bash
cargo fmt --all -- --check
cargo test --workspace mcp
cargo test --workspace tool_broker
cargo test --workspace tool_batch
python3 scripts/check_tool_broker_boundary.py
cargo clippy --workspace --all-targets -- -D warnings
bash scripts/verify.sh quick
```

Verify script paths and target names before executing; if a command is incompatible with the current layout, record the replacement and reason. Full matrix/platform-dependent checks are recorded as run/not-run, not implied.

## 8. Acceptance, stop and closure

M001 closes only when a raw MCP model call is provably broker-mediated end-to-end, wrong grants or stale descriptors fail before transport, structured error and artifact semantics are correct, hooks and audits are not bypassed, all old model-side direct MCP branch code is gone, and managed wrappers/plugin tools remain available under the old configuration.

Stop if requiring a second MCP service instance, generalized broker redesign, new user authorization grant from remote metadata, incompatible tool-wire rename, or model-visible ability to invoke raw MCP without the intended opt-in.

Closure record at `plans/closure/agent-tool-runtime-reconciliation/001-status.md` must link implementation SHAs, requirement/evidence matrix, exact tests/guard output, errors/uncertainties, compatibility comparison, and classify unresolved findings. M002 is not marked ready until this closure is accepted.
