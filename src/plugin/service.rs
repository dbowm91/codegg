use std::sync::Arc;
use std::time::Duration;

use crate::plugin::hooks::{HookContext, HookRegistration, HookResult, HookType};
use crate::plugin::loader::LoadError;
use crate::plugin::loader::LoadedPlugin;
use crate::plugin::manifest::{PluginManifest, PluginRuntimeSpec, PluginTrustClass};
use crate::plugin::policy::PluginPolicy;
use crate::plugin::registry::{PluginInfo, PluginRegistry, PluginRegistryError};
use crate::plugin::runtime::builtin::BuiltinRuntime;
use crate::plugin::runtime::process::{ProcessRuntime, ProcessRuntimeSpec};
#[cfg(feature = "plugins")]
use crate::plugin::runtime::wasm::{WasmRuntime, WasmRuntimeSpec};
use crate::plugin::runtime::{PluginRuntime, RuntimeLimits};
use crate::protocol::plugin::{
    PluginCapabilityInvocation, PluginContext, PluginInvocation, PluginResponse,
    PLUGIN_PROTOCOL_VERSION,
};

pub struct PluginService {
    registry: Arc<PluginRegistry>,
    hook_timeout: Duration,
    builtin_runtime: Option<Arc<BuiltinRuntime>>,
    policy: Option<Arc<PluginPolicy>>,
}

impl PluginService {
    pub fn new(registry: Arc<PluginRegistry>) -> Self {
        Self {
            registry,
            hook_timeout: Duration::from_secs(5),
            builtin_runtime: None,
            policy: None,
        }
    }

    pub fn with_hook_timeout(mut self, timeout: Duration) -> Self {
        self.hook_timeout = timeout;
        self
    }

    /// Set the builtin runtime for dispatching builtin plugin invocations.
    pub fn with_builtin_runtime(mut self, runtime: Arc<BuiltinRuntime>) -> Self {
        self.builtin_runtime = Some(runtime);
        self
    }

    /// Set the composite plugin policy for invocation and hook gating.
    pub fn with_policy(mut self, policy: Arc<PluginPolicy>) -> Self {
        self.policy = Some(policy);
        self
    }

    /// Get a reference to the current policy, if set.
    pub fn policy(&self) -> Option<&Arc<PluginPolicy>> {
        self.policy.as_ref()
    }

    pub fn registry(&self) -> &Arc<PluginRegistry> {
        &self.registry
    }

    /// Load and register a plugin from a `LoadedPlugin` (backward compatible).
    pub async fn load_and_register(&self, loaded: LoadedPlugin) -> Result<(), LoadError> {
        let plugin_id = format!("plugin:{}", loaded.manifest.name);

        let hook_specs: Vec<HookRegistration> = loaded
            .manifest
            .hooks
            .iter()
            .filter_map(|hs| {
                HookType::parse(&hs.hook_type).map(|ht| HookRegistration {
                    plugin_id: plugin_id.clone(),
                    hook_type: ht,
                    priority: hs.priority.unwrap_or(0),
                })
            })
            .collect();

        let info = PluginInfo {
            id: plugin_id.clone(),
            manifest: loaded.manifest.clone(),
            enabled: true,
            trust: PluginTrustClass::from_runtime_kind("wasm"),
            diagnostics: Vec::new(),
        };

        let hook_count = hook_specs.len();
        self.registry
            .register_with_hooks(info, hook_specs)
            .await
            .map_err(|e| LoadError::Manifest(e.to_string()))?;

        tracing::info!(plugin = plugin_id, hooks = hook_count, "plugin registered");

        Ok(())
    }

    /// Load and register a plugin from a legacy manifest (backward compatible).
    pub async fn register_manifest(
        &self,
        plugin_id: &str,
        manifest: PluginManifest,
        _path: std::path::PathBuf,
    ) -> Result<(), PluginRegistryError> {
        let hook_specs: Vec<HookRegistration> = manifest
            .hooks
            .iter()
            .filter_map(|hs| {
                HookType::parse(&hs.hook_type).map(|ht| HookRegistration {
                    plugin_id: plugin_id.to_string(),
                    hook_type: ht,
                    priority: hs.priority.unwrap_or(0),
                })
            })
            .collect();

        let info = PluginInfo {
            id: plugin_id.to_string(),
            manifest,
            enabled: true,
            trust: PluginTrustClass::Builtin,
            diagnostics: Vec::new(),
        };

        self.registry.register_with_hooks(info, hook_specs).await
    }

