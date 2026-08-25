# Permission Module

The `permission` module enforces access control for tool execution and
file paths. It provides a multi-layered ruleset system with cached
decisions, HMAC-signed persistence, DoomLoop detection, and
mode-based permission workflows.

## Purpose

Enforce per-tool, per-path, and per-command permission decisions before
any tool executes. Prevent prompt fatigue via cached decisions, detect
stuck-agent loops, and provide mode-based permission envelopes
(review/debug/docs) for specialized workflows.

## Where It Lives

| Artifact | Location |
|----------|----------|
| `PermissionChecker`, `PermissionStore`, `DoomLoopDetector`, ruleset types | `src/permission/mod.rs` |
| `ModeDefinition`, `BuiltinModes` (review/debug/docs) | `src/permission/modes.rs` |
| `PermissionRegistry` (ask-response broker) | `crates/codegg-core/src/bus/mod.rs` |
| `PermissionDecision` (bus DTO) | `crates/codegg-core/src/bus/mod.rs` |
| `ToolCategory` enum | `src/tool/mod.rs` |
| Destructive bash patterns | `src/tool/destructive.rs` |

> **Note:** `PermissionRegistry` is in `codegg-core`, not in the
> permission module. The permission module defines domain types; the bus
> owns the ask-response broker.

## How It Works

### Tool-Category Short-Circuit

Every `Tool` reports a `ToolCategory` (`ReadOnly | SafeMutating |
Mutating | ShellExec`). The function `tool_category_for_name()` in
`src/permission/mod.rs:99` maps tool names to categories without a
`Tool` instance.

Categories with `is_permission_free() == true` (`ReadOnly`,
`SafeMutating`) short-circuit `PermissionChecker::check()` to `Allow`
before any store/rule/glob lookup, unless a persistent `Deny` is in
the store. This covers `read`, `glob`, `grep`, `list`, `webfetch`,
`websearch`, `codesearch`, `lsp`, `diff`, `security`, `skill`,
`tool_search`, `plan_enter`, `plan_exit`, `todowrite`, `todoread`,
`question`.

### Check Flow

```
ToolCallRequested
    │
    ▼
PermissionChecker::check()  (or check_with_args for shell)
    │
    ├──► Tool category short-circuit (ReadOnly / SafeMutating)
    │         │
    │         ├── Persistent Deny in store → Deny
    │         └── otherwise                → Allow
    │
    ├──► Check PermissionStore (cached, HMAC-verified)
    │         │
    │         ├── Allow → Return Allow
    │         └── Deny  → Return Deny
    │
    ├──► Check tool rules (agent > session > config priority)
    │         │
    │         ├── Allow → Return Allow
    │         ├── Deny  → Return Deny
    │         └── Ask   → Continue
    │
    ├──► Check path globs (on canonicalized paths)
    │         │
    │         ├── Allow → Continue
    │         └── Deny  → Return Deny
    │
    ├──► (shell tools only) Destructive-pattern fallback
    │         │
    │         ├── Matches DESTRUCTIVE_BASH_PATTERNS → Ask
    │         └── Non-destructive                   → Allow
    │
    └──► Return default (Ask/Allow/Deny)

--- If result is Ask, AgentLoop handles the dialog: ---

AgentLoop::check_tool_permission()
    │
    ├──► Create oneshot channel
    │
    ├──► PermissionRegistry::register(perm_id, tx)
    │         [Registration-before-publish; sync fn, NOT async]
    │
    ├──► GlobalEventBus::publish(PermissionPending { ... })
    │
    ├──► Wait for response (300s timeout)
    │
    ├──► User responds → PermissionRegistry::respond(perm_id, choice)
    │
    └──► Cache decision if AlwaysAllow/AlwaysDeny
```

### Bash Command Handling

`check_with_args()` adds a destructive-pattern fallback for
`ShellExec` tools. If the user's ruleset would allow the command but
it matches a destructive pattern (`rm -rf /`, `mkfs`, `dd of=...`,
`:(){:|:&};:`, `shutdown`, etc.), the result is `Ask`. If the command
is non-destructive, the result is `Allow` — auto-approving safe
commands like `ls`, `cat`, `cargo build`, `git status` even in a
strict `default = "ask"` config.

Safe bash patterns are defined in `default_bash_allow_patterns()`
(`src/permission/mod.rs:1315`). Users can extend or override these via
`bash_allow_patterns` and `bash_deny_patterns` config fields.

## Key Types & APIs

### PermissionLevel (`src/permission/mod.rs:115`)

```rust
pub enum PermissionLevel {
    Deny,
    Ask,
    Allow,
}
```

### PermissionResult (`src/permission/mod.rs:133`)

```rust
pub enum PermissionResult {
    Allow,
    Deny,
    Ask(PermissionRequest),
}
```

