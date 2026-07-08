use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use crate::plugin::hooks::{HookContext, HookResult, HookType};
use crate::plugin::runtime::{PluginRuntime, RuntimeError};
use crate::protocol::plugin::{PluginInvocation, PluginResponse};

/// Type alias for a builtin hook handler function.
pub type BuiltinHookHandler = fn(HookContext) -> HookResult;

/// Registry mapping handler IDs to their hook handler functions.
pub struct BuiltinHandlerRegistry {
    handlers: HashMap<String, BuiltinHookHandler>,
}

impl BuiltinHandlerRegistry {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Register a handler for the given handler ID.
    pub fn register(&mut self, handler_id: String, handler: BuiltinHookHandler) {
        self.handlers.insert(handler_id, handler);
    }

    /// Look up a handler by ID.
    pub fn get(&self, handler_id: &str) -> Option<BuiltinHookHandler> {
        self.handlers.get(handler_id).copied()
    }

    /// Check if a handler is registered.
    pub fn contains(&self, handler_id: &str) -> bool {
        self.handlers.contains_key(handler_id)
    }

    /// Return the number of registered handlers.
    pub fn len(&self) -> usize {
        self.handlers.len()
    }

    /// Check if the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.handlers.is_empty()
    }
}

impl Default for BuiltinHandlerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Runtime implementation for first-party native Rust builtin plugins.
///
/// Dispatches plugin invocations to registered Rust handler functions.
/// Built-in plugins are fully trusted and not sandboxed.
pub struct BuiltinRuntime {
    handlers: Arc<BuiltinHandlerRegistry>,
}

impl BuiltinRuntime {
    pub fn new(handlers: Arc<BuiltinHandlerRegistry>) -> Self {
        Self { handlers }
    }

    /// Create a BuiltinRuntime from a mutable registry reference (for initial setup).
    pub fn from_registry(registry: &BuiltinHandlerRegistry) -> Self {
        Self {
            handlers: Arc::new(Self::clone_registry(registry)),
        }
    }

    fn clone_registry(registry: &BuiltinHandlerRegistry) -> BuiltinHandlerRegistry {
        let mut cloned = BuiltinHandlerRegistry::new();
        for (id, handler) in &registry.handlers {
            cloned.register(id.clone(), *handler);
        }
        cloned
    }
}

#[async_trait]
impl PluginRuntime for BuiltinRuntime {
    async fn invoke(&self, invocation: PluginInvocation) -> Result<PluginResponse, RuntimeError> {
        let handler_id = extract_handler_id(&invocation)?;

        let handler = self.handlers.get(&handler_id).ok_or_else(|| {
            RuntimeError::Unsupported(format!("unknown builtin handler: {}", handler_id))
        })?;

        let ctx = invocation_to_hook_context(&invocation)?;
        let result = handler(ctx);

        Ok(hook_result_to_plugin_response(result))
    }
}

/// Convert a `PluginInvocation` into a `HookContext` for builtin dispatch.
///
/// This adapter bridges the runtime invocation model with the hook handler
/// model. Builtin runtime only handles hook invocations; command, panel,
/// status-widget, and event invocations are rejected because no builtin
/// command handler registry exists yet. Unknown hook type strings are also
/// rejected — previously they silently fell back to `HookType::Auth`, which
/// is unsafe at a runtime trust boundary.
pub fn invocation_to_hook_context(
    invocation: &PluginInvocation,
) -> Result<HookContext, RuntimeError> {
    use crate::protocol::plugin::PluginCapabilityInvocation;

    let hook_type_str = match &invocation.capability {
        PluginCapabilityInvocation::Hook { hook_type } => hook_type.as_str(),
        PluginCapabilityInvocation::Command { name } => {
            return Err(RuntimeError::Unsupported(format!(
                "builtin runtime does not support command invocations: '{}' \
                 (builtin runtime is hook-only)",
                name
            )));
        }
        _ => {
            return Err(RuntimeError::Unsupported(format!(
                "builtin runtime does not support capability type: {:?}",
                invocation.capability
            )));
        }
    };

    let hook_type = HookType::parse(hook_type_str).ok_or_else(|| {
        RuntimeError::Unsupported(format!("unsupported builtin hook type: {hook_type_str}"))
    })?;

    Ok(HookContext {
        hook_type,
        input: invocation.input.clone(),
    })
}

