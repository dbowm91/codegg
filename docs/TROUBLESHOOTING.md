# Troubleshooting Guide

Common issues and solutions for codegg.

## Daemon Issues

### Daemon already running

**Symptoms:** Second `codegg` invocation connects to the existing daemon instead of starting a new one.

**This is expected behavior.** Codegg runs exactly one user-scoped daemon per OS user (singleton invariant). The second invocation connects to the running daemon via `connect_or_start_daemon`.

The ordinary `codegg` frontend is not the daemon's lifetime owner. If it autostarts the daemon, the daemon is detached from the initiating Unix terminal/process group and remains available to later clients. Startup requires a bounded handshake and live identity probe; diagnostics are written to the user-scoped `daemon.log` shown by `codegg daemon logs`.

**Solutions:**
1. Use `codegg daemon status` to see the active daemon's identity, PID, generation, and uptime.
2. Use `codegg daemon stop` to stop the running daemon if needed.
3. Use `--standalone` to bypass the daemon and run an in-process core (non-production mode).

### Stale socket after crash

**Symptoms:** Daemon refuses to start; the socket is unreachable.

**Solutions:**
1. Try `codegg daemon start` again. A stale socket is removed when no process holds the daemon lock.
2. Run `codegg daemon status` and `codegg daemon logs` to diagnose the active process.
3. Do not delete `daemon.lock` manually. It is an OS lock held by the daemon process and is released when that process exits.

### Daemon not reachable

**Symptoms:** `codegg` reports it cannot connect to the daemon.

**Solutions:**
1. Run `codegg daemon status` to check if the daemon is running.
2. Check `CODEGG_DAEMON_HOME` if you have overridden the daemon path.
3. Verify the lock file is not held by a dead process: `ls -la <daemon-home>/daemon.lock`.
4. Try `codegg daemon start` to start a fresh daemon.
5. If startup times out, run `codegg daemon logs` and inspect the reported user-scoped log path. A non-responsive or incompatible socket is not treated as a ready daemon.

### Endpoint overrides

Use `--endpoint /path/to/core.sock` (Unix) or a `npipe://<name>` endpoint
(Windows) with daemon commands or `codegg daemon attach --endpoint`. (The
top-level `codegg attach` is a different, feature-gated remote HTTP command that
takes a server URL, not a socket; `codegg attach-daemon` is a hidden deprecated
alias of `codegg daemon attach`.) The explicit CLI endpoint takes precedence
over `CODEGG_CORE_ENDPOINT`; without either, CodeGG uses the platform default
under the user-scoped daemon home. All forms use the same lock and metadata root.
Windows named-pipe operation remains best-effort until live Windows qualification
is recorded.

### Graceful daemon stop

`codegg daemon stop` verifies the live daemon identity before sending SIGTERM on Unix. SIGTERM and SIGINT follow the same graceful shutdown path. The configured `shutdown_timeout_ms` limits only local-client draining; after cleanup, the endpoint, PID file, metadata, and singleton lock are released so a subsequent daemon can start immediately. A timeout is reported without force-killing an unverified PID. Windows daemon stop is not yet available pending lifecycle qualification.

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
1. Check the selected provider's credential with `codegg auth status` and `codegg providers`.
2. Check network connectivity to the provider.
3. Press `Ctrl+C` to interrupt, then try again.
4. Use `/model` to switch models.

### Context window full

**Symptoms:** Agent says context is full, cannot continue conversation.

**Solutions:**
1. Start a new session with `/new`
2. The system has auto-compaction enabled (if configured)
3. Use `/compact` to manually trigger compaction

### Session history lost

**Symptoms:** Previous messages disappear after restart.

**Solutions:**
- Session messages live in a project-local SQLite database at `<project>/.codegg/sessions.db`; the user-scoped `<data dir>/codegg/codegg.db` is the daemon *project catalog*, not session history
- Check file permissions
- Verify disk space available

## Permission Issues

### Permission dialog not appearing

**Symptoms:** Tool call hangs, no permission prompt.

**Solutions:**
1. Run `/doctor` to confirm the daemon is reachable. Permission requests are registered with the daemon, so the prompt cannot appear if the client cannot reach it.
2. Try pressing `Esc` to cancel, then retry
3. Check that `<config dir>/codegg/permissions.json` is writable (`~/Library/Application Support/codegg/permissions.json` on macOS, `~/.config/codegg/permissions.json` on Linux).
4. Restart the application

### Approval accepted but the tool never runs

**Symptoms:** You answer the approval prompt, the dialog closes, and the turn
then sits there indefinitely. In earlier builds the reply could be dropped by
the daemon and the human wait, which was unbounded by default, never woke up.

**What to look for:** a red toast reading `Approval was not delivered
(<code>): <detail>`. That means the reply reached the daemon but was refused;
the code tells you why (`invalid_permission_id` = the request was no longer
pending, usually because it already expired or was answered elsewhere).

**Solutions:**
1. Re-run the turn. A stale approval is not reusable.
2. If the daemon was restarted mid-turn, pending approvals do not survive it
3. Run `/doctor` to confirm the daemon is reachable

### Approval keys

The permission box accepts direct keys as well as arrow/`j`/`k` + `Enter`:

| Key | Effect |
|---|---|
| `y` | Allow once |
| `p` | Always allow (asks for a second confirming keypress) |
| `n` | Deny once |
| `1`–`4` | Same four rows by position; answer without pressing Enter |
| `↑↓`/`j`/`k`, `Enter` | Navigate / commit the highlighted option |
| `Esc` | Cancel, or back out of the confirm step |

