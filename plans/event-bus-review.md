# Event Bus Module Review

## Verified Claims

### GlobalEventBus (`src/bus/global.rs`)
- **Location**: `src/bus/global.rs` ✓
- **Singleton pattern**: `static GLOBAL_BUS: LazyLock<GlobalEventBus>` ✓
- **Channel capacity**: 2048 ✓ (`broadcast::channel(2048)`)
- **pub fn publish(event: AppEvent)** ✓ - matches implementation
- **Log levels**: `Ok(0)` → `debug!`, `Ok(n)` → `trace!`, `Err(e)` → `warn!` ✓
- **pub fn subscribe()** → `broadcast::Receiver<AppEvent>` ✓
- **pub fn subscriber_count()** → `usize` ✓

### AppEvent Enum (`src/bus/events.rs`)
- **36 event variants** (confirmed via count) ✓ - architecture doc says 36
- **Session Events (7)**: All present and match ✓
- **Message Events (2)**: All present and match ✓
- **Tool Events (3)**: All present and match ✓
- **MCP Events (3)**: All present and match ✓
- **Permission Events (2)**: All present and match ✓
- **Question Events (2)**: All present and match ✓
- **Streaming Events (3)**: All present and match ✓
- **Subagent Events (4)**: All present and match ✓
- **Diff Events (2)**: All present and match ✓
- **Other Events (9)**: All present and match ✓
- **event_type() method** for SSE filtering ✓

### PermissionRegistry & QuestionRegistry (`src/bus/mod.rs`)
- **Struct definitions match** ✓
- **300-second TTL** with cleanup on each `register()` call ✓
- **Static lazy initialization** pattern matches ✓

### PermissionChoice Enum (`src/permission/mod.rs`)
- **All 4 variants present**: `AllowOnce`, `AlwaysAllow`, `DenyOnce`, `AlwaysDeny` ✓
- **`allowed()` and `persist()` helper methods** ✓

### Registration-Before-Publish Pattern
- Verified in `src/agent/loop.rs:401` (QuestionRegistry) and `src/agent/loop.rs:475` (PermissionRegistry) ✓
- Pattern correctly documented ✓

### SSE Handler (`src/server/routes/event.rs`)
- Directly subscribes to `GlobalEventBus::subscribe()` ✓
- 15-second heartbeat with `keep_alive` interval ✓
- Formats as `event: {event_type}\ndata: {json}\n\n` ✓

## Bugs/Discrepancies Found

### 1. Skill file incorrect event count (Priority: LOW)
**File**: `.opencode/skills/event-bus/SKILL.md:71`

The skill says "All 38 event variants" but implementation has **36 variants**.

The skill also incorrectly categorizes "Other" as having 7 events (line 84) when it's actually 8:
- Counted: `ConfigChanged`, `AgentChanged`, `ModelChanged`, `CompactionTriggered`, `Error`, `Info`, `TodoUpdated`, `FileChanged` = **8 events**

**Fix**: Update line 71 and 84 in skill file.

## Improvement Suggestions

### Priority: LOW

1. **Update event-bus skill count**: Change "38 event variants" → "36 event variants" in `.opencode/skills/event-bus/SKILL.md:71`

2. **Fix Other category count**: Change "(7)" → "(8)" in `.opencode/skills/event-bus/SKILL.md:84`

## Summary

The architecture document (`architecture/event-bus.md`) is **accurate** and matches the implementation exactly. All types, methods, fields, and behaviors are correctly documented.

The minor discrepancy found is in the **skill file** (`.opencode/skills/event-bus/SKILL.md`), not the architecture document. This skill file has an incorrect event count (says 38 instead of 36) and wrong category counts.