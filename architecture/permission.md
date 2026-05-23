# Permission Module

The `permission` module enforces access control for tool execution and path access.

## Overview

**Location**: `src/permission/`

**Key Responsibilities**:
- Tool permission enforcement
- Path access restrictions
- DoomLoop detection (repetitive tool call detection)
- Mode-based permissions (Review/Debug/Docs)

**Note**: `PermissionRegistry` is located in `src/bus/mod.rs`, not in the permission module.

## Key Types

### PermissionLevel

```rust
pub enum PermissionLevel {
    Allow,   // Always allow
    Deny,    // Always deny
    Ask,     // Prompt user
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

### PermissionResponse (src/server/routes/permission.rs)

HTTP API type used by server routes (distinct from `PermissionResponse` in permission module):

```rust
pub struct PermissionResponse {
    pub id: String,
    pub choice: String,
}
```

### PermissionRuleset

```rust
pub struct PermissionRuleset {
    pub default: PermissionLevel,
    pub tool_rules: Vec<ToolRule>,
    pub path_rules: Vec<PathRule>,
}

pub struct ToolRule {
    pub tool: String,              // Tool name (supports glob patterns)
    pub level: PermissionLevel,
    pub paths: Option<Vec<String>>,     // Path restrictions (canonicalized)
    pub bash_patterns: Option<Vec<String>>, // Bash command patterns
}
```

## Components

### PermissionChecker

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

impl PermissionChecker {
    pub async fn check(&self, tool: &str, path: Option<&str>, session_id: Option<&str>) -> PermissionResult;
    pub async fn check_legacy(&self, tool: &str, path: Option<&str>) -> PermissionResult;  // Uses None for session_id
    pub async fn check_bash(&self, path: Option<&str>, command: Option<&str>, session_id: Option<&str>) -> PermissionResult;
    pub async fn check_bash_legacy(&self, path: Option<&str>, command: Option<&str>) -> PermissionResult;  // Uses None for session_id
    pub async fn check_git(&self, path: Option<&str>, subcommand: Option<&str>, session_id: Option<&str>) -> PermissionResult;
    pub async fn always_allow(&self, tool: &str, path: Option<&str>, session_id: Option<&str>);
    pub async fn always_allow_legacy(&self, tool: &str, path: Option<&str>);  // Uses None for session_id
    pub async fn always_deny(&self, tool: &str, path: Option<&str>, session_id: Option<&str>);
    pub async fn always_deny_legacy(&self, tool: &str, path: Option<&str>);  // Uses None for session_id
    pub async fn clear_decisions(&self);  // Clear all cached decisions
}
```

**Check Flow**:
1. Check PermissionStore (cached decisions with HMAC verification)
2. Check tool rules (agent > session > config priority)
3. Check path globs (on canonicalized paths)
4. Return default if no rule matches
5. If `Ask`, return `PermissionResult::Ask(...)` - caller handles registration

**Important**: `PermissionChecker::check()` does NOT directly register with `PermissionRegistry`. The caller (`agent/loop.rs`) handles the ask flow by:
1. Checking permission
2. If `Ask`, registering with `PermissionRegistry` and publishing `PermissionPending`
3. Waiting for user response

### PermissionStore

HMAC-signed persistent decision cache:

```rust
pub struct PermissionStore {
    decisions: Vec<PersistentDecision>,  // Uses Vec, not HashMap
    store_path: Option<PathBuf>,
}

pub struct PersistentDecision {
    pub tool: String,
    pub path: Option<String>,
    pub level: PermissionLevel,
    pub created_at: i64,
    pub signature: String,           // HMAC-SHA256 signature
    pub session_id: Option<String>,  // Per-session isolation
}
```

**Features**:
- HMAC signature to prevent tampering (uses `CODEGG_PERM_KEY` env var)
- Per-session isolation (session-specific decisions checked first)
- Persists to `~/.config/codegg/permissions.json`

### DoomLoopDetector

Detects repetitive tool call patterns using window-based counting:

