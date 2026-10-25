# Command Module

The `command` module provides slash command registry loaded from markdown
files and configuration.

## Purpose

Parses user-typed `/command args` into structured `Command` objects,
resolves them from built-in, config, and file sources, and executes
template substitution or process-backed execution.

**The catalog's owner is the daemon.** The single definition of a command —
its identity, its name and aliases, its description, its **kind**, and the
shape of its argument forms — is the daemon's catalog in
`crates/codegg-core/src/command_catalog.rs`. The TUI's built-in registry is
a **cache** of that catalog, not a second definition of it. A command's
*kind* decides who runs it: a **daemon effect** is executed by the daemon
under `command.execute` and audited as a canonical `Run`; a **frontend
effect** is executed locally by the frontend against the declared
frontend-action set and creates **no** `Run`.

## Where It Lives

- `crates/codegg-core/src/command_catalog.rs` — **the catalog**: the single
  definition of every command, its forms, its kinds, and the catalog's
  bounds. `the_catalog()` (`:3178`) is the accessor of record.
- `crates/codegg-core/src/frontend_action.rs` — the **declared
  frontend-action set**: the finite set of actions a frontend may execute
  for a frontend-effect form (`FrontendAction`, `:41`; `action_for_form()`,
  `:323`).
- `src/core/daemon_commands.rs` — the daemon's **executor**: the
  table-driven dispatch from a catalog form to a daemon request
  (`build_inner_request()`, `:148`), the serve-ability predicate
  (`serves_form()`, `:519`), and the audit as a canonical `Run`.
- `src/command/` — Core `Command` struct, file loading, template processing
- `src/tui/command.rs` — TUI `CommandRegistry`: a **cache** of the daemon's
  catalog with 169 built-in rows, plus the TUI's own local dynamic sources
- `crates/codegg-config/src/schema.rs` — `CommandConfig` for config-file
  commands (re-exported as `crate::config::schema`)

## How It Works

### Command Loading (priority order)

1. **Built-in commands**: 169 rows cached from the daemon's catalog (highest
   priority)
2. **Config commands**: From the `commands` section of the config file
   (`codegg.jsonc` / `codegg.json` / `config.json`)3. **Project commands**: From `command/` or `commands/` directories under the
   active project's explicit workspace root

Built-in commands take precedence — duplicates from config/files are
skipped.

**The built-in rows are a cache, not a definition.** The daemon's catalog
owns each row's identity, name, aliases, description, kind, and argument
shape; the TUI's registry mirrors it. Rows 2 and 3 are the TUI's **own
local dynamic sources**: they stay local, are outside the daemon's catalog,
and cannot change daemon authorization or execution scope. ADR-0052 defers
dynamic command sources deliberately — a dynamic source has different trust
properties from a built-in one — so the desktop does not expose them and a
plugin- or MCP-contributed command is **not** a catalog command.

`/skills [name]` is a read-only TUI inspection command backed by the active
workspace's immutable asset snapshot. It reports effective skills and shadow counts;
`/reload` requests the existing transactional refresh for subsequent turns.
The model-facing activation path remains the `skill` tool, not a slash command.

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

### TUI Command (`src/tui/command.rs:325`)

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
built-in executable operation (161 variants, `src/tui/command.rs:112`).
Adding a new built-in command requires touching this one typed
registry/action authority;
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

### CommandCategory (`src/tui/command.rs:10`)

```rust
pub enum CommandCategory {
    Session,
    Agent,
    System,
}
```

### CommandConfig (`crates/codegg-config/src/schema.rs:1823`)

