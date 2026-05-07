# Permission Module

The `permission` module enforces access control for tool execution and path access.

## Overview

**Location**: `src/permission/`

**Key Responsibilities**:
- Tool permission enforcement
- Path access restrictions
- DoomLoop detection (repetitive tool call detection)
- Mode-based permissions (Review/Debug/Docs)

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
    Ask(Request),
}

pub struct Request {
    pub tool_name: String,
    pub params: Value,
    pub reason: String,
}
```

### PermissionRuleset

```rust
pub struct PermissionRuleset {
    pub tool_rules: Vec<ToolRule>,
    pub path_rules: Vec<PathRule>,
}

pub struct ToolRule {
    pub pattern: String,
    pub permission: PermissionLevel,
}

pub struct PathRule {
    pub pattern: String,
    pub permission: PermissionLevel,
}
```

## Components

### PermissionChecker

Main enforcement point:

```rust
pub struct PermissionChecker {
    rules: PermissionRuleset,
    store: PermissionStore,
    doom_loop: DoomLoopDetector,
}

impl PermissionChecker {
    pub async fn check(&self, request: ToolRequest) -> Result<PermissionResult>;
    pub fn check_sync(&self, request: &ToolRequest) -> PermissionResult;
}
```

**Check Flow**:
1. Check PermissionStore (cached decisions)
2. Check rules (agent > session > config priority)
3. Check path globs
4. If `Ask`, register with PermissionRegistry and return pending

### PermissionStore

Caches permission decisions:

```rust
pub struct PermissionStore {
    decisions: Arc<Mutex<HashMap<DecisionKey, Decision>>>,
}

#[derive(Hash, Clone)]
pub struct DecisionKey {
    pub tool_name: String,
    pub params_hash: u64,
    pub session_id: String,
}
```

**Features**:
- HMAC signature to prevent tampering
- TTL-based expiration
- Per-session isolation

### DoomLoopDetector

Detects repetitive tool call patterns:

```rust
pub struct DoomLoopDetector {
    window: usize,       // Number of calls to track
    threshold: usize,     // Threshold for detection
}

impl DoomLoopDetector {
    pub fn check(&self, tool_name: &str) -> bool;
}
```

**Implementation Note**: Comment says "consecutive" but implementation uses window-based counting.

### modes.rs - Mode System

Specialized permission workflows:

```rust
pub enum Mode {
    Review,  // Code review mode
    Debug,   // Debugging mode
    Docs,    // Documentation mode
}

pub struct ModeConfig {
    pub default_tools: Vec<String>,
    pub restricted_tools: Vec<String>,
    pub auto_allow_paths: Vec<String>,
}
```

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
    ├──► Check rules (agent > session > config)
    │         │
    │         ├── Allow → Cache & Return Allow
    │         ├── Deny  → Cache & Return Deny
    │         └── Ask   → Continue
    │
    ├──► Check path globs
    │         │
    │         ├── Allow → Continue
    │         └── Deny  → Return Deny
    │
    └──► Ask user
              │
              ▼
        PermissionRegistry::register()
              │
              ▼
        GlobalEventBus::publish(PermissionPending)
              │
              ▼
        TUI shows dialog
              │
              ▼
        User responds
              │
              ▼
        PermissionRegistry::respond()
              │
              ▼
        Cache decision & Return
```

## Rule Priority

Rules are evaluated in order:
1. **Agent-level rules** - Most specific
2. **Session-level rules** - Per-session overrides
3. **Config rules** - Default configuration

## Registration-Before-Publish Pattern

When asking user:

```rust
// CORRECT
let (tx, rx) = oneshot::channel();
registry.register(request_id, tx).await?;
bus.publish(PermissionPending { ... });
let choice = rx.await?;
```

## Configuration

```toml
[permission]
default_level = "Ask"

[permission.tools]
"bash" = "Ask"
"read" = "Allow"
"delete" = "Deny"

[permission.paths]
"/home/user/project" = "Allow"
"/etc" = "Deny"

[permission.doom_loop]
window = 10
threshold = 5
```

## Security Features

1. **HMAC-signed decisions** - Prevents tampering with cached permissions
2. **Per-session isolation** - Decisions scoped to sessions
3. **Path canonicalization** - Resolves symlinks before checking
4. **DoomLoop detection** - Prevents infinite loops

## See Also

- [tool.md](tool.md) - Tools that use PermissionChecker
- [event-bus.md](event-bus.md) - PermissionRegistry pattern
- [security.md](security.md) - Additional security measures
