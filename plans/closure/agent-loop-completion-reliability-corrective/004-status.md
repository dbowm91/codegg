# Agent Loop Completion Reliability C004 — Closure Status

Status: closed

Source implementation plan: `plans/implementation/agent-loop-completion-reliability-corrective/004-mcp-tool-error-is-not-a-successful-result.md`

Source subsystem roadmap: `plans/subsystems/agent-loop-completion-reliability-corrective-addendum.md#c004--an-mcp-tool-error-is-not-a-successful-result`

Repository baseline reviewed: `d585e75f895cf41cd2241512d8d7e7c37fcece99`

Implementation state: uncommitted working-tree changes in this worktree. This record
does not claim a commit or a verified commit head.

## 1. Executive finding

The MCP batch arm no longer drops the protocol-level error flag. It calls
`call_tool_structured` inside the existing 60-second timeout and classifies the result
through `mcp_tool_outcome`: `isError: true` is `ToolError` with the server text
retained and prefixed, `isError: false` is still success.

## 2. Requirement-to-evidence matrix

| Requirement | Evidence | Result | Notes |
|---|---|---|---|
| An `isError: true` result is not success | `an_mcp_error_result_is_not_a_successful_tool_result` | pass | Status is `ToolError`; the text contains the server text and does not read as a bare success. |
| An `isError: false` result is still success | same test | pass | Status `Success`, text unchanged. |
| The session tool event follows the outcome | `AppEvent::ToolResult.success` derives from `tool_outcome_is_success` | pass (source) | No second event was added. |
| Transport errors and the timeout are unchanged | source: the `Ok(Err(e))` and `Err(_)` arms are untouched | pass | `mcp_timeout` is still 60 seconds. |
| No broker migration | source: the arm still calls `McpService` directly | pass | `ToolBroker` is untouched. |

## 3. Production implementation evidence

- `src/agent/tool_batch.rs` `mcp_tool_outcome` is the single classifier for the arm.
- The arm passes `mcp.call_tool_structured(server, tool, arguments)` through the
  existing `tokio::time::timeout(mcp_timeout, ...)` and the existing three-attempt
  `try_read` retry.
- The text is not grown: the outcome goes through the batch's existing projection and
  output bounds.

## 4. Verification executed

```bash
cargo test -p codegg --lib agent::tool_batch               # 39 passed
cargo test -p codegg --lib modern_probe_preserves_protocol_tool_metadata_and_result_envelope  # 1 passed
cargo test -p codegg --lib agent::                         # 538 passed, 0 failed
scripts/verify.sh quick                                    # passed
```

## 5. Invariant review

- JSON-RPC and transport failures remain `Err`; a tool-level `isError` remains an
  `Ok` result whose flag is consumed. The two were not collapsed.
- Structured wrappers that already require `success = false` were not weakened.
- The 60-second timeout and the three-attempt lock retry are unchanged.
- A genuine `isError: false` text result is still a successful tool result.

## 6. Failure and recovery review

A timeout still reports the existing timeout outcome and is not relabeled as
`isError`. Batch cancellation is unchanged. No new lock was added; the arm still
uses the existing retry budget rather than a fourth attempt.

## 7. Migration and compatibility review

No schema change. Old transcripts are left as recorded; new calls preserve the server
text so a model that keys off it still sees it, with a failure status.

## 8. Security review

No permission, sandbox, or fence change. The call still goes through the same MCP
service and timeout; nothing was routed around the broker because nothing native was
involved.

## 9. Documentation and operations

The agent guide's tool-result note states that an MCP tool-level `isError` is a failed
tool result. No new document was created.

## 10. Unresolved findings

- The focused test exercises the classifier, not a live MCP transport. The existing
  local envelope fixture (
  `modern_probe_preserves_protocol_tool_metadata_and_result_envelope`) still passes and proves
  the protocol retains `is_error`; a transport-level test would need a real stdio
  MCP server.
- Search-wrapper `isError` handling was not reopened.

## 11. Roadmap disposition

C004 is closed.

## 12. Registry updates

The addendum and the registry row for C004 are set to closed.