```rust
pub struct CommandConfig {
    pub template: String,
    pub description: Option<String>,
    pub agent: Option<String>,
    pub model: Option<String>,
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

### CommandRegistry (`src/tui/command.rs:557`)

```rust
pub struct CommandRegistry {
    commands: Vec<Command>,
}
```

`COMMAND_REGISTRY` exposes the immutable built-in/global rows for
compatibility and tests. Each TUI `App` owns a `CommandRegistry` containing
those rows plus project-local commands discovered from its active
tab's explicit workspace root. Switching tabs replaces the project-local
catalog and re-filters the command palette; discovery never reads process
cwd. Dynamic commands cannot change daemon authorization or execution scope.
They are also **outside** the daemon's catalog: a dynamic source is a
separate trust class, and the desktop does not expose it.

### Built-in Commands (169 total)

The **169** is the length of the TUI's cached built-in rows. It is a count
of the *cache*, and it is deliberately **not** the count of commands the
product has: the daemon's catalog is the definition, and it carries **161**
commands over **166** argument forms — 134 daemon-effect forms and 32
frontend-effect forms. The two numbers answer different questions and both
are guarded.

Representative built-ins:

| Command | Aliases | Description |
|---------|---------|-------------|
| `/init` | | Preview and explicitly publish bounded root `AGENTS.md` guidance |
| `/network` | | Set the child network policy: offline or unrestricted (`/network offline` / `/network unrestricted confirm`) |
| `/files` | | Browse workspace files |
| `/skills` | | Inspect effective skills and shadowed alternatives (`/skills [name]`) |
| `/open` | | M006-A: open one workspace-relative document in the editor primary view (`/open <path>`) |
| `/terminal` | `/term` | Open an interactive terminal in a box; `Shift+L` docks it into the side panel |
| `/panels` | `/panel`, `/panel-manager` | Side-panel manager: choose which panel the side column holds and bind its `Ctrl+1..Ctrl+0` hotkey. Same box as `⇧S`, but typed — so it also works mid-prompt, where `⇧S` is the letter `S` |
| `/terminal-clear` | | Clear a terminal's scrollback and screen; the process keeps running |
| `/connect` | | Add/configure a provider connection (catalog selection + credential form; Eggpool is one optional upstream) |
| `/connections` | `/connection`, `/select-connection` | Inspect/manage/select existing durable connections |
| `/exit` | `quit`, `q` | Exit the app |
| `/status` | | View status |
| `/policy` | | Show effective approval mode and sandbox profile |
| `/approval` | `/approval-mode` | Select approval mode (interactive/automatic/yolo) |
| `/sandbox` | `/sandbox-profile` | Select sandbox profile (read-only/workspace-write/full-host) |
| `/themes` | `/theme` | Switch theme |
| `/help` | | Help |
| `/sessions` | `resume`, `continue` | Switch session. `resume` stays the session-switch spelling; carrying on after a stopped turn is `/start` (below) |
| `/projects` | `/project` | Open the project picker (`Shift+F` in Normal mode) |
| `/new` | `clear` | New session |
| `/stop` | | Interrupt the agent's running turn. Also `x` twice in Normal mode |
| `/start` | | Carry on after a stopped turn. Re-submits: a cancelled turn cannot be resurrected |
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
| `/thinking` | `toggle-thinking` | Toggle thinking display |
| `/expand-all` | `expand-disclosures`, `collapse-all` | Expand or collapse every tool-call and reasoning disclosure in the transcript; the same gesture as the `Ctrl+Alt+O` chord |
| `/reasoning` | `reasoning-effort`, `thinking-budget` | Cycle extended thinking: default / effort / off |
| `/models` | `/model` | Switch model |
| `/models-refresh` | `refresh-models` | Refresh model list |
| `/variants` | | Switch model variant |
| `/agents` | | Open the "which panel?" box, then load the agent list into it; `Enter` in the list opens the agent's config editor |
| `/mcps` | | Manage MCP servers |
| `/workspaces` | | Manage workspaces |
| `/tree` | | Show file tree |
| `/editor` | | Open editor |
| `/keybinds` | | Customize keybindings |
| `/context` | | View context window usage |
| `/cost` | | View token usage and cost |
| `/usage` | | View rate limits and quota |
| `/stats` | | View session analytics and cost breakdown |
| `/subagent-send` | | Address the next composer prompt to one running subagent (`<name>` or `<task id>`, or bare for the focused tab); `/subagent-send off` clears it. Sends `TurnSteer` to the **parent turn** — the agent running now reads it and decides whether to relay |
| `/queue` | | Open this session's waiting-prompt box: `j`/`k` move, `r` refresh, `a` add (text then delay), `e` edit in place, `[`/`]` reorder, `d` delete, `s` send the selected prompt to the turn running now |
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
| `/logs` | | Show recent daemon log lines and past toast notifications |
| `/nodes` | | Eggwork M002: show the configured remote nodes' host telemetry |
| `/worktree` | | List worktrees for the active workspace |
| `/pr` | | GitHub pull requests |
| `/issue` | `bugs`, `features` | GitHub issues |
| `/lsp-servers` | `/lsp-detail` | List active LSP servers |
| `/lsp-preview` | `/preview-show` | Show LSP preview detail |
| `/review` | | Review changed files |
| `/review-change` | `/preview-review` | M006-E: review a pending agent change before applying it (`<preview-id>`); `[a]pply or `Esc` to reject |
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

