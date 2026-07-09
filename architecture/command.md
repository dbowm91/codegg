# Command Module

The `command` module provides slash command registry loaded from markdown files and configuration.

## Overview

**Location**: `src/command/`

**Key Responsibilities**:
- Slash command registration from markdown files (`command/` and `commands/` directories)
- Command resolution from configuration (`opencode.jsonc`)
- Template variable substitution with deterministic ordering
- Command name validation

## Key Types

### Command (src/command/mod.rs)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    pub name: String,
    pub description: Option<String>,
    pub template: String,
    pub agent: Option<String>,
    pub model: Option<String>,
    #[deprecated(since = "2026-05-22", note = "subtask field is not yet implemented")]
    pub subtask: Option<bool>,
    pub source: String,
    pub process: Option<ProcessCommandSpec>,
}

pub struct ProcessCommandSpec {
    pub command: String,
    pub args: Vec<String>,
    pub stdin: CommandStdinMode,
    pub stdout: CommandStdoutMode,
    pub timeout_ms: u64,
    pub cwd: Option<String>,
    pub env: Vec<String>,
    pub output: Vec<String>,
}
```

Note: The TUI `Command` struct with aliases is in `src/tui/command.rs`.

### CommandConfig (src/config/schema.rs)

```rust
pub struct CommandConfig {
    pub template: String,
    pub description: Option<String>,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub subtask: Option<bool>,
    pub runtime: Option<CommandRuntimeKind>,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub stdin: Option<CommandStdinMode>,
    pub stdout: Option<CommandStdoutMode>,
    pub timeout_ms: Option<u64>,
    pub cwd: Option<String>,
    pub env: Option<Vec<String>>,
    pub output: Option<Vec<String>>,
}
```

## Command Loading

### Sources (in priority order)

1. **Built-in commands**: 96 hardcoded commands (highest priority)
2. **Config commands**: From `opencode.jsonc` `commands` section
3. **File commands**: From `command/` or `commands/` directories in CWD

### File Format (Markdown with YAML Frontmatter)

**Template command** (existing behavior):
```markdown
---
description: A test command
agent: build
template: "Review the file: {file}"
---
Fallback body template if template not specified in frontmatter
```

**Process-backed command** (Phase 4):
```markdown
---
description: Show quota
runtime: process
command: python3
args: ["scripts/quota.py"]
stdout: text
timeout_ms: 5000
---
```

If `runtime` is absent, existing template behavior is preserved. When `runtime: process`, the command field `command` is required.

### Validation Rules

Command names must:
- Not be empty
- Not contain whitespace
- Not start with `/`

Invalid commands are logged and skipped with a warning.

## Template Processing

### Variable Substitution

```rust
pub fn execute_command_template(template: &str, variables: &HashMap<String, String>) -> String
```

- Supports both `{{variable}}` and `{variable}` syntax
- **Deterministic ordering**: Keys are sorted before replacement to ensure consistent output
- Missing variables remain as literal placeholders (e.g., `{name}` stays if `name` not provided)

### Available Variables (TUI Execution)

Currently only `args` is available during TUI execution:
- `{args}` - Everything after the command name (space-separated arguments)

## TUI Integration

### CommandRegistry

```rust
#[derive(Debug, Clone)]
pub struct Command {
    pub name: String,
    pub aliases: Vec<String>,
    pub description: String,
    pub category: CommandCategory,
    pub dialog: Option<Dialog>,
    pub template: Option<String>,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub subtask: Option<bool>,
    pub source: Option<String>,
    pub process: Option<ProcessCommandSpec>,
}
```

### Built-in Commands (96 total)

`src/tui/command.rs::CommandRegistry::built_in_commands()` is the
source of truth for the complete list. The count is covered by
`built_in_command_count_matches_release_docs` so documentation drift is
caught in unit tests.

Representative built-ins:

| Command | Aliases | Description |
|---------|---------|-------------|
| `/connect` | | Connect provider |
| `/exit` | `quit`, `q` | Exit the app |
| `/status` | | View status |
| `/themes` | | Switch theme |
| `/help` | | Help |
| `/sessions` | `resume`, `continue` | Switch session |
| `/new` | `clear` | New session |
| `/share` | | Share session |
| `/unshare` | | Unshare session |
| `/rename` | | Rename session |
| `/compact` | `summarize` | Compact session |
| `/timeline` | | Jump to message |
| `/fork` | | Fork from message |
| `/undo` | | Undo previous message |
| `/redo` | | Redo |
| `/export` | | Export session transcript |
| `/import` | | Import session |
| `/timestamps` | `toggle-timestamps` | Toggle timestamps |
| `/thinking` | `toggle-thinking` | Toggle thinking |
| `/models` | | Switch model |
| `/models-refresh` | `refresh-models` | Refresh model list |
| `/variants` | | Switch model variant |
| `/agents` | | Switch agent |
| `/mcps` | | Manage MCP servers |
| `/workspaces` | | Manage workspaces |
| `/tree` | | Show file tree |
| `/editor` | | Open editor |
| `/keybinds` | | Customize keybindings |
| `/context` | | View context window usage |
| `/cost` | | View token usage and cost |
| `/usage` | | View rate limits and quota |
| `/stats` | | View session analytics and cost breakdown |
| `/tui` | `fullscreen` | Toggle fullscreen mode |
| `/tts` | `voice` | Toggle text-to-speech |
| `/loop` | | Schedule periodic task (e.g. /loop 5m "check status") |
| `/tasks` | | List background tasks |
| `/task-del` | | Delete background task |
| `/memory` | | Memory dashboard |
| `/memory-search` | | Search memories (args: query) |
| `/memory-list` | | List memories (args: namespace) |
| `/memory-remember` | | Remember something (args: text) |
| `/memory-forget` | | Forget a memory (args: id) |
| `/memory-consolidate` | | Consolidate session into memories |
| `/checkpoint` | | Create a checkpoint of current session |
| `/pr` | | GitHub pull requests |
| `/issue` | `bugs`, `features` | GitHub issues |
| `/lsp-servers` | `/lsp-detail` | List active LSP servers with status, root, generation |
| `/lsp-preview` | `/preview-show` | Show LSP preview detail |
| `/tool-backends` | `/tools`, `/backends` | Show resolved backend for each model-facing tool |
| `/security-review` | | Security review of changed files |
| `/shell-list` | | List recent shell commands |
| `/test` | | Run supervised tests (/test, /test workspace, /test changed, /test package <name>, /test file <path>, /test previous|prev|last, /test custom <command>). Previous failures scope reruns the most recent failing test from the bounded index. Custom commands are validated as argv-prefix matches against a strict allowlist — see `architecture/test_runner.md`. |
| `/tui-stats` | | Show TUI runtime diagnostics |

### Dynamic Commands

Dynamic commands from config and files are appended to built-in commands. **Built-in commands take precedence** - duplicates are skipped.

### Plugin Commands (`src/command/plugin.rs`)

Plugin commands via the `/plugin` subcommand:

```rust
#[derive(Debug, Subcommand)]
pub enum PluginCommand {
    /// List installed plugins
    List,
    /// Search available plugins
    Search { query: String },
    /// Install a plugin
    Install { source: String },
}
```

### Command Execution (src/tui/app/mod.rs)

When a command is executed:

1. If command has `dialog` set → open that dialog
2. If command has `process` set (process-backed):
   - Extract args from user input after command name
   - Send `TuiCommand::PluginCommandRun { spec, args }` through command channel
   - Process spawns as child with timeout, output capping
   - Completion arrives as `PluginCommandFinished`
3. If command has `template`:
   - Extract `args` from user input after command name
   - Render template with `{args}` variable
   - Add rendered text as user message
   - Trigger agent processing

### Test Lifecycle Events

The `/test` command publishes lifecycle events through the AppEvent bus:

- `test_run:started` — A supervised test run began. Includes job ID, command, and working directory.
- `test_run:progress` — Throttled progress updates (test counts, failures detected).
- `test_run:completed` — The run finished with status, summary, and log directory path.

Events are throttled to at most one progress event per 500ms to avoid flooding the bus.

Stale completion protection: each `/test` invocation captures a monotonic `AsyncUiRequestState` request ID. If a newer `/test` invocation begins before the previous one finishes, the previous result is silently dropped instead of overwriting the UI state. See `src/tui/app/state/async_request.rs`.

## Error Handling

- **File read failures**: Logged with `tracing::warn`
- **Parse failures**: Logged and skipped
- **Invalid command names**: Logged and skipped
- **Config load failures**: Falls back to empty config (non-fatal)

## Async File Operations

`find_command_files()` is an async wrapper that calls a sync function internally. `load_command_from_file()` is truly async using `tokio::fs` for non-blocking I/O:

```rust
pub async fn find_command_files(base: &Path) -> Vec<Command>
pub async fn load_command_from_file(path: &Path) -> Result<Command, String>
```

## See Also

- [.opencode/skills/command/SKILL.md](../.opencode/skills/command/SKILL.md) - Agent guidance for command module

- [tui.md](tui.md) - TUI command input handling
- [agent-loop/SKILL.md](../.opencode/skills/agent-loop/SKILL.md) - Agent execution with command templates
