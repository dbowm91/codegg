pub mod agent;
pub mod async_request;
pub mod chat;
pub mod diagnostics;
pub mod dialog;
pub mod manifest;
pub mod messages;
pub mod observe;
pub mod persistence;
pub mod plugin_ui;
pub mod presence;
pub mod project_picker;
pub mod project_tabs;
pub mod projection_client;
pub mod prompt;
pub mod restore;
pub mod routing;
pub mod session;
pub mod snapshot;
pub mod ui;
pub mod view_switch;

pub use agent::AgentState;
pub use async_request::AsyncUiRequestState;
pub use chat::{
    extract_mentions, message_line, ChatState, ChatStatus, ProjectChat, MAX_CHAT_DISPLAY_MESSAGES,
    MAX_CHAT_DRAFT_LEN, MAX_CHAT_MESSAGES_PER_CHANNEL, MAX_CHAT_PROJECTS,
};
pub use diagnostics::TuiDiagnostics;
pub use dialog::DialogState;
pub use messages::MessagesState;
pub use observe::{
    is_observer_allowed_command, observer_blocked_message, ObserveStatus, ObservedTarget,
    ObserverState,
};
pub use plugin_ui::{PluginUiApplyResult, PluginUiState};
pub use presence::{
    activity_label, display_principal, CollaboratorEntry, PresenceState, PresenceStatus,
    ProjectPresence, MAX_PRESENCE_DISPLAY_PRINCIPALS, MAX_PRESENCE_DISPLAY_SESSIONS,
    MAX_PRESENCE_PROJECTS, MAX_PRINCIPAL_DISPLAY_LEN,
};
pub use project_picker::{
    PickerPhase, ProjectPickerState, RegistrationDraft, SessionSummaryCacheEntry,
    MAX_OPEN_PROJECT_TABS, MAX_PROJECT_LIST_ITEMS, MAX_TAB_LABEL_LEN,
};
pub use project_tabs::{ProjectCatalogState, ProjectTabId, ProjectTabState, ProjectTabs};
pub use projection_client::{
    ArtifactExcerptCacheEntry, ArtifactHandleCacheEntry, ProjectionClientState,
    ProjectionCursorInfo, ProjectionTabSummary, MAX_ARTIFACT_EXCERPTS_PER_TAB,
    MAX_ARTIFACT_EXCERPT_BYTES, MAX_ARTIFACT_HANDLES_PER_TAB, MAX_ARTIFACT_READS_PER_TAB,
    MAX_TAB_PROJECTION_SUMMARIES,
};
pub use prompt::PromptState;
pub use routing::{
    apply_inactive_summary, classify_event, event_project_id, event_session_id,
    InactiveSummaryKind, RouteCheck, RouteDecision, RoutingRegistry, TabActivitySummary,
    UiRouteToken, MAX_TAB_HEALTH_SUMMARY_LEN, MAX_TAB_LAST_ERROR_LEN, MAX_TAB_UNREAD_DISPLAY,
};
pub use session::SessionState;
pub use ui::{AppMode, UiState};
pub use view_switch::ViewSwitchCoordinator;
