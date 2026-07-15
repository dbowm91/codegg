# Troubleshooting Guide

Common issues and solutions for codegg.

## Daemon Issues

### Daemon already running

**Symptoms:** Second `codegg` invocation connects to the existing daemon instead of starting a new one.

**This is expected behavior.** Codegg runs exactly one user-scoped daemon per OS user (singleton invariant). The second invocation connects to the running daemon via `connect_or_start_daemon`.

**Solutions:**
1. Use `codegg daemon status` to see the active daemon's identity, PID, generation, and uptime.
2. Use `codegg daemon stop` to stop the running daemon if needed.
3. Use `--standalone` to bypass the daemon and run an in-process core (non-production mode).

### Stale socket after crash

**Symptoms:** Daemon refuses to start; lock file exists but socket is unreachable.

**Solutions:**
1. A new daemon start automatically removes stale sockets when the lock is free — try starting again.
2. If the lock is still held by a dead process, check `codegg daemon status` for the PID and verify the process is alive (`ps -p <pid>`).
3. Manually remove the lock file only if you are sure no daemon is running: `rm ~/.config/codegg/daemon.lock` (or the equivalent path for your OS).

### Daemon not reachable

**Symptoms:** `codegg` reports it cannot connect to the daemon.

**Solutions:**
1. Run `codegg daemon status` to check if the daemon is running.
2. Check `CODEGG_DAEMON_HOME` if you have overridden the daemon path.
3. Verify the lock file is not held by a dead process: `ls -la <daemon-home>/daemon.lock`.
4. Try `codegg daemon start` to start a fresh daemon.

### Server mode refuses to start

**Symptoms:** `codegg server` exits with an error about `--standalone-core`.

**Explanation:** In Phase 1, the HTTP server requires `--standalone-core` to construct its own core. Without it, the server would silently create a second daemon that defeats the singleton invariant.

**Solutions:**
1. Use `codegg server --standalone-core --host 127.0.0.1 --port 8080`.
2. Daemon-proxying server mode (where the server forwards to the singleton daemon) is planned for a later phase.

## Session Issues

### Session not responding

**Symptoms:** Input is sent but no response, spinner keeps spinning.

**Solutions:**
1. Check API key is valid: `OPENAI_API_KEY` or `ANTHROPIC_API_KEY`
2. Check network connectivity
3. Press `Ctrl+C` to interrupt and try again
4. Try `/model` to switch to a different model

### Context window full

**Symptoms:** Agent says context is full, cannot continue conversation.

**Solutions:**
1. Start a new session with `/new`
2. The system has auto-compaction enabled (if configured)
3. Use `/compact` to manually trigger compaction

### Session history lost

**Symptoms:** Previous messages disappear after restart.

**Solutions:**
- Sessions are stored in SQLite at `~/.local/share/codegg/sessions.db`
- Check file permissions
- Verify disk space available

## Permission Issues

### Permission dialog not appearing

**Symptoms:** Tool call hangs, no permission prompt.

**Solutions:**
1. Check TUI is running in foreground (not daemon mode)
2. Try pressing `Esc` to cancel, then retry
3. Check `~/.config/codegg/permissions.json` is writable
4. Restart the application

### Permission always denied

**Symptoms:** Even with "Always Allow", tools are denied.

**Solutions:**
1. Check HMAC key is consistent: `CODEGG_PERM_KEY` env var
2. Clear permissions: delete `~/.config/codegg/permissions.json`
3. Check path rules in config - ensure paths are correctly specified

## LSP Issues

### Diagnostics not showing

**Symptoms:** No error highlighting in editor.

**Solutions:**
1. Ensure `lsp_tool: true` in experimental config
2. Check language server is installed: `rust-analyzer`, `pyright`, etc.
3. Run codegg from project directory (LSP needs project root)
4. Check server logs with `RUST_LOG=debug`

### LSP server won't start