A standing grant ("Always Allow" / "Always Deny" / `p`) always requires the
second `y`, so no single keypress creates a persistent permission.

### Permission always denied

**Symptoms:** Even with "Always Allow", tools are denied.

**Solutions:**
1. Check HMAC key is consistent: `CODEGG_PERM_KEY` env var
2. To reset saved grants, stop CodeGG, back up `permissions.json`, then remove only that file. This clears all saved permissions.
3. Check path rules in config and confirm the paths are correct.

## LSP Issues

### Diagnostics not showing

**Symptoms:** No error highlighting in editor.

**Solutions:**
1. Ensure `lsp_tool: true` in experimental config
2. Check language server is installed and on `PATH`: `rust-analyzer`, `pyright-langserver`, `typescript-language-server`, `gopls`, `clangd`, etc. (`pyright` is the server id, not the executable.)
3. Run codegg from project directory (LSP needs project root)
4. Check server logs with `RUST_LOG=debug`

### LSP server won't start

**Symptoms:** `server launch failed: ...` or `server not found: ...`

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
1. Check a supported manifest exists and is valid (`manifest.toml`,
   `plugin.json`, or `.claude-plugin/plugin.json`)
2. Verify the WASM module path the manifest points at resolves to a file
3. Ensure plugin API version matches codegg version
4. Check plugin has required function exports
5. Review logs for WASM loading errors

### Plugin fuel exhausted

**Symptoms:** Plugin stops responding.

**Solutions:**
1. Reduce plugin complexity
2. Check for infinite loops in plugin code
3. Fuel is a one-shot per-plugin budget (`MAX_PLUGIN_FUEL_BUDGET`, 10M instructions), not replenished on a timer. A plugin that has used its budget stays exhausted until the process restarts.
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
1. Check config file location: the global config is `<config dir>/codegg/codegg.jsonc` (`~/Library/Application Support/codegg/codegg.jsonc` on macOS, `~/.config/codegg/codegg.jsonc` on Linux); `codegg.json` and `config.json` in the same directory are also accepted, and `<project>/.codegg/codegg.json` overrides it
2. Validate the config with `codegg validate --config <path>`.
3. Ensure the file is readable.
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
1. Run with `RUST_BACKTRACE=1` for a backtrace.
2. Run `codegg doctor` and inspect `codegg daemon logs` for startup errors.
3. Back up relevant data before attempting database repair. Do not delete the whole CodeGG data or config directory; it may contain credentials or encryption keys.

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
3. In the main TUI, press `q` or `Ctrl+C` twice within three seconds to quit. The first press arms the exit and shows which key to press again. `Esc` does not quit.
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

1. Check existing issues at https://github.com/dbowm91/codegg/issues
2. Enable debug logging and capture output
3. Note your platform (`uname -a`)
4. Include config (redact API keys)

## Source verification

Checked 2026-10-23 against the source tree.

- Permission dialog keys (`y`, `p`, `n`, `1`–`4`, `j`/`k`, `Esc`) and the
  second-confirmation requirement for a standing grant:
  `src/tui/components/dialogs/permission.rs`.
- The `Approval was not delivered (<code>): <detail>` toast:
  `src/tui/app/mod.rs`.
- Permissions store path `<config dir>/codegg/permissions.json`:
  `src/permission/mod.rs`.
- `CODEGG_PERM_KEY` signature key: `src/permission/mod.rs`.
- Plugin fuel ceiling of 10,000,000 instructions:
  `src/plugin/runtime/wasm_cache.rs`.
- `compaction.max_tokens` and `subagent.max_concurrent` config keys:
  `crates/codegg-config/src/schema.rs`.
- Project catalog path `<data dir>/codegg/codegg.db`:
  `crates/codegg-core/src/storage/paths.rs` and
  `crates/codegg-core/src/storage/mod.rs`.
- `experimental.lsp_tool` gate: `crates/codegg-config/src/schema.rs`.
- Slash commands referenced here (`/model`, `/new`, `/compact`, `/doctor`,
  `/keybinds`, `/cost`, `/usage`, `/logs`, `/mcps`, `/files`, `/tasks`,
  `/diff`, `/plan`, `/init`, `/security-review`, `/review`,
  `/theme`, `/lsp-preview-apply`, `/skills`, `/reload`): `src/tui/command.rs`.
- The "permission dialog not appearing" checklist: the daemon owns the
  permission registry and the TUI is its client, so the actionable first step
  is daemon reachability (`/doctor`), not a foreground/background mode choice.
  The previous "not daemon mode" advice was stale and was replaced.

- Corrected 2026-10-27: `/project-init` does not exist; the command is
  `/init` (`src/tui/command.rs:579`). Plugin troubleshooting names the accepted
  manifest formats (`src/plugin/package.rs:73-95`) instead of a fixed
  `plugin.wasm` name.
- Corrected: daemon locks are OS-managed advisory locks, not stale marker files
  to delete (`src/core/instance.rs`). The startup recovery advice now uses the
  daemon commands.
- Corrected: do not recommend deleting user data directories because they may
  contain credentials or encryption keys (`crates/codegg-providers/src/auth_types.rs`,
  `crates/codegg-config/src/encryption.rs`). The TUI's `q` and `Ctrl+C` exits
  require a second press within three seconds (`src/tui/commands/exit_control.rs`).
