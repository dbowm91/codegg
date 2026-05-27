# Deferred Items - Technical Implementation Plans

**Last Updated**: 2026-05-27

---

## EXEC-3: Token Flow - Display Wiring

### Problem
Tokens are parsed from API responses (`cached_tokens` from SSE) but never flow to the TUI's `SessionState` for display.

### Root Cause
```
Provider.stream() → ChatEvent::Finish { usage }
     ↓
EventProcessor::process() extracts tokens
     ↓
AgentLoop publishes AppEvent::AgentFinished WITHOUT token data
     ↓
TUI receives AgentFinished but session_state.token_in/out remain 0
```

### Files to Modify

| File | Lines | Change |
|------|-------|--------|
| `src/bus/events.rs` | 109-112 | Add token fields to `AgentFinished` |
| `src/agent/loop.rs` | 885-889 | Pass tokens to `AppEvent::AgentFinished` |
| `src/core/mod.rs` | 783-790 | Update `map_app_event_to_core_event` |
| `src/tui/mod.rs` | 1859 | Extract tokens and call `app.set_tokens()` |

### Implementation Steps

1. **Add fields to `AppEvent::AgentFinished`**:
```rust
AgentFinished {
    session_id: String,
    stop_reason: String,
    input_tokens: Option<usize>,      // ADD
    output_tokens: Option<usize>,     // ADD
    cached_tokens: Option<usize>,     // ADD
},
```

2. **Update AgentLoop** to extract and pass tokens:
```rust
ChatEvent::Finish { stop_reason, usage } => {
    crate::bus::global::GlobalEventBus::publish(AppEvent::AgentFinished {
        session_id: self.session_id.clone(),
        stop_reason: stop_reason.to_string(),
        input_tokens: Some(usage.input_tokens),
        output_tokens: Some(usage.output_tokens),
        cached_tokens: usage.cached_tokens,
    });
}
```

3. **Update TUI handler** to set tokens:
```rust
AppEvent::AgentFinished { stop_reason, input_tokens, output_tokens, cached_tokens, .. } => {
    if stop_reason == "completed" {
        // ... existing code ...
        if let (Some(in_tok), Some(out_tok)) = (input_tokens, output_tokens) {
            app.set_tokens(in_tok as u64, out_tok as u64);
            if let Some(ct) = cached_tokens {
                app.session_state.cached_tokens = ct as u64;
            }
        }
    }
}
```

---

## TUI-4: 75ms Resize Debounce

### Current State
- Resize events handled immediately at `src/tui/mod.rs:1817-1818`
- `on_resize()` sets `auto_scroll = true` but no layout recalculation happens
- No debounce mechanism exists

### Files to Modify

| File | Lines | Change |
|------|-------|--------|
| `src/tui/app/state/ui.rs` | 27-74 | Add `resize_debounce: Option<Instant>` field |
| `src/tui/mod.rs` | 1817-1818 | Start debounce timer instead of immediate call |
| `src/tui/mod.rs` | 1795-2137 | Add `tokio::select!` branch for 75ms delay |

### Implementation Steps

1. **Add debounce state** to `UiState`:
```rust
pub struct UiState {
    // ... existing fields ...
    pub resize_debounce: Option<tokio::time::Instant>,
}
```

2. **Modify event loop** - Instead of calling `on_resize()` immediately:
```rust
if let Event::Resize(_, _) = event {
    app.ui_state.resize_debounce = Some(tokio::time::Instant::now());
}
```

3. **Add debounce branch** to `tokio::select!`:
```rust
_ = async {
    tokio::time::sleep(Duration::from_millis(75)).await;
} if app.ui_state.resize_debounce.is_some() => {
    app.ui_state.resize_debounce = None;
    app.on_resize();
}
```

---

## MODEL-1: Thinking Params for Anthropic/OpenAI

### Current State
- `ModelVariant` in config only has `disabled: Option<bool>`
- `ModelVariant` in provider has `extra_params: serde_json::Value` but isn't wired
- Thinking/reasoning delta events are parsed (`ChatEvent::ReasoningDelta`)
- Reasoning tokens tracked in `TokenUsage`

### Files to Modify

| File | Lines | Change |
|------|-------|--------|
| `src/config/schema.rs` | 267-271 | Add thinking fields to `ModelVariant` |
| `src/provider/mod.rs` | 98-107 | Add thinking fields to `ChatRequest` |
| `src/provider/anthropic.rs` | 33-141 | Add thinking param to `build_body()` |
| `src/provider/openai.rs` | 115-254 | Add reasoning_effort to `build_body()` |
| `src/agent/loop.rs` | 930-942 | Wire variant config to request |

