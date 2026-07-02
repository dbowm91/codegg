//! # TUI Components
//!
//! Reusable widget components for building the terminal UI.
//!
//! ## Architecture
//!
//! Components are organized by their role:
//!
//! ### Core Components (always visible)
//!
//! - [`messages`]: Message display widget with streaming, search, and tool call rendering
//! - [`prompt`]: Input prompt widget with cursor management and history
//! - [`sidebar`]: Side panel showing session info, agents, and MCP servers
//! - [`status_bar`]: Bottom status bar with status, transient indicators, and token usage
//!
//! ### Overlay Components (conditionally visible)
//!
//! - [`completion_overlay`]: Slash (`/`) and file (`@`) completion popups
//! - [`toast`]: Toast notification manager
//! - [`spinner`]: Loading spinner widget
//!
//! ### Dialog Components (modal)
//!
//! - [`dialogs`]: Various dialog boxes (session, model, theme picker, etc.)
//!
//! ### Tool Output
//!
//! - [`tool_output`]: Tool call result display with expandable sections
//!
//! ## Widget Implementation Pattern
//!
//! Widgets follow a consistent pattern:
//!
//! ```rust,ignore
//! pub struct MyWidget {
//!     state: WidgetState,
//!     theme: Theme,
//! }
//!
//! impl MyWidget {
//!     pub fn new() -> Self { ... }
//!     pub fn set_theme(&mut self, theme: Theme) { self.theme = theme; }
//! }
//!
//! impl Widget for MyWidget {
//!     fn render(self, area: Rect, buf: &mut Buffer) { ... }
//! }
//! ```
//!
//! Key conventions:
//! - `set_theme()` updates theme before rendering
//! - Widget holds its own state (no separate state object)
//! - Selection/cursor state managed internally
//! - Theme passed via `set_theme()` or constructor

pub mod completion_overlay;
pub mod component;
pub mod dialogs;
pub mod diff;
pub mod image;
pub mod messages;
pub mod notification;
pub mod plugin_renderer;
pub mod prompt;
pub mod scroll;
pub mod sidebar;
pub mod spinner;
pub mod status_bar;
pub mod toast;
pub mod tool_output;
pub mod ui_node_renderer;
