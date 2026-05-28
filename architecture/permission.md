# Permission Module

The `permission` module enforces access control for tool execution and path access.

## Overview

**Location**: `src/permission/`

**Key Responsibilities**:
- Tool permission enforcement via `PermissionChecker`
- Path access restrictions via glob patterns
- DoomLoop detection (repetitive tool call detection)
- Mode-based permissions (Review/Debug/Docs)
- HMAC-signed persistent decision cache

**Note**: `PermissionRegistry` is located in `src/bus/mod.rs`, not in the permission module.

## Key Types

### PermissionLevel

```rust
pub enum PermissionLevel {
    Allow,   // Always allow
    Deny,    // Always deny
    Ask,     // Prompt user
}

impl PermissionLevel {
    pub fn as_str(&self) -> &'static str;
}
```

### PermissionResult

```rust
pub enum PermissionResult {
    Allow,
    Deny,
    Ask(PermissionRequest),
}

pub struct PermissionRequest {
    pub tool: String,
    pub path: Option<String>,
    pub args: Option<serde_json::Value>,
}
```

### PermissionChoice

```rust
pub enum PermissionChoice {
    AllowOnce,
    AlwaysAllow,
    DenyOnce,
    AlwaysDeny,
}

impl PermissionChoice {
    pub fn allowed(&self) -> bool;
    pub fn persist(&self) -> bool;  // true for AlwaysAllow/AlwaysDeny
}
```

### PermissionRuleset

```rust
pub struct PermissionRuleset {
    pub default: PermissionLevel,
    pub tool_rules: Vec<ToolRule>,
    pub path_rules: Vec<PathRule>,
}
```

### ToolRule

```rust
pub struct ToolRule {
    pub tool: String,                      // Tool name (supports glob patterns)
    pub level: PermissionLevel,
    pub paths: Option<Vec<String>>,        // Path restrictions (canonicalized)
    pub bash_patterns: Option<Vec<String>>, // Bash command subcommand patterns
}
```

Tool rules support glob matching for tool names (e.g., `mcp_*` matches all MCP tools). Bash patterns allow restricting git subcommands (e.g., `read` git commands are allowed, `write` git commands prompt ask).

### PathRule

```rust
pub struct PathRule {
    pub pattern: String,           // Glob pattern for path matching
    pub level: PermissionLevel,
}
```

## All 16 Permission Types

The following permission types are defined in `src/permission/mod.rs:70-87`:

| Type | Description |
|------|-------------|
| `read` | Read file contents |
| `edit` | Edit/modify file contents |
| `glob` | Glob pattern file search |
| `grep` | Search file contents |
| `list` | List directory contents |
| `bash` | Execute bash commands |
| `git` | Git operations |
| `task` | Task/subagent spawning |
| `todowrite` | Todo list modifications |
| `question` | Ask user questions |
| `webfetch` | Fetch web content |
| `websearch` | Web search |
| `codesearch` | Code search (cross-reference) |
| `lsp` | Language Server Protocol |
| `doom_loop` | Doom loop detection override |
| `skill` | Skill invocation |

## PermissionChecker

Main enforcement point located at `src/permission/mod.rs:418-428`:

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

### Check Flow

The `check()` method at lines 469-546 evaluates permissions in this order:

1. **Check PermissionStore** (cached HMAC-verified decisions)
   - Per-session decisions checked first
   - Global decisions checked second

2. **Check tool rules** (agent > session > config priority)
   - Returns immediately if non-Ask level found

3. **Check path globs** (on canonicalized paths)
   - Uses `globset::GlobMatcher` for efficient matching
   - Paths are canonicalized (symlinks resolved) with 1s cache TTL

4. **Return default** if no rule matches

### Key Methods

```rust
impl PermissionChecker {
    // Core check methods
    pub async fn check(&self, tool: &str, path: Option<&str>, session_id: Option<&str>) -> PermissionResult;
    pub async fn check_legacy(&self, tool: &str, path: Option<&str>) -> PermissionResult;
    
    // Tool-specific checks with args
    pub async fn check_bash(&self, path: Option<&str>, command: Option<&str>, session_id: Option<&str>) -> PermissionResult;
    pub async fn check_git(&self, path: Option<&str>, subcommand: Option<&str>, session_id: Option<&str>) -> PermissionResult;
    
    // Persistent decision management
    pub async fn always_allow(&self, tool: &str, path: Option<&str>, session_id: Option<&str>);
    pub async fn always_deny(&self, tool: &str, path: Option<&str>, session_id: Option<&str>);
    pub async fn clear_decisions(&self);
}
```

## PermissionStore (HMAC-Signed Persistent Decisions)

Located at `src/permission/mod.rs:232-368`, the store provides tamper-resistant persistent decisions:

