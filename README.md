# codegg

A lightweight, pure-Rust implementation of an AI coding agent.

## Features

- **Pure Rust** - No runtime dependencies, fast compilation and execution
- **Multiple Providers** - Use Anthropic, OpenAI, Google, Azure, Bedrock, and more
- **LSP Support** - Built-in bidirectional Language Server Protocol with code intelligence (semantic context, preview-only semantic checks), capability-gated operations, and supervision/restart lifecycle. Preview artifact registry with stale-base detection, list/detail inspection, and safe apply handoff (gated by `validate_preview_apply`). Model-tier-aware context policy with centralized policy resolver (`resolve_model_tier`) and workflow-specific defaults for all 12 recipe types. Stale evidence and unavailable server policies. Optional semantic memory cache (Phase 12) opt-in via `[lsp_semantic_cache]` mode = "memory", with conservative eviction on generation/hash/capability changes; `/lsp-cache-status` and `/lsp-cache-clear` for inspection. Tier 1 (rust-analyzer, basedpyright) and Tier 2 (gopls, typescript-language-server, clangd) server compatibility verified via CI; 39 servers configurable. Phase 13 `/lsp-doctor` (read-only, never starts servers) plus 10 Phase 14 workflow UX commands (`/lsp-repair-local`, `/lsp-repair-hunk`, `/lsp-review-file`, `/lsp-review-diff`, `/lsp-security-review`, `/lsp-impact`, `/lsp-test-repair`, `/lsp-interface`, `/lsp-cross-repair`, `/lsp-call-neighbors`) and Phase 15 `/lsp-context-diagnostics` (on-demand, no agent prompt bloat) all dispatch verified. **Phase 16 disk cache and Phase 17 manual lifecycle (`/lsp-start`, `/lsp-replay-docs`) are explicitly deferred** — memory-only cache and auto-start/restart remain the only active modes. See architecture/lsp.md and .codegg/skills/lsp/SKILL.md.
- **Plugin System** - WASM-based plugin extensibility with process-backed command plugins (Phase 4/6) and first-class builtin runtime (Phase 8): project-local slash commands can declare `runtime: process` to run executables with structured JSON output, timeout, and output capping. Runtime abstraction layer (`src/plugin/runtime/`) provides `PluginRuntime` trait with three implementations: `ProcessRuntime`, `WasmRuntime`, and `BuiltinRuntime` for native Rust handlers. Unified `PluginResponse` type, hardened registry semantics, and builtin plugins (copilot, codex, gitlab, poe) register through the same capability-indexed registry as external plugins. Six reference plugins + two SDKs ship under `examples/plugins/` (Phase 13): a vendorable Python helper (stdlib-only, 24 tests), a Rust WASM helper with the `codegg_plugin!` macro (11 tests), and runnable process / wasm / hook / status-widget examples.
- **TUI Interface** - Terminal user interface with syntax highlighting, scrollable dialogs for long output, unified status bar with activity indicators, and standardized dialog hints
- **Server Mode** - Headless HTTP server for remote access
- **Session Management** - Persistent conversations with SQLite storage
- **Context System** - Artifact storage, tool-output projection, cache-aware context packing (observe/diagnostic), and hardened gated active context policy (first use: phase-scoped tool-palette reduction (base-derived, non-cumulative, backoff/starvation, Warn dry-run, threshold gate; still disabled by default); volatile-tail compaction for late-context token reduction of old tool results with recovery handles; see architecture/cache-aware-context.md and `[context_policy]` config)
- **Human Shell** - Run shell commands with `!command` (ephemeral, hidden from model) or `!!command` (auto-promoted into context). Bounded output storage, safety policy blocking destructive commands, and TUI commands for listing, viewing details, including, rerunning, and killing commands. Shell detail view shows full metadata with promoted state and truncation info. The shell-output projection pipeline adds a parallel `CommandOutputStore` for durable raw stdout/stderr retention with stable `cmd://<id>/<stream>` handles (Phase 1), the `CommandOutputProjector` trait with `RawProjector` / `TruncatedProjector` / `ErrorRetentionProjector` plus a centralised `ProjectionSelector` and redaction-hook placeholder (Phase 2). Native structured projectors for Git and Rust toolchains (Phase 3). RTK discovery, eligibility classification, and real `RtkProjector` invocation (PostProcess/Wrapper modes) with strict wrapper grammar fallback and structured `ProjectionRawSemantics` (Phase 5+6). See `architecture/human_shell.md` and `plans/shell_output_projection_rtk_roadmap.md`.
- **Security** - SSRF protection, path validation, Landlock sandboxing, and security review workflow (diff-based preset selection, risk-marker-to-prompt synthesis, read-only evidence gathering via `securityContext`)

## Installation

### From Source

```bash
git clone https://github.com/anomalyco/codegg
cd codegg
cargo build --release
```

### Using cargo

```bash
cargo install --git https://github.com/anomalyco/codegg
```

## Quick Start

```bash
# Start a new session
codegg

# Resume last session
codegg -c

# Use specific model
codegg -m claude-sonnet-4-20250514

# Run a single prompt
codegg --run "Explain this code"
```

## Configuration

Create `~/.config/codegg/config.json` (or use `codegg.example.jsonc` as reference):

```json
{
  "model": "anthropic/claude-sonnet-4-20250514",
  "provider": "anthropic",
  "api_key": "sk-ant-..."
}
```

### Provider Setup

The tool supports multiple LLM providers:

