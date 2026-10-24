//! Desktop todo channel — the TUI's `Ctrl+T` task surface.
//!
//! Todos are *not* part of the canonical session projection. The daemon
//! keeps them behind a dedicated request
//! (`CoreRequest::TodoList`, `crates/codegg-protocol/src/core.rs:3002`)
//! and publishes updates as `CoreEvent::TodoListUpdated`
//! (`crates/codegg-protocol/src/core.rs:4398`). This module reads that
//! list for the route-selected session and carries it into the
//! presentation view.
//!
//! Rules, all of them borrowed from the TUI's own consumer:
//!
//! - the read is fenced on the route and connection generation exactly
//!   like the control-plane refresh
//!   (`apps/desktop/src-tauri/src/control.rs:90-124`): a read that
//!   completes after a route switch is discarded, never merged;
//! - the read **fails closed**. A reply that is not a `Json` payload is
//!   an error, not an empty list — the TUI draws the same distinction
//!   (`src/tui/commands/goals.rs:322-330`: "a reply that is not a `Json`
//!   payload is a *failed* read, not an empty list");
//! - the list is session-scoped: only the route-selected session is
//!   read, and the completion must still name that session;
//! - a malformed entry is **skipped, not fatal**
//!   (`tests/tui_render/tui_presentation_regressions.inc:302`).

use codegg_protocol::core::{CoreRequest, CoreResponse, RequestEnvelope, PROTOCOL_VERSION};

use super::bridge::{SessionPresentationView, TodoItemView, TodoListView};
use super::HostState;

/// Renderer cap on todo entries in one list.
///
/// The daemon answers with the session's whole stored list; the bridge
/// stays bounded so a pathological list cannot become an unbounded
/// payload. The cap is generous relative to the TUI's own working set
/// (a task list is tens of entries, not thousands).
pub(crate) const MAX_TODO_ITEMS: usize = 256;

/// The status a todo entry carries when the daemon omits one.
const DEFAULT_TODO_STATUS: &str = "pending";

/// The priority a todo entry carries when the daemon omits one.
const DEFAULT_TODO_PRIORITY: &str = "medium";

/// Decode the daemon's todo payload into renderer views.
///
/// The daemon answers `CoreRequest::TodoList` with
/// `{"items": [...]}` where each item is a `TodoItemSnapshot`
/// (`crates/codegg-core/src/bus/events.rs:10-15`). The items cross the
/// wire as opaque JSON, so the decode is defensive and mirrors the
/// TUI's event-side rule (`src/tui/app/mod.rs:2459-2480`): an entry
/// without a string `content` is skipped rather than discarding the
/// whole list, and a missing `status`/`priority` falls back to
/// "pending"/"medium" instead of rendering empty.
pub(crate) fn parse_todo_items(data: &serde_json::Value) -> Vec<TodoItemView> {
    let Some(items) = data.get("items").and_then(|value| value.as_array()) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let content = item.get("content")?.as_str()?.to_string();
            Some(TodoItemView {
                id: item
                    .get("id")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .to_string(),
                content,
                status: item
                    .get("status")
                    .and_then(|value| value.as_str())
                    .unwrap_or(DEFAULT_TODO_STATUS)
                    .to_string(),
                priority: item
                    .get("priority")
                    .and_then(|value| value.as_str())
                    .unwrap_or(DEFAULT_TODO_PRIORITY)
                    .to_string(),
            })
        })
        .take(MAX_TODO_ITEMS)
        .collect()
}

impl HostState {
    /// Read the route-selected session's todo list and carry it into the
    /// presentation view.
    ///
    /// Returns the whole presented view so the renderer keeps exactly
    /// one source of truth for session state: the host reads the list,
    /// the renderer renders it. A failure returns `Err` and leaves the
    /// renderer's previous view untouched — the host never fabricates
    /// an empty list to paper over a daemon fault.
    pub(crate) async fn route_todo_refresh(
        &self,
        expected_generation: u64,
    ) -> Result<SessionPresentationView, String> {
        let (session_id, connection_generation) = {
            let route = self.route.lock().await;
            if route.connection_generation != self.current_generation()
                || route.project_id.is_none()
            {
                return Err("no current route; select a project first".into());
            }
            if route.route_generation != expected_generation {
                return Err("stale route; refresh before reading tasks".into());
            }
            let Some(session_id) = route.session_id.clone() else {
                return Err("no session selected for the current route".into());
            };
            (session_id, route.connection_generation)
        };
        let (client, _) = self.route_request().await?;
        let response = client
            .request(RequestEnvelope {
                protocol_version: PROTOCOL_VERSION,
                request_id: uuid::Uuid::new_v4().to_string(),
                payload: CoreRequest::TodoList {
                    session_id: session_id.clone(),
                },
            })
            .await
            .map_err(|error| error.to_string())?;
        let CoreResponse::Json { data } = response else {
            // A non-`Json` reply is a failed read, not an empty list.
            return Err(crate::route::unexpected_daemon_response(&response));
        };
        let items = parse_todo_items(&data);
        {
            let route = self.route.lock().await;
            if self.current_generation() != connection_generation
                || route.connection_generation != connection_generation
                || route.route_generation != expected_generation
                || route.session_id.as_deref() != Some(session_id.as_str())
            {
                return Err("stale route; task refresh discarded".into());
            }
        }
        let mut view = self.projection_current().await?;
        view.todos = Some(TodoListView { session_id, items });
        Ok(view)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(value: serde_json::Value) -> Vec<TodoItemView> {
        parse_todo_items(&value)
    }

    #[test]
    fn a_full_entry_keeps_its_state() {
        let parsed = items(serde_json::json!({
            "items": [{
                "id": "pos-0",
                "content": "Track A: implement",
                "status": "in_progress",
                "priority": "high",
            }],
        }));
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].id, "pos-0");
        assert_eq!(parsed[0].content, "Track A: implement");
        assert_eq!(parsed[0].status, "in_progress");
        assert_eq!(parsed[0].priority, "high");
    }

    #[test]
    fn malformed_entries_are_skipped_not_fatal() {
        let parsed = items(serde_json::json!({
            "items": [
                { "status": "pending" },
                { "content": 42 },
                { "content": "survivor" },
            ],
        }));
        // The TUI's own rule: a malformed entry is skipped rather than
        // discarding the whole list
        // (tests/tui_render/tui_presentation_regressions.inc:302).
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].content, "survivor");
        // Missing fields fall back rather than producing an empty state.
        assert_eq!(parsed[0].status, "pending");
        assert_eq!(parsed[0].priority, "medium");
    }

    #[test]
    fn a_payload_without_items_is_an_empty_list() {
        assert!(items(serde_json::json!({})).is_empty());
        assert!(items(serde_json::json!({ "items": "not-a-list" })).is_empty());
    }

    #[test]
    fn the_list_is_capped() {
        let many: Vec<serde_json::Value> = (0..MAX_TODO_ITEMS + 10)
            .map(|index| serde_json::json!({ "content": format!("task {index}") }))
            .collect();
        let parsed = items(serde_json::json!({ "items": many }));
        assert_eq!(parsed.len(), MAX_TODO_ITEMS);
    }
}