### PermissionDecisionReceipt (`src/permission/mod.rs:145`)

Ephemeral receipt produced when the permission boundary accepts a call.
Contains `decision_id`, `outcome`, `source`, `issued_at`, and optional
`policy_revision`. Callers must not manufacture policy revisions from
unrelated session identifiers after evaluation.

### PermissionChoice (`src/permission/mod.rs:180`)

```rust
pub enum PermissionChoice {
    AllowOnce,
    AlwaysAllow,
    DenyOnce,
    AlwaysDeny,
}
```

Bidirectional `From` impls convert between `PermissionChoice` (domain)
and `PermissionDecision` (bus DTO).

### PermissionRuleset (`src/permission/mod.rs:279`)

```rust
pub struct PermissionRuleset {
    pub default: PermissionLevel,
    pub tool_rules: Vec<ToolRule>,
    pub path_rules: Vec<PathRule>,
}
```

### ToolRule (`src/permission/mod.rs:226`)

```rust
pub struct ToolRule {
    pub tool: String,                       // Supports glob patterns
    pub level: PermissionLevel,
    pub paths: Option<Vec<String>>,         // Path restrictions (canonicalized)
    pub bash_patterns: Option<Vec<String>>, // Bash command patterns
}
```

`matches()` supports `*` wildcard and glob compilation.
`matches_bash_command()` checks bash command patterns similarly.

### PermissionChecker (`src/permission/mod.rs:489`)

Main enforcement point:

```rust
pub struct PermissionChecker {
    config_rules: PermissionRuleset,
    session_rules: PermissionRuleset,
    agent_rules: PermissionRuleset,
    store: Arc<RwLock<PermissionStore>>,
    compiled_globs: Vec<(globset::GlobMatcher, PermissionLevel)>,
    canonicalized_config_tool_rules: Vec<CanonicalizedToolRule>,
    canonicalized_session_tool_rules: Vec<CanonicalizedToolRule>,
    canonicalized_agent_tool_rules: Vec<CanonicalizedToolRule>,
    path_cache: Arc<RwLock<HashMap<String, (PathBuf, Instant)>>>,
}
```

Key methods:
- `check(tool, path, session_id)` — main entry point (async)
- `check_bash(path, command, session_id)` — bash-specific with
  destructive-pattern fallback
- `check_git(path, subcommand, session_id)` — git-specific
- `check_with_args(tool, path, args, session_id)` — generic with args
- `with_session_rules(rules)` — per-session overrides
- `with_agent_rules(rules)` — per-agent overrides
- `with_active_mode(config)` — wires built-in modes into agent rules
- `with_exec_mode()` — CI/CD mode, all destructive tools auto-allowed
- `always_allow(tool, path, session_id)` / `always_deny(...)` — persist
- `clear_decisions()` — wipe cached decisions

### PermissionStore (`src/permission/mod.rs:306`)

HMAC-signed persistent decision cache:

```rust
pub struct PermissionStore {
    decisions: Vec<PersistentDecision>,
    store_path: Option<PathBuf>,
}

pub struct PersistentDecision {
    pub tool: String,
    pub path: Option<String>,
    pub level: PermissionLevel,
    pub created_at: i64,
    pub signature: String,           // HMAC-SHA256 via CODEGG_PERM_KEY
    pub session_id: Option<String>,  // Per-session isolation
}
```

- Session-specific decisions checked first, then global
- HMAC signature prevents tampering (`CODEGG_PERM_KEY` env var)
- Persists to `~/.config/codegg/permissions.json`

### DoomLoopDetector (`src/permission/mod.rs:1574`)

Detects repetitive tool call patterns using window-based counting:

```rust
pub struct DoomLoopDetector {
    history: VecDeque<String>,
    counts: HashMap<String, usize>,
    max_window: usize,    // Capped at 1000
    threshold: usize,     // Capped at 100, min 1
}
```

- `record_tool_call(tool_name, arguments)` — records a composite key
  (tool + JSON argument hash) into the sliding window
- `is_doom_loop()` — returns true if the **most recent** tool has been
  called `threshold` or more times anywhere in the window
- `current_doom_tool()` — returns the tool name from the last call
- `reset()` — clears history

### Mode System (`src/permission/modes.rs`)

```rust
pub struct ModeDefinition {
    pub name: String,
    pub description: String,
    pub default: PermissionLevel,
    pub allowed_tools: Vec<String>,
    pub restricted_tools: Vec<String>,
    pub tool_overrides: Vec<(String, PermissionLevel)>,
}
```

**Built-in Modes:**

| Mode | Default | Restricted Tools |
|------|---------|------------------|
| `review` | Ask | edit, apply_patch, replace, multiedit, write, bash, terminal, git, commit, image, task |
| `debug` | Allow | task, image, commit |
| `docs` | Ask | bash, terminal, git, commit, task, image |