| Provider              | Environment Variable                | Notes                        |
| --------------------- | ----------------------------------- | ---------------------------- |
| Anthropic             | `ANTHROPIC_API_KEY`                 | Primary recommended provider |
| OpenAI                | `OPENAI_API_KEY`                    |                              |
| Google Vertex         | `GOOGLE_API_KEY` / `VERTEX_PROJECT` |                              |
| AWS Bedrock           | AWS credentials                     | Uses AWS SDK                 |
| Azure OpenAI          | `AZURE_OPENAI_KEY`                  |                              |
| OpenRouter            | `OPENROUTER_API_KEY`                |                              |
| Cloudflare Workers AI | `CLOUDFLARE_API_TOKEN`              |                              |
| GitLab                | `GITLAB_TOKEN`                      |                              |
| Copilot               | `GITHUB_TOKEN`                      |                              |

#### Auth & Credentials

Beyond environment variables, providers can be configured through a typed
`auth` block on each `provider.<id>` entry, resolved by `codegg-providers`'s
`AuthResolver`:

```json
{
  "provider": {
    "openai": {
      "auth": { "type": "api_key", "env": "OPENAI_API_KEY" }
    },
    "anthropic": {
      "auth": { "type": "api_key" }
    },
    "xai": {
      "auth": { "type": "stored", "account_id": "default" }
    }
  }
}
```

Supported `auth.type` values:

| Type               | Status      | Notes |
|--------------------|-------------|-------|
| `api_key`          | Supported   | `env`, inline `value`, or `encrypted_value` |
| `stored`           | Supported (API keys) | Reference into the user-level credential store. Today this resolves stored API keys only; a future OAuth/bearer-token policy will gate stored `BearerToken` records. |
| `external_command` | Recognized  | Parsed; both `AuthResolver::resolve` and `ExternalCommandProvider::fetch` return `AuthError::Unsupported("ExternalCommand")` for any non-empty command. The previous `std::process::Command` shell-out path has been removed. Async timeout plumbing is a follow-up. |
| `oauth_device`     | Scaffolded  | Typed parsing only; resolution returns `AuthError::Unsupported` |
| `none`             | Supported   | Explicit "no auth" marker; bypasses env / store lookups |

Resolution order is: explicit `auth.env`, conventional `{PROVIDER}_API_KEY`,
inline `value`, decrypted `encrypted_value`, the user store, and finally the
legacy `api_key` field. Provider registration has a **single resolution
path** that runs through `resolve_provider_credential(...)`; no helper
reads `cfg.api_key` directly anymore. A `Credential` carries
`CredentialKind` (`ApiKey` or `BearerToken`) and an optional `expires_at`
so OpenAI-compatible providers can preserve metadata across registration.

A user-level encrypted credential store lives at
`~/.config/codegg/credentials.json` (or the platform config-dir equivalent).
Each `StoredCredentialRecord`'s `encrypted_secret` is encrypted with the
existing `CODEGG_MASTER_KEY` / `CODEGG_ENCRYPTION_KEY` /
`OPENCODE_ENCRYPTION_KEY` master key. Reading plaintext still works
without a master key for env / config-backed paths; **storing** a new
credential requires a master key and returns `AuthError::MasterKeyMissing`
if none is configured.

Manage stored credentials from the CLI:

```bash
codegg auth status                    # list stored credentials (metadata only)
printf '%s' "$OPENAI_API_KEY" | codegg auth set-key openai
                                       # read key from stdin, store under default account
printf '%s' "$OPENAI_API_KEY" | codegg auth set-key openai --account work
                                       # multi-account
codegg auth logout openai             # remove default-account record
codegg auth logout openai --account '*'    # remove all accounts for a provider
```

The `codegg auth` CLI validates provider and account ids
(`[A-Za-z0-9_-]`, with `*` allowed for `logout` only) and never echoes
key material in success or error messages. `status` never prints
encrypted ciphertext, raw secrets, or secret-derived fingerprints.

The `auth::mask_secret` helper renders any secret as a fixed 16-bullet
mask (`••••••••••••••••`) and never returns prefix or suffix of the input.
API keys entered into the TUI (e.g. in `/connect`) are rendered as this
fixed mask while typing, with a non-secret length hint (e.g. `(42 chars)`)
appended so users can still confirm the value was entered correctly.

> **Intentionally not implemented:** SuperGrok, Claude, ChatGPT, Copilot and
> other consumer-session / app-token flows. They require account-token reuse
> that is not part of any provider's documented public third-party API
> surface, and the CLI / TUI refuse to model them.

### Configuration Options

- `model` - Model ID (e.g., `anthropic/claude-sonnet-4-20250514`)
- `provider` - Provider name (default: `anthropic`)
- `api_key` - API key (or use environment variable)
- `base_url` - Custom endpoint for compatible APIs
- `timeout` - Request timeout in seconds (default: 300)
- `max_tokens` - Maximum tokens per response
- `temperature` - Sampling temperature (0.0-1.0)
- ` compaction` - Context compaction settings
- `tools` - Enabled tools and configurations
- `lsp` - LSP server configurations

## CLI Commands

```bash
# Session management
codegg sessions              # List sessions
codegg session <id>          # View session
codegg export <id> -o file  # Export to JSON
codegg import <file>        # Import from JSON

# Model discovery
codegg providers            # List providers (config-aware; same as `models`)
codegg models              # List all models
codegg models -p anthropic  # List provider models

# Server mode
codegg server --host 0.0.0.0 --port 8080
codegg attach http://localhost:8080 --token TOKEN

# Other
codegg upgrade             # Upgrade to latest version

# Credential store management
codegg auth status
codegg auth set-key <provider>
codegg auth logout <provider>
```

## TUI Slash Commands

The TUI supports inline slash commands for quick actions.

