use std::sync::Arc;

pub mod api;
pub mod builtin;
pub mod event_bus;
pub mod hooks;
pub mod install;
pub mod lifecycle;
pub mod loader;
pub mod management;
pub mod management_ui;
pub mod manifest;
pub mod marketplace;
pub mod policy;
pub mod registry;
pub mod runtime;
pub mod service;
pub mod tui;

pub use crate::protocol::plugin::PluginResponse;
pub use api::{ApiVersion, Stability, API_VERSION};
pub use event_bus::{PluginEventBus, PluginEventSubscription};
pub use hooks::{HookContext, HookResult, HookType};
pub use install::{install_from_path, install_from_url, uninstall};
pub use lifecycle::{
    EventHookInput, LifecycleHooks, MessageTransformInput, MessageTransformOutput,
    PluginHookOutcome, ShellEnvHookInput, ShellEnvHookOutput, ToolAfterHookInput, ToolBeforeAction,
    ToolBeforeHookInput, ToolBeforeHookOutput,
};
pub use loader::{load_plugin, LoadedPlugin};
pub use manifest::{
    FilesystemPermission, LegacyHookSpec, LegacyManifest, PluginCapability, PluginCommandSpec,
    PluginDiagnostic, PluginDiagnosticLevel, PluginEventSubscriptionSpec, PluginHookSpec,
    PluginManifest, PluginOutputSurface, PluginPanelContribution, PluginPermissionSet,
    PluginRuntimeSpec, PluginStatusContribution, PluginTrustClass,
};
pub use policy::{classify_hook, HookCategory, PluginLifecyclePolicy};
pub use registry::{
    normalize_command_name, PluginCommandRegistration, PluginEventRegistration,
    PluginHookRegistration, PluginInfo, PluginPanelRegistration, PluginRegistry,
    PluginRegistryError, PluginStatusRegistration,
};
pub use runtime::builtin::{BuiltinHandlerRegistry, BuiltinRuntime};
pub use runtime::wasm_cache::WasmModuleCache;
pub use service::{PluginError, PluginService};
pub use tui::{TuiComponent, TuiPluginRegistry, TuiRoute};

/// Create a default [`PluginService`] with builtin plugins registered.
///
/// Returns `None` if no plugins are configured. The returned service
/// includes the four builtin auth hook plugins (copilot, codex, gitlab, poe).
pub async fn create_default_plugin_service() -> Option<Arc<PluginService>> {
    let registry = Arc::new(registry::PluginRegistry::new());
    builtin::register_builtins(&registry).await;

    let handler_registry = Arc::new(builtin::builtin_runtime_registry());
    let builtin_runtime = Arc::new(BuiltinRuntime::new(handler_registry));

    let service = Arc::new(PluginService::new(registry).with_builtin_runtime(builtin_runtime));
    Some(service)
}
