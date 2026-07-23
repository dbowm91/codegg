# Tool Programs Milestone 002 — Tool Contracts and Canonical Broker

Status: closed

Repository baseline: `2f715941516a1d49be578fdef56714ad3ddfe8bf` (`main`)

Source roadmap:

- `plans/subsystems/tool-programs-roadmap.md#milestone-2--tool-contracts-and-canonical-broker`

Long-term requirements:

- `plans/000-long-term-specification.md#4-architectural-principles`
- `plans/000-long-term-specification.md#8-deployment-profiles-and-authentication`
- `plans/000-long-term-specification.md#17-job-scheduling-and-execution-backends`
- `plans/001-terminology-and-domain-model.md` — execution context, authorization decision, job, run, artifact

Applicable ADRs:

- `plans/adrs/ADR-0001-programmatic-tool-execution-authority.md`

Primary class: invariant / infrastructure

## 1. Objective

Introduce explicit structured tool contracts and one canonical Tool Broker, then migrate ordinary agent tool execution through that broker without changing model-facing tool names or user-visible behavior.

This milestone creates the policy and execution boundary that later Tool Programs will call. It does not add a `tool_program` tool or permit programmatic callers.

## 2. Readiness boundary

Hard dependency: accepted closure for M001. The broker must route Python through the scheduler-owned path rather than preserving the current direct call.

When M001 closes, no additional architecture decision is required. If M001 changes the execution-context or artifact contract materially, update this plan before handoff.

## 3. Current implementation evidence

- `Tool` exposes name, description, input parameters, category, `execute`, optional `execute_structured`, deferred loading, and model exposure.
- `StructuredToolResult` wraps a string, success flag, and provenance; it is not a typed value contract.
- `ToolExecutionContext` carries backend, optional session, CWD, permission mode, and timeout, but not project/workspace identity, turn/job/attempt lineage, caller type, cancellation, deadline, authority, cache, or artifact policy.
- Permission, path, command, scheduler, RunStore, and backend behavior is distributed across tools and AgentLoop call sites.
- Legacy tools may report success through a string even when the string describes failure.
- Input schemas exist, but runtime validation and output schema validation are not one canonical boundary.

## 4. Invariants that must not regress

- Existing direct agent calls remain behaviorally compatible.
- The broker must not broaden tool authority or suppress current permission prompts.
- Workspace and path policy are evaluated from immutable execution context, never process-global CWD.
- Scheduler-owned tools cannot be invoked directly by the broker implementation.
- Tool output provenance, trust, truncation, and artifact identity remain available.
- One call produces one durable invocation identity and one terminal result.
- Secrets are redacted before persistence, projection, caching, or diagnostics.
- A legacy tool without explicit metadata is conservative: direct-only, mutating/non-cacheable, no automatic retry, string output.

## 5. Scope

### In scope

- `ToolContract`, `ToolCallerPolicy`, `ToolEffectClass`, implementation/version identity, input/output schemas, retry/cache/projection metadata.
- A richer immutable `ToolInvocationContext` and typed `ToolCaller` lineage.
- Canonical Tool Broker lookup, validation, authorization, dispatch, artifact/provenance, and result normalization.
- Migration of AgentLoop ordinary calls through the broker.
- Conservative compatibility adapters for legacy `Tool` implementations.
- Contract/catalog diagnostics and static guards.

### Explicitly out of scope

- Tool Program storage, parser, interpreter, or program caller activation.
- Broad output-schema migration for every tool.
- Changing tool names or merging tool surfaces.
- New authorization roles or frontend permission UX.
- Persistent result caching beyond interface seams.

## 6. Required production changes

### Core/domain

Define stable internal contracts:

```rust
ToolContract {
    name,
    input_schema,
    output_schema,
    caller_policy,
    effect_class,
    idempotency,
    implementation_id,
    implementation_version,
    retry_policy,
    cache_policy,
    projection_policy,
}

ToolInvocationContext {
    caller,
    principal/project/session/turn/agent/job/attempt scope,
    workspace_id,
    execution_context,
    cancellation,
    deadline,
    permission_mode,
    submission_key,
}

ToolValue {
    value,
    display,
    artifacts,
    provenance,
    terminal_status,
}
```

Use existing typed IDs where available. Do not introduce lossy numeric IDs.

### Tool trait and registry

- Add contract access without forcing every implementation to migrate immediately.
- Generate model-facing definitions from contract input schema and existing descriptions.
- Validate unique names and stable implementation identity at registry construction.
- Default legacy contracts to `DirectOnly`, conservative side effects, no cache, no retry, and string output schema.
- Add diagnostics for explicit output schema without typed execution, unsafe caller policy, or missing implementation version.

### Tool Broker

The broker must perform, in order:

1. registry lookup and contract snapshot;
2. caller-policy check;
3. input-schema validation and bounded normalization;
4. authority, permission, sensitive-path, and workspace policy;
5. deadline/cancellation precheck;
6. route selection: inline native/MCP or scheduler-owned subsystem;
7. execution with invocation identity and provenance;
8. output-schema validation;
9. artifact registration and bounded display projection;
10. terminal result recording/event emission.

Return typed failures; do not encode policy or schema failure only in output text.

### AgentLoop migration

- Route normal tool calls through the broker while preserving transcript/tool-call protocol behavior.
- Maintain existing parallel-call ordering and per-turn tool budgets.
- Ensure permission requests and tool results retain their current IDs and model-visible structure.
- Remove alternate production paths that call registry tools directly.

