# Command Module

The `command` module provides slash command registry loaded from markdown
files and configuration.

## Purpose

Parses user-typed `/command args` into structured `Command` objects,
resolves them from built-in, config, and file sources, and executes
template substitution or process-backed execution.

## Where It Lives

- `src/command/` — Core `Command` struct, file loading, template processing
- `src/tui/command.rs` — TUI `CommandRegistry` with 149 built-in commands
- `src/config/schema.rs` — `CommandConfig` for config-file commands

## How It Works

### Command Loading (priority order)

1. **Built-in commands**: 149 hardcoded commands (highest priority)
2. **Config commands**: From `opencode.jsonc` `commands` section
3. **Project commands**: From `command/` or `commands/` directories under the
   active project's explicit workspace root

Built-in commands take precedence — duplicates from config/files are
skipped.

### File Format (Markdown with YAML Frontmatter)

**Template command**:
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

If `runtime` is absent, existing template behavior is preserved.

### Validation Rules

Command names must: not be empty, not contain whitespace, not start
with `/`. Invalid commands are logged and skipped.

### Template Processing

```rust
pub fn execute_command_template(template: &str, variables: &HashMap<String, String>) -> String
```

Supports `{{variable}}` and `{variable}` syntax. Keys sorted before
replacement for deterministic output. Missing variables remain as
literal placeholders.

### TUI Execution Variables

- `{args}` — Everything after the command name (space-separated)

### Command Execution Flow

Raw slash text → registry name/alias resolution → typed `CommandAction`
→ existing domain handler. Dispatch matches the resolved action, never
a raw command string.

1. If action is `Dialog(dialog)` → open that dialog
2. If action is `Process` (process-backed):
   - Extract args from user input after command name
   - Send `TuiCommand::PluginCommandRun { spec, args }` through channel
   - Process spawns as child with timeout, output capping
   - Completion arrives as `PluginCommandFinished`
3. If action is `Template`:
   - Extract `args` from user input after command name
   - Render template with `{args}` variable
   - Add rendered text as user message
   - Trigger agent processing
4. If action is `Builtin(action)` → exhaustive `BuiltinSlashAction`
   dispatch in `App::dispatch_builtin_command`
5. If action is `Plugin` → bounded discovery-only toast directing to
   the plugin panel

## Key Types & APIs

### Core Command (`src/command/mod.rs:39`)

```rust
pub struct Command {
    pub name: String,
    pub description: Option<String>,
    pub template: String,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub source: String,
    pub process: Option<ProcessCommandSpec>,
}
```

### ProcessCommandSpec (`src/command/mod.rs:12`)

