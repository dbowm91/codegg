# Bus Module Architecture Review

## Verified Correct Items

1. **GlobalEventBus** (`src/bus/global.rs`):
   - `LazyLock` singleton pattern ✓
   - Broadcast channel capacity 2048 ✓
   - `publish()` returns subscriber count on success (tracing trace) ✓
   - `subscribe()` returns broadcast::Receiver ✓
   - `subscriber_count()` via receiver_count() ✓

2. **PermissionRegistry** (`src/bus/mod.rs:9-74`):
   - DashMap with 300s TTL ✓
   - `register()`, `respond()`, `unregister()`, `is_registered()`, `pending_permission_ids()` ✓
   - `cleanup_now()` public helper ✓

3. **QuestionRegistry** (`src/bus/mod.rs:76-141`):
   - Same pattern as PermissionRegistry ✓
   - Uses 300s TTL ✓
   - `answer_question()` (not "respond") correctly named ✓

4. **AppEvent** (`src/bus/events.rs`):
   - 36 variants verified by direct count ✓
   - `event_type()` method at lines 149-189 for SSE filtering ✓

5. **PermissionChoice** (`src/permission/mod.rs:129`):
   - 4 variants: AllowOnce, AlwaysAllow, DenyOnce, AlwaysDeny ✓

6. **SSE Handler** (`src/server/routes/event.rs:12-32`):
   - Directly subscribes to `GlobalEventBus::subscribe()` ✓
   - 15-second heartbeat interval ✓

7. **Registration-before-publish pattern**: Documented correctly ✓

## Incorrect/Stale Items

None found - architecture document is accurate.

## Bugs in Related Code

None found - implementation is correct.

## Summary

The architecture document at `architecture/bus.md` is **correct**. All claims verified against source:
- 36 AppEvent variants
- 300-second TTL for both registries
- Broadcast channel capacity 2048
- Correct SSE handler path `src/server/routes/event.rs`
- Correct event categorization
- Correct PermissionChoice enum

No bugs found.