### Config file (`codegg.jsonc`)

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

Place `.md` files in `command/` or `commands/` directories under the active
project's explicit workspace root (never the process cwd). Frontmatter
supports: `description`, `agent`, `model`, `template`, `runtime`,
`command`, `args`, `stdin`, `stdout`, `timeout_ms`, `cwd`, `env`, `output`.

## Invariants & Gotchas

- **One catalog, not two.** The daemon's catalog in
  `crates/codegg-core/src/command_catalog.rs` is the single definition of
  every command's identity, name, aliases, description, kind and argument
  shape. The TUI's registry is a cache of it; the desktop renderer holds
  **no** command list, **no** command names, and **no** command-kind table.
  `scripts/check_command_catalog_ownership.py` fails the build if a second
  catalog appears in the daemon, the TUI, the desktop host, or the renderer.
- **A command's name is addressable by its own identity.** Two commands may
  share one effect and still be two commands: `/memory` and `/memory-list`
  share a memory-summary effect, and each is executed under the id the user
  typed. A shared effect is not a shared identity.
- **A kind is never inferred.** A form's kind comes from the catalog. The
  daemon may never execute a frontend-effect form, and the frontend may
  never execute a daemon-effect form directly — a daemon effect goes
  through `CoreRequest::CommandExecute`, and the daemon authorizes it under
  `command.execute` and audits it as a canonical `Run`.
- **Built-in count is 169**: Guarded by
  `built_in_command_count_matches_release_docs` and
  `command_docs_count_matches_registry` in `src/tui/command.rs`. The
  docs test parses this file and fails on drift, so update the test
  assertion and every count in this doc together when adding built-ins.
  The authoritative catalog is `CommandRegistry::built_in_commands`.
- **No `subtask` field anywhere**: Neither `src/command::Command` nor
  `CommandConfig` declares `subtask`; a grep across `src/` and
  `crates/` returns nothing.
- **`find_command_files()` is async wrapper**: Internally calls the sync
  `find_command_files_sync()`. `load_command_from_file()` is truly async via
  `tokio::fs`; `load_command_from_file_sync()` is its blocking sibling.
- **Template ordering is deterministic**: Keys sorted before replacement.
- **The representative table above is not exhaustive**: the registry is the
  authority, and `CommandRegistry::built_in_commands()` is the only list of
  record. Aliases participate in the reservation set — a dynamic command
  whose *name or alias* collides with a built-in loses.

## Testing

```bash
cargo test -p codegg -- command     # Command module unit tests

# The three count guards, individually
cargo test -p codegg -- built_in_command_count_matches_release_docs
cargo test -p codegg -- command_docs_count_matches_registry
cargo test -p codegg -- command_docs_builtin_slash_action_variant_count_matches_source

# Registry/action coherence
cargo test -p codegg -- every_builtin_action_is_referenced_by_a_canonical_command
cargo test -p codegg -- every_builtin_entry_has_coherent_action
```