| Command | Description |
|---------|-------------|
| `/help` | Show help dialog |
| `/tree` | Open file tree dialog |
| `/model [name]` | Open model selection or switch to model |
| `/agent [name]` | Open agent selection dialog or switch to named agent |
| `/agents` | List visible agents |
| `/agents --all` | List all agents including hidden |
| `/agents show <name>` | Show agent details and configuration |
| `/agents diff <name>` | Compare agent against built-in base |
| `/agents validate` | Validate agent configuration and diagnostics |
| `/agents reload` | Reload agents from files and config |
| `/clear` or `/new` | Clear session and start new one |
| `/compact` | Trigger manual context compaction |
| `/connect` | Open API key connection dialog |
| `/status` | Show session status and token usage |
| `/lsp-status` | Show LSP server status and context packet details |
| `/lsp-servers` | List active LSP servers with root, state, generation, pending requests, open docs |
| `/lsp-capabilities <key>` | Show effective capability snapshot for a server |
| `/lsp-errors <key>` | Show error/health info including stderr tail |
| `/lsp-root <path>` | Diagnose root detection for a file path (read-only, no server start) |
| `/lsp-restart <key>` | Manually restart a specific LSP server |
| `/lsp-stop [key]` | Stop LSP servers (all or specific) |
| `/lsp-preview-apply <id>` | Apply a previewed patch to disk with SHA-256 hash revalidation; blocks stale previews |
| `/lsp-cache-status` | Show LSP semantic memory cache status (mode, entries, bytes, hits, evictions) |
| `/lsp-cache-clear [--all\|<root>]` | Clear LSP semantic cache (all entries or for a specific workspace root) |
| `/lsp-doctor <path>` | Read-only diagnostic report for a file (no server start) |
| `/lsp-context-diagnostics <path>` | Show context-shaping diagnostics on demand (model tier, policy, cache state) |
| `/lsp-repair-local <path[:line]>` | Repair localized issue around a target line or diagnostic |
| `/lsp-repair-hunk <path> [hunk]` | Repair code around changed diff hunks |
| `/lsp-review-file <path>` | Semantic review of a single file |
| `/lsp-review-diff` | Review changed files in current diff |
| `/lsp-security-review [path\|diff]` | Enriched security review with LSP evidence |
| `/lsp-impact <path:line:col>` | Impact analysis for a symbol change |
| `/lsp-test-repair <file> [text]` | Test failure repair with heuristic symbol extraction |
| `/lsp-interface <path[:symbol]>` | API/interface boundary review |
| `/lsp-cross-repair <primary> [related]` | Cross-file repair context |
| `/lsp-call-neighbors <path:line:col> [dir]` | Call neighborhood (incoming/outgoing/both) |
| `/context` | Open context dialog |
| `/cost` | Show cost/usage statistics |
| `/usage` | Open usage details dialog |
| `/themes` | Open theme selection dialog |
| `/tui` | Toggle fullscreen mode |
| `/sessions` | Open session management dialog |
| `/goto <id>` | Jump to message by ID |
| `/share` | Share session (get URL) |
| `/unshare` | Unshare current session |
| `/timeline` | Show session timeline |
| `/undo` | Undo last message |
| `/redo` | Redo undone message |
| `/export` | Export session to clipboard |
| `/import` | Open session import dialog |
| `/timestamps` | Toggle message timestamps |
| `/thinking` | Toggle thinking/reasoning visibility |
| `/models-refresh` | Refresh model cache from providers |
| `/variants` | Show model variants |
| `/mcps` | Show MCP server status |
| `/fork` | Fork session from selected message |
| `/worktree` | Manage git worktrees |
| `/editor` | Open external editor for prompt |
| `/loop <interval> "<msg>"` | Schedule recurring background task |
| `/tasks` | List background tasks |
| `/task-del <id>` | Delete background task |
| `/shell-list` | List recent shell commands with status |
| `/shell-show <id>` | Show full details of a shell command in a scrollable dialog |
| `/shell-include <id> [mode]` | Promote shell output into context (stdout/stderr/all) |
| `/shell-ask <id>` | Ask the agent about a shell command's output |
| `/shell-rerun <id>` | Rerun a previous shell command |
| `/shell-kill <id>` | Kill a running shell command |
| `/test` | Run supervised project tests with compact reporting and full logs under `.codegg/test-runs/`. Custom commands (`/test custom <argv>`) are validated as argv-prefix matches against a strict 12-entry allowlist — shell metacharacters, redirection, pipes, command substitution, and newlines are rejected. |
| `/exit`, `/quit`, `/q` | Exit the application |
| `/skill:<name>` | Activate a skill |
| `/skills` | List available skills |
| `/commit` | Generate commit message with AI |
| `/init` | Initialize project memory |
| `/memory` | List, edit, toggle memories |

## Keyboard Shortcuts

The TUI provides keyboard shortcuts for common actions.

> **Mode-aware help:** The help dialog (`?`) displays different shortcuts depending
> on the current input mode. In **insert mode**, only modifier-based shortcuts
> (Ctrl+*, Shift+*) are shown — bare `?` and `/` insert text, and `j`/`k` are
> regular characters. In **normal mode**, bare navigation keys (`?`, `/`, `j`, `k`,
> `h`, `l`) appear as shortcuts. Press `?` in normal mode to open the help dialog.

### Global Shortcuts

| Key | Action |
|-----|--------|
| `Enter` | Send message |
| `Shift+Enter` | Insert newline in prompt |
| `Esc` / `Ctrl+C` | Cancel / close dialog |
| `Tab` | Switch agent |
| `Ctrl+L` | Open model selector |
| `Ctrl+K` | Clear session |
| `Ctrl+N` | New session |
| `Ctrl+T` | Toggle sidebar |
| `Ctrl+W` | Close session |
| `Ctrl+Q` | Quit application |
| `j/k` or `↑/↓` | Navigate messages |
| `PgUp/PgDown` | Scroll viewport |
| `/` | Focus prompt with slash |
| `?` | Show help dialog |
| `@` | Mention subagent in prompt |
| `↑/↓` | Prompt history navigation |
| `Ctrl+S` | Stash prompt |
| `Ctrl+R` | Restore prompt |
| `Ctrl+P` | Cycle model forward |
| `Ctrl+Shift+P` | Cycle model backward |
| `Ctrl+Y` | Toggle TTS (speak) |
| `Ctrl+Shift+Y` | Stop TTS playback |
| `Ctrl+Shift+F` / `/tui` | Toggle fullscreen |
| `t` | Toggle reasoning/thinking visibility |
| `Ctrl+E` | Open external editor |
| `Ctrl+F` | Search within messages |
| `F11` | Toggle IDE diff view |