### Implementation Steps

1. **Add fields to config `ModelVariant`**:
```rust
pub struct ModelVariant {
    pub disabled: Option<bool>,
    pub thinking_budget: Option<usize>,    // Anthropic: budget_tokens
    pub reasoning_effort: Option<String>, // OpenAI: low/medium/high
}
```

2. **Add fields to `ChatRequest`**:
```rust
pub struct ChatRequest {
    // ... existing fields ...
    pub thinking_budget: Option<usize>,
    pub reasoning_effort: Option<String>,
}
```

3. **Update Anthropic `build_body()`**:
```rust
// After max_tokens handling:
if let Some(budget) = req.thinking_budget {
    body["thinking"] = json!({ "type": "enabled", "budget_tokens": budget });
}
```

4. **Update OpenAI `build_body()`**:
```rust
// After max_tokens handling:
if let Some(effort) = &req.reasoning_effort {
    body["reasoning_effort"] = json!(effort);
}
```

5. **Wire in AgentLoop** - Add variant resolution to `apply_agent_config()`:
```rust
fn apply_variant_config(&self, request: &mut ChatRequest) {
    // Get variant from agent config and apply thinking params
}
```

---

## AGENT-7: Three-Mode Sandbox System

### Current State
- Landlock sandbox exists in `src/security/sandbox.rs`
- Access flags are hardcoded: `READ | WRITE | EXEC` (danger-full-access)
- No configuration for sandbox mode
- BashTool has `landlock_sandbox: Option<SandboxConfig>`

### Files to Modify

| File | Lines | Change |
|------|-------|--------|
| `src/security/sandbox.rs` | 7-11, 85-119 | Define `SandboxMode` enum, modify `enforce_landlock()` |
| `src/tool/bash.rs` | 69-78, 153-167, 345-355 | Add mode field integration |
| `src/config/schema.rs` | 22-64, 345-365 | Add `sandbox_mode` config field |
| `src/tool/mod.rs` | 89-120 | Wire sandbox mode to BashTool creation |
| `src/agent/loop.rs` | 574-669 | Extract and pass sandbox mode |

### Implementation Steps

1. **Define `SandboxMode` enum** in `src/security/sandbox.rs`:
```rust
#[derive(Clone, Debug, Default, PartialEq)]
pub enum SandboxMode {
    #[default]
    ReadOnly,          // LANDLOCK_ACCESS_FS_READ
    WorkspaceWrite,    // READ + WRITE
    DangerFullAccess,   // READ + WRITE + EXEC (current)
}

impl SandboxMode {
    pub fn access_flags(&self) -> u64 {
        match self {
            SandboxMode::ReadOnly => LANDLOCK_ACCESS_FS_READ,
            SandboxMode::WorkspaceWrite => LANDLOCK_ACCESS_FS_READ | LANDLOCK_ACCESS_FS_WRITE,
            SandboxMode::DangerFullAccess => {
                LANDLOCK_ACCESS_FS_READ | LANDLOCK_ACCESS_FS_WRITE | LANDLOCK_ACCESS_FS_EXEC
            }
        }
    }
}
```

2. **Modify `SandboxConfig`**:
```rust
pub struct SandboxConfig {
    pub enabled: bool,
    pub mode: SandboxMode,  // ADD
    pub allowed_paths: Vec<String>,
    pub deny_paths: Vec<String>,
}
```

3. **Update `enforce_landlock()`** to use mode:
```rust
let handled_access = self.mode.access_flags();
```

4. **Add `sandbox_mode` to `PermissionConfig`** in `src/config/schema.rs`:
```rust
pub struct PermissionConfig {
    // ... existing fields ...
    pub sandbox_mode: Option<String>,  // "read_only", "workspace_write", "danger_full_access"
}
```

5. **Add builder method to `BashTool`**:
```rust
pub fn with_sandbox_mode(mut self, mode: SandboxMode) -> Self {
    // Configure landlock_sandbox with mode
}
```

6. **Wire through AgentLoop** - Extract mode from config, pass to ToolRegistry

---

## Summary

| Item | Complexity | Time Estimate | Priority |
|------|------------|---------------|----------|
| EXEC-3: Token Display Wiring | Low-Medium | 2-3 hours | HIGH |
| TUI-4: Resize Debounce | Low | 1-2 hours | MEDIUM |
| MODEL-1: Thinking Params | Medium | 3-4 hours | MEDIUM |
| AGENT-7: Sandbox Modes | Medium-High | 4-6 hours | MEDIUM-LOW |