    /// Register a plugin with a canonical manifest (Phase 5).
    pub async fn register_plugin(&self, info: PluginInfo) -> Result<(), PluginRegistryError> {
        self.registry.register(info).await
    }

    /// Invoke a plugin command by name.
    ///
    /// This is the Phase 6 entry point for command invocation. Dispatches
    /// to the appropriate runtime (process, builtin, WASM) based on the
    /// plugin's manifest runtime spec.
    pub async fn invoke_command(
        &self,
        command_name: &str,
        args: Vec<String>,
        input: serde_json::Value,
    ) -> Result<PluginResponse, PluginError> {
        let cmd = self
            .registry
            .command(command_name)
            .await
            .ok_or_else(|| PluginError::CommandNotFound(command_name.to_string()))?;

        let plugin_id = &cmd.plugin_id;
        let plugin_info = self
            .registry
            .get(plugin_id)
            .await
            .ok_or_else(|| PluginError::PluginNotFound(plugin_id.clone()))?;

        if !plugin_info.enabled {
            return Err(PluginError::PluginDisabled(plugin_id.clone()));
        }

        let invocation = PluginInvocation {
            protocol_version: PLUGIN_PROTOCOL_VERSION,
            invocation_id: uuid::Uuid::new_v4().to_string(),
            plugin_id: plugin_id.clone(),
            capability: PluginCapabilityInvocation::Command {
                name: command_name.to_string(),
            },
            args,
            input,
            context: PluginContext {
                project_dir: std::env::current_dir()
                    .ok()
                    .map(|p| p.to_string_lossy().to_string()),
                ..PluginContext::default()
            },
        };

        // Policy check: invocation allowed
        if let Some(ref policy) = self.policy {
            let decision = crate::plugin::permission::check_invocation_allowed(
                &plugin_info.manifest,
                &invocation.capability,
                &plugin_info.trust,
                policy,
            );
            if !decision.is_allowed() {
                return Err(PluginError::Runtime(format!(
                    "policy denied invocation: {}",
                    decision.reason().unwrap_or("unknown reason")
                )));
            }
        }

        match &plugin_info.manifest.runtime {
            PluginRuntimeSpec::Builtin { handler } => {
                // Builtin runtime is hook-only: there is no command dispatch path
                // for builtin plugins. Reject explicitly so callers see a clear
                // error rather than a misleading success placeholder.
                if let Some(ref builtin_rt) = self.builtin_runtime {
                    builtin_rt
                        .invoke(invocation)
                        .await
                        .map_err(|e| PluginError::Runtime(e.to_string()))
                } else {
                    Err(PluginError::Runtime(format!(
                        "builtin plugin '{}' has no command runtime handler \
                         (handler={}, command={}); builtin runtime is hook-only",
                        plugin_id, handler, command_name
                    )))
                }
            }
            PluginRuntimeSpec::Process { .. } => {
                let spec: Option<ProcessRuntimeSpec> = (&plugin_info.manifest.runtime).into();
                let spec = spec.ok_or_else(|| {
                    PluginError::Runtime("failed to extract process runtime spec".to_string())
                })?;
                let mut runtime = ProcessRuntime::new(spec, RuntimeLimits::default());
                if let Some(ref policy) = self.policy {
                    runtime = runtime.with_env_policy(policy.permissions.clone());
                }
                runtime
                    .invoke(invocation)
                    .await
                    .map_err(|e| PluginError::Runtime(e.to_string()))
            }
            PluginRuntimeSpec::Wasm {
                module,
                timeout_ms,
                memory_max_mb,
                fuel_per_call,
            } => {
                #[cfg(feature = "plugins")]
                {
                    let plugin_dir = crate::plugin::install::plugins_dir()
                        .join(plugin_id.strip_prefix("plugin:").unwrap_or(plugin_id));
                    let spec = WasmRuntimeSpec::from_manifest(
                        module,
                        &plugin_dir,
                        *timeout_ms,
                        *memory_max_mb,
                        *fuel_per_call,
                    );
                    if let Some(ref policy) = self.policy {
                        if let Err(e) = crate::plugin::install::validate_wasm_module_path(
                            &spec.module_path,
                            &plugin_dir,
                            &policy.install,
                        ) {
                            return Err(PluginError::Runtime(format!(
                                "WASM module path policy violation: {}",
                                e
                            )));
                        }
                    }
                    let runtime = WasmRuntime::with_defaults(spec);
                    runtime
                        .invoke(invocation)
                        .await
                        .map_err(|e| PluginError::Runtime(e.to_string()))
                }
                #[cfg(not(feature = "plugins"))]
                {
                    let _ = (module, timeout_ms, memory_max_mb, fuel_per_call);
                    let _ = invocation;
                    Ok(PluginResponse {
                        ok: false,
                        effects: Vec::new(),
                        data: serde_json::json!({
                            "command": command_name,
                            "status": "wasm_runtime_disabled",
                        }),
                        diagnostics: vec![crate::protocol::plugin::PluginDiagnostic {
                            level: crate::protocol::plugin::PluginDiagnosticLevel::Info,
                            message: "WASM runtime requires the 'plugins' feature".to_string(),
                        }],
                    })
                }
            }
        }
    }