### Prompt Editing

| Key | Action |
|-----|--------|
| `Backspace` | Delete character before cursor |
| `Delete` | Delete character at cursor |
| `←/→` | Move cursor left/right |
| `Home` / `End` | Move to start/end of line |
| `Ctrl+V` | Paste from clipboard |

### Session Dialog

| Key | Action |
|-----|--------|
| `b` | Toggle bulk mode |
| `Space` | Select/deselect session (in bulk mode) |
| `Ctrl+A` | Select all sessions (in bulk mode) |
| `d` | Delete selected session(s) |
| `a` | Archive selected session(s) |
| `e` | Export selected session(s) (in bulk mode) |
| `D` | Cycle date presets |
| `Enter` | Rename session / open session |
| `g` | Open goto dialog |

### Vim Mode

Enable vim bindings in config: `"vim_mode": true`

| Key | Action |
|-----|--------|
| `j/k` | Navigate down/up |
| `h/l` | Move cursor left/right |
| `i` | Focus prompt |
| `:` | Enter command mode |
| `g` | Go to top |
| `Ctrl+d` | Page down |
| `Ctrl+u` | Page up |
| `n` | New session |
| `q` | Quit |

## Usage Workflows

### Code Review

```bash
# Start review session
codegg

# Activate review skill
/skill:code-review

# Review changes
@code-review Review changes in src/

# Or use built-in review
Review the recent commits and summarize key changes.
```

### Debugging

```bash
# Start debugging session
codegg

# Describe the bug, agent will analyze and propose fixes
The application crashes when clicking the login button.

# Use specific agent for debugging
@debugger Analyze the stack trace in logs/error.log

# Step-by-step debugging
Debug why the API returns 401. Check: auth middleware, token validation, headers.
```

### Refactoring

```bash
# Start refactoring session
codegg -m claude-sonnet-4-20250514

# Request refactoring with context
Refactor the authentication module to use async/await.
Focus on: error handling, performance, testability.

# Gradual refactoring with subagents
@refactor Refactor src/auth/ to use tokio async patterns
@test-generator Add tests for the refactored auth module
```

### Test-Driven Development

```bash
# Start TDD session
codegg

# Generate tests first
Write unit tests for the UserManager class. Cover: create, delete, update, permissions.

# Implement to pass tests
Now implement the UserManager to make those tests pass.

# Run and iterate
/loop 5m "run tests and report failures"
```

### Documentation Generation

```bash
# Generate docs for a module
@docs Generate README and API docs for src/api/

# Update existing docs
Update CLAUDE.md with the new configuration options added in this PR.
```

### Git Operations

```bash
# Generate commit message
/commit

# Analyze PR changes
Review this PR and suggest improvements: focus on security, performance, code quality.

# Generate release notes
Generate release notes from git log since v1.0.0. Group by: features, fixes, breaking changes.
```

## MCP Servers

The Model Context Protocol (MCP) enables integration with external tools and services. Configure MCP servers in your config file:

```json
{
  "mcp": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/path/to/allowed"]
    },
    "brave-search": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-brave-search"],
      "env": {
        "BRAVE_API_KEY": "your-api-key"
      }
    }
  }
}
```

### Local MCP Servers

Local MCP servers run as child processes. Configure in `config.json`:

```json
{
  "mcp": {
    "server-name": {
      "command": "/path/to/server",
      "args": ["arg1", "arg2"],
      "env": {
        "ENV_VAR": "value"
      }
    }
  }
}
```

### Remote MCP Servers

Remote MCP servers connect over HTTP. Configure with `url`:

```json
{
  "mcp": {
    "remote-server": {
      "url": "https://mcp.example.com/server",
      "auth": {
        "type": "bearer",
        "token": "your-token"
      }
    }
  }
}
```

### OAuth Configuration

MCP supports OAuth authentication:

```json
{
  "mcp": {
    "server-name": {
      "url": "https://mcp.example.com/server",
      "oauth": {
        "client_id": "your-client-id",
        "client_secret": "your-client-secret",
        "auth_url": "https://auth.example.com/authorize",
        "token_url": "https://auth.example.com/token"
      }
    }
  }
}
```

## Skills System

Skills provide specialized capabilities to the agent. They are loaded from markdown files with YAML frontmatter.

### Location

Skills are loaded from:
- `~/.config/codegg/skills/` - User-level skills
- `.codegg/skills/` - Project-level skills

### Format

Skills are defined in markdown files with YAML frontmatter:

```markdown
---
name: code-review
description: Performs thorough code reviews
version: "1.0"
tags: [review, quality]
---

You are a code review agent specialized in finding bugs and security issues.
```

### Usage

Activate a skill during a session using the `/skill:` command:

```
/skill:code-review
```

List available skills with `/skills` command.

## TTS (Text-to-Speech)

The TUI includes text-to-speech support for reading agent responses aloud.

### Controls

| Key | Action |
|-----|--------|
| `Ctrl+Y` | Toggle TTS - speaks the selected message |
| `Ctrl+Shift+Y` | Stop TTS playback |

### Platform Support

- **macOS**: Uses built-in `say` command
- **Linux**: Requires `speech-dispatcher` package

When TTS is enabled, the footer displays a speaker icon indicating speaking status.

## Subagents

The agent can spawn subagents for parallel task execution. Subagents run independently with shared context from the parent session.

### @mention Syntax

Invoke subagents during a conversation using `@agent_name`:

```
@code-review Review the changes in src/
```

### Defining Subagents

