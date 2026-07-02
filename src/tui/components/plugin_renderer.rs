//! Backwards-compat shim. The canonical renderer is now `UiNodeRenderer`
//! at `super::ui_node_renderer`. This module is kept so existing call
//! sites and `use` statements continue to compile.

pub use super::ui_node_renderer::UiNodeRenderer as PluginUiRenderer;
