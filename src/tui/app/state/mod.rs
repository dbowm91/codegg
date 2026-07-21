pub mod agent;
pub mod async_request;
pub mod diagnostics;
pub mod dialog;
pub mod messages;
pub mod plugin_ui;
pub mod project_picker;
pub mod project_tabs;
pub mod prompt;
pub mod session;
pub mod ui;
pub mod view_switch;

pub use agent::AgentState;
pub use async_request::AsyncUiRequestState;
pub use diagnostics::TuiDiagnostics;
pub use dialog::DialogState;
pub use messages::MessagesState;
pub use plugin_ui::{PluginUiApplyResult, PluginUiState};
pub use project_picker::{
    PickerPhase, ProjectPickerState, RegistrationDraft, SessionSummaryCacheEntry,
    MAX_OPEN_PROJECT_TABS, MAX_PROJECT_LIST_ITEMS, MAX_TAB_LABEL_LEN,
};
pub use project_tabs::{ProjectCatalogState, ProjectTabId, ProjectTabState, ProjectTabs};
pub use prompt::PromptState;
pub use session::SessionState;
pub use ui::{AppMode, UiState};
pub use view_switch::ViewSwitchCoordinator;