Define subagents in your config file:

```json
{
  "agent": {
    "code-review": {
      "description": "Code review agent",
      "model": "anthropic/claude-sonnet-4-20250514",
      "temperature": 0.1,
      "prompt": "You are a code review agent. Focus on finding bugs."
    }
  }
}
```

Subagents support up to 5 concurrent tasks.

## Agent Customization

Define custom agents via TOML or Markdown files. Place them in `~/.config/codegg/agents/` (global) or `.codegg/agents/` (project-specific).

### Quick Start

Create `.codegg/agents/code-reviewer.toml`:

```toml
[agent]
name = "code-reviewer"
description = "Focused code review agent"
mode = "subagent"
color = "yellow"

[agent.permissions]
read = "allow"
grep = "allow"
write = "deny"
edit = "deny"
bash = "ask"
```

Then use it: `/agent code-reviewer` to switch, or `@code-reviewer review this code` to spawn as subagent.

### Commands

| Command | Description |
|---------|-------------|
| `/agents` | List visible agents |
| `/agents --all` | List all agents including hidden |
| `/agents show <name>` | Show agent details, permissions, source |
| `/agents diff <name>` | Compare against built-in base |
| `/agents validate` | Check agent configuration |
| `/agents reload` | Reload agents from disk |
| `/agent <name>` | Switch to a different agent (primary/all modes only; use @mention for subagents) |

### TOML Format

```toml
[agent]
name = "my-agent"
mode = "subagent"          # primary, subagent, or all (lowercase required)
description = "What this agent does"
model = "anthropic/claude-sonnet-4-20250514"  # optional override
color = "cyan"             # optional

[agent.permissions]
read = "allow"
write = "deny"
bash = "ask"

[agent.bash_permission]    # fine-grained bash control (TOML only)
action = "ask"
allow_patterns = ["git diff*", "cargo test*"]
deny_patterns = ["rm *", "sudo *"]

[agent.path_permission]    # fine-grained file access control (TOML only)
allow = ["src/**", "tests/**"]
deny = [".git/**", "target/**"]
```

### Markdown Format

```markdown
---
name: my-agent
mode: subagent
description: A custom agent
---

You are a helpful assistant focused on code quality.
```

Markdown frontmatter supports: `name`, `mode`, `description`, `model`, `temperature`, `color`, `steps`, `hidden`, `prompt`, `prompt_file`, `permission` (flat map), `disable`. The markdown body becomes the agent's prompt unless `prompt` or `prompt_file` is set.

> **Note:** Markdown is a **prompt-first, merge-only** format. It does not support overlay flags (`replace`, `merge`) or structured permission sections (`[bash_permission]`, `[path_permission]`). Use TOML for those features.

### Overlay Flags

Control how file-based agents interact with built-in agents (TOML only):

- `replace = false` (default): Merge mode — overlay fields applied on top of base
- `replace = true`: Full replacement — overlay completely replaces the base agent
- `merge = true`: Explicitly enable merge mode (same as default, for clarity)
- `disable = true`: Remove agent from resolution entirely

### Permission Actions

| Action | Behavior |
|--------|----------|
| `allow` | Execute without asking |
| `ask` | Prompt for user confirmation |
| `deny` | Block execution entirely |

See `examples/agents/` for complete examples including code-reviewer, test-writer, docs-writer, and custom build overrides.

## Server Mode

Start the HTTP server for remote access:

```bash
codegg server --host 0.0.0.0 --port 8080
```

Set `CODEGG_SERVER_PASSWORD` for authentication.

## Environment Variables

The tool uses various environment variables for configuration beyond provider API keys.

### Server Configuration

| Variable | Description |
|---------|-------------|
| `CODEGG_SERVER_TOKEN` | Token for WebSocket/API authentication |
| `CODEGG_SERVER_PASSWORD` | Password for HTTP server authentication |
| `CODEGG_SERVER_HOST` | Server bind address (default: `127.0.0.1`) |
| `CODEGG_SERVER_PORT` | Server port (default: `31415`) |
| `CODEGG_CORS_ORIGINS` | CORS allowed origins (comma-separated) |
| `CODEGG_SERVER_AUTH_DISABLED` | Disable auth if set |
| `CODEGG_SERVER_USERNAME` | Username for server auth |

### Rate Limiting

| Variable | Description |
|---------|-------------|
| `REDIS_URL` | Redis URL for distributed rate limiting |
| `RATE_LIMIT requests` | Requests per minute (default: 60) |
| `RATE_LIMIT_TOKENS` | Tokens per minute (default: 100000) |

### Logging

| Variable | Description |
|---------|-------------|
| `RUST_LOG` | Tracing log level (`error`, `warn`, `info`, `debug`, `trace`) |
| `RUST_LOG_FORMAT` | Log format (`pretty`, `json`) |
| `CODEGG_LOG_LEVEL` | Alternative log level setting |

### Tool Configuration

| Variable | Description |
|---------|-------------|
| `CODEGG_ALLOWED_PATHS` | Allowed filesystem paths |
| `CODEGG_DENIED_PATHS` | Denied filesystem paths |
| `CODEGG_WORKDIR` | Restrict agent to working directory |
| `EDITOR` | Editor for file editing (`vim`, `code`, `nano`) |
| `GIT_EDITOR` | Editor for git operations |

### Provider API Keys