    /// Dispatch a hook through the registry (Phase 5 name, same as existing).
    pub async fn dispatch_hook(&self, ctx: HookContext) -> HookResult {
        let hook_type = ctx.hook_type;
        let hooks = self.registry.hooks_for(hook_type).await;

        if hooks.is_empty() {
            return HookResult::ok(ctx.input);
        }

        let mut current_input = ctx.input;
        let mut all_effects = Vec::new();

        for hook in hooks {
            // Policy check: hook allowed
            if let Some(ref policy) = self.policy {
                let plugin_info = self.registry.get(&hook.plugin_id).await;
                if let Some(info) = &plugin_info {
                    let decision = crate::plugin::permission::check_lifecycle_hook_allowed(
                        hook_type,
                        &info.trust,
                        policy,
                    );
                    if !decision.is_allowed() {
                        tracing::warn!(
                            plugin = hook.plugin_id,
                            hook_type = hook_type.as_str(),
                            reason = decision.reason().unwrap_or("unknown"),
                            "hook denied by policy"
                        );
                        continue;
                    }
                }
            }

            let hook_ctx = HookContext {
                hook_type,
                input: current_input.clone(),
            };

            let result = self
                .execute_hook_with_timeout(&hook.plugin_id, hook_ctx)
                .await;

            match result {
                Ok(res) => {
                    all_effects.extend(res.effects);
                    if res.blocked {
                        return HookResult {
                            effects: all_effects,
                            ..res
                        };
                    }
                    if let Some(err) = &res.error {
                        tracing::warn!(
                            plugin = hook.plugin_id,
                            error = err,
                            "hook execution error"
                        );
                        return HookResult {
                            effects: all_effects,
                            ..res
                        };
                    }
                    current_input = res.output;
                }
                Err(err) => {
                    tracing::error!(
                        plugin = hook.plugin_id,
                        error = err,
                        "hook execution failed"
                    );
                    let mut err_result =
                        HookResult::error(format!("{}: hook timeout: {}", hook.plugin_id, err));
                    err_result.effects = all_effects;
                    return err_result;
                }
            }
        }

        let mut final_result = HookResult::ok(current_input);
        final_result.effects = all_effects;
        final_result
    }

