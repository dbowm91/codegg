use std::sync::Arc;
use std::time::Duration;

use crate::plugin::hooks::{HookContext, HookRegistration, HookResult, HookType};
use crate::plugin::loader::LoadError;
use crate::plugin::loader::LoadedPlugin;
use crate::plugin::registry::{PluginInfo, PluginRegistry};

pub struct PluginService {
    registry: Arc<PluginRegistry>,
    hook_timeout: Duration,
}

impl PluginService {
    pub fn new(registry: Arc<PluginRegistry>) -> Self {
        Self {
            registry,
            hook_timeout: Duration::from_secs(5),
        }
    }

    pub fn with_hook_timeout(mut self, timeout: Duration) -> Self {
        self.hook_timeout = timeout;
        self
    }

    pub fn registry(&self) -> &Arc<PluginRegistry> {
        &self.registry
    }

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
            path: loaded.plugin_dir.clone(),
            enabled: true,
            error: None,
        };

        let hook_count = hook_specs.len();
        self.registry.register(info, hook_specs).await;

        tracing::info!(plugin = plugin_id, hooks = hook_count, "plugin registered");

        Ok(())
    }

    pub async fn dispatch_hook(&self, ctx: HookContext) -> HookResult {
        let hook_type = ctx.hook_type;
        let hooks = self.registry.hooks_for(hook_type).await;

        if hooks.is_empty() {
            return HookResult::ok(ctx.input);
        }

        let mut current_input = ctx.input;

        for hook in hooks {
            if !self.registry.is_enabled(&hook.plugin_id).await {
                continue;
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
                    if res.blocked {
                        return res;
                    }
                    if let Some(err) = &res.error {
                        tracing::warn!(
                            plugin = hook.plugin_id,
                            error = err,
                            "hook execution error"
                        );
                        return res;
                    }
                    current_input = res.output;
                }
                Err(err) => {
                    tracing::error!(
                        plugin = hook.plugin_id,
                        error = err,
                        "hook execution failed"
                    );
                    return HookResult::error(format!("{}: hook timeout: {}", hook.plugin_id, err));
                }
            }
        }

        HookResult::ok(current_input)
    }

    async fn execute_hook_with_timeout(
        &self,
        plugin_id: &str,
        ctx: HookContext,
    ) -> Result<HookResult, String> {
        let timeout = self.hook_timeout;

        tokio::time::timeout(timeout, async move {
            if plugin_id.starts_with("builtin:") {
                let name = plugin_id.strip_prefix("builtin:").unwrap_or(plugin_id);
                crate::plugin::builtin::builtin_hook_handler(name, ctx)
            } else {
                #[cfg(feature = "plugins")]
                {
                    crate::plugin::loader::execute_wasm_hook(plugin_id, ctx).await
                }
                #[cfg(not(feature = "plugins"))]
                {
                    let _ = plugin_id;
                    HookResult::ok(ctx.input)
                }
            }
        })
        .await
        .map_err(|_| "hook execution timed out".to_string())
    }

    pub async fn dispatch_auth(&self, input: serde_json::Value) -> HookResult {
        let ctx = HookContext {
            hook_type: HookType::Auth,
            input,
        };
        self.dispatch_hook(ctx).await
    }

    pub async fn dispatch_tool_definition(&self, input: serde_json::Value) -> HookResult {
        let ctx = HookContext {
            hook_type: HookType::ToolDefinition,
            input,
        };
        self.dispatch_hook(ctx).await
    }

    pub async fn dispatch_tool_execute_before(&self, input: serde_json::Value) -> HookResult {
        let ctx = HookContext {
            hook_type: HookType::ToolExecuteBefore,
            input,
        };
        self.dispatch_hook(ctx).await
    }

    pub async fn dispatch_tool_execute_after(&self, input: serde_json::Value) -> HookResult {
        let ctx = HookContext {
            hook_type: HookType::ToolExecuteAfter,
            input,
        };
        self.dispatch_hook(ctx).await
    }

    pub async fn dispatch_chat_params(&self, input: serde_json::Value) -> HookResult {
        let ctx = HookContext {
            hook_type: HookType::ChatParams,
            input,
        };
        self.dispatch_hook(ctx).await
    }

    pub async fn dispatch_chat_headers(&self, input: serde_json::Value) -> HookResult {
        let ctx = HookContext {
            hook_type: HookType::ChatHeaders,
            input,
        };
        self.dispatch_hook(ctx).await
    }

    pub async fn dispatch_event(&self, input: serde_json::Value) -> HookResult {
        let ctx = HookContext {
            hook_type: HookType::Event,
            input,
        };
        self.dispatch_hook(ctx).await
    }

    pub async fn dispatch_config(&self, input: serde_json::Value) -> HookResult {
        let ctx = HookContext {
            hook_type: HookType::Config,
            input,
        };
        self.dispatch_hook(ctx).await
    }

    pub async fn dispatch_shell_env(&self, input: serde_json::Value) -> HookResult {
        let ctx = HookContext {
            hook_type: HookType::ShellEnv,
            input,
        };
        self.dispatch_hook(ctx).await
    }

    pub async fn dispatch_text_complete(&self, input: serde_json::Value) -> HookResult {
        let ctx = HookContext {
            hook_type: HookType::TextComplete,
            input,
        };
        self.dispatch_hook(ctx).await
    }

    pub async fn dispatch_session_compacting(&self, input: serde_json::Value) -> HookResult {
        let ctx = HookContext {
            hook_type: HookType::SessionCompacting,
            input,
        };
        self.dispatch_hook(ctx).await
    }

    pub async fn dispatch_messages_transform(&self, input: serde_json::Value) -> HookResult {
        let ctx = HookContext {
            hook_type: HookType::MessagesTransform,
            input,
        };
        self.dispatch_hook(ctx).await
    }

    pub async fn dispatch_provider(&self, input: serde_json::Value) -> HookResult {
        let ctx = HookContext {
            hook_type: HookType::Provider,
            input,
        };
        self.dispatch_hook(ctx).await
    }
}
