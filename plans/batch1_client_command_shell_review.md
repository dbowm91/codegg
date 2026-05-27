# Client & Command & Shell Session Architecture Review

## Verified Claims

### Client Module (src/client/)

| Claim | Verified | Location |
|-------|----------|----------|
| `mod.rs` re-exports `run_attach` as public API | ✅ | `src/client/mod.rs:4` |
| `run_attach(url, token)` async function signature | ✅ | `src/client/attach.rs:14` |
| Health check 10s timeout | ✅ | `src/client/sdk.rs:26` (connect_timeout), `sdk.rs:40` (request timeout) |
| WebSocket 30s timeout per attempt | ✅ | `src/client/attach.rs:43` |
| Up to 3 retries with exponential backoff (1s, 2s, 4s) | ✅ | `src/client/attach.rs:38-41` |
| Resume handshake sends `TuiMessage::Resume { from_event_seq: 0 }` | ✅ | `src/client/attach.rs:73-75` |
| Two background tasks: `event_task` and `send_task` | ✅ | `src/client/attach.rs:85-127` |
| `catch_unwind` used in event_task | ✅ | `src/client/attach.rs:86` |
| `RemoteClient` struct with `base_url` and `http` fields | ✅ | `src/client/sdk.rs:7-10` |
| `RemoteClient::health()` uses `GET /health` | ✅ | `src/client/sdk.rs:36` |
| `ClientError` enum with 5 variants | ✅ | `src/error.rs:504-519` |
| `ClientError::Unreachable` for health check failures | ✅ | `src/error.rs:509`, `sdk.rs:47` |
| Protocol uses `TuiMessage` with `#[serde(tag = "type")]` | ✅ | Not in client module, but documented correctly as being in `src/protocol/tui.rs` |
| `event_tx/rx` and `out_tx/rx` channel pattern | ✅ | `src/client/attach.rs:79-80` |
| `tui::App::new_remote()` called with `url.to_string()` | ✅ | `src/client/attach.rs:77` |

### Command Module (src/command/)

| Claim | Verified | Location |
|-------|----------|----------|
| `Command` struct has `name`, `description`, `template`, `agent`, `model`, `subtask`, `source` | ✅ | `src/command/mod.rs:9-18` |
| `subtask` has `#[deprecated]` attribute | ✅ | `src/command/mod.rs:15-16` |
| `execute_command_template()` with sorted key deterministic ordering | ✅ | `src/command/mod.rs:160-170` |
| `find_command_files()` is async wrapper | ✅ | `src/command/mod.rs:20-25` |
| `load_command_from_file()` uses `tokio::fs` | ✅ | `src/command/mod.rs:78-83` |
| Validation: not empty, no whitespace, not starting with `/` | ✅ | `src/command/mod.rs:65-76` |
| `CommandConfig` in `src/config/schema.rs` | ✅ | (External reference, correct) |
| `find_command_files_sync()` uses `std::fs::read_dir` | ✅ | `src/command/mod.rs:27-63` |
| File format supports markdown with YAML frontmatter | ✅ | `src/command/mod.rs:91-140` |
| Empty template falls back to body | ✅ | `src/command/mod.rs:128` |
| `normalize_name()` lowercases and strips leading `/` | ✅ | `src/tui/command.rs:259-261` |
| Plugin commands defined in `src/command/plugin.rs` | ✅ | `src/command/plugin.rs:5-19` |
| `CommandRegistry::new()` registers all built-in commands | ✅ | `src/tui/command.rs:82-186` |

### Shell Session Module (src/shell_session/)

| Claim | Verified | Location |
|-------|----------|----------|
| `ShellSession` struct with all 7 fields | ✅ | `src/shell_session/mod.rs:5-14` |
| `CreateShellSession` struct with all fields | ✅ | `src/shell_session/mod.rs:16-23` |
| `ShellResize` struct with `cols` and `rows` | ✅ | `src/shell_session/mod.rs:25-29` |
| `ShellManager` with `sessions: Arc<RwLock<HashMap<...>>>` | ✅ | `src/shell_session/session.rs:9-11` |
| Default shell is `bash` | ✅ | `src/shell_session/session.rs:28` |
| Default terminal size is 80x24 | ✅ | `src/shell_session/session.rs:29-30` |
| `created_at` uses milliseconds since epoch | ✅ | `src/shell_session/session.rs:22` |
| Sessions stored in-memory only (no persistence) | ✅ | Architecture doc matches implementation |
| Unit tests present (11 tests covering all operations) | ✅ | `src/shell_session/session.rs:89-273` |

