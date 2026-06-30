pub mod api;
pub mod builtin;
pub mod event_bus;
pub mod hooks;
pub mod install;
pub mod loader;
pub mod manifest;
pub mod registry;
pub mod runtime;
pub mod service;
pub mod tui;

pub use crate::protocol::plugin::PluginResponse;
pub use api::{ApiVersion, Stability, API_VERSION};
pub use event_bus::{PluginEventBus, PluginEventSubscription};
pub use hooks::{HookContext, HookResult, HookType};
pub use install::{install_from_path, install_from_url, uninstall};
pub use loader::{load_plugin, LoadedPlugin};
pub use manifest::{
    FilesystemPermission, LegacyHookSpec, LegacyManifest, PluginCapability, PluginCommandSpec,
    PluginDiagnostic, PluginDiagnosticLevel, PluginEventSubscriptionSpec, PluginHookSpec,
    PluginManifest, PluginOutputSurface, PluginPanelContribution, PluginPermissionSet,
    PluginRuntimeSpec, PluginStatusContribution, PluginTrustClass,
};
pub use registry::{
    normalize_command_name, PluginCommandRegistration, PluginEventRegistration,
    PluginHookRegistration, PluginInfo, PluginPanelRegistration, PluginRegistry,
    PluginRegistryError, PluginStatusRegistration,
};
pub use runtime::wasm_cache::WasmModuleCache;
pub use service::{PluginError, PluginService};
pub use tui::{TuiComponent, TuiPluginRegistry, TuiRoute};
