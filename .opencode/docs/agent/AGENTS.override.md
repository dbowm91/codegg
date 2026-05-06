# Agent Module Override

This file contains agent-specific guidance and overrides root AGENTS.md.

## TuiCommand Enum

The `TuiCommand` enum (`src/tui/app/mod.rs`) is the primary way to communicate intent from UI components to the main application loop:

```rust
pub enum TuiCommand {
    DeleteSession { session_id: String },
    ArchiveSession { session_id: String, unarchive: bool },
    UndoDelete { session_id: String },
    ForkSession { session_id: String },
    ShareSession { session_id: String },
    UnshareSession { session_id: String },
    ExportSession { session_id: String },
    RenameSession { session_id: String, new_title: String },
    BulkDelete { session_ids: Vec<String> },
    BulkArchive { session_ids: Vec<String>, unarchive: bool },
    BulkExport { session_ids: Vec<String> },
    ReloadSessions,
    OpenTreeDialog,
    PreviewImport { source: ImportSource },
    ConfirmImport { source: ImportSource },
    CreateFromTemplate { key: String, template: SessionTemplate },
    LoadSessionMessages { session_id: String },
    SpawnSubagent { agent_name: String, prompt: String },
    ListTasks,
    DeleteTask { id: String },
    CompactSession,
    OpenDiffDialog { old_content: String, new_content: String, title: String },
    SendNotification { notification_type: NotificationType, body: String },
}
```

## TuiMsg Enum

The `TuiMsg` enum (`src/tui/app/types.rs`) provides a centralized message type for UI intentions, enabling decoupled event handling. All dialogs emit explicit TuiMsg for user-visible effects:

```rust
pub enum TuiMsg {
    // Navigation & Submission
    SubmitPrompt,
    NavigateUp,
    NavigateDown,
    // Dialog Open/Close
    OpenModelDialog,
    OpenAgentDialog,
    OpenSessionDialog,
    CloseDialog,
    // Dialog-Specific Results
    SelectModel { model: String },
    SelectAgent { agent_name: String },
    SelectSession { session_id: String },
    ConnectConfigured { provider_name: String, env_var: Option<String>, api_key: Option<String> },
    SelectTheme { theme_name: String },
    SubmitPermission { choice_index: usize },
    SubmitQuestionAnswers { answers_json: String },
    SelectTreeSession { session_id: String },
    ForkTreeSession { session_id: String },
    SubmitImportPreview,
    ConfirmImport,
    SelectTemplate { key: String },
    GotoMessage { index: usize },
    CopyShareUrl,
    McpAction { server_name: String, action: String },
    KeybindChanged { action: String, binding: String },
    // Confirmation
    ConfirmDeleteSession { session_id: String },
    ConfirmArchiveSession { session_id: String, unarchive: bool },
    ConfirmBulkDelete { count: usize },
    ConfirmBulkArchive { count: usize, unarchive: bool },
    ConfirmResult(Option<bool>),
    ForkSession { session_id: String },
    // Input
    CharInput(char),
    // ... and more
}
```

## Component Trait

The `Component` trait (`src/tui/components/component.rs`) provides a standardized interface for UI elements:

```rust
pub trait Component: Send {
    fn handle_key(&mut self, key: KeyEvent) -> Option<TuiMsg>;
    fn update(&mut self, msg: TuiMsg) -> Option<TuiMsg>;
    fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Arc<Theme>);
    fn dialog_type(&self) -> DialogType;
    fn is_modal(&self) -> bool { self.dialog_type().is_modal() }
}
```

Implemented by ALL dialogs: HelpDialog, InfoDialog, ModelDialog, AgentDialog, SessionDialog, ShareDialog, ConfirmDialog, QuestionDialog, PermissionDialog, ThemePickerDialog, TreeDialog, ImportDialog, TemplateDialog, ConnectDialog, GotoDialog, PlanDialog, DiffDialog, McpDialog, KeybindDialog

## FocusManager

The `FocusManager` (`src/tui/components/component/focus.rs`) maintains a stack of `Box<dyn Component>` for modal focus handling. Events are routed to the top of the stack.

**Helper methods added in Wave 7.6**:
- `active_dialog_type()` - returns the DialogType of the top component
- `push_dialog()` / `close_dialog()` / `replace_dialog()` - helper methods on App