| Variable | Description |
|---------|-------------|
| `ANTHROPIC_API_KEY` | Anthropic API key |
| `OPENAI_API_KEY` | OpenAI API key |
| `GOOGLE_API_KEY` | Google AI API key |
| `VERTEX_PROJECT` | GCP project for Vertex AI |
| `VERTEX_LOCATION` | GCP location for Vertex AI |
| `AZURE_OPENAI_KEY` | Azure OpenAI API key |
| `OPENROUTER_API_KEY` | OpenRouter API key |
| `OPENCODE_ZEN_API_KEY` | OpenCode Zen API key |
| `MISTRAL_API_KEY` | Mistral AI API key |
| `GROQ_API_KEY` | Groq API key |
| `DEEPINFRA_API_KEY` | DeepInfra API key |
| `CEREBRAS_API_KEY` | Cerebras API key |
| `COHERE_API_KEY` | Cohere API key |
| `TOGETHERAI_API_KEY` | Together AI API key |
| `PERPLEXITY_API_KEY` | Perplexity API key |
| `XAI_API_KEY` | xAI API key |
| `VENICE_API_KEY` | Venice AI API key |
| `EXA_API_KEY` / `EXA_CODE_API_KEY` | For websearch/codesearch tools |

### Provider Overrides

| Variable | Description |
|---------|-------------|
| `ANTHROPIC_BASE_URL` | Override Anthropic API endpoint |
| `OPENAI_BASE_URL` | Override OpenAI API endpoint |
| `CLOUDFLARE_API_TOKEN` | Cloudflare Workers AI token |
| `GITLAB_TOKEN` | GitLab API token |
| `GITHUB_TOKEN` | GitHub/Copilot token |

### Security Keys

| Variable | Description |
|---------|-------------|
| `CODEGG_MASTER_KEY` / `CODEGG_ENCRYPTION_KEY` / `OPENCODE_ENCRYPTION_KEY` | Master key for the user-level credential store at `~/.config/codegg/credentials.json` and for any `provider.<id>.auth.encrypted_value` field. Required to **store** new credentials; not required to read env / config-backed keys. |
| `CODEGG_TOKEN_KEY` | Encryption key for MCP OAuth tokens |
| `CODEGG_PERM_KEY` | Permission signature key |

### IDE Integration

| Variable | Description |
|---------|-------------|
| `VSCODE_IPC_HOOK` | Set when VS Code is running |
| `JETBRAINS_REMOTE` | Set when JetBrains IDE is running |
| `JETBRAINS_TOOL` | JetBrains tool name |
| `CODEGG_IDE` | Set to `jetbrains` by plugin |
| `CODEGG_EXECUTABLE` | Path to codegg executable |

### Session Configuration

| Variable | Description |
|---------|-------------|
| `CODEGG_SHARE_DURATION_DAYS` | Share link duration in days |
| `CODEGG_TUI_CONFIG` | Custom TUI config path |

### Misc

| Variable | Description |
|---------|-------------|
| `HOME` | User home directory |
| `USER` | Current username |
| `SHELL` | User shell |
| `TERM` | Terminal type |
| `PATH` | Used by bash tool and LSP |
| `PWD` | Current directory (used by agent) |

## Compaction Configuration

The agent supports automatic context compaction to manage token limits efficiently.

### Configuration

Configure in your config file:

```json
{
  "compaction": {
    "enabled": true,
    "threshold": 0.85,
    "max_tokens": 128000,
    "prune_tool_outputs": true
  }
}
```

### Options

- `enabled` - Enable auto-compaction (default: false)
- `threshold` - Token usage percentage to trigger compaction (0.1-1.0, default: 0.85)
- `max_tokens` - Context window limit (minimum 1000)
- `prune_tool_outputs` - Prune old tool outputs during compaction (default: false)

### Strategies

The compaction system automatically selects strategies based on message patterns:
- `drop_middle` - Removes middle messages
- `prune_tool_outputs` - Reduces tool output detail
- `summarize` - Uses LLM to summarize context

### Volatile-Tail Compaction

A gated, late-context-only compaction policy for reducing token pressure from old tool results. Disabled by default; configure under `[context_policy]`:

```json
{
  "context_policy": {
    "volatile_tail_compaction": false,
    "volatile_tail_mode": "observe",
    "min_volatile_tokens_for_compaction": 12000,
    "preserve_recent_messages": 12,
    "max_compacted_tail_tokens": 8000,
    "require_effective_cost_signal": true,
    "compact_tool_results_only_first": true
  }
}
```

- **Rollout**: observe → warn → compact (all disabled by default)
- **Preserves**: system prefix, user messages, assistant tool-call messages, recent tail
- **Requires**: recovery handles (`ctx://`) on tool results for compaction
- **Recovery**: use `context_read` with the recovery handle from the tombstone
- **Idempotent**: already-compacted messages are skipped on repeated application
- See `architecture/cache-aware-context.md` for full details.

## Permission System

Path-based access control with three permission levels.

### Permission Levels

| Level | Description |
|-------|-------------|
| `allow` | Tool executes immediately |
| `deny` | Returns error to LLM, no execution |
| `ask` | Shows dialog, execution pauses until response |

### Configuration

Configure permissions in your config file:

```json
{
  "permission": {
    "default": "ask",
    "skill": "allow",
    "bash": "ask",
    "paths": ["/src/**", "!/**/test/**"],
    "bash_patterns": ["git *", "ls"]
  }
}
```

### Tool Rules

Use glob patterns for tool name matching:

```json
{
  "permission": {
    "tool_rules": [
      { "tool": "bash", "level": "ask", "bash_patterns": ["git *", "ls"] },
      { "tool": "write", "level": "deny", "paths": ["/etc/**", "/root/**"] }
    ]
  }
}
```

### Bash Command Patterns

Control specific bash commands with patterns:

- `git *` - Matches all git commands
- `rm *` - Matches all remove commands
## Project Structure