```rust
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

### TUI Command (`src/tui/command.rs:27`)

```rust
pub struct Command {
    pub name: String,
    pub aliases: Vec<String>,
    pub description: String,
    pub category: CommandCategory,
    pub domain: CommandDomain,
    pub scope: CommandScope,
    pub dialog: Option<Dialog>,
    pub template: Option<String>,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub source: Option<String>,
    pub source_kind: CommandSource,
    pub keywords: Vec<String>,
    pub process: Option<ProcessCommandSpec>,
    pub action: CommandAction,
}
```

### CommandAction (`src/tui/command.rs`)

```rust
pub enum CommandAction {
    Dialog(Dialog),
    Builtin(BuiltinSlashAction),
    Template,
    Process,
    Plugin,
}
```

`BuiltinSlashAction` is an exhaustive enum with one variant per
built-in executable operation (142 variants). Adding a new built-in
command requires touching this one typed registry/action authority;
the compiler (exhaustive match in `dispatch_builtin_command`) and the
`every_builtin_action_is_referenced_by_a_canonical_command` /
`every_builtin_entry_has_coherent_action` tests make a registry-only
or dispatcher-only command impossible to land unnoticed.

`CommandCategory` is retained as a compatibility classification. Discovery
surfaces use the stable `CommandDomain` taxonomy (Project, Session, Agent,
Execution, Git/Review, Research, Provider, Collaboration, Memory,
Diagnostics, System). `scope` and `source_kind` are presentation metadata;
they never grant execution or authorization. Built-ins win deterministic
name/alias collisions, config commands are global, and project-file commands
are scoped to the active workspace. Plugin registrations are shown with their
plugin id and lose collisions to existing entries.

The palette searches the canonical name, aliases, description, domain, and
keywords, and caps visible fuzzy results. Switching project tabs replaces the
project catalog without rediscovering files during palette input.

### CommandCategory (`src/tui/command.rs:9`)

```rust
pub enum CommandCategory {
    Session,
    Agent,
    System,
}
```

### CommandConfig (`src/config/schema.rs`)

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

### Headless plugin management (no `codegg plugin` CLI)

There is deliberately no `codegg plugin ...` process CLI. An early
`src/command/plugin.rs` Clap module (list/search/install over the old
marketplace service) was never wired into the root CLI and was removed
rather than preserved: it duplicated install policy instead of calling
the shared authority, and its marketplace tier model predates the
current manifest/policy implementation.

Headless plugin operations go through the shared authority instead:

- install/uninstall policy and path validation: `src/plugin/install.rs`
  (`install_from_path`, `install_from_url`, `uninstall`);
- lifecycle/management views: `src/plugin/management.rs`
  (`PluginManager`, also used by the TUI plugin commands);
- contribution inventory: `src/plugin/manifest.rs` / `registry.rs`.

A future headless plugin surface must call that shared authority
without duplicating install policy, and must add parser tests plus
docs in the same change.

### CommandRegistry (`src/tui/command.rs:84`)

```rust
pub struct CommandRegistry {
    commands: Vec<Command>,
}
```

`COMMAND_REGISTRY` exposes the immutable built-in/global catalog for
compatibility and tests. Each TUI `App` owns a `CommandRegistry` containing
those definitions plus project-local commands discovered from its active
tab's explicit workspace root. Switching tabs replaces the project-local
catalog and re-filters the command palette; discovery never reads process
cwd. Dynamic commands cannot change daemon authorization or execution scope.

### Built-in Commands (149 total)

Representative built-ins:

| Command | Aliases | Description |
|---------|---------|-------------|
| `/connect` | | Connect provider |
| `/connections` | | Manage connections |
| `/exit` | `quit`, `q` | Exit the app |
| `/status` | | View status |
| `/policy` | | Show effective approval mode and sandbox profile |
| `/approval` | `/approval-mode` | Select approval mode (interactive/automatic/yolo) |
| `/sandbox` | `/sandbox-profile` | Select sandbox profile (read-only/workspace-write/full-host) |
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
| `/models` | `/model` | Switch model |
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
| `/loop` | | Schedule periodic task |
| `/tasks` | | List background tasks |
| `/task-del` | | Delete background task |
| `/memory` | | Memory dashboard |
| `/memory-search` | | Search memories |
| `/memory-list` | | List memories |
| `/memory-remember` | | Remember something |
| `/memory-forget` | | Forget a memory |
| `/memory-consolidate` | | Consolidate session into memories |
| `/edit-undo` | | Undo the latest durable edit checkpoint |
| `/edit-reapply` | | Reapply the latest undone edit checkpoint |
| `/edit-checkpoints` | `/checkpoints`, `/history` | List durable edit checkpoints |
| `/tool-contracts` | | Show tool contract diagnostics |
| `/worktree` | | List worktrees for the active workspace |
| `/pr` | | GitHub pull requests |
| `/issue` | `bugs`, `features` | GitHub issues |
| `/lsp-servers` | `/lsp-detail` | List active LSP servers |
| `/lsp-preview` | `/preview-show` | Show LSP preview detail |
| `/tool-backends` | `/tools`, `/backends` | Show resolved tool backends |
| `/security-review` | | Security review of changed files |
| `/shell-list` | | List recent shell commands |
| `/shell-show` | | Show shell command detail |
| `/shell-ask` | | Ask about a shell command |
| `/test` | | Run supervised tests |
| `/tui-stats` | | Show TUI runtime diagnostics |

`/checkpoint` was removed: it was registered without any execution
branch and no session-checkpoint operation with the promised semantics
exists (goal checkpoints are goal-scoped via `/goal checkpoint`; edit
checkpoints list via `/edit-checkpoints`). It is not redirected.

### Dynamic Commands

Dynamic commands from config and files are appended to built-in commands.
Built-in commands take precedence.

## Configuration Surface

### Config file (`opencode.jsonc`)

```jsonc
{
  "commands": {
    "my-command": {
      "template": "Do something with {args}",
      "description": "My custom command",
      "agent": "build",
      "model": "claude-3.5-sonnet"
    }
  }
}
```

### Process-backed config

```jsonc
{
  "commands": {
    "quota": {
      "description": "Show quota",
      "runtime": "process",
      "command": "python3",
      "args": ["scripts/quota.py"],
      "stdout": "text",
      "timeout_ms": 5000
    }
  }
}
```

### File-based commands

Place `.md` files in `command/` or `commands/` directories in CWD.
Frontmatter supports: `description`, `agent`, `model`, `template`,
`runtime`, `command`, `args`, `stdin`, `stdout`, `timeout_ms`, `cwd`,
`env`, `output`.

## Invariants & Gotchas

- **Built-in count is 149**: Guarded by
  `built_in_command_count_matches_release_docs` and
  `command_docs_count_matches_registry` in `src/tui/command.rs`. The
  docs test parses this file and fails on drift, so update the test
  assertion and every count in this doc together when adding built-ins.
  The authoritative catalog is `CommandRegistry::built_in_commands`.
- **Core Command has no `subtask` field**: The `subtask` field exists
  only in `CommandConfig` (config schema), not in `src/command::Command`.
- **`find_command_files()` is async wrapper**: Internally calls sync
  function. `load_command_from_file()` is truly async via `tokio::fs`.
- **Template ordering is deterministic**: Keys sorted before replacement.

## Testing

```bash
cargo test -p codegg -- command     # Command module unit tests
cargo test -p codegg -- command     # includes built_in_command_count test
```

The `built_in_command_count_matches_release_docs` test ensures the
149 count stays in sync with this documentation.

## Related Docs

- [tui.md](tui.md) — TUI command input handling and dispatch
- [agent.md](agent.md) — Agent execution with command templates