The `built_in_command_count_matches_release_docs` test ensures the
169 count stays in sync with this documentation. The two
`command_docs_*` tests parse **this file** and fail on drift, so a count
bump means updating this document and the registry pin together. The
`command_docs_*` pair also guards the `BuiltinSlashAction` variant count
that the registry-length guard cannot see.
## Related Docs

- [tui.md](tui.md) — TUI command input handling and dispatch
- [agent.md](agent.md) — Agent execution with command templates

## Source verification

Verified 2026-10-23 against `src/tui/command.rs`, `src/command/mod.rs`,
`crates/codegg-config/src/schema.rs`, `crates/codegg-config/src/paths.rs`
and the root `Cargo.toml`.

Changes made in this pass:

- **Two stale source anchors.** The `CommandRegistry` section heading said
  `src/tui/command.rs:458` while the verification paragraph below already
  said `:500`; the struct is at `:500`. The `CommandConfig` heading said
  `crates/codegg-config/src/schema.rs:1416` and the verification paragraph
  said `:1821`; the struct declaration is at `:1823`. The document
  contradicted itself on both, and neither matched source.
- **The config file name was wrong.** The document named
  `opencode.jsonc` in two places. The config layer resolves
  `codegg.jsonc` / `codegg.json` / `config.json`
  (`crates/codegg-config/src/paths.rs:66`), and the `commands` section is
  `Option<HashMap<String, CommandConfig>>` on the config schema
  (`schema.rs:254`). Renamed both occurrences.
- **The representative table had a duplicate `/init` row** (it appeared
  twice) and was missing two aliases the registry actually declares:
  `/connections` has `/connection` and `/select-connection`, and `/themes`
  has `/theme`. Removed the duplicate and added the aliases.
- **Added three built-ins the table omitted**: `/network` (the per-principal
  network intent policy — the command that moved the count to 168),
  `/files` and `/skills`.
- **"in CWD" was wrong for file-based commands.** Discovery runs from the
  active project's explicit workspace root (`find_command_files_sync`
  joins `command`/`commands` onto the given base), never the process cwd.
- **Added `load_command_from_file_sync`** — the blocking sibling of the
  async `load_command_from_file()` — and named
  `find_command_files_sync()`.
- **Added the alias-reservation rule** to the invariants: a dynamic
  command whose *name or alias* collides with a built-in loses, not only
  on a name collision.
- **Rewrote the Testing section** to name the three count guards and the
  two registry/action coherence guards individually instead of repeating
  the same `cargo test -p codegg -- command` line twice.

Verified accurate and left unchanged: the count claims — 169 built-in
commands and 161 `BuiltinSlashAction` variants — both re-measured against
source (`CommandRegistry::built_in_commands()` yields 169 entries and the
enum has 161 variants); `CommandDomain` (11, `command.rs:22`),
`CommandScope` (2, `command.rs:56`), `CommandSource` (4, `command.rs:76`),
`CommandCategory` (3, `command.rs:10`), `CommandAction` (5,
`command.rs:316`), the TUI `Command` struct fields (`command.rs:325`), core
`Command` (`src/command/mod.rs:39`), `ProcessCommandSpec`
(`src/command/mod.rs:12`), `execute_command_template`
(`src/command/mod.rs:320`), the `CommandConfig` field list, the
`command`/`commands` directory names, the `PluginCommandRun` /
`PluginCommandFinished` / `dispatch_builtin_command` symbols, the
absence of any `subtask` field in `src/` and `crates/`, and that
`/checkpoint` is absent from the registry (its only remaining occurrence
is the `CommandDomain::Execution` keyword list).

Both count-guard tests live in `src/tui/command.rs`
(`built_in_command_count_matches_release_docs` at `:1277`,
`command_docs_count_matches_registry` at `:1429`, and
`command_docs_builtin_slash_action_variant_count_matches_source` at
`:1465`); the latter two parse this document, so they fail when it
drifts.