## App Helper Methods (Wave 7.6)

```rust
fn push_dialog(&mut self, dialog: Dialog, component: Box<dyn Component>);
fn close_dialog(&mut self);
fn replace_dialog(&mut self, dialog: Dialog, component: Box<dyn Component>);
fn active_dialog_type(&self) -> DialogType;
```

## Async Command Pattern

Commands that need async operations should use the `TuiCommand` pattern:

1. Add variant to `TuiCommand` enum in `src/tui/app/mod.rs`
2. Add async handler in `src/tui/mod.rs` (e.g., `handle_your_command`)
3. Add match arm in `run_event_loop` to route to handler
4. From sync handlers, use `tui_cmd_tx.try_send(TuiCommand::YourCommand { ... })`

## Context Compaction

When context is full, the agent uses `src/agent/compaction.rs` with tiered strategies:

- **Tier 1**: TruncateToolOutputs - truncate long tool outputs to ~500 chars
- **Tier 2**: DropMiddleMessages - keep first/last messages, drop middle
- **Tier 3**: SummarizeOldTurns - LLM summarization with async support

The `compact_messages_async()` function calls the provider to generate summaries when `SummarizeOldTurns` strategy is selected. Falls back to placeholder text if provider unavailable.

Use `auto_compact_async()` for async contexts or `auto_compact()` for sync contexts.

## Snapshot Checkpointing

The `SnapshotManager` (`src/snapshot/mod.rs`) provides checkpointing capability:

- **Wired to AgentLoop**: `snapshot_manager` field added to `AgentLoop` struct
- **Capture trigger**: Snapshots captured before file-modifying tools (write, edit, replace, multiedit, apply_patch)
- **Config-driven**: Enable via `snapshot: true` in config
- **Implementation**: `capture_snapshot_if_needed()` method checks if any pending tool is file-modifying

Usage in `execute_tool_calls()`:
```rust
let has_file_modifying = allowed_tools.iter().any(|(_, tc)| is_file_modifying_tool(&tc.name));
if has_file_modifying {
    self.capture_snapshot_if_needed().await;
}
```

The `is_file_modifying_tool()` helper identifies tools that modify files.



## Model Auto-Routing

The `ModelRouter` in `src/agent/router.rs` automatically routes tasks:

- **Simple** (ls, cat, read): routed to fast model
- **Medium** (edit, write): routed to medium model
- **Complex** (debug, plan, review): routed to reasoning model

Enable via `auto_route_models: true` in config.

## Multi-Agent Teams

Agents can collaborate via file-based inbox/outbox in `.opencode/team/{team_name}/`. See `src/agent/team.rs`.

## Async Patterns

- Prefer `tokio::sync::Mutex` over `parking_lot`.
- Use the `TuiCommand` pattern to bridge sync UI event handlers to async logic.
- Avoid `rt.block_on()` in async contexts; use `tokio::spawn` or pass handles.

## Event Subscription Pattern

When testing code that publishes to `GlobalEventBus`, subscribe BEFORE spawning the task that emits events:

```rust
// BAD: Subscribe after spawn - may miss events
let handle = tokio::spawn(async move { agent_loop.run(request).await });
let mut rx = GlobalEventBus::subscribe();
wait_for_event(&mut rx, ...).await;

// GOOD: Subscribe before spawn
let mut rx = GlobalEventBus::subscribe();
let handle = tokio::spawn(async move { agent_loop.run(request).await });
wait_for_event(&mut rx, ...).await;
```

The same pattern applies to permission and question handling in tests. `wait_for_question_pending` and `wait_for_permission_pending` helpers in `tests/agent_loop_harness.rs` handle this correctly when used with a pre-spawned receiver.

## Related Skills

- `.opencode/skills/permission/` - PermissionChoice, PermissionRegistry patterns, and registry-based recovery
- `.opencode/skills/event-bus/` - GlobalEventBus, AppEvent types, and EventCollector for tests
- `.opencode/skills/subagent/` - SubAgentPool shutdown with CancellationToken and TaskStatus::Interrupted

## Follow-Up Contract

The `AgentLoop::follow_up_sender()` returns a channel sender with this contract:

- **Follow-ups queued BEFORE `run()`**: processed by that `run()` call
- **Follow-ups arriving AFTER `run()` returns**: NOT consumed by the completed run; require a new `run()` call

