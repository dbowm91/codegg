# TUI Phase 10: Dialog Loading, Cancellation, and Stale Result Handling

## Objective

Standardize async dialog behavior across the TUI. Dialogs that load data, preview files, query core, or perform background work should share consistent semantics for loading, cancellation, stale results, disabled actions, and error display.

## Current Shape

Recent TUI work introduced request IDs and `start_*`/`apply_*` completion paths for several operations. That closed many responsiveness issues. However, each dialog and command still tends to manage its own loading flags and stale-result behavior. The next pass should turn those ad hoc patterns into a small, consistent state machine.

## Core Problem

Async UI surfaces commonly need the same lifecycle:

1. Idle.
2. Start request.
3. Show loading state.
4. Allow cancellation/close.
5. Receive result.
6. Ignore stale result if a newer request superseded it.
7. Apply success or error.
8. Clear loading state.

If every dialog implements this independently, it is easy to forget one of these steps. That causes stale previews, stuck spinners, double-submit bugs, or errors appearing after a dialog has been closed.

## Target Dialogs and Surfaces

Prioritize:

- session selector reload and message counts
- tree dialog loading
- import preview and confirm
- research browser list/run/section loads
- memory search/summary
- model selector if model discovery becomes async
- shell show/detail if it later loads persisted history
- doctor/TUI stats/info surfaces
- share/export dialogs
- permission/question dialogs only if they gain async side effects

## Shared State Model

Introduce a small reusable request state type:

```rust
#[derive(Debug, Clone)]
pub struct AsyncUiRequestState {
    pub request_id: u64,
    pub loading: bool,
    pub cancelled: bool,
    pub started_at: Option<std::time::Instant>,
    pub last_error: Option<String>,
}

impl AsyncUiRequestState {
    pub fn begin(&mut self) -> u64;
    pub fn cancel(&mut self);
    pub fn is_current(&self, request_id: u64) -> bool;
    pub fn finish(&mut self, request_id: u64) -> bool;
    pub fn fail(&mut self, request_id: u64, error: String) -> bool;
}
```

If storing `Instant` inside persistent-ish dialog state is awkward, keep it runtime-only or omit it. The key pieces are request ID, loading, cancelled, and last error.

## State Machine Rules

### Begin

- Increment request ID.
- Set loading true.
- Clear cancelled.
- Clear previous transient error unless it should remain visible.
- Disable destructive confirm buttons where relevant.

### Cancel/Close

- Set cancelled true.
- Set loading false.
- Increment request ID or invalidate the current one.
- If Phase 7 task registry exists, cancel associated task.
- Do not show a cancellation toast for normal dialog close.

### Apply success

- If request ID is not current, ignore.
- If cancelled, ignore.
- Set loading false.
- Clear last error.
- Apply data.

### Apply error

- If request ID is not current, ignore.
- If cancelled, ignore.
- Set loading false.
- Store last error.
- Show inline error where possible; use toast only for global operations.

## Implementation Steps

### 1. Add shared request-state helper

Place in `src/tui/app/state/async_request.rs` or near dialog state. Keep it independent from specific dialogs.

Add tests for begin/cancel/finish/fail semantics before migrating dialogs.

### 2. Migrate import dialog

Import preview/confirm is the highest-value migration because it has explicit request IDs and user-visible stale-result risk.

- Replace ad hoc `import_request_id` fields with `AsyncUiRequestState` if possible.
- Preview source changes should cancel/invalidate prior preview.
- Confirm should be disabled while confirm request is loading.
- Closing dialog should cancel/invalidate preview and confirm.

### 3. Migrate research browser

Research browser has list/run/section request semantics. Consider separate request states:

```rust
pub runs_request: AsyncUiRequestState,
pub run_request: AsyncUiRequestState,
pub section_request: AsyncUiRequestState,
```

Or one request state if only one load may be active at a time. Separate states are clearer if section loading should not invalidate a run list refresh.

### 4. Migrate session/tree loading

Session selector reload and tree dialog loading should use the same helper. Closing the dialog should invalidate outstanding requests.

For session message loading, stale handling should check both request ID and target session ID. A delayed message load for session A should never replace visible messages after the user switched to session B.

### 5. Migrate memory/doctor/info operations where appropriate

Memory and doctor often produce toasts rather than dialogs. Use request state only if repeated invocations can overlap or if a loading UI exists. Otherwise keep simple completion handling but add stale protection if repeated requests can overwrite state.

### 6. Standardize loading visuals

Define common loading/error conventions:

- loading spinner or `Loading...` line in dialog content
- disabled confirm button text such as `Importing...`
- inline error line for dialog-local failures
- toast for global background operation failures

### 7. Standardize close behavior

Dialog close should call a central method that invalidates associated async request state. Avoid close handlers that only set `Dialog::None` and leave request state live.

Suggested methods:

```rust
impl App {
    pub fn close_dialog_with_cancellation(&mut self);
}

impl DialogState {
    pub fn cancel_dialog_requests(&mut self, dialog: Dialog);
}
```

## Testing Plan

Unit tests:

1. `AsyncUiRequestState::begin` increments and marks loading.
2. Stale finish returns false and does not clear current loading.
3. Cancel invalidates current request.
4. Error application stores last error only for current request.
5. Closing import dialog invalidates preview/confirm request.
6. Research run stale completion is ignored.
7. Session messages for old session cannot replace current session messages.

Integration-style tests:

1. Start import preview A, then preview B, then apply A: B remains active.
2. Start research load, close browser, apply result: no dialog reopens and no stale toast appears.
3. Start session message load, switch session, apply old result: visible messages unchanged.

Manual smoke tests:

1. Rapidly switch import sources while previewing.
2. Open/close research browser during slow loads.
3. Switch sessions during slow message load.
4. Repeatedly run memory search or doctor command and verify no stale output overrides latest visible state.

## Acceptance Criteria

- Dialog async request state uses a shared helper for key dialogs.
- Closing/superseding a dialog invalidates in-flight results.
- Stale completions are ignored consistently.
- Loading and error display semantics are consistent across migrated dialogs.
- Tests cover stale success, stale error, cancellation, and close behavior.
- Workspace checks pass.

## Out of Scope

- Full task cancellation implementation if Phase 7 is not yet complete; invalidating stale results is enough for this phase.
- Reworking every dialog visual design.
- Persistent request history.
- Remote-client cancellation propagation.