### Pass of 2026-10-25 (M007-D)

Re-verified against `src/tui/command.rs` while closing the daemon-owned
command catalog milestone.

- **Six stale source anchors, all in `src/tui/command.rs`.** The two section
  headings pointed at `:281` (the `Command` struct, actually `:325`) and
  `:500` (the `CommandRegistry`, actually `:557`). `CommandAction` was cited
  at `:313` (actually `:316`), the `Command` struct at `:322` (actually
  `:325`), and the three count-guard tests at `:1217`/`:1364`/`:1400`
  (actually `:1277`/`:1429`/`:1465`). Every anchor was re-read against
  source; the counts they guarded — 169 built-in commands and 161
  `BuiltinSlashAction` variants — were correct and are unchanged.
- **The anchors below were already correct** and are left unchanged:
  `BuiltinSlashAction` `:112`, `CommandCategory` `:10`, `CommandDomain`
  `:22`, `CommandScope` `:56`, `CommandSource` `:76`.

### Earlier passes

Verified 2026-10-13 against `src/tui/command.rs` and `src/command/mod.rs`,
re-verified 2026-10-19 after C007 added `/subagent-send`, and re-verified
2026-10-20 after Eggwork M002 added `/nodes`, and re-verified
2026-10-23 after the TUI text-rendering line added `/expand-all`:
both count-guard tests pass — `built_in_command_count_matches_release_docs`
(`command.rs:1277`) asserts 169, and every `(\d+)\s+(hardcoded|built-in|total)`
plus `[Cc]ount is (\d+)` match in this document resolves to 169.
Also verified the 169 built-in registry length, `BuiltinSlashAction`
(161 variants, `command.rs:112`), `CommandDomain` (11, `command.rs:22`),
`CommandScope` (2, `command.rs:56`), `CommandSource` (4, `command.rs:76`),
`CommandCategory` (3, `command.rs:10`), `CommandAction`
(5, `command.rs:316`), the TUI `Command` struct fields
(`command.rs:325`), `CommandRegistry` (`command.rs:557`), core `Command`
(`src/command/mod.rs:39`), `ProcessCommandSpec`
(`src/command/mod.rs:12`), `execute_command_template`
(`src/command/mod.rs:320`), `CommandConfig`
(`crates/codegg-config/src/schema.rs:1821`), and that `/checkpoint` is
absent from the registry (its only remaining occurrence is the
`CommandDomain::Execution` keyword list).

Corrected again 2026-10-19 while closing the corrective addendum: the 2026-10-13
pass above had been edited in place without re-reading source, so this paragraph
asserted `command.rs:1181`/`:298`/`:307`/`:485`/`schema.rs:1416` for five
anchors that had all moved, and the body of this document still claimed **149**
`BuiltinSlashAction` variants while this paragraph claimed 158. The five anchors
and the 149 are corrected above against source on 2026-10-19.

149 is worth recording rather than quietly fixing, because it is the third
distinct wrong figure these pins have carried (156 at the 2026-10-13 pass, 157
before C007, 158 after) and no guard saw any of them:
`command_docs_count_matches_registry` matches only `(\d+)\s+(hardcoded|built-in|total)`
and `[Cc]ount is (\d+)`, so a `N variants` claim is invisible to it.
`command_docs_builtin_slash_action_variant_count_matches_source` now covers
it. (The name was wrong until 2026-10-21: this paragraph credited a guard
called `command_docs_enum_variant_counts_match_source`, which does not exist
in the tree — the two `N variants` claims were, in fact, uncovered until
`command_docs_builtin_slash_action_variant_count_matches_source` was added.)

