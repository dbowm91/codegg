# Agent Loop Completion Reliability C004 — An MCP Tool Error Is Not a Successful Result

Status: closed
Repository baseline: `d585e75f895cf41cd2241512d8d7e7c37fcece99`

Source roadmap: `plans/subsystems/agent-loop-completion-reliability-corrective-addendum.md#c004--an-mcp-tool-error-is-not-a-successful-result`
Long-term requirements: `plans/000-long-term-specification.md#26-reliability-and-recovery`; `plans/000-long-term-specification.md#28-observability`
Applicable ADRs: none new. The structured MCP result contract already exists. This plan does not add a protocol variant.
Primary class: invariant

## 1. Objective

When an MCP server returns `isError: true`, the agent loop does not insert that call into the model transcript as a successful tool result.

## 2. Why this milestone is ready

The structured result type already carries `is_error`. The defect is one caller that drops the flag and then marks the text successful. No open milestone owns the `mcp__` batch path. C001–C003 do not.

## 3. Current implementation evidence

- `McpToolCallResult` retains `is_error` (`src/mcp/mod.rs`). `McpService::call_tool` returns only `.text` and drops the flag. `call_tool_structured` keeps it.
- The `mcp__` batch path calls `call_tool` and, on `Ok(result)`, returns `ToolExecutionOutcome::success(result)` (`src/agent/tool_batch.rs`). A transport `Err` is marked `ToolError`. A protocol-level tool error that arrives as `Ok` is not.
- The modern local fixture returns `isError: true` with text `"done"` and asserts `result.is_error` (`src/mcp/local.rs`). That fixture does not cover this batch caller. A focused library test of the envelope passed; it does not exercise `tool_batch`.
- Native registered tools go through `ToolBroker`. This `mcp__` route calls `McpService` directly. The success mislabel is the dropped flag, not the missing broker hop. Do not migrate the route onto the broker in this milestone.
- The loop publishes the outcome's success bit (`src/agent/loop.rs`, the tool-result event next to the batch). A successful `"done"` is what the model continues from.

## 4. Invariants that must not regress

- JSON-RPC and transport failures stay `Err`. Tool-level `isError` stays an `Ok` result whose flag is visible. Do not collapse those two into one error type.
- Structured wrappers that already preserve `isError` as `success = false` stay as they are. This milestone must not weaken them to match the batch path.
- The 60-second MCP call timeout stays. Do not raise it, and do not add a second timeout.
- `ToolBroker` remains the boundary for native registered tools. This milestone does not move `mcp__` onto it.
- A genuine `isError: false` text result still becomes a successful tool result.

## 5. Scope

In:

- The `mcp__` batch path reads `is_error`. `is_error: true` is `ToolExecutionStatus::ToolError`, or the existing non-success status the loop already treats as a failed tool. The model text must not be a bare success. Keep the server text, prefixed so it cannot be read as success, and do not grow it past the batch's existing output cap.
- A test drives this path with `isError: true` and text `done` and asserts the outcome is not success.
- The session tool-result event for that call is not success.

Out:

- Routing `mcp__` through `ToolBroker`, permission, or the writer fence.
- A new MCP parser, a new retry, or a change to the 60-second timeout.
- Search-wrapper `isError` handling. Those closures already require `success = false`. Do not reopen them.
- Compaction, provider retry, and the execution-limit publication. Those are other milestones or closed lines.

## 6. Required production changes

- `src/agent/tool_batch.rs`: the `mcp__` call arm. Call the structured API, or otherwise read `is_error` before marking success.
- `src/mcp/mod.rs` only if `call_tool` cannot retain the flag without breaking its text-only contract. Prefer the structured call at the batch site. Do not change every text-only caller.
- Docs: one sentence in `architecture/agent.md` or the MCP note that already describes tool results, stating that `mcp__` tool-level `isError` is a failed tool result. If no such sentence exists, add it to the agent tool-result note rather than inventing a new document.

## 7. Ordered work packages

### WP-A — Read the flag

Intent: the batch arm cannot mark an error result successful.

Required changes: use `call_tool_structured` (or an equivalent that returns `is_error`) inside the existing 60-second timeout. `is_error: true` takes the same non-success status arm as `Ok(Err(...))`, with the server text retained and marked as an error. `is_error: false` stays `success`.

Acceptance evidence: the local fixture shape (`isError: true`, text `done`) produces a non-success outcome whose model text contains `done` and is not a bare success.

### WP-B — The session event follows the outcome

Intent: the model and the session see the same failure.

Required changes: no new event. The existing tool-result publication must take its success bit from the outcome WP-A returns. If that publication hard-codes success for every `mcp__` `Ok`, that hard-code is the bug. Do not add a second event.

Acceptance evidence: the published tool result for the fixture is not success.

## 8. Failure, cancellation, restart, contention semantics

- A timeout remains the existing timeout outcome. It is not relabeled as `isError`.
- Cancellation of the batch remains the existing cancel path.
- No new lock. The call still uses the existing `try_read` retry (three attempts, 50ms backoff). Do not add a fourth.
- Restart: no durable migration. A turn already recorded as success is not rewritten.

## 9. Compatibility and migration

No schema change. Old transcripts are left as recorded. New calls preserve the server text so a model that keys off the text still sees it, but the status is a failure.

## 10. Required tests

- One focused test of the `mcp__` batch arm, not a new integration executable. It must fail on the baseline: `isError: true` with text `done` is `ToolExecutionOutcome::success`.
- The existing modern-envelope library test must still pass. Do not delete the fixture's `is_error` assertion.
- No live MCP server. No workspace sweep as the proof.

## 11. Required verification commands

```bash
cargo test -p codegg --lib modern_probe_preserves_protocol_tool_metadata_and_result_envelope
```

Plus the focused batch test named in the implementation, selected by its full member path. Do not use a `--test` filter that matches zero tests and exits 0.

`scripts/verify.sh quick` is the merge bar for the implementation turn, not for this planning turn.

## 12. Documentation updates

One sentence, as in §6. No roadmap rewrite beyond this addendum's status table, which the planning turn owns.

## 13. Acceptance criteria

- An MCP `isError: true` result is not a successful tool result in the model transcript or the session tool event.
- An MCP `isError: false` result is still success.
- Transport errors and the 60-second timeout are unchanged.
- No `ToolBroker` migration, no new protocol variant, no timeout change.

## 14. Stop conditions

- `call_tool_structured` does not populate `is_error` for this transport. Stop. Do not invent a second parser.
- Making the status honest requires a new session-event variant. Stop. Do not add one.
- The only way to read the flag is to move `mcp__` onto `ToolBroker`. Stop. That is a different milestone.

## 15. Closure evidence required

- The focused test, shown failing on the baseline behavior and passing after the change.
- The existing envelope test, still passing.
- A citation of the batch arm that now reads `is_error`.
- An explicit statement that the broker migration was not done.

## 16. Handoff notes

The parent confirmed the batch arm and `call_tool` against the baseline after the tool-failure audit. The audit's other rows are not this milestone: the unbounded approval wait is the documented default, and the unreproduced storage-await stall is not a plan.
