# exec Architecture Review Findings

## Verified Claims

- **Location**: `src/exec.rs` - CORRECT (single file)
- **ExecInput struct**: `prompt`, `model`, `agent` with `camelCase` serde - CORRECT (exec.rs:10-16)
- **ExecOutput struct**: `success`, `result`, `tools_used`, `tokens_used`, `duration_ms`, `error`, `code` with `camelCase` serde - CORRECT (exec.rs:18-28)
- **ExecMode struct**: `quiet`, `json_output`, `session_id` - CORRECT (exec.rs:61-65)
- **ExecMode::new()**: Takes `quiet`, `json_output`, `session_id` - CORRECT (exec.rs:68-74)
- **ExecMode::run()**: Takes `ExecInput`, returns `Result<ExecOutput, AppError>` - CORRECT (exec.rs:76)
- **Session ID handling**: Uses provided `session_id` or generates UUID - CORRECT (exec.rs:119)
- **Question channel setup**: `loop_instance.setup_question_channel()` called - CORRECT (exec.rs:121)
- **Config loading**: `Config::load()` with errors as CONFIG_ERROR - CORRECT (exec.rs:83)
- **MCP service hardcoded to None**: Line 107 - CORRECT (documented limitation)
- **classify_error function**: Maps AppError to error codes - CORRECT (exec.rs:189-259)
- **All error codes documented**: PERMISSION_ERROR, AUTH_ERROR, RATE_LIMIT, TIMEOUT, MODEL_NOT_FOUND, CIRCUIT_OPEN, API_ERROR, STREAM_ERROR, PROVIDER_NOT_FOUND, IO_ERROR, CONFIG_ERROR, STORAGE_ERROR, TOOL_NOT_FOUND, TOOL_TIMEOUT, TOOL_PERMISSION, TOOL_DISABLED, TOOL_ERROR, MCP_ERROR, LSP_ERROR, PLUGIN_ERROR, AGENT_ERROR, JSON_ERROR, HTTP_ERROR, EXECUTION_ERROR, WORKTREE_ERROR, UPGRADE_ERROR, CLIPBOARD_ERROR, TUI_ERROR - ALL PRESENT (exec.rs:189-259)
- **Default agent**: "build" - CORRECT (exec.rs:99)
- **print_output method**: Handles both json_output and text modes - CORRECT (exec.rs:261-275)
- **exit_code function**: Returns 0 for success, 1 for failure - CORRECT (exec.rs:277-283)
- **ExecOutput::success() and ExecOutput::error()**: Helper constructors - CORRECT (exec.rs:30-58)
- **AgentLoop usage**: Exec mode uses AgentLoop directly (no TUI) - CORRECT (verified)

## Stale Information

- **No stale information found**: All documented behavior verified

## Bugs Found

- **No bugs found**: Implementation matches documentation

## Improvements Suggested

- **Documentation clarification**: The flow diagram at lines 47-71 shows execution flow but the "loop_instance.setup_question_channel()" note at line 169 could be clearer about what it does. Currently accurate but brief.
- **MCP note prominent**: Document should highlight more prominently that MCP is NOT available in exec mode (line 175) since this is a significant limitation. Consider moving to a "Limitations" section at top.

## Cross-Module Issues

- **AgentLoop dependency**: exec module directly instantiates and runs AgentLoop (line 109-117)
- **ProviderRegistry dependency**: Uses provider system for model resolution (lines 84-96)
- **ToolRegistry dependency**: Uses default tool registry (line 105)
- **PermissionChecker dependency**: Creates PermissionChecker for exec context (line 104)
- **Config dependency**: Loads config for provider/agent resolution (line 83)
- **EventProcessor usage**: Processes ChatEvent stream to extract text and tool usage (lines 142-160)
- **Question handling**: Question channel setup enables question tool (line 121) - note questions timeout behavior is "inherited from AgentLoop's general processing" per doc