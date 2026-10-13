# Agent Tool Runtime Post-Closure Corrective C001 — Contract-Safe Tool Retry Admission

Status: implemented

Repository baseline: `a54d18c7e7b92d1c9ac4971d7cdb8cc916418a5d` on `planning/agent-tool-runtime-reconciliation`. Rebaseline before implementation against latest branch head; do not overwrite subsequent changes.

Source roadmap: `plans/subsystems/agent-tool-runtime-reconciliation-post-closure-corrective-addendum.md#4-milestones-and-dependencies`.

Original plan/closure: `plans/implementation/agent-tool-runtime-reconciliation/001-mcp-broker-transport-adapter.md`; `plans/closure/agent-tool-runtime-reconciliation/001-status.md`. Shared retry predecessor: `architecture/tool_broker.md`, `src/tool/retry.rs`.

Long-term requirements:

- `plans/000-long-term-specification.md#26-reliability-and-recovery`
- `plans/000-long-term-specification.md#27-security-requirements`
- `plans/000-long-term-specification.md#29-system-invariants`
- `plans/000-long-term-specification.md#47-correctness-before-transparent-magic`
- `plans/003-planning-process.md#7-corrective-passes`.

Applicable ADRs: existing canonical broker/permission ownership. ADR only if retry admission must move to a different system.

Primary class: invariant.

## 1. Objective

Eliminate independent retries of potentially effectful model tool calls outside the canonical broker's retry policy. A call dispatched remotely with an ambiguous timeout must never be automatically repeated unless the owning tool contract, effect/acknowledgement classification and shared retry budget explicitly authorize safe replay. Preserve bounded retry of proven-safe operations where already supported.

## 2. Why this corrective is ready

The M001 adapter/broker and existing retry contract/chain are implemented. This fix can be confined to `src/agent/tool_batch.rs`, the broker retry seam and tests without changing MCP protocol, user approval policy or storage.

## 3. Current evidence / why original verification missed it

At baseline:

- `src/agent/tool_batch.rs` runs `ToolBroker::execute` inside `for attempt in 0..2` with a 500 ms backoff. `Err(e) if e.is_retryable()` relies on `ToolError::is_retryable`.
- `codegg_core::error::ToolError::is_retryable` returns true for `Io`, `Network`, and `Timeout`, regardless of the tool's effect class or acknowledgement state.
- Broker terminal `ToolTerminalStatus::TimedOut` is translated into `ToolError::Timeout` and then retried by the outer loop. A broker timeout can occur after the MCP server receives a write.
- `McpToolAdapter::contract` correctly declares `DirectOnly`, `NonIdempotent`, `ToolRetryPolicy::none()`. `ToolBroker::execute` itself has single-attempt semantics; `execute_with_retry` accepts an explicit shared retry chain and the broker's `decide_tool_retry` policy.
- M001 tests checked broker-level no-retry and an adapter cancellation, but did not exercise the *outer batch loop* with a timed-out, effectful server and a dispatch counter. The closure describes broker-internal safety while missing the second caller layer.

The scope includes `ProcessExec` shell commands and any other non-idempotent native or plugin effects; it is not safe to fix only names prefixed with `mcp__`.

## 4. Invariants

- No client-owned outer-loop replay of a tool with `NonIdempotent` or `ProcessExec` effect after dispatch acknowledgement is uncertain, including timeouts, cancelled sends or connection breaks.
- The authoritative replay decision uses the tool's frozen `ToolContract`, idempotency class, acknowledgement phase, invocation key and bounded broker `RetryContext`; string/error-class tests do not override it.
- A retry does not re-consume approval, regenerate authority, change workspace/principal/sandbox/profile, rebind an MCP descriptor, repeat hooks as a new logical model call, or reset the chain budget.
- If a retry would exceed a deadline, explicit attempt budget, descriptor generation, cancellation or grant validity, it is refused. A timeout must never be recorded as a successful tool result.
- Preserve per-tool timeouts and grace layering (tool T, broker T+5s, batch T+10s) so shell partial output survives.
- Preserve exact output ordering, concurrency semaphore, broker artifacts, provenance, progress recovery and pre/post plugin/host hooks.

