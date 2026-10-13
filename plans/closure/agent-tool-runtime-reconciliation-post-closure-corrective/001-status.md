# Agent Tool Runtime Post-Closure Corrective C001 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/agent-tool-runtime-reconciliation-post-closure-corrective/001-contract-safe-tool-retry-admission.md`

Source subsystem roadmap: `plans/subsystems/agent-tool-runtime-reconciliation-post-closure-corrective-addendum.md#4-milestones-and-dependencies`

Repository baseline reviewed: `424e7604` (`planning/agent-tool-runtime-reconciliation`)

Implementation commits: `8d13bfd6` — remove batch-owned tool retries, add ambiguous MCP/native dispatch-count regressions, and reconcile retry ownership documentation.

## 1. Executive finding

C001 is closed. `ToolBatchExecutor` now issues one broker dispatch for each accepted model tool call. It no longer wraps `ToolBroker::execute` in a retry loop based on generic `ToolError` classes. Automatic replay remains owned by `ToolBroker::execute_with_retry`, which requires an explicit bounded `RetryContext` and admits retries using the frozen tool contract, acknowledgement class, stable invocation key, cancellation and chain budget. The current AgentLoop tool batch supplies no qualified retry chain, so it intentionally makes one attempt.

The fake MCP trajectory commits its side-effect counter before delaying the acknowledgement past the tool deadline. The broker surfaces the timeout and the batch sends no second transport call. A same-batch native read completes and its tool result retains accepted call order.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result |
|---|---|---|
| Ambiguous non-idempotent MCP dispatch is not replayed | `ambiguous_mcp_timeout_does_not_repeat_transport_dispatch` in `tests/agent_loop_harness.rs`; fake server increments before a 7-second response delay; one-second tool deadline; exactly one transport dispatch | Pass |
| Native non-idempotent timeout is not replayed | `ambiguous_non_idempotent_tool_failure_is_dispatched_once` in `tests/agent_loop_harness.rs` | Pass |
| Broker retry classification preserves safe-read retries and refuses unsafe post-dispatch replay | `cargo test -p codegg --lib -- tool::retry` (11 tests), including `effect_matrix_safe_reads_retry`, `non_idempotent_dispatched_timeout_is_uncertain`, and `raw_shell_ambiguous_is_never_replayed` | Pass |
| Parallel batch result order and independent completion remain intact | MCP timeout plus native read in the same AgentLoop batch; result IDs asserted in original call order; full `agent_loop_harness` target (54 tests) | Pass |
| Broker, MCP and legacy integration compatibility | `tool::broker` (15 tests), `tool_broker_integration` (25 tests), `fake_eggsearch_mcp` (28 tests), and full `agent_loop_harness` (54 tests) | Pass |
| Retry ownership and timeout layering are documented | `architecture/tool_broker.md`, `architecture/tool.md`, `architecture/mcp.md`, `architecture/retry.md` | Pass |

## 3. Production implementation evidence

- Removed the unconditional two-attempt loop and its 500 ms caller-owned backoff from `src/agent/tool_batch.rs`.
- A model call now has one broker invocation and returns the typed result from that invocation. The outer timeout remains a final backstop; tool, broker and batch timeout/grace layering is preserved.
- The implementation does not synthesize a fresh retry chain. If an AgentLoop tool batch later shares the remaining provider-turn budget, that context must be threaded through the logical turn boundary rather than reset for each tool.
- The broker's explicit `execute_with_retry` path and `decide_tool_retry` effect/acknowledgement matrix remain the only automatic tool-retry authority.

## 4. Verification executed

The environment shell defaults to an x86_64 Homebrew Rust toolchain, while this host's working Rust 1.89 toolchain is arm64. Successful Rust commands below used the installed 1.89 cargo and an explicit matching `RUSTC`; this is local macOS arm64 evidence.