```rust
pub struct PermissionStore {
    decisions: Vec<PersistentDecision>,
    store_path: Option<std::path::PathBuf>,
}

pub struct PersistentDecision {
    pub tool: String,
    pub path: Option<String>,
    pub level: PermissionLevel,
    pub created_at: i64,
    pub signature: String,           // HMAC-SHA256
    pub session_id: Option<String>, // Per-session isolation
}
```

### HMAC Signature Verification

Located at lines 26-68:

1. **Key Retrieval**: Uses `CODEGG_PERM_KEY` environment variable
2. **Signature Computation** (lines 42-57): HMAC-SHA256 of `(tool + path + level + timestamp)`
3. **Verification** (lines 59-68): Recomputes signature and compares

```rust
fn compute_signature(
    tool: &str,
    path: Option<&str>,
    level: &PermissionLevel,
    timestamp: i64,
    key: &[u8; 32],
) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("HMAC can take key of any size");
    mac.update(tool.as_bytes());
    if let Some(p) = path {
        mac.update(p.as_bytes());
    }
    mac.update(level.as_str().as_bytes());
    mac.update(timestamp.to_string().as_bytes());
    hex::encode(mac.finalize().into_bytes())
}
```

### Decision Lookup (lines 278-315)

- **Session-specific decisions** checked first with HMAC verification
- **Global decisions** checked second (require valid signature if key configured)
- Rejects signatures that don't match or use different keys
- Persists to `~/.config/codegg/permissions.json`

## DoomLoopDetector

Located at `src/permission/mod.rs:1181-1257`, detects when an agent gets stuck in repetitive tool calls:

### Algorithm

```
1. Maintain a sliding window of recent tool calls (up to max_window, capped at 1000)
2. Use HashMap for O(1) count lookups
3. Each entry is keyed by "tool_name:hash(tool_name + arguments)" (per-tool+args, not per-tool)
4. Consider it a doom loop when the most recent key appears threshold times anywhere in window
```

### Implementation Details

- **Time complexity**: O(1) for both `record_tool_call()` and `is_doom_loop()`
- **Window enforcement**: When window is full, oldest entry is evicted and count decremented
- **Normalization**: Tool names are lowercased and trimmed for comparison
- **Limits**: 
  - `max_window` capped at 1000
  - `threshold` capped at 100, minimum 1

### Detection Logic (lines 1229-1242)

```rust
pub fn is_doom_loop(&self) -> bool {
    if self.history.is_empty() || self.threshold == 0 {
        return false;
    }

    let Some(last_key) = self.history.back() else {
        return false;
    };

    self.counts.get(last_key).map(|&c| c >= self.threshold).unwrap_or(false)
}
```

**Important**: Detection is NOT consecutive - it checks if the **most recently added** key (which includes both tool name and arguments) has been called `threshold` or more times anywhere in the window.

### Agent Integration

DoomLoopDetector is checked in `AgentLoop::check_tool_permission()` at `src/agent/loop.rs:461-468`:
- If doom loop detected, tool is denied immediately with message about repeated identical calls
- Happens BEFORE permission registry registration

## Mode System

Located at `src/permission/modes.rs`, provides specialized permission workflows:

### ModeDefinition

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

### Built-in Modes

| Mode | Default | Allowed Tools | Restricted Tools |
|------|---------|---------------|------------------|
| `review` | Ask | read, glob, grep, list, question, webfetch, websearch, codesearch, lsp | edit, bash, task, todowrite |
| `debug` | Allow | read, glob, grep, list, bash, question, webfetch, websearch, codesearch, edit, lsp | task, todowrite |
| `docs` | Ask | read, glob, grep, list, question, webfetch, websearch, codesearch, edit, **write**, lsp | bash, task, todowrite |

**Note**: The `docs` mode correctly excludes `write` from restricted tools per `modes.rs:174-178`. The `write` tool is in `allowed_tools` (line 171) which includes `edit` and `write` as separate tools.

### Mode Rule Conversion

`ModeDefinition::to_ruleset()` at lines 15-52:
- Tools in `allowed_tools` but NOT in `restricted_tools` get `Allow` level
- Tools in `restricted_tools` get `Deny` level
- `tool_overrides` can explicitly set any level

## Permission Flow

```
ToolCallRequested
    │
    ▼
PermissionChecker::check()
    │
    ├──► Check PermissionStore (cached)
    │         │
    │         ├── Allow → Return Allow
    │         └── Deny  → Return Deny
    │
    ├──► Check tool rules (agent > session > config)
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
    └──► Return default

--- If result is Ask, AgentLoop handles the dialog: ---

AgentLoop::check_tool_permission()
    │
    ├──► DoomLoop check (immediate denial if detected)
    │
    ├──► Create oneshot channel
    │
    ├──► PermissionRegistry::register(perm_id, tx)  [Registration-before-publish]
    │
    ├──► GlobalEventBus::publish(PermissionPending { ... })
    │
    ├──► Wait for response (300s timeout, default DenyOnce)
    │
    ├──► User responds via PermissionRegistry::respond(perm_id, choice)
    │
    └──► Cache decision if AlwaysAllow/AlwaysDeny
```

## Registration-Before-Publish Pattern

When asking user for permission, the responder MUST be registered BEFORE publishing the event (`src/agent/loop.rs:473-487`):

```rust
// CORRECT - Register BEFORE publish
let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
PermissionRegistry::register(perm_id.clone(), resp_tx);
crate::bus::global::GlobalEventBus::publish(AppEvent::PermissionPending {
    session_id: self.session_id.clone(),
    perm_id: perm_id.clone(),
    tool: req.tool.clone(),
    path: req.path.clone(),
    args: req.args.clone(),
});
let choice = match tokio::time::timeout(Duration::from_secs(300), resp_rx).await {
    Ok(Ok(choice)) => choice,
    _ => PermissionChoice::DenyOnce,  // Timeout = deny
};
PermissionRegistry::unregister(&perm_id);
```

This ensures the response channel is ready when the event reaches subscribers.

## Rule Priority

Rules are evaluated in order:
1. **Agent-level rules** - Most specific (via `with_agent_rules()`)
2. **Session-level rules** - Per-session overrides (via `with_session_rules()`)
3. **Config rules** - Default configuration (via `config_ruleset()`)

For `default`, the first non-Ask level is used (agent > session > config).

## PermissionRegistry (bus/mod.rs)

Located in `src/bus/mod.rs:11-68`:

```rust
pub struct PermissionRegistry {
    senders: DashMap<String, (tokio::sync::oneshot::Sender<PermissionChoice>, Instant)>,
}

impl PermissionRegistry {
    pub fn register(perm_id: String, tx: tokio::sync::oneshot::Sender<PermissionChoice>);
    pub fn respond(perm_id: String, choice: PermissionChoice) -> bool;
    pub fn unregister(perm_id: &str);
    pub fn is_registered(perm_id: &str) -> bool;
    pub fn pending_permission_ids() -> Vec<String>;
    fn cleanup();  // Removes entries older than 310s
}
```

**Important**: All methods are synchronous (`fn`), NOT `async fn`. TTL of 310s for entries.

### Permission ID Format

Permission IDs consist of `{tool_call_id}-{tool_name}` (e.g., `call_abc123-bash`). Note that session context is NOT embedded in the key, which limits session-based filtering.

## Configuration

```toml
[permission]
default = "ask"

# Tool-specific rules
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

# Path-based restrictions
[permission.paths]
"/home/user/project/**" = "ask"

# DoomLoop settings
[permission.doomloop]
max_window = 100   # Default: 100
threshold = 5      # Default: 5
```

## Security Features

1. **HMAC-signed decisions** - Prevents tampering with cached permissions via `CODEGG_PERM_KEY`
2. **Per-session isolation** - Decisions scoped to sessions, session-specific checked first
3. **Path canonicalization** - Resolves symlinks before checking (cached with 1s TTL, not-found with 1s TTL)
4. **DoomLoop detection** - Prevents infinite loops via O(1) window-based counting
5. **Glob pattern matching** - Supports `*` for tool names and bash commands
6. **External directory check** - `check_external_directory()` validates paths stay within project root

## Utility Functions

### check_external_directory

```rust
pub fn check_external_directory(path: &str, project_root: &str) -> bool
```

Security utility that checks if a path is within a project root directory. Returns `true` if inside (safe), `false` if outside (security risk). Uses canonicalization when possible, falls back to prefix matching.

**Note**: This function is marked `#[allow(dead_code)]`.

## Default Ruleset

The `default_ruleset()` function at lines 1025-1082 provides baseline permissions:

**Allowed tools** (no prompting):
- `read`, `glob`, `grep`, `list`, `question`, `webfetch`, `websearch`, `codesearch`

**Ask tools** (prompt user):
- `edit`, `bash`, `task`, `todowrite`

**Git read-only** (read operations allowed):
- `status`, `log`, `diff`, `branch`, `show`, `ls-files`, `cat-file`, `rev-parse`, `remote`

**Git write** (prompts for write operations):
- `add`, `commit`, `push`, `pull`, `merge`, `checkout`, `reset`, `rebase`, `stash`, `branch`, `tag`, `clone`, `fetch`, `clean`, `mv`, `rm`

## Known Architectural Limitations

| Issue | Location | Impact |
|-------|----------|--------|
| Session filtering not possible | `PermissionRegistry` key format | `get_pending_permissions_for_session()` cannot filter |
| check_external_directory unused | `src/permission/mod.rs:1265-1276` | Marked #[allow(dead_code)] |

## See Also

- [tool.md](tool.md) - Tools that use PermissionChecker
- [bus.md](bus.md) - PermissionRegistry pattern
- [security.md](security.md) - Additional security measures