/// Convert a `HookResult` into a `PluginResponse` for runtime dispatch.
///
/// Preserves transformed output, error/blocking state, and diagnostics.
pub fn hook_result_to_plugin_response(result: HookResult) -> PluginResponse {
    let mut diagnostics = Vec::new();

    if let Some(ref error) = result.error {
        diagnostics.push(crate::protocol::plugin::PluginDiagnostic {
            level: crate::protocol::plugin::PluginDiagnosticLevel::Error,
            message: error.clone(),
        });
    }

    PluginResponse {
        ok: !result.blocked && result.error.is_none(),
        effects: result.effects,
        data: result.output,
        diagnostics,
    }
}

/// Extract the handler ID from a builtin plugin invocation.
///
/// For builtin plugins, the plugin_id format is `builtin:<name>`.
/// The handler ID is derived from the manifest's runtime handler field,
/// but at invocation time we use the plugin name portion.
fn extract_handler_id(invocation: &PluginInvocation) -> Result<String, RuntimeError> {
    let plugin_id = &invocation.plugin_id;

    // Strip "builtin:" prefix if present
    if let Some(name) = plugin_id.strip_prefix("builtin:") {
        return Ok(name.to_string());
    }

    Err(RuntimeError::Unsupported(format!(
        "builtin runtime requires plugin_id to start with 'builtin:': got '{}'",
        plugin_id
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::plugin::{PluginCapabilityInvocation, PLUGIN_PROTOCOL_VERSION};

    fn make_test_handler() -> BuiltinHookHandler {
        |_ctx: HookContext| HookResult::ok(serde_json::json!({"handled": true}))
    }

    fn make_error_handler() -> BuiltinHookHandler {
        |_ctx: HookContext| HookResult::error("test error")
    }

    #[allow(dead_code)]
    fn make_block_handler() -> BuiltinHookHandler {
        |_ctx: HookContext| HookResult::blocked()
    }

    #[allow(dead_code)]
    fn make_transform_handler() -> BuiltinHookHandler {
        |ctx: HookContext| {
            let mut output = ctx.input.clone();
            if let Some(obj) = output.as_object_mut() {
                obj.insert("transformed".into(), serde_json::Value::Bool(true));
            }
            HookResult::ok(output)
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn builtin_runtime_rejects_command_invocation() {
        let mut registry = BuiltinHandlerRegistry::new();
        registry.register("test_handler".into(), make_test_handler());
        let runtime = BuiltinRuntime::new(Arc::new(registry));

        let invocation = PluginInvocation {
            protocol_version: PLUGIN_PROTOCOL_VERSION,
            invocation_id: "inv-cmd".into(),
            plugin_id: "builtin:test_handler".into(),
            capability: PluginCapabilityInvocation::Command {
                name: "do_thing".into(),
            },
            args: Vec::new(),
            input: serde_json::json!({}),
            context: Default::default(),
        };

        let err = runtime.invoke(invocation).await.unwrap_err();
        assert!(
            matches!(err, RuntimeError::Unsupported(_)),
            "command invocation must be rejected, got: {err:?}"
        );
        let msg = err.to_string();
        assert!(
            msg.contains("do_thing"),
            "error should include command name, got: {msg}"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn builtin_runtime_rejects_unknown_hook_type() {
        let mut registry = BuiltinHandlerRegistry::new();
        registry.register("test_handler".into(), make_test_handler());
        let runtime = BuiltinRuntime::new(Arc::new(registry));

        let invocation = PluginInvocation {
            protocol_version: PLUGIN_PROTOCOL_VERSION,
            invocation_id: "inv-unknown-hook".into(),
            plugin_id: "builtin:test_handler".into(),
            capability: PluginCapabilityInvocation::Hook {
                hook_type: "no.such.hook".into(),
            },
            args: Vec::new(),
            input: serde_json::json!({}),
            context: Default::default(),
        };

        let err = runtime.invoke(invocation).await.unwrap_err();
        assert!(matches!(err, RuntimeError::Unsupported(_)));
        let msg = err.to_string();
        assert!(
            msg.contains("no.such.hook"),
            "error should include the unknown hook type, got: {msg}"
        );
        // Critical regression guard: must NOT silently treat it as Auth.
        assert!(
            !msg.contains("Auth"),
            "error must not mention Auth fallback, got: {msg}"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn builtin_runtime_dispatches_known_handler() {
        let mut registry = BuiltinHandlerRegistry::new();
        registry.register("test_handler".into(), make_test_handler());
        let runtime = BuiltinRuntime::new(Arc::new(registry));

        let invocation = PluginInvocation {
            protocol_version: PLUGIN_PROTOCOL_VERSION,
            invocation_id: "inv-1".into(),
            plugin_id: "builtin:test_handler".into(),
            capability: PluginCapabilityInvocation::Hook {
                hook_type: "auth".into(),
            },
            args: Vec::new(),
            input: serde_json::json!({}),
            context: Default::default(),
        };

        let response = runtime.invoke(invocation).await.unwrap();
        assert!(response.ok);
        assert_eq!(response.data, serde_json::json!({"handled": true}));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn builtin_runtime_returns_error_for_unknown_handler() {
        let registry = BuiltinHandlerRegistry::new();
        let runtime = BuiltinRuntime::new(Arc::new(registry));

        let invocation = PluginInvocation {
            protocol_version: PLUGIN_PROTOCOL_VERSION,
            invocation_id: "inv-2".into(),
            plugin_id: "builtin:unknown".into(),
            capability: PluginCapabilityInvocation::Hook {
                hook_type: "auth".into(),
            },
            args: Vec::new(),
            input: serde_json::json!({}),
            context: Default::default(),
        };

        let err = runtime.invoke(invocation).await.unwrap_err();
        assert!(matches!(err, RuntimeError::Unsupported(_)));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn builtin_runtime_returns_error_for_non_builtin_plugin_id() {
        let registry = BuiltinHandlerRegistry::new();
        let runtime = BuiltinRuntime::new(Arc::new(registry));

        let invocation = PluginInvocation {
            protocol_version: PLUGIN_PROTOCOL_VERSION,
            invocation_id: "inv-3".into(),
            plugin_id: "plugin:external".into(),
            capability: PluginCapabilityInvocation::Hook {
                hook_type: "auth".into(),
            },
            args: Vec::new(),
            input: serde_json::json!({}),
            context: Default::default(),
        };

        let err = runtime.invoke(invocation).await.unwrap_err();
        assert!(matches!(err, RuntimeError::Unsupported(_)));
    }

    #[test]
    fn hook_result_ok_preserves_output() {
        let result = HookResult::ok(serde_json::json!({"key": "value"}));
        let response = hook_result_to_plugin_response(result);
        assert!(response.ok);
        assert_eq!(response.data, serde_json::json!({"key": "value"}));
        assert!(response.diagnostics.is_empty());
    }

    #[test]
    fn hook_result_error_maps_to_diagnostics() {
        let result = HookResult::error("something failed");
        let response = hook_result_to_plugin_response(result);
        assert!(!response.ok);
        assert!(response.data.is_null());
        assert_eq!(response.diagnostics.len(), 1);
        assert_eq!(response.diagnostics[0].message, "something failed");
    }

    #[test]
    fn hook_result_blocked_sets_ok_false() {
        let result = HookResult::blocked();
        let response = hook_result_to_plugin_response(result);
        assert!(!response.ok);
    }

    #[test]
    fn invocation_to_hook_context_extracts_hook_type() {
        let invocation = PluginInvocation {
            protocol_version: PLUGIN_PROTOCOL_VERSION,
            invocation_id: "inv-4".into(),
            plugin_id: "builtin:test".into(),
            capability: PluginCapabilityInvocation::Hook {
                hook_type: "auth".into(),
            },
            args: Vec::new(),
            input: serde_json::json!({"token": "abc"}),
            context: Default::default(),
        };

        let ctx = invocation_to_hook_context(&invocation).unwrap();
        assert_eq!(ctx.hook_type, HookType::Auth);
        assert_eq!(ctx.input, serde_json::json!({"token": "abc"}));
    }

    #[test]
    fn invocation_to_hook_context_command_capability_is_rejected() {
        let invocation = PluginInvocation {
            protocol_version: PLUGIN_PROTOCOL_VERSION,
            invocation_id: "inv-5".into(),
            plugin_id: "builtin:test".into(),
            capability: PluginCapabilityInvocation::Command {
                name: "test_cmd".into(),
            },
            args: Vec::new(),
            input: serde_json::json!({}),
            context: Default::default(),
        };

        // Command invocations must NOT be silently mapped onto a hook handler.
        let err = invocation_to_hook_context(&invocation).unwrap_err();
        assert!(matches!(err, RuntimeError::Unsupported(_)));
        let msg = err.to_string();
        assert!(
            msg.contains("command"),
            "error should mention command, got: {msg}"
        );
    }

    #[test]
    fn invocation_to_hook_context_unknown_hook_type_is_rejected() {
        let invocation = PluginInvocation {
            protocol_version: PLUGIN_PROTOCOL_VERSION,
            invocation_id: "inv-7".into(),
            plugin_id: "builtin:test".into(),
            capability: PluginCapabilityInvocation::Hook {
                hook_type: "totally.unknown.hook".into(),
            },
            args: Vec::new(),
            input: serde_json::json!({}),
            context: Default::default(),
        };

        // Unknown hook type strings must NOT fall back to Auth.
        let err = invocation_to_hook_context(&invocation).unwrap_err();
        assert!(matches!(err, RuntimeError::Unsupported(_)));
        let msg = err.to_string();
        assert!(
            msg.contains("totally.unknown.hook"),
            "error should mention the unknown hook type, got: {msg}"
        );
    }

    #[test]
    fn invocation_to_hook_context_rejects_unsupported_capability() {
        let invocation = PluginInvocation {
            protocol_version: PLUGIN_PROTOCOL_VERSION,
            invocation_id: "inv-6".into(),
            plugin_id: "builtin:test".into(),
            capability: PluginCapabilityInvocation::StatusWidget {
                id: "widget-1".into(),
            },
            args: Vec::new(),
            input: serde_json::json!({}),
            context: Default::default(),
        };

        let err = invocation_to_hook_context(&invocation).unwrap_err();
        assert!(matches!(err, RuntimeError::Unsupported(_)));
    }

    #[test]
    fn handler_registry_crud() {
        let mut registry = BuiltinHandlerRegistry::new();
        assert!(registry.is_empty());

        registry.register("h1".into(), make_test_handler());
        assert_eq!(registry.len(), 1);
        assert!(registry.contains("h1"));
        assert!(registry.get("h1").is_some());
        assert!(!registry.contains("h2"));

        registry.register("h2".into(), make_error_handler());
        assert_eq!(registry.len(), 2);
    }

    #[test]
    fn extract_handler_id_strips_prefix() {
        let inv = PluginInvocation {
            protocol_version: PLUGIN_PROTOCOL_VERSION,
            invocation_id: "inv".into(),
            plugin_id: "builtin:copilot".into(),
            capability: PluginCapabilityInvocation::Hook {
                hook_type: "auth".into(),
            },
            args: Vec::new(),
            input: serde_json::json!({}),
            context: Default::default(),
        };
        assert_eq!(extract_handler_id(&inv).unwrap(), "copilot");
    }

    #[test]
    fn extract_handler_id_fails_without_prefix() {
        let inv = PluginInvocation {
            protocol_version: PLUGIN_PROTOCOL_VERSION,
            invocation_id: "inv".into(),
            plugin_id: "copilot".into(),
            capability: PluginCapabilityInvocation::Hook {
                hook_type: "auth".into(),
            },
            args: Vec::new(),
            input: serde_json::json!({}),
            context: Default::default(),
        };
        assert!(extract_handler_id(&inv).is_err());
    }
}
