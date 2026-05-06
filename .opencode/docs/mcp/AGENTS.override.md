# MCP Module Override

This file contains MCP-specific guidance and overrides root AGENTS.md.

## MCP Connection Manager

The `McpConnectionManager` in `src/mcp/remote.rs` handles:

- Automatic reconnection with exponential backoff (1s-60s, max 5 retries)
- Heartbeat every 30s to keep connection alive
- State transitions: Connected, Disconnected, Reconnecting