use std::sync::Arc;

pub mod activation;
pub mod api;
pub mod builtin;
pub mod contributions;
pub mod event_bus;
pub mod hooks;
pub mod install;
pub mod lifecycle;
pub mod loader;
pub mod management;
pub mod management_ui;
pub mod manifest;
pub mod marketplace;
pub mod permission;
pub mod policy;
pub mod registry;
pub mod runtime;
pub mod service;
pub mod tui;

pub use crate::protocol::plugin::PluginResponse;
pub use activation::{
    PluginActivationError, PluginActivationRecord, PluginActivationScope, PluginActivationSource,
    PluginActivationStore, ResolvedPluginActivation, ResolvedPluginActivationSet,
};
pub use api::{ApiVersion, Stability, API_VERSION};
pub use contributions::{
    canonical_mcp_name, PluginAssetPath, ResolvedPluginContribution, ResolvedPluginContributionSet,
    ResolvedPluginMcpServer,
};
pub use event_bus::{PluginEventBus, PluginEventSubscription};
pub use hooks::{HookContext, HookResult, HookType};
pub use install::{
    install_from_path, install_from_path_into, install_from_url, uninstall,
    validate_install_source, validate_local_install_source, validate_uninstall_target,
    validate_wasm_module_path,
};
pub use lifecycle::{
    EventHookInput, LifecycleHooks, MessageTransformInput, MessageTransformOutput,
    PluginHookOutcome, ShellEnvHookInput, ShellEnvHookOutput, ToolAfterHookInput, ToolBeforeAction,
    ToolBeforeHookInput, ToolBeforeHookOutput,
};
pub use loader::{load_plugin, LoadedPlugin};
pub use manifest::{
    FilesystemPermission, LegacyHookSpec, LegacyManifest, PluginCapability, PluginCommandSpec,
    PluginContributions, PluginDiagnostic, PluginDiagnosticLevel, PluginEventSubscriptionSpec,
    PluginHookSpec, PluginManifest, PluginMcpServerContribution, PluginOutputSurface,
    PluginPanelContribution, PluginPermissionSet, PluginRuntimeSpec, PluginStatusContribution,
    PluginTrustClass,
};
pub use permission::{
    check_invocation_allowed, check_lifecycle_hook_allowed, check_secret_access_allowed,
    check_ui_effect_allowed, PolicyDecision,
};
pub use policy::{
    classify_hook, HookCategory, PluginInstallPolicy, PluginLifecyclePolicy,
    PluginPermissionPolicy, PluginPolicy, PluginRuntimePolicy, PluginUiPolicy,
};
pub use registry::{
    normalize_command_name, PluginCommandRegistration, PluginEventRegistration,
    PluginHookRegistration, PluginInfo, PluginInstallKind, PluginPanelRegistration, PluginRegistry,
    PluginRegistryError, PluginSourceMetadata, PluginStatusRegistration,
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

    // Rehydrate installed manifests on every daemon/service construction.
    // Activation state remains the authority for visibility; loading a
    // manifest does not execute its runtime or implicitly grant authority.
    if let Ok(mut entries) = tokio::fs::read_dir(install::plugins_dir()).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let manifest_path = path.join("manifest.toml");
            let Ok(raw) = tokio::fs::read_to_string(&manifest_path).await else {
                continue;
            };
            let Ok(manifest) = toml::from_str::<manifest::PluginManifest>(&raw) else {
                tracing::warn!(path = %manifest_path.display(), "skipping installed plugin with invalid manifest");
                continue;
            };
            if manifest.name.is_empty() || manifest.version.is_empty() {
                tracing::warn!(path = %manifest_path.display(), "skipping installed plugin without name/version");
                continue;
            }
            let info = registry::PluginInfo {
                id: format!("plugin:{}", manifest.name),
                trust: manifest.trust_class(),
                manifest,
                enabled: true,
                diagnostics: Vec::new(),
                source: Some(registry::PluginSourceMetadata::registry_loaded(path)),
            };
            if let Err(error) = registry.register(info).await {
                tracing::warn!(%error, "skipping installed plugin that could not be registered");
            }
        }
    }

    let handler_registry = Arc::new(builtin::builtin_runtime_registry());
    let builtin_runtime = Arc::new(BuiltinRuntime::new(handler_registry));

    let activation_store = match PluginActivationStore::load(
        crate::core::instance::DaemonPaths::resolve().plugin_activation_path(),
    )
    .await
    {
        Ok(store) => Arc::new(store),
        Err(error) => {
            tracing::error!(%error, "plugin activation state is unavailable; plugins disabled");
            return None;
        }
    };
    let service = Arc::new(
        PluginService::new(registry)
            .with_builtin_runtime(builtin_runtime)
            .with_activation_store(activation_store),
    );
    Some(service)
}
