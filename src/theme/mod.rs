//! Frontend-neutral theme system.
//!
//! ## Layout
//!
//! - [`schema`]: the canonical, frontend-neutral [`SemanticTheme`].
//! - [`color`]: RGB primitive and contrast math.
//! - [`error`]: theme error type.
//! - [`native`]: native codegg TOML importer/exporter.
//! - [`halloy`]: Halloy TOML compatibility importer.
//! - [`validate`]: contrast and structural diagnostics.
//! - [`registry`]: collection of available themes + resolution rules.
//! - [`target`]: projections to specific frontends (currently ratatui).
//!
//! ## Pipeline
//!
//! ```text
//! native codegg TOML  ┐
//! Halloy TOML         ├─►  SemanticTheme  ──►  ratatui::Theme
//! future Base16       ┘
//! ```
//!
//! When a future iced GUI is added, add a new file under `target/` that
//! projects `SemanticTheme` into the GUI's style system. Do not parse Halloy
//! themes directly into frontend-specific types.

pub mod color;
pub mod error;
pub mod halloy;
pub mod native;
pub mod registry;
pub mod schema;
pub mod target;
pub mod validate;

#[cfg(test)]
pub(crate) mod theme_registry_test_helpers;

pub use color::{Rgb, ThemeColor};
pub use error::ThemeError;
pub use registry::{
    resolve_theme_for_app, expand_home, builtin_fallback, ThemeRegistry, ThemeResolutionConfig,
    ThemeSourceConfig,
};
pub use schema::{
    AgentColors, BaseColors, CodeColors, ConversationColors, DiffColors, SemanticTheme,
    StatusColors, TextColors, ThemeSource, UiColors,
};
pub use validate::{validate_theme, ThemeDiagnostic, ThemeDiagnosticLevel};