```
src/
├── agent/        # Agent loop, compaction, routing, team coordination
├── auth/         # AuthConfig, Credential, AuthResolver, user credential store (re-exports from codegg-providers)
├── bus/          # Event bus for internal messaging (re-exports from codegg-core)
├── client/       # Client for server mode
├── command/      # CLI command implementations
├── config/       # Configuration (re-exports from codegg-config)
├── eggsact/      # Eggsact adapter: in-process deterministic tool runtime
├── lsp/          # Language Server Protocol support (thin shim; authoritative impl in crates/egglsp)
├── mcp/          # Model Context Protocol client
├── permission/   # Permission checking, DoomLoop detection, mode system
├── plugin/       # WASM plugin system with process/wasm/builtin runtimes
├── preflight/    # Harness-side preflight validation (wraps eggsact)
├── provider/     # LLM providers (re-exports from codegg-providers)
├── search_backend/ # Search backend dispatch (eggsearch MCP + builtin fallback)
├── security/     # SSRF protection, Landlock sandboxing, security review workflow
├── server/       # HTTP/WebSocket server (feature-gated behind `server`)
├── session/      # Session management and storage (re-exports from codegg-core)
├── shell/        # Human shell (! commands), projection pipeline, RTK integration
├── skills/       # Skill system (re-exports from codegg-config)
├── snapshot/     # State snapshots (re-exports from codegg-core)
├── storage/      # SQLite storage layer (re-exports from codegg-core)
├── tool/         # ~40 built-in tools (bash, read, edit, grep, glob, lsp, git, websearch, webfetch, deterministic tools, etc.)
├── tui/          # Terminal UI (app, components, runtime, commands)
├── upgrade/      # Self-upgrade functionality
├── util/         # Utilities (clipboard, fuzzy search, pricing, metrics)

crates/
├── codegg-config/      # Configuration schema, paths, loading, validation, watching
├── codegg-core/        # Domain types: bus, error, goal, memory, session, storage, snapshot, worktree, resilience
├── codegg-protocol/    # Core protocol types (CoreRequest, CoreResponse, CoreEvent, TuiMessage)
├── codegg-providers/   # LLM provider implementations, auth types, CircuitBreaker
├── eggsentry/          # Security scanning (secrets, commands, deps, profiles)
├── eggcontext/         # Token counting and context utilities
├── egggit/             # Read-only git facts (status, diff, changed files)
├── egglsp/             # Language Server Protocol client/service/operations (39 servers)
└── egglsp-test-server/ # Fake LSP server binary for integration tests
```

### Tool Backends

Codegg supports multiple tool backends:

- **Native** — In-process workspace crates (`egglsp`, `egggit`, `eggsentry`, `eggsact`)
- **MCP** — External MCP servers (default: `eggsearch` for web search/fetch)
- **Builtin** — Legacy in-tree fallback implementations

The `eggsearch` MCP server provides web search, repository search, and
evidence gathering. It is the default backend for `websearch`, `webfetch`,
and 7 additional wrapper tools (`repo_search`, `repo_fetch`, `repo_map`,
`security_search`, `research_search`, `batch_fetch`, `evidence_bundle`).

The `eggsact` crate provides in-process deterministic tools for text
comparison, config validation, and security inspection. These run
entirely in-process with no external dependencies.

Use `/tool-backends` in the TUI to inspect the active backend for each
tool. Use `codegg doctor search` to check eggsearch connectivity and
`codegg doctor deterministic-tools` to verify eggsact status.

### Tool Backend Configuration

Configure backends in `opencode.json` (TOML or JSON):

#### Search Backend (eggsearch)

Default — no config needed if `eggsearch` is installed. To customize:

```toml
[search]
backend = "eggsearch"           # "eggsearch" | "builtin" | "disabled"
expose_raw_mcp_tools = false   # hide raw mcp__eggsearch__* tools
fallback_to_builtin = false     # fall back to legacy if eggsearch unavailable
max_search_output_chars = 12000
max_fetch_output_chars = 20000

[search.eggsearch]
server_name = "eggsearch"
command = "eggsearch"
args = ["mcp", "stdio"]
timeout_ms = 60000
```

Install `eggsearch` separately — it is not bundled with Codegg:

```bash
cargo install eggsearch --version 0.3.4
```

#### Deterministic Tools (eggsact)

Enabled by default. No external process needed — eggsact is an in-process
Rust dependency. To customize:

```toml
[deterministic_tools]
enabled = true
backend = "native"                # "native" | "disabled"
profile = "codegg_core"           # "codegg_core" | "codegg_core_min" | "default" | "full"
expose_expert_tools = false       # expose deferred tools to model
max_output_chars = 12000
```

#### Preflight

Harness-side validation before mutating operations. Enabled by default in
`warn` mode (surfaces findings without blocking). To customize:

```toml
[preflight]
enabled = true
mode = "warn"                     # "off" | "observe" | "warn" | "block_on_definite"
patch = true                      # validate edits/replacements
config = true                     # validate config writes
shell = true                      # validate shell commands
unicode = true                    # validate unicode safety
```

## Security

### Threat Model

This tool provides an agent system with access to powerful tools including shell execution and file operations. Key security considerations:

- **No Sandbox** - The agent runs with user privileges. Use container isolation if needed.
- **Server Mode** - Enable authentication with `CODEGG_SERVER_PASSWORD`.
- **Permissions** - Configure permission rulesets to limit tool access.
- **File Access** - Use path restrictions to limit filesystem access.

### Security Review Workflow

The agent includes a built-in security review workflow (`src/security/workflow/`) for defensive code review. It parses unified diffs, applies path exclusions, selects `securityContext` presets per file, builds context-gathering requests, and converts risk markers into review prompts. The workflow is read-only and never mutates files. Risk markers are review prompts, not confirmed findings.

The async orchestrator `run_security_review_workflow(root, base, options)` runs the full pipeline (discover targets → preflight checks → evidence-based synthesis → assemble output). It does not execute `securityContext` LSP requests. `SecurityReviewWorkflowOptions` controls which stages run and caps output counts. Evidence-based findings are heuristic defensive review outputs, not proof of exploitability.

