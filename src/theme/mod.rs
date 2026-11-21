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
//!
//! ## Pipeline
//!
//! Frontend-specific projections live in their owning frontend crates. This
//! module remains framework-neutral: importers decode to `SemanticTheme`,
//! and frontends project that value into their own style systems.

pub mod color;
pub mod error;
pub mod halloy;
pub mod native;
pub mod registry;
pub mod schema;
pub mod validate;

#[cfg(test)]
pub(crate) mod theme_registry_test_helpers;

pub use color::{Rgb, ThemeColor};
pub use error::ThemeError;
pub use registry::{
    builtin_fallback, expand_home, ThemeRegistry, ThemeResolutionConfig, ThemeSourceConfig,
};
pub use schema::{
    AgentColors, BaseColors, CodeColors, ConversationColors, DiffColors, SemanticTheme,
    StatusColors, TextColors, ThemeSource, UiColors,
};
pub use validate::{validate_theme, ThemeDiagnostic, ThemeDiagnosticLevel};
