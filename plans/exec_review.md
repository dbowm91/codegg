# exec Architecture Review

## Summary
The exec.md architecture document is mostly accurate but contains one significant inaccuracy: the doc claims "if the question tool is used, it will timeout after 300 seconds" (line 169), but the actual code at `src/exec.rs:121` only calls `setup_question_channel()` without any timeout handling. The 300-second timeout exists elsewhere in the agent loop but is not documented as being applied here.

## Verified Correct
- `ExecInput` struct at `src/exec.rs:10-16` — matches doc lines 20-27
- `ExecOutput` struct at `src/exec.rs:18-28` — matches doc lines 30-43
- `ExecMode` struct and `new()`, `run()`, `print_output()`, `exit_code()` methods — all match documented behavior
- `classify_error()` function at `src/exec.rs:189-259` — covers all error codes in doc table (lines 125-154)
- Error code mapping table in doc (lines 125-154) matches `classify_error()` implementation exactly
- Session ID generation at `src/exec.rs:119` — doc line 166 correct ("If session_id is provided via ExecMode::new(), it will be used. Otherwise, a new UUID is generated")
- Config loading error handling at `src/exec.rs:83` — doc line 172 correct ("errors are properly returned as CONFIG_ERROR")
- MCP service hardcoded to `None` at `src/exec.rs:107` — doc line 175 correct
- Exit code 0 for success, 1 for failure at `src/exec.rs:277-283` — matches doc lines 158-161

## Discrepancies Found
- **Question channel timeout claim**: Doc line 169 states "If the question tool is used, it will timeout after 300 seconds (same as interactive mode)". The `setup_question_channel()` at `src/exec.rs:121` only creates the channel endpoints; it does not set any timeout. The 300-second timeout at `src/agent/loop.rs:1859` is applied in the agent's `run()` method when processing pending questions, but this behavior is not specific to exec mode — it's part of the general agent loop. The doc implies exec mode has explicit question timeout handling, which it does not.

## Bugs Identified
- No functional bugs found — exec mode works as documented for the error classification, input/output, and execution flow.

## Improvement Suggestions
- Clarify doc line 169: the question tool timeout is inherited from the agent loop's general processing, not a special exec-mode configuration. Consider rephrasing: "Question tool handling is delegated to the AgentLoop; pending questions timeout at the same 300-second default as interactive mode."
- The agent-loop skill guide may be a more appropriate home for the 300-second timeout detail than the exec module doc.

## Stale Items in Architecture Doc
- Line 175: "Currently mcp_service is hardcoded to None" — while true, this limitation could be documented as a known limitation rather than a neutral statement, if not already tracked as a feature gap.
