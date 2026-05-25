---
name: notifications
description: Desktop notifications for long-running task completion
version: 1.0.0
tags: [notifications, desktop, ui, ux]
---

Use the `/skill:notifications` command to load context about desktop notification support.

## Overview

Desktop notifications provide non-blocking alerts for long-running task completion, errors, and important events. They use the `notify-rust` crate for cross-platform support.

## Configuration

```yaml
notifications:
  enabled: true
  on_task_complete: true
  on_error: true
```

## Notification Types

```rust
pub enum NotificationType {
    Info,       // General information
    Success,    // Task completed successfully
    Warning,    // Warning message
    Error,      // Error occurred
}
```

## Usage

From async context via TuiCommand:
```rust
let _ = tx.try_send(TuiCommand::SendNotification {
    notification_type: NotificationType::Success,
    body: "Task completed successfully".to_string(),
});
```

Or directly (blocking):
```rust
NotificationManager::blocking_send(
    NotificationType::Info,
    "Done!",
    true
);
```

## Module

`src/tui/components/notification.rs` contains:
- `NotificationType` enum
- `NotificationManager` for sending notifications

## Platform Support

- Linux: libnotify (via notify-rust)
- macOS: NSUserNotification
- Windows: Windows Toast Notifications

## Integration Points

Notifications are sent when:
- Long-running task completes
- Agent completes a significant step
- Errors occur that need attention

They are non-blocking and don't pause the TUI.