All three modes allow todowrite/todoread for in-flight planning.
Modes are activated via `[mode.review]` (etc.) in config; the
permission checker merges mode rules into agent-level rules via
`with_active_mode()`.

### PermissionRegistry (`crates/codegg-core/src/bus/mod.rs:88`)

The ask-response broker. **All methods are synchronous (`fn`), NOT
async.** Uses a `DashMap` with 310s TTL auto-cleanup.

```rust
pub struct PermissionRegistry {
    senders: DashMap<String, PendingPermission>,
    last_cleanup_ms: AtomicU64,
}
```

Key methods:
- `register(perm_id, tx)` — backward-compatible, session = "default"
- `register_with_session(session_id, turn_id, perm_id, tx)` — full
  session/turn scoping
- `respond(perm_id, choice)` — backward-compatible
- `respond_scoped(session_id, perm_id, choice)` — session-verified
- `unregister(perm_id)` / `unregister_scoped(session_id, perm_id)`
- `is_registered(perm_id)` / `is_registered_scoped(session_id, perm_id)`
- `pending_permission_ids()` — all pending across sessions
- `get_pending_for_session(session_id)` — session-scoped with metadata

### Registration-Before-Publish Pattern

```rust
// CORRECT
let (tx, rx) = tokio::sync::oneshot::channel();
PermissionRegistry::register(perm_id.clone(), tx);
GlobalEventBus::publish(AppEvent::PermissionPending { ... });
let choice = match tokio::time::timeout(Duration::from_secs(300), rx).await {
    Ok(Ok(choice)) => choice,
    _ => PermissionDecision::DenyOnce,
};
PermissionRegistry::unregister(&perm_id);
```

## Configuration Surface

```toml
[permission]
default = "ask"
read = "allow"
edit = "ask"
glob = "allow"
grep = "allow"
list = "allow"
bash = "ask"
task = "ask"
lsp = "ask"
skill = "allow"
todowrite = "ask"
question = "ask"
webfetch = "ask"
websearch = "ask"
codesearch = "ask"
doom_loop = "ask"

# Custom tool rules
[permission.tools]
"custom_tool" = "deny"

# Path-based rules
[permission.paths]
"/home/user/project/**" = "ask"

# DoomLoop threshold
[permission.doomloop_threshold]
5

# Bash-specific patterns
bash_allow_patterns = ["cargo *"]
bash_deny_patterns = ["rm -rf *"]
allow_all_bash = false
```

### PERMISSION_TYPES Constant (`src/permission/mod.rs:71`)

19 recognized tool permission names: `read`, `edit`, `glob`, `grep`,
`list`, `bash`, `git`, `task`, `todowrite`, `todoread`, `question`,
`webfetch`, `websearch`, `codesearch`, `lsp`, `doom_loop`, `skill`,
`plan_enter`, `plan_exit`.

## Invariants & Gotchas

1. **PermissionRegistry is synchronous.** `register()`, `respond()`,
   `answer_question()` are `fn`, not `async fn`. Do NOT `.await` them.
2. **Registration-before-publish.** Always register the oneshot channel
   BEFORE publishing the `PermissionPending` event.
3. **Agent loop does not treat external origin as approval.** Unknown
   raw MCP tools follow the normal mutating default and remain `Ask`
   until explicit policy or user decision allows them.
4. **Path canonicalization TTL is 1 second** (`PATH_CANONICALIZE_CACHE_TTL_SECS`).
   Not-found entries also cache for 1s.
5. **Session isolation.** Session-specific decisions are checked before
   global decisions. A Deny at any layer overrides allows at lower
   layers.
6. **Exec mode** (`with_exec_mode()`) sets `default = Allow` and allows
   bash, edit, task, todowrite — for CI/CD where no TUI is available.
7. **`check_external_directory()`** is `#[allow(dead_code)]` — exists
   for potential future use.

## Testing

```bash
cargo test -p codegg --lib permission    # unit tests for permission module
cargo test -p codegg --lib permission::tests  # specific test module
```

Key test patterns:
- `read_only_tools_short_circuit_to_allow` — 14 read-only tools
- `safe_mutating_tools_short_circuit_to_allow` — todowrite, todoread,
  question, invalid
- `destructive_bash_prompts` — rm -rf /, mkfs, fork bomb, shutdown
- `non_destructive_bash_auto_allows` — ls, cat, cargo test, git status
- `builtin_mode_review_blocks_mutation` — review mode Deny rules
- `builtin_modes_all_allow_todo_tools` — all modes allow todos

## Related Docs

- [tool.md](tool.md) — Tools that use PermissionChecker
- [bus.md](bus.md) — PermissionRegistry pattern
- [security.md](security.md) — Additional security measures