## Incorrect/Stale Claims

### Client Module

1. **Backoff formula discrepancy**: The architecture says "(1s, 2s, 4s)" which is client backoff. This matches `src/client/attach.rs:39` - the formula is `2u64.saturating_pow((attempt - 1) as u32)` which gives 1s, 2s, 4s. ✅ Verified CORRECT.

2. **`catch_unwind` placement**: Documentation says "Event handling uses `catch_unwind`" but this is only on `event_task` (line 86), not on `send_task`. The documentation is slightly imprecise but not wrong.

### Command Module

1. **Built-in command count**: The architecture documents "39 hardcoded commands" at line 51 and in the table (lines 114-158). However, actual code has **46 `Command::new()` calls** in `CommandRegistry::new()` (lines 84-178), not 39. The architecture is **INCORRECT** - the count is 46, not 39.

2. **Missing commands in table**: The documented table in `command.md:114-158` is missing:
   - `/stats` - has `Dialog::Stats` (line 147)
   - `/tts` - has aliases `["voice"]` (line 152)
   - `/pr` - has template (lines 175-177)
   - `/issue` - has aliases `/bugs`, `/features` and template (lines 178-181)
   - `/checkpoint` - (line 173-174)

3. **`load_command_from_file` description**: The architecture at line 200 says "load_command_from_file() is truly async using tokio::fs". But `load_command_from_file()` at line 78-83 uses `tokio::fs::read_to_string`, while `find_command_files()` at line 20-25 just wraps sync `find_command_files_sync()`. The description is partially misleading - `load_command_from_file` is async but `find_command_files` is not.

4. **Historical note about `src/tui/app/commands.rs`**: Architecture line 212 says "Removed orphaned src/tui/app/commands.rs file". This file no longer exists, so the documentation is correct about the historical note but this is stale info - it's already removed.

### Shell Session Module

**No issues found** - all claims verified correct.

## Bugs Found

### None identified

The implementations in all three modules appear correct. No bugs were found in:
- Client: connection flow, error handling, channel setup, background tasks
- Command: template processing, validation, file loading, registry
- Shell Session: manager operations, session lifecycle, tests

## Improvements Identified

### Command Module

1. **Command count documentation is stale**: The architecture states 39 built-in commands but there are actually 46. The documentation should be updated to reflect the actual count.

2. **Command table incomplete**: The table at lines 116-158 documents only 39 commands and omits `/stats`, `/tts`, `/pr`, `/issue`, `/checkpoint`. This table should be regenerated from source code.

3. **Missing `/pr` and `/issue` command details**: These commands use templates that route to GitHub MCP. The table doesn't capture:
   - `/pr` has template `Use GitHub MCP (mcp__github) to {args}`
   - `/issue` has aliases `/bugs`, `/features` and the same template

### Client Module

4. **Minor: `handle_remote_event` location in docs**: The architecture says "App::handle_remote_event() (in src/tui/app/mod.rs)" at line 108. While this is correct, it would be more useful to include the actual line number (794 per AGENTS.md).

5. **Minor: TuiMessage protocol reference incomplete**: The architecture references `src/protocol/tui.rs` for `TuiMessage` but doesn't verify the exact variant count or fields. This is fine as-is since protocol can change.

### Shell Session Module

6. **Architecture file is very brief (80 lines)**: Could benefit from more detail on the session lifecycle and how it integrates with tool::terminal.

## Stale References

1. **Command module historical implementation notes (lines 207-217)**: These were relevant during a past refactoring but are now stale. Items like "Fixed unused variable warnings", "Removed orphaned `src/tui/app/commands.rs`" should either be removed or moved to a changelog.

2. **`command.md:212`**: "Removed orphaned `src/tui/app/commands.rs` file (was never module-declared, contained duplicate command handlers)" - This is stated as a historical note but should be removed as it's no longer relevant.

3. **`command.md:219-227`**: The `normalize_name()` function description is accurate but placed in "Historical Implementation Notes" - it should be moved to the main TUI Integration section since it's current functionality.

## Recommendations

1. **Update built-in command count** in `architecture/command.md` from "39" to "46" and regenerate the command table from actual source.

2. **Remove stale historical notes** from `command.md` lines 207-217 or move them to a CHANGELOG file.

3. **Add line numbers** to key function references in architecture docs for easier code navigation.

4. **Add cross-references** between modules (e.g., shell_session.md should reference tool.md for actual PTY creation).