### Documentation and guards

- Add `architecture/tool_broker.md` and update `architecture/tool.md`.
- Add a guard that production AgentLoop code cannot invoke `Tool::execute` or `execute_structured` except through the broker.
- Document legacy contract defaults and migration procedure.

## 7. Ordered work packages

### Work package A — Contract types and conservative defaults

- Add caller/effect/idempotency/retry/cache/projection enums and schemas.
- Define compatibility defaults and validation.
- Add serialization only for stable diagnostic/protocol forms; internal trait objects remain internal.
- Unit-test contract hashing and deterministic catalog ordering.

### Work package B — Invocation context and typed results

- Replace or supersede `ToolExecutionContext` with explicit scope, cancellation, deadline, and caller lineage.
- Add typed terminal status and artifact handles.
- Preserve legacy display strings through adapters.

### Work package C — Broker policy pipeline

- Implement one service with injectable authorization, permission, scheduler, RunStore/artifact, and event dependencies.
- Ensure every denial/failure is attributed to an invocation ID.
- Make schema and output validation bounded and fail closed.

### Work package D — AgentLoop adoption

- Route all production agent calls through the broker.
- Preserve provider message/tool-result compatibility and current user prompts.
- Add parity fixtures comparing pre-migration expected transcripts with brokered output.

### Work package E — Diagnostics, docs, and guards

- Add `/tool-contracts` or equivalent bounded diagnostic output.
- Update architecture docs and semantic source guards.
- Document how a tool becomes eligible for future programmatic use.

## 8. Failure, cancellation, restart, and contention semantics

- Lookup, schema, caller-policy, authorization, and permission denial occur before side effects.
- Cancellation before dispatch returns cancelled without execution.
- Inline tools must observe cancellation where meaningful; scheduler-owned calls delegate cancellation to their job.
- Output-schema mismatch is a tool implementation failure and is never cached or treated as success.
- Broker retries are disabled by default and may occur only when the contract, caller, and effect class permit them.
- Duplicate submission keys return the same scheduler job where the routed subsystem supports idempotent submission.
- The broker itself owns no unbounded task or queue.

## 9. Compatibility and migration

- Preserve `Tool` and model definitions during staged migration.
- Legacy tools execute through a wrapper that produces `ToolValue::String` and conservative metadata.
- Do not require providers to understand output schemas; they remain an internal runtime contract until later capability negotiation.
- Existing tests that invoke tools directly may use a test broker or explicitly marked low-level unit path.
- Removal of legacy `execute` is deferred until all tools migrate and a separate plan approves it.

## 10. Required tests

### Focused unit tests

- contract defaults, hashing, validation, caller checks, input/output schemas;
- typed failure and artifact projection;
- cancellation/deadline prechecks.

### Integration tests

- AgentLoop direct calls through broker for read, edit, Bash, Python, test, Git, MCP, and disabled tools;
- permission prompt and response parity;
- scheduler-owned route proof for Python/test/build.

### Contention and cancellation tests

- concurrent calls preserve per-turn limits and result ordering;
- cancellation before and during inline/scheduler calls;
- no unbounded broker task growth.

### Security and negative tests

- caller-policy bypass, malformed schemas, oversized arguments/results, sensitive path denial, forged lineage, secret redaction;
- output-schema mismatch and legacy string failure handling.

### Migration and compatibility tests

- existing provider transcript fixtures;
- deferred-loading/tool-search definitions unchanged;
- direct tool unit compatibility through adapters.

## 11. Required verification commands

```bash
cargo test -p codegg --lib tool
cargo test -p codegg --test agent_loop_harness
cargo test -p codegg --test command_routing_execution_ownership
cargo test -p codegg --test tool_broker_integration
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets
```

## 12. Documentation updates

- `architecture/tool.md`
- new `architecture/tool_broker.md`
- Python/test/Bash ownership docs where routing changes
- tool-author guidance for schemas, effects, retries, artifacts, and future program eligibility

## 13. Acceptance criteria

1. Every production agent tool call enters one canonical broker.
2. Direct user-visible behavior and permission semantics remain compatible.
3. Caller policy, input schema, authorization, cancellation, output schema, provenance, artifacts, and terminal status are enforced in one ordered pipeline.
4. Scheduler-owned operations cannot be executed directly by broker adapters.
5. Legacy tools are conservative and cannot accidentally become program-callable.
6. Static guards block new AgentLoop direct execution paths.
7. No unresolved high or medium finding remains.

## 14. Stop conditions

Stop and report if:

- M001 is not strictly closed;
- broker adoption would require changing authorization ownership or provider wire semantics;
- an existing tool cannot be adapted without broad unrelated redesign;
- output schemas would be fabricated from unstable prose rather than explicit values;
- scope expands into program storage or runtime.

## 15. Closure evidence required

Create `plans/closure/tool-programs/002-status.md` with exact commits, contract migration inventory, direct-call parity matrix, security/negative results, cancellation/contention evidence, static guard output, broad test results, and severity-ranked residual findings.

## 16. Handoff notes

Prefer an additive compatibility layer and vertical AgentLoop adoption over a repository-wide mechanical trait rewrite. Preserve existing resource-limited test policy. Do not mark any tool program-callable in this milestone.