- `cargo fmt --all -- --check` — passed.
- `cargo test -p codegg --lib -- agent::tool_batch` — 21 passed.
- `cargo test -p codegg --lib -- tool::broker` — 15 passed.
- `cargo test -p codegg --lib -- tool::retry` — 11 passed.
- `cargo test -p codegg --test tool_broker_integration` — 25 passed.
- `cargo test -p codegg --test fake_eggsearch_mcp` — 28 passed.
- `cargo test -p codegg --test agent_loop_harness` — 54 passed, including both new counted-dispatch trajectories.
- `python3 scripts/check_mcp_broker_adapter.py` — passed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings` — passed.
- `bash scripts/verify.sh quick` — passed, including workspace/all-target check.
- `cargo check -p codegg --lib` and `cargo check -p codegg --test agent_loop_harness` — passed.

An initial attempt through Homebrew Rust 1.98/x86_64 failed at host linking because its objects did not match the installed arm64 native libraries. Re-running with the matching Rust 1.89 arm64 toolchain passed the required executable tests; the toolchain mismatch is not an unresolved product issue.

## 5. Invariant review

- The batch layer no longer decides replay from error type, so `Timeout`, `Network`, `Io`, cancellation, and ambiguous broker outcomes cannot trigger a second dispatch there.
- MCP contracts remain `DirectOnly`, `NonIdempotent`, and retry-disabled. Remote annotations or model arguments do not affect replay policy.
- Legitimate broker retries remain contract- and acknowledgement-qualified, reuse the existing invocation key and grant, and consume the passed shared budget.
- The new MCP trajectory confirms the remote side effect can occur before the acknowledgement stalls while the batch dispatch count remains one.
- Tool output order and concurrent completion remain covered by the same-batch MCP/native fixture and existing AgentLoop harness tests.

## 6. Failure and recovery review

An ambiguous timeout is returned as the broker's typed timeout/uncertain failure and is never converted into a retry by `ToolBatchExecutor`. Cancellation and batch deadline expiry also terminate the one dispatch. There is no automatic replay after process restart; AgentLoop tool calls remain turn-local and no durable retry state was added. Broker timeout grace and shell partial-output behavior remain unchanged.

The regression fixture uses an in-process fake MCP server rather than a network server. Its synchronous handler records the effect and withholds its acknowledgement beyond the per-tool deadline; the AgentLoop batch then returns with one dispatch. The test verifies transport invocation count and call-result order, not remote rollback semantics.

## 7. Migration and compatibility review

No storage, protocol, model schema, or configuration migration was introduced. Tool names, permission receipt handling, approval policy, per-tool timeout, shell output grace, semaphore admission and plugin hook placement are unchanged. AgentLoop model tool calls that previously retried a generic error now surface that error after one attempt; callers requiring automatic retries must use the broker retry API with a qualified chain.

## 8. Security review

The change removes an effect-unsafe replay path across native, plugin, shell and MCP tools without relying on tool-name prefixes or untrusted MCP hints. The broker contract, grant, workspace, cancellation and retry-chain checks remain authoritative on eligible retries. No arguments or secrets are added to diagnostics.

## 9. Documentation and operations

Updated `architecture/tool_broker.md`, `architecture/tool.md`, `architecture/mcp.md`, and `architecture/retry.md` to describe single-dispatch AgentLoop behavior and the explicit broker retry boundary. The retry document now accurately states that the provider turn's remaining chain is not yet threaded into AgentLoop tool batches; it no longer claims shared provider/tool budget use that the implementation did not provide.

## 10. Unresolved findings

None at medium severity or above. A future shared provider/tool retry-chain integration remains unimplemented and is not claimed by this milestone. The current safe default is one tool attempt per AgentLoop call.

## 11. Roadmap disposition

C001 is closed at implementation `8d13bfd6`. C002 remains independently ready and is the next sequential milestone. C003 remains blocked until C002 has an accepted closure. No historical M001–M004 closure record was changed.

## 12. Registry updates

The registry dependency audit found C003 as the only registered plan with C001 as a hard dependency. C003 remains blocked because its other hard dependency, C002, is not closed. C002's dependencies were already satisfied and it remains ready; it is moved to active as the next sequential plan. No other registered plan was unblocked by C001.
