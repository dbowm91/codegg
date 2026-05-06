# Tool Module Override

This file contains tool-specific guidance and overrides root AGENTS.md.

## Async Command Pattern

Commands that need async operations should use the `TuiCommand` pattern:

1. Add variant to `TuiCommand` enum in `src/tui/app/mod.rs`
2. Add async handler in `src/tui/mod.rs` (e.g., `handle_your_command`)
3. Add match arm in `run_event_loop` to route to handler
4. From sync handlers, use `tui_cmd_tx.try_send(TuiCommand::YourCommand { ... })`

## Tool Path Validation

Always use `validate_path()` and `check_path_for_symlinks()` from `src/tool/util.rs` before performing filesystem operations.

## Tool Search (On-Demand Tool Discovery)

The `tool_search` tool (`src/tool/tool_search.rs`) enables on-demand tool discovery:
- Allows LLM to search for tools by name or description
- Uses `ToolCatalog` for search capabilities
- Tool can be used to discover available tools based on context

### Usage
- LLM can call `tool_search` with a query to find relevant tools
- Returns tool names, descriptions, and parameters
- Use this for on-demand tool loading scenarios