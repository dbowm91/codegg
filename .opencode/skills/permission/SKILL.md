---
name: Permission
description: Permission system architecture and registration patterns in opencode-rs
tags: [security, permission, agent, mode]
---

Use the `/skill:Permission` command to load additional permissions context for file access control in the current session.

## Key Modules

| Module        | Purpose                                       |
| ------------- | --------------------------------------------- |
| `permission/` | Access control and path restrictions          |
| `permission/modes.rs` | Mode system for specialized workflows |

## Permission Architecture

### PermissionFlow

1. `AgentLoop` calls `permission_checker.check(tool, path)` (async, must await)
2. If `Ask`, `AgentLoop` publishes `PermissionPending` via GlobalEventBus
3. `AgentLoop` registers with `PermissionRegistry` and waits (300s timeout)
4. TUI shows permission dialog
5. User responds → `PermissionRegistry::respond(perm_id, choice)`
6. `AgentLoop` resumes based on user's choice

### Config Schema

Permissions are defined in config:

```json
{
  "permission": {
    "default": "ask",
    "skill": "allow",
    "bash": "ask",
    "paths": ["/src/**", "!/**/test/**"]
  }
}
```

### Tool Permission Checking

AgentTool permissions are checked at the AgentLoop level before any tool executes:

- `AgentLoop::check_tool_permission(tc)` checks tool name and path
- `PermissionChecker::check(tool, path)` uses `config_ruleset()` to build rules from config
- Skills like `read`, `edit`, `bash`, `skill`, etc. are controllable via config schema

### PermissionStore

The `PermissionStore` persists decisions to a JSON file at `~/.config/codegg/permissions.json`:
- `add_decision(tool, path, level)` stores Allow/Deny with HMAC signature
- `get_decision(tool, path)` retrieves cached decisions with signature verification
- Session-specific decisions are checked first, then global decisions

### ToolRule Pattern Matching

ToolRule supports glob patterns for tool name matching:
- `git *` matches git commit, git push, etc.
- `*` matches all tools

### Bash Command Patterns

ToolRule also supports `bash_patterns` for per-command permission control:

```rust
pub struct ToolRule {
    pub tool: String,
    pub level: PermissionLevel,
    pub paths: Option<Vec<String>>,
    pub bash_patterns: Option<Vec<String>>,
}
```

The `check_bash()` method checks bash tool permissions with command argument matching:
- Patterns use glob syntax (e.g., `git *`, `rm *`)
- `*` matches any command
- Empty patterns allow all commands
- No patterns (None) allows all commands

Example usage:
```rust
let ruleset = PermissionRuleset {
    default: PermissionLevel::Deny,
    tool_rules: vec![ToolRule {
        tool: "bash".to_string(),
        level: PermissionLevel::Ask,
        paths: None,
        bash_patterns: Some(vec!["git *".to_string(), "ls".to_string()]),
    }],
    path_rules: Vec::new(),
};
let checker = PermissionChecker::new(None, None).with_session_rules(ruleset);
let result = checker.check_bash(None, Some("git push")).await;
```

### DoomLoopDetector

Prevents agents from repeating the same tool call:

```rust
pub struct DoomLoopDetector {
    history: VecDeque<String>,  // Recent tool call names (for ordering)
    counts: HashMap<String, usize>,  // Count of each tool name for O(1) lookup
    max_window: usize,
    threshold: usize,
}
```

**Important**: DoomLoopDetector uses window-based counting (O(1) HashMap), NOT consecutive repetitions. The count reflects how many times a tool has been called within the window, regardless of whether other tools interrupted it.

### Mode System

Specialized permission configurations for different workflows (`src/permission/modes.rs`):

```yaml
mode:
  review:
    description: "Code review mode"
    default: "ask"
    tools:
      read: "allow"
      glob: "allow"
      grep: "allow"
      bash: "deny"
      edit: "deny"
  debug:
    description: "Debug mode"
    default: "allow"
    tools:
      bash: "allow"
      read: "allow"
      edit: "ask"
  docs:
    description: "Documentation mode"
    default: "allow"
    tools:
      edit: "allow"
      read: "allow"
      bash: "deny"
```

## PermissionChoice Enum

Defined in `src/permission/mod.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionChoice {
    AllowOnce,      // Allow this single invocation
    AlwaysAllow,    // Allow and persist decision
    DenyOnce,      // Deny this single invocation
    AlwaysDeny,     // Deny and persist decision
}

impl PermissionChoice {
    pub fn allowed(&self) -> bool;   // true for AllowOnce/AlwaysAllow
    pub fn persist(&self) -> bool;   // true for AlwaysAllow/AlwaysDeny
}
```

## PermissionRegistry Usage

The `PermissionRegistry` in `src/bus/mod.rs` manages pending permission requests:

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

**Note**: All methods are synchronous (`fn`), NOT async. Entries have a 300s TTL.

### Test Patterns

**Ask/Allow Pattern** (Packet 4):
```rust
// Register permission request
let (tx, rx) = tokio::sync::oneshot::channel();
PermissionRegistry::register("test-perm-1".to_string(), tx);

// Verify registered
assert!(PermissionRegistry::is_registered("test-perm-1"));

// Respond with AllowOnce
PermissionRegistry::respond("test-perm-1".to_string(), PermissionChoice::AllowOnce);

// Verify response received
let response = rx.await.unwrap();
assert!(response.allowed());
```

**Ask/Deny Pattern**:
```rust
let (tx, rx) = tokio::sync::oneshot::channel();
PermissionRegistry::register("test-perm-2".to_string(), tx);

// Respond with DenyOnce
PermissionRegistry::respond("test-perm-2".to_string(), PermissionChoice::DenyOnce);

let response = rx.await.unwrap();
assert!(matches!(response, PermissionChoice::DenyOnce));
```

**Always Allow Pattern** (persists decision):
```rust
PermissionRegistry::respond("test-perm".to_string(), PermissionChoice::AlwaysAllow);
// Decision is persisted - future calls to same tool/path will auto-allow
```

## QuestionRegistry

For handling question tool responses:

```rust
pub struct QuestionRegistry {
    senders: DashMap<String, tokio::sync::oneshot::Sender<String>>,
}

impl QuestionRegistry {
    pub fn register(question_id: String, tx: tokio::sync::oneshot::Sender<String>);
    pub fn answer_question(question_id: String, answers: String) -> bool;
    pub fn unregister(question_id: &str);
}
```

**Important**: These are synchronous functions (`fn`), NOT async. Do NOT use `await` when calling these.

Example usage in tests:
```rust
// Set session ID on agent loop (Packet 3)
agent_loop.set_session_id("test-session-123");

// Register question
let (tx, rx) = tokio::sync::oneshot::channel();
QuestionRegistry::register("test-session-123".to_string(), tx);  // NOT async

// Answer the question
let answers = serde_json::json!({"q1": "red"}).to_string();
QuestionRegistry::answer_question("test-session-123".to_string(), answers);  // NOT async

// Verify response
let response = rx.await.unwrap();
assert!(response.contains("red"));
```

## Reference

- **Security Implementation Guide**: `.opencode/skills/security/SKILL.md`
- **Agent Loop Guide**: `.opencode/skills/agent-loop/SKILL.md`
- **Agent Loop Harness**: `tests/agent_loop_harness.rs` for PermissionRegistry/QuestionRegistry test patterns