An optional LSP enrichment pass (`--enrich`) executes bounded, read-only `securityContext` requests for escalated targets via the `SecurityContextExecutor` trait and reruns finding synthesis with enriched evidence. The `LspSecurityContextExecutor` adapter wraps `LspTool` for real LSP delegation; `validate_security_context_request()` guards request payloads. No-executor runtimes fail soft with clear notes. Enrichment is opt-in, read-only, bounded, and never mutates files. `securityContext` reuses the shared diagnostic freshness metadata and capability snapshot used by the semantic-context path, but still produces a security-filtered packet rather than a verdict.

An optional `hunkSourceContext` evidence pass (`--hunk-context` flag, or `enable_hunk_source_context` in workflow options) causes best-effort invocation of `hunkSourceContext` for changed files via the `HunkSourceContextExecutor` trait and injects hunk navigation evidence (enclosing symbols, diagnostics, definitions, references) into the evidence-based synthesis. `LspHunkSourceContextExecutor` (`src/security/lsp_executor.rs`) is the real adapter that calls `LspTool::execute_hunk_source_context_typed()` directly with a typed `HunkSourceNavigationRequest` — no JSON round-trip. The model-facing tool schema remains patch-only; internal pre-parsed hunk descriptors are used via the typed API. A `HunkSourceContextPolicy` decides whether to invoke `hunkSourceContext` based on file extension, patch size (using actual per-file patch data), and hunk count. Per-file selection order is deterministic and bounded. Policy skip decisions are routing metadata, never security evidence — only real `HunkSourceNavigationResponse` produces `HunkNavigation` evidence. Errors are fail-open: noted but never blocking. LSP results remain server-dependent and fail-open. `SecurityEvidenceKind::HunkNavigation` evidence is recognized by `is_finding_eligible()` as a supporting dimension but requires `RiskMarker` or `Preflight` alongside to form a finding. Multi-file patches are processed one file at a time in deterministic sorted order. Semantic collection is first-hunk-centered.

The `/security-review` TUI command exposes the workflow with flags: `--changed`, `--base <ref>`, `--json`, `--prompts-only`, `--findings-only`, `--no-content`, `--no-filename`, `--max-findings N`, `--max-prompts N`, `--enrich`, `--max-enriched-targets N`, `--lsp-timeout-ms N`, `--hunk-context`, `--panel`. By default the report goes to the timeline and the result panel can be reopened with `/security-review-show`. The `--panel` flag auto-opens the result panel on completion. The command runs asynchronously in the TUI so the UI remains responsive while the review is in flight; a reentrancy guard (`App.security_review_running`) blocks repeated invocations and the full report is delivered via the message timeline as an Assistant message with a `[Security Review]` label, plus a brief toast confirming completion.

Each successful run stores a structured `SecurityReviewReceipt` on the App so the result can be reopened later without rerunning the review:

- `/security-review-show` reopens the latest result panel (`Dialog::SecurityReview`) from the in-memory receipt.
- `/security-review-cancel` aborts an in-flight review via `AbortHandle::abort()`; cancellation is best-effort and stale completions are ignored.
- The result panel supports master/detail navigation (`j`/`k` or `↑`/`↓`, `PgUp`/`PgDn`), filter cycling (`f` — including `HunkBacked` to show only items with hunk context), notes toggle (`n`), prompts toggle (`p`), and `Enter` to open a read-only source preview dialog for the finding's file. The source preview is root-scoped and falls back to copying `path[:line]` to the clipboard if the file cannot be opened. When a finding or prompt has a matching hunk (derived from the reviewed diff, not live files), the detail section renders hunk context with added/removed/context line styling. Items without matching hunks render gracefully. The review itself is read-only by design — no file mutations.
- Receipt persistence is in-memory only (`App.latest_security_review`); cleared on app restart.

### Best Practices

1. **Use containers** for untrusted code review
2. **Configure permissions** to restrict dangerous tools
3. **Review commands** before approving
4. **Secure server** with authentication in production

## Migrating to eggsearch/eggsact

As of this release, Codegg uses two external backends for search and
deterministic tool operations:

### Search (eggsearch)

- **Before:** Codegg shipped in-tree web search providers (Exa, Tavily,
  Brave, DuckDuckGo, Mojeek, etc.) behind the `websearch` and `webfetch`
  tools.
- **After:** Codegg defaults to `eggsearch`, an external MCP server that
  provides web search, repository search, and evidence gathering.
- **Migration:** Install `eggsearch` (`cargo install eggsearch --version 0.3.4`). Codegg
  auto-detects it on startup.
- **Fallback:** If `eggsearch` is unavailable, Codegg falls back to the
  legacy built-in providers when `[search].fallback_to_builtin = true`.
  Set `[search].backend = "builtin"` to force the legacy path.
- **Removal:** The built-in providers are maintained but will not grow
  new features. New search providers belong in the eggsearch project.

### Deterministic tools (eggsact)

- **Before:** Some text comparison and config validation was done inline.
- **After:** Codegg uses `eggsact`, an in-process Rust library providing
  deterministic tools (`text_equal`, `validate_json`, `validate_toml`,
  `command_preflight`, `path_normalize`, `text_security_inspect`, etc.).
- **Migration:** No action needed — eggsact is a Rust dependency bundled
  with Codegg.
- **Discovery:** Some eggsact tools are deferred and discoverable via
  `tool_search` rather than always visible to the model.

### Configuration

All backends are configurable in `opencode.json`. See the
[Tool Backend Configuration](#tool-backend-configuration) section for
TOML examples. Key config sections:

- `[search]` — backend selection, output caps, eggsearch connection
- `[deterministic_tools]` — eggsact profile, tool exposure
- `[preflight]` — pre-edit/command validation mode

## Contributing

See [AGENTS.md](./AGENTS.md) for development guidelines:

- Use single-word names for variables and functions
- Prefer early returns over else branches
- Run `cargo test` before submitting PRs
- Run `cargo clippy -- -D warnings` to check code quality
- Keep functions focused and composable

## License

MIT