    async fn execute_hook_with_timeout(
        &self,
        plugin_id: &str,
        ctx: HookContext,
    ) -> Result<HookResult, String> {
        let timeout = self.hook_timeout;

        tokio::time::timeout(timeout, async move {
            if plugin_id.starts_with("builtin:") {
                // Dispatch through BuiltinRuntime if available
                if let Some(ref builtin_rt) = self.builtin_runtime {
                    use crate::protocol::plugin::{
                        PluginCapabilityInvocation, PLUGIN_PROTOCOL_VERSION,
                    };

                    let invocation = PluginInvocation {
                        protocol_version: PLUGIN_PROTOCOL_VERSION,
                        invocation_id: uuid::Uuid::new_v4().to_string(),
                        plugin_id: plugin_id.to_string(),
                        capability: PluginCapabilityInvocation::Hook {
                            hook_type: ctx.hook_type.as_str().to_string(),
                        },
                        args: Vec::new(),
                        input: ctx.input.clone(),
                        context: PluginContext::default(),
                    };

                    match builtin_rt.invoke(invocation).await {
                        Ok(response) => Ok(HookResult::from_plugin_response(response, ctx.input)),
                        Err(e) => Ok(HookResult::error(format!("builtin hook failed: {}", e))),
                    }
                } else {
                    // Fallback to direct handler lookup
                    let name = plugin_id.strip_prefix("builtin:").unwrap_or(plugin_id);
                    Ok(crate::plugin::builtin::builtin_hook_handler(name, ctx))
                }
            } else {
                #[cfg(feature = "plugins")]
                {
                    Ok(crate::plugin::loader::execute_wasm_hook(plugin_id, ctx).await)
                }
                #[cfg(not(feature = "plugins"))]
                {
                    let _ = plugin_id;
                    Ok(HookResult::ok(ctx.input))
                }
            }
        })
        .await
        .map_err(|_| "hook execution timed out".to_string())?
    }

    // --- Convenience dispatch methods (backward compatible) ---

    pub async fn dispatch_auth(&self, input: serde_json::Value) -> HookResult {
        self.dispatch_hook(HookContext {
            hook_type: HookType::Auth,
            input,
        })
        .await
    }

    pub async fn dispatch_tool_definition(&self, input: serde_json::Value) -> HookResult {
        self.dispatch_hook(HookContext {
            hook_type: HookType::ToolDefinition,
            input,
        })
        .await
    }

    pub async fn dispatch_tool_execute_before(&self, input: serde_json::Value) -> HookResult {
        self.dispatch_hook(HookContext {
            hook_type: HookType::ToolExecuteBefore,
            input,
        })
        .await
    }

    pub async fn dispatch_tool_execute_after(&self, input: serde_json::Value) -> HookResult {
        self.dispatch_hook(HookContext {
            hook_type: HookType::ToolExecuteAfter,
            input,
        })
        .await
    }

    pub async fn dispatch_chat_params(&self, input: serde_json::Value) -> HookResult {
        self.dispatch_hook(HookContext {
            hook_type: HookType::ChatParams,
            input,
        })
        .await
    }

    pub async fn dispatch_chat_headers(&self, input: serde_json::Value) -> HookResult {
        self.dispatch_hook(HookContext {
            hook_type: HookType::ChatHeaders,
            input,
        })
        .await
    }

    pub async fn dispatch_event(&self, input: serde_json::Value) -> HookResult {
        self.dispatch_hook(HookContext {
            hook_type: HookType::Event,
            input,
        })
        .await
    }

    pub async fn dispatch_config(&self, input: serde_json::Value) -> HookResult {
        self.dispatch_hook(HookContext {
            hook_type: HookType::Config,
            input,
        })
        .await
    }

    pub async fn dispatch_shell_env(&self, input: serde_json::Value) -> HookResult {
        self.dispatch_hook(HookContext {
            hook_type: HookType::ShellEnv,
            input,
        })
        .await
    }

    pub async fn dispatch_text_complete(&self, input: serde_json::Value) -> HookResult {
        self.dispatch_hook(HookContext {
            hook_type: HookType::TextComplete,
            input,
        })
        .await
    }

    pub async fn dispatch_session_compacting(&self, input: serde_json::Value) -> HookResult {
        self.dispatch_hook(HookContext {
            hook_type: HookType::SessionCompacting,
            input,
        })
        .await
    }

    pub async fn dispatch_messages_transform(&self, input: serde_json::Value) -> HookResult {
        self.dispatch_hook(HookContext {
            hook_type: HookType::MessagesTransform,
            input,
        })
        .await
    }

    pub async fn dispatch_provider(&self, input: serde_json::Value) -> HookResult {
        self.dispatch_hook(HookContext {
            hook_type: HookType::Provider,
            input,
        })
        .await
    }
}

/// Error from plugin service operations.
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("command not found: {0}")]
    CommandNotFound(String),
    #[error("plugin not found: {0}")]
    PluginNotFound(String),
    #[error("plugin disabled: {0}")]
    PluginDisabled(String),
    #[error("registry error: {0}")]
    Registry(#[from] PluginRegistryError),
    #[error("runtime error: {0}")]
    Runtime(String),
}