The **159** was the fourth distinct figure these pins have carried, and the
158 was not merely stale: Eggwork M002 added `/nodes` to the
`BuiltinSlashAction` enum and to the registry, but omitted it from
`all_builtin_variants()`. The registry therefore referenced 159 distinct
builtin actions while the exhaustive list held 158, and the "no surplus"
arm of `every_builtin_action_is_referenced_by_a_canonical_command` failed.
The M002 closure recorded that both count-guard tests passed; they did
not, and the `N variants` claim stayed invisible to
`command_docs_count_matches_registry` because the registry-length guard
does not read it. Corrected on 2026-10-20 while closing Eggwork M004:
`BuiltinSlashAction::Nodes` was added to `all_builtin_variants()`, and the
two `N variants` claims in this document were corrected to 159 against the
enum. The lesson repeats the one above — a count that is only asserted
against a *different* count is not a count that is checked.

Re-verified 2026-10-21 after this branch was rebased onto `main`. The merged
tree holds **168** built-in commands and **160** `BuiltinSlashAction` variants,
one more than either side alone: `main` registered `/nodes` and this branch
registered `/network`, and each side's pin had reached 167 commands and 159
variants on its own. The two `N variants` claims in this document are therefore
corrected to 160, and the five source anchors above are re-read against the
merged tree — `CommandAction` moved from `command.rs:309` to `:313`, the TUI
`Command` struct from `:318` to `:322`, `CommandRegistry` from `:496` to
`:500`, the count test from `:1209` to `:1217`, and `CommandConfig` from
`schema.rs:1493` to `:1821`.

Corrected this pass: the paragraph above previously carried three mutually
inconsistent figures ("asserts 157", "resolves to 157", "150 variants")
alongside a stale registry total, left over from earlier partial updates.
All now read from source on 2026-10-12. The bump is `/panels`, the second
spelling of the side-panel manager: `⇧S` is the letter `S` in Insert mode,
so without a typed route the switcher became unreachable mid-prompt once
`Ctrl+Alt+W` was retired.

Verified 2026-10-06 against source after upstream `2573f9c0` ("Decision runtime
ownership migration and M006 closure"), which added 30 lines near the top of
`crates/codegg-config/src/schema.rs`. Corrected: `CommandConfig`
`crates/codegg-config/src/schema.rs:1381`->`:1416` in both the section heading
and the note below it, re-checked to land on the struct declaration. No
`src/command/` reference in this document was affected.

One row moved on **2026-10-23** when the TUI text-rendering line added its
expand-all gesture: the built-in command count is **169** and the
`BuiltinSlashAction` variant count is **161**. The new command is
`/expand-all` (aliases `expand-disclosures`, `collapse-all`), which expands or
collapses every tool-call and reasoning disclosure in the transcript; it is the
same gesture as the `Ctrl+Alt+O` chord and dispatches to the same handler, so
the toast is identical. The count pins live in two files and move together:
`built_in_command_count_matches_release_docs` in `src/tui/command.rs` (the
registry length) and `command_docs_count_matches_registry` (this document's
counts, which the test parses and fails on drift). The variant total has its
own source-backed assertion,
`command_docs_builtin_slash_action_variant_count_matches_source`, which reads
the `(N variants` claims here against the enum. The two `N variants` claims in
this document were corrected to 161 against the enum, and the four
built-in-total claims to 169 against the registry; the **2026-10-21** paragraph
above is left as it was written, because it is a dated historical record of
that merge rather than a live claim.

Re-verified 2026-10-25 against `crates/codegg-core/src/command_catalog.rs`
(`CommandKind` `:37`, `CommandCatalog` `:175`, `the_catalog()` `:3178`,
`bounds` `:186`, `check_catalog_bounds()` `:3234`),
`crates/codegg-core/src/frontend_action.rs` (`FrontendAction` `:41`,
`action_for_form()` `:323`), `src/core/daemon_commands.rs`
(`build_inner_request()` `:148`, `serves_form()` `:519`) and
`src/tui/command.rs`: the cached built-in rows still number **169** and
`BuiltinSlashAction` still has **161** variants, byte-identical to
`origin/main` (`cb4ceed9d`). The daemon's catalog carries **161** commands
over **166** argument forms — 134 daemon-effect forms and 32 frontend-effect
forms.