`drain_follow_up()` uses non-blocking `try_recv()`, so late follow-ups are not processed. The test `test_follow_up_queued_before_run_is_processed` in `tests/agent_loop_harness.rs` verifies the queued-before-run case.

## Test Harness Patterns

Key patterns from `tests/agent_loop_harness.rs`:

- `ScriptedProvider` - scripted responses for deterministic testing
- `wait_for_question_pending()` / `wait_for_permission_pending()` - event-based coordination with timeouts
- `EventCollector` - collects and asserts on GlobalEventBus events
- For async subagent tests: use bounded polling (e.g., `wait_for_request()`) instead of fixed sleeps

## Permission and Question Registry Patterns

### Registration-Before-Publish Pattern

When publishing `PermissionPending` or `QuestionPending` events, always register the responder BEFORE publishing the event to avoid race conditions where a fast client observes the event before the registry entry exists:

```rust
// CORRECT: Register first, then publish
let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
PermissionRegistry::register(perm_id.clone(), resp_tx).await;
GlobalEventBus::publish(AppEvent::PermissionPending { ... });

// WRONG: Publish before register - response may be lost
GlobalEventBus::publish(AppEvent::PermissionPending { ... });
let (resp_tx, resp_rx) = tokio::sync::oneshot::channel();
PermissionRegistry::register(perm_id.clone(), resp_tx).await;
```

This applies to both `AgentLoop::check_tool_permission()` and `handle_tui_message()` in websocket code.

### Registry-Based Recovery for Critical Events

Critical events (`PermissionPending`, `QuestionPending`) are recoverable from registries even if the GlobalEventBus event is missed:

```rust
// Check pending items
let pending_perms = PermissionRegistry::pending_permission_ids();
let pending_questions = QuestionRegistry::pending_question_ids();

// Check if specific item is registered
let is_registered = PermissionRegistry::is_registered(&perm_id);
```

### Remote Question Response Flow

Remote question responses (HTTP and websocket) wire into `QuestionRegistry::answer_question()`:

1. HTTP `POST /session/{session_id}/question` → `submit_question()` → `QuestionRegistry::answer_question()`
2. Websocket `QuestionResponse` message → `handle_tui_message()` → `QuestionRegistry::answer_question()`

If no pending question exists, `answer_question()` returns `false`.

## SubAgentPool Shutdown

`SubAgentPool::shutdown()` uses `CancellationToken` for cooperative cancellation:

```rust
pub async fn shutdown(&self) {
    self.cancel_token.cancel();
    drop(self.request_tx.clone());  // Close sender to stop accepting new work
    
    // Wait briefly for cooperative cancellation (up to 1 second)
    // Abort only as a fallback
    for handle in active_handles.drain(..) {
        handle.abort();
    }
    
    // Wait for worker loop handle
    for handle in workers {
        let _ = handle.await;
    }
}
```

Tasks cancelled during shutdown are marked `TaskStatus::Interrupted` (not `TaskStatus::Failed`).

**Key changes (Wave 2)**:
- Added RAII-style `ActiveCountGuard` to ensure `active_count` is properly decremented
- `shutdown()` now waits briefly for cooperative cancellation before aborting
- `set_interrupted()` method added to `TaskStore` for preserving Interrupted status
- `TaskStatus` now derives `PartialEq` for proper status comparison

## Tool Definition Cache Invalidation

`AgentLoop::permission_version()` uses JSON serialization of the full `PermissionConfig` for deterministic hashing:

```rust
fn permission_version(&self) -> u64 {
    if let Some(ref perm) = self.config.permission {
        let json = serde_json::to_string(perm).unwrap_or_default();
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        json.hash(&mut hasher);
        hasher.finish()
    } else {
        0
    }
}
```

This ensures any permission config change invalidates the tool-definition cache.

## MCP Lock Behavior

MCP tool availability uses `mcp_arc.try_read()` to avoid blocking the agent loop during MCP writes:

- If the read succeeds, tools are available
- If the read fails (locked), an error "MCP service locked, please retry" is returned
- The error message encourages retry rather than treating as permanent failure
- Debug logging is emitted for transient unavailability

Cache invalidation for MCP tools uses count-based tracking. If MCP tool identities change without count changing, the cache may be stale - this is a known limitation documented in `src/agent/loop.rs`.