```rust
pub struct DoomLoopDetector {
    history: VecDeque<String>,        // Ordered recent calls
    counts: HashMap<String, usize>,  // O(1) count lookups
    max_window: usize,               // Max history size (capped at 1000)
    threshold: usize,                // Detection threshold (capped at 100)
}

impl DoomLoopDetector {
    pub fn record_tool_call(&mut self, tool_name: &str);
    pub fn is_doom_loop(&self) -> bool;  // Returns true if last tool has count >= threshold
    pub fn reset(&mut self);
}
```

**Implementation**: Uses window-based counting (NOT consecutive). The `is_doom_loop()` check returns true if the **most recent** tool has been called `threshold` or more times anywhere in the window.

### Mode System (modes.rs)

Specialized permission workflows:

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

**Built-in Modes**:

| Mode | Default | Allowed Tools | Restricted Tools |
|------|---------|---------------|------------------|
| `review` | Ask | read, glob, grep, list, question, webfetch, websearch, codesearch, lsp | edit, bash, task, todowrite |
| `debug` | Allow | read, glob, grep, list, bash, question, webfetch, websearch, codesearch, edit, lsp | task, todowrite |
| `docs` | Ask | read, glob, grep, list, question, webfetch, websearch, codesearch, edit, write, lsp | bash, task, todowrite |

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
    └──► Return default (Ask/Allow/Deny)

--- If result is Ask, AgentLoop handles the dialog: ---

AgentLoop::check_tool_permission()
    │
    ├──► Create oneshot channel
    │
    ├──► PermissionRegistry::register(perm_id, tx)  [Registration-before-publish]
    │
    ├──► GlobalEventBus::publish(PermissionPending { ... })
    │
    ├──► Wait for response (300s timeout)
    │
    ├──► User responds → PermissionRegistry::respond(perm_id, choice)
    │
    └──► Cache decision if AlwaysAllow/AlwaysDeny
```

## Rule Priority

Rules are evaluated in order:
1. **Agent-level rules** - Most specific (via `with_agent_rules()`)
2. **Session-level rules** - Per-session overrides (via `with_session_rules()`)
3. **Config rules** - Default configuration

## Registration-Before-Publish Pattern

When asking user for permission:

```rust
// CORRECT
let (tx, rx) = tokio::sync::oneshot::channel();
PermissionRegistry::register(perm_id.clone(), tx);
GlobalEventBus::publish(AppEvent::PermissionPending { ... });
let choice = match tokio::time::timeout(Duration::from_secs(300), rx).await {
    Ok(Ok(choice)) => choice,
    _ => PermissionChoice::DenyOnce,
};
PermissionRegistry::unregister(&perm_id);
```

## Configuration

```toml
[permission]
default = "ask"

[permission]
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

[permission]
tools = { "custom_tool" = "deny" }

[permission.paths]
"/home/user/project/**" = "ask"

[permission.doomloop_threshold]
5  # Threshold for DoomLoopDetector (default: 5)
```

## Security Features

1. **HMAC-signed decisions** - Prevents tampering with cached permissions via `CODEGG_PERM_KEY`
2. **Per-session isolation** - Decisions scoped to sessions, session-specific checked first
3. **Path canonicalization** - Resolves symlinks before checking (cached with 1s TTL)
4. **DoomLoop detection** - Prevents infinite loops via window-based counting
5. **Glob pattern matching** - Supports `*` for tool names and bash commands
6. **External directory check** - `check_external_directory()` validates paths stay within project root

## Utility Functions

### check_external_directory

```rust
pub fn check_external_directory(path: &str, project_root: &str) -> bool
```

Security utility that checks if a path is within a project root directory. Returns `true` if the path is inside the project root (safe), `false` if outside (potential security risk). Uses canonicalization when possible, falls back to prefix matching.

**Note**: This function is marked `#[allow(dead_code)]` - it exists for potential future use.

## PermissionRegistry (in bus/mod.rs)

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
}
```

**Note**: All methods are synchronous (`fn`), NOT async. TTL of 300s for entries.

## See Also

- [tool.md](tool.md) - Tools that use PermissionChecker
- [event-bus.md](event-bus.md) - PermissionRegistry pattern
- [security.md](security.md) - Additional security measures