## 5. Scope

### In scope

All model-controlled native, plugin and raw MCP direct tool execution through `ToolBatchExecutor` and `ToolBroker`; cancellation, retry semantics, result typing and tests.

### Out of scope

Provider call retries, raw MCP connection handshakes, user-initiated manual resubmissions, rewriting `ToolError` across crates, changing shell permission policy, durable job scheduler retry semantics, new idempotency databases, native provider tool search.

## 6. Required production changes

### Core/domain

Audit `ToolContract.effect_class`, `idempotency`, `retry_policy`, `ToolRetryDecision` and `RetryContext`; document exact conditions permitting replay. Inspect whether the broker already deduplicates a recognized invocation and whether the batch supplies the correct stable submission key.

### Storage and migrations

None expected. Do not silently convert an ambiguous non-idempotent outcome into an idempotent durable success; reuse existing event/result status and artifact lifecycle.

### Protocol and DTOs

No breaking change. Prefer an explicit terminal `UncertainSideEffect` or equivalent actionable status when the remote effect may have occurred; do not mask it as an ordinary network error permitting retry. Preserve model-facing error visibility.

### Runtime and concurrency

Remove the unconditional two-attempt caller-owned retry loop, or make it a thin single-dispatch caller that delegates all eligible retry attempts to `ToolBroker::execute_with_retry` and its *single shared budget*. The default must be one logical attempt when no qualified chain is supplied. If safe read retries are required, construct a reviewed bounded chain from the existing retry owner and retain one authority/attempt lineage rather than wrapping the broker again.

Explicitly distinguish failure **before dispatch** from ambiguous failure **after dispatch**; do not infer absence of side effects merely because the adapter future was cancelled. Bound contention/backoff by absolute deadline. Never re-resolve server generation or tool contract on a replay.

### Frontend/operator surface

Use existing progress/result presentation. Expose the ambiguous-outcome caveat to the agent without telling it the operation certainly failed or inviting automatic retry.

### Security and authorization

Re-use the original grant and independently enforce caller policy, effect ceiling, workspace identity and cancellation at every legitimate retry. No MCP annotations or model-supplied `retryable` field can override host-assigned idempotency.

### Documentation and static guards

Update `architecture/tool_broker.md`, `architecture/tool.md` and `architecture/mcp.md`. Extend `scripts/check_mcp_broker_adapter.py` or add a targeted `check_tool_retry_admission.py` detecting retry loops around `ToolBroker::execute` not authorized by a contract. Keep tests semantic rather than relying on a purely syntactic guard.

## 7. Ordered work packages

### Work package A — Caller/broker retry inventory

Intent: find every retry wrapper outside the broker that may invoke a tool.

Required changes: trace `tool_batch`, `ToolBroker`, `ToolProgram`, TaskTool and plugin adapters, recording effect class, acknowledgement ownership, call-site budgets and submission-key semantics.

Acceptance evidence: a one-page decision table and a failing batch-level test proving duplicate MCP dispatch before correction.

### Work package B — One replay authority

Intent: ensure contract policy and budget determine replay.

Required changes: remove or reconcile the outer retry loop; only explicit safe retries may pass `RetryContext` to the broker. Ensure `NonIdempotent`/`ProcessExec` have no automatic retry after ambiguous dispatch, even on `ToolError::Timeout` or `Network`.

Acceptance evidence: stale-effect/timeout fixtures record exactly one remote invocation; safe-read fixture records bounded attempts and stable key.

### Work package C — Result/timeout/hook parity

Intent: preserve existing usability without concealing uncertainty.

Required changes: pin result status, error display, partial shell output grace, single pre/post hook logical pairing, event/progress ordering, cancellation and backpressure.

Acceptance evidence: test matrix under simultaneous 2–4 tool calls with one hung server.

### Work package D — Static regression + documentation

Intent: ensure a later refactor cannot reintroduce caller-owned retries.

Required changes: static or structural guard plus integration test that starts at accepted model tool call, runs through permission and broker, and counts side effects.

Acceptance evidence: positive guard and deliberate-negative self-test.

