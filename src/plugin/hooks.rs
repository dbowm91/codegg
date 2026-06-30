use std::fmt;
use strum::{Display, EnumIter, EnumString};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumString, Display, EnumIter)]
#[strum(serialize_all = "snake_case")]
pub enum HookType {
    Auth,
    Provider,
    ToolDefinition,
    ToolExecuteBefore,
    ToolExecuteAfter,
    ChatParams,
    ChatHeaders,
    Event,
    Config,
    ShellEnv,
    TextComplete,
    SessionCompacting,
    MessagesTransform,
}

impl HookType {
    pub fn as_str(&self) -> &'static str {
        match self {
            HookType::Auth => "auth",
            HookType::Provider => "provider",
            HookType::ToolDefinition => "tool.definition",
            HookType::ToolExecuteBefore => "tool.execute.before",
            HookType::ToolExecuteAfter => "tool.execute.after",
            HookType::ChatParams => "chat.params",
            HookType::ChatHeaders => "chat.headers",
            HookType::Event => "event",
            HookType::Config => "config",
            HookType::ShellEnv => "shell.env",
            HookType::TextComplete => "text.complete",
            HookType::SessionCompacting => "session.compacting",
            HookType::MessagesTransform => "messages.transform",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "auth" => Some(HookType::Auth),
            "provider" => Some(HookType::Provider),
            "tool.definition" => Some(HookType::ToolDefinition),
            "tool.execute.before" => Some(HookType::ToolExecuteBefore),
            "tool.execute.after" => Some(HookType::ToolExecuteAfter),
            "chat.params" => Some(HookType::ChatParams),
            "chat.headers" => Some(HookType::ChatHeaders),
            "event" => Some(HookType::Event),
            "config" => Some(HookType::Config),
            "shell.env" => Some(HookType::ShellEnv),
            "text.complete" => Some(HookType::TextComplete),
            "session.compacting" => Some(HookType::SessionCompacting),
            "messages.transform" => Some(HookType::MessagesTransform),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct HookContext {
    pub hook_type: HookType,
    pub input: serde_json::Value,
}

impl HookContext {
    /// Convert this `HookContext` into a `PluginInvocation` for runtime dispatch.
    pub fn into_plugin_invocation(
        self,
        plugin_id: String,
        invocation_id: String,
    ) -> crate::protocol::plugin::PluginInvocation {
        use crate::protocol::plugin::{
            PluginCapabilityInvocation, PluginContext, PLUGIN_PROTOCOL_VERSION,
        };

        crate::protocol::plugin::PluginInvocation {
            protocol_version: PLUGIN_PROTOCOL_VERSION,
            invocation_id,
            plugin_id,
            capability: PluginCapabilityInvocation::Hook {
                hook_type: self.hook_type.as_str().to_string(),
            },
            args: Vec::new(),
            input: self.input,
            context: PluginContext::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct HookResult {
    pub output: serde_json::Value,
    pub blocked: bool,
    pub error: Option<String>,
}

impl HookResult {
    pub fn ok(output: serde_json::Value) -> Self {
        Self {
            output,
            blocked: false,
            error: None,
        }
    }

    pub fn blocked() -> Self {
        Self {
            output: serde_json::Value::Null,
            blocked: true,
            error: None,
        }
    }

    pub fn error(msg: impl Into<String>) -> Self {
        Self {
            output: serde_json::Value::Null,
            blocked: false,
            error: Some(msg.into()),
        }
    }

    /// Create a `HookResult` from a `PluginResponse`.
    ///
    /// Maps `ok == false` to an error. Uses response `data` as output,
    /// or falls back to `fallback_input` if data is null.
    pub fn from_plugin_response(
        response: crate::protocol::plugin::PluginResponse,
        fallback_input: serde_json::Value,
    ) -> Self {
        let output = if response.data.is_null() {
            fallback_input
        } else {
            response.data
        };

        if !response.ok {
            let error_msg = response
                .diagnostics
                .iter()
                .find(|d| {
                    matches!(
                        d.level,
                        crate::protocol::plugin::PluginDiagnosticLevel::Error
                    )
                })
                .map(|d| d.message.clone())
                .unwrap_or_else(|| "plugin returned ok=false".to_string());
            return Self {
                output: serde_json::Value::Null,
                blocked: false,
                error: Some(error_msg),
            };
        }

        Self {
            output,
            blocked: false,
            error: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct HookRegistration {
    pub plugin_id: String,
    pub hook_type: HookType,
    pub priority: i32,
}

impl fmt::Display for HookRegistration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}::{}(priority={})",
            self.plugin_id, self.hook_type, self.priority
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::plugin::{
        PluginCapabilityInvocation, PluginResponse, PLUGIN_PROTOCOL_VERSION,
    };

    #[test]
    fn hook_context_into_plugin_invocation() {
        let ctx = HookContext {
            hook_type: HookType::Auth,
            input: serde_json::json!({"token": "test"}),
        };
        let inv = ctx.into_plugin_invocation("plugin:test".into(), "inv-1".into());
        assert_eq!(inv.protocol_version, PLUGIN_PROTOCOL_VERSION);
        assert_eq!(inv.plugin_id, "plugin:test");
        assert_eq!(inv.invocation_id, "inv-1");
        assert_eq!(
            inv.capability,
            PluginCapabilityInvocation::Hook {
                hook_type: "auth".into()
            }
        );
        assert_eq!(inv.input, serde_json::json!({"token": "test"}));
        assert!(inv.args.is_empty());
    }

    #[test]
    fn hook_result_from_plugin_response_ok() {
        let resp = PluginResponse {
            ok: true,
            effects: Vec::new(),
            data: serde_json::json!({"transformed": true}),
            diagnostics: Vec::new(),
        };
        let result = HookResult::from_plugin_response(resp, serde_json::json!({}));
        assert!(result.error.is_none());
        assert!(!result.blocked);
        assert_eq!(result.output, serde_json::json!({"transformed": true}));
    }

    #[test]
    fn hook_result_from_plugin_response_null_data_uses_fallback() {
        let resp = PluginResponse {
            ok: true,
            effects: Vec::new(),
            data: serde_json::Value::Null,
            diagnostics: Vec::new(),
        };
        let fallback = serde_json::json!({"original": true});
        let result = HookResult::from_plugin_response(resp, fallback.clone());
        assert_eq!(result.output, fallback);
    }

    #[test]
    fn hook_result_from_plugin_response_not_ok_maps_error() {
        let resp = PluginResponse {
            ok: false,
            effects: Vec::new(),
            data: serde_json::Value::Null,
            diagnostics: vec![crate::protocol::plugin::PluginDiagnostic {
                level: crate::protocol::plugin::PluginDiagnosticLevel::Error,
                message: "something went wrong".into(),
            }],
        };
        let result = HookResult::from_plugin_response(resp, serde_json::json!({}));
        assert_eq!(result.error.as_deref(), Some("something went wrong"));
        assert!(result.output.is_null());
    }

    #[test]
    fn hook_result_from_plugin_response_not_ok_no_error_diagnostic() {
        let resp = PluginResponse {
            ok: false,
            effects: Vec::new(),
            data: serde_json::Value::Null,
            diagnostics: Vec::new(),
        };
        let result = HookResult::from_plugin_response(resp, serde_json::json!({}));
        assert_eq!(result.error.as_deref(), Some("plugin returned ok=false"));
    }
}