**Symptoms:** "Failed to launch language server"

**Solutions:**
1. Install server manually: `npm install -g typescript-language-server`
2. Check server binary is in PATH
3. Try explicit server path in config
4. Check disk space (server download may have failed)

## MCP Issues

### MCP server not connecting

**Symptoms:** `mcp__server__tool` calls fail immediately.

**Solutions:**
1. Verify server type (local vs remote) in config
2. For local: check command and args are correct
3. For remote: verify URL is accessible
4. Check server logs for startup errors
5. Try with `RUST_LOG=debug` for detailed logs

### Tools not appearing

**Symptoms:** MCP server connected but no tools available.

**Solutions:**
1. Restart after adding MCP server to config
2. Check server implementation supports tool discovery
3. Verify `handle_tool_list_changed` permissions

### OAuth not working

**Symptoms:** Remote MCP server returns auth errors.

**Solutions:**
1. Verify OAuth token is fresh
2. Check `Authorization` header format
3. Ensure token has required scopes
4. Try regenerating OAuth token

## Plugin Issues

### Plugin not loading

**Symptoms:** Plugin hook never called.

**Solutions:**
1. Check `manifest.toml` exists and is valid
2. Verify WASM file is named correctly (`plugin.wasm`)
3. Ensure plugin API version matches codegg version
4. Check plugin has required function exports
5. Review logs for WASM loading errors

### Plugin fuel exhausted

**Symptoms:** Plugin stops responding.

**Solutions:**
1. Reduce plugin complexity
2. Check for infinite loops in plugin code
3. Fuel resets every 60 seconds automatically
4. Consider splitting into smaller plugins

## Performance Issues

### Slow responses

**Symptoms:** Everything works but feels sluggish.

**Solutions:**
1. Check network latency to API endpoint
2. Reduce model for faster responses
3. Disable LSP if not needed
4. Reduce context window size

### High memory usage

**Symptoms:** Memory keeps growing.

**Solutions:**
1. Restart session periodically
2. Reduce `max_tokens` in compaction config
3. Limit concurrent subagents via config
4. Check for memory leaks with `RUST_LOG=debug`

## Configuration Problems

### Config not loading

**Symptoms:** Default values always used.

**Solutions:**
1. Check config file location: `~/.config/codegg/config.json`
2. Verify JSON is valid (use `jq` to validate)
3. Ensure file is readable
4. Check for duplicate/conflicting settings

### Model not found

**Symptoms:** "Model not found" error.

**Solutions:**
1. Check model name is correct (case-sensitive)
2. Verify API key has access to model
3. Try explicit provider: `provider/model` format
4. List available models with `/model` command

## Crash Issues

### Panic on startup

**Symptoms:** Crashes immediately after start.

**Solutions:**
1. Run with `RUST_BACKTRACE=1` for full trace
2. Check database is not corrupted
3. Verify all required directories exist
4. Try deleting `~/.cache/codegg`

### Crash during tool execution

**Symptoms:** Crashes when running specific tool.

**Solutions:**
1. Check tool-specific error output
2. Verify file/directory permissions
3. Check disk space
4. Try the tool in isolation

## Keyboard Shortcuts Not Working

**Symptoms:** Shortcuts don't respond.

**Solutions:**
1. Ensure keyboard focus is on main window
2. Check terminal supports special keys
3. Try alternative shortcuts (e.g., `Esc` instead of `Ctrl+Q`)
4. Restart with clean terminal state

## Debug Mode

Enable detailed logging:

```bash
# Basic debug
RUST_LOG=debug codegg

# Very verbose
RUST_LOG=trace codegg -vvv

# Feature-gated debug-logging (tracing-based, no file output by default)
cargo run --features debug-logging
```

## Getting Help

1. Check existing issues at https://github.com/anomalyco/codegg/issues
2. Enable debug logging and capture output
3. Note your platform (`uname -a`)
4. Include config (redact API keys)