## 8. Failure, cancellation, restart and contention semantics

A fake remote writes its side-effect counter and then withholds its response; the caller times out and reports `TimedOut`/`UncertainSideEffect` without issuing a second `tools/call`. Cancellation races and bridge/network failures follow the same non-replay path. Retry-safe reads may replay only if explicitly admitted, inside original total timeout and budget. An interrupted process restart never replays an incomplete non-idempotent model call automatically. Concurrent different tool calls remain admitted under the existing semaphore and preserve call-ID result order. No global mutex may be held across transport waits.

## 9. Compatibility/migration

No schema or configuration migration. Model-visible tool names, approved permission receipts, MCP raw opt-in, plugin contracts and existing TaskTool APIs do not change. Do not opportunistically alter other retry subsystems; if a retry policy must be shared across scheduler/provider, stop and register an architectural change.

## 10. Required tests

- **Focused:** outer-loop-before/after regression, timeout after server-side effect, network/I/O failure, broker `UncertainSideEffect`, safe idempotent failure before dispatch and bounded safe replay, `ProcessExec` timeout, cancellation and deadline expiry.
- **Integration:** agent loop with fake MCP + counting plugin/native tools; same call ID/invocation key, one permission receipt and one terminal result, hook sequence.
- **Restart/recovery:** retained transcript does not trigger implicit re-execution; no duplicate checkpoint/artifact.
- **Contention/cancellation:** slow MCP and shell parallel batch, semaphore permit release, cancellation before/during send.
- **Security:** remote `readOnlyHint` cannot cause unsafe retry; forged retry hints or model arguments cannot expand broker policy.
- **Compatibility:** preserve existing `agent_loop_harness`, MCP, plugin, Tool Program, `tool_broker_integration`, shell timeout grace and static guard behavior.

## 11. Required verification commands

```bash
cargo fmt --all -- --check
cargo test -p codegg --lib -- agent::tool_batch
cargo test -p codegg --lib -- tool::broker
cargo test -p codegg --test tool_broker_integration
cargo test -p codegg --test fake_eggsearch_mcp
cargo test -p codegg --test agent_loop_harness
python3 scripts/check_mcp_broker_adapter.py
# Run the newly implemented retry guard/self-test if created.
cargo clippy --workspace --all-targets --locked -- -D warnings
bash scripts/verify.sh quick
```

Verify test selectors exist before running; any replacements and not-run checks belong in closure.

## 12. Documentation updates

`architecture/tool_broker.md`, `architecture/tool.md`, `architecture/mcp.md`, any retry ownership documentation, and guard documentation. Do not alter historical M001 closure.

## 13. Acceptance criteria

1. A model tool call invoking a non-idempotent MCP write can reach the remote fake server **at most once** if it times out after dispatch. Count transport requests and side effects, not just broker attempts.
2. The same non-replay invariant holds for native non-idempotent/plugin/ProcessExec classifications.
3. Legitimate explicitly admitted idempotent retries stay within a single budget/grant/identity and do not regress.
4. Timeout, cancelled and uncertain statuses are faithfully reported; progress/hook and ordering parity are verified.
5. Focused tests, guard and quick verification pass.

## 14. Stop conditions

Stop and escalate if the only viable fix needs persistent exactly-once semantics across process restarts, remote idempotency keys from uncooperative MCP servers, a second retry service, permission or sandbox changes, or modifying Tool Program retry policy without its owner’s tests.

## 15. Closure evidence

Write `plans/closure/agent-tool-runtime-reconciliation-post-closure-corrective/001-status.md` with a replacement implementation SHA, minimal defect reproduction (transport counter), after-fix matrix, retry-owner decision table, executed commands/results, documented uncertainty/restart limits, and statuses of guards. C003 cannot start before accepted closure.

## 16. Handoff notes

Do not infer remote write absence from a timeout. The broker's legacy `execute()` returns a `BrokerResult` even for typed failures, and the batch currently maps that typed status back to `ToolError`; that conversion is the retry hazard. Avoid changing unrelated provider resilience code. Preserve progressing main-branch work when integrating.
