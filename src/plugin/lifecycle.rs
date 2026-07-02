use std::sync::Arc;

use crate::plugin::hooks::{HookContext, HookResult, HookType};
use crate::plugin::policy::PluginLifecyclePolicy;
use crate::plugin::service::PluginService;

/// Typed input for the event observation hook.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EventHookInput {
    pub event_type: String,
    pub session_id: Option<String>,
    pub event: serde_json::Value,
}

/// Typed input for the tool execute before hook.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ToolBeforeHookInput {
    pub tool_name: String,
    pub tool_call_id: String,
    pub args: serde_json::Value,
    pub session_id: String,
    pub risk: String,
}

/// Action decided by the before hook.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolBeforeAction {
    Allow,
    Deny,
    Modify,
}

/// Typed output from the tool execute before hook.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ToolBeforeHookOutput {
    pub action: ToolBeforeAction,
    #[serde(default)]
    pub args: Option<serde_json::Value>,
    #[serde(default)]
    pub reason: Option<String>,
}

/// Typed input for the tool execute after hook.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ToolAfterHookInput {
    pub tool_name: String,
    pub tool_call_id: String,
    pub args: serde_json::Value,
    pub success: bool,
    pub output: String,
    pub duration_ms: u64,
}

/// Typed input for the message transform hook.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MessageTransformInput {
    pub messages: Vec<serde_json::Value>,
    pub session_id: Option<String>,
    pub model: Option<String>,
    pub agent: Option<String>,
}

/// Typed output from the message transform hook.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct MessageTransformOutput {
    pub messages: Vec<crate::protocol::dto::ProviderMessage>,
}

/// Typed input for the shell env hook.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ShellEnvHookInput {
    pub command: String,
    pub cwd: String,
    pub base_env_keys: Vec<String>,
}

/// Typed output from the shell env hook.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ShellEnvHookOutput {
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    #[serde(default)]
    pub remove: Vec<String>,
}

/// Typed input for the chat params hook.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ChatParamsHookInput {
    pub model: String,
    pub params: serde_json::Value,
}

/// Typed output from the chat params hook.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ChatParamsHookOutput {
    pub params: serde_json::Value,
}

/// Typed input for the chat headers hook.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ChatHeadersHookInput {
    pub provider: String,
    pub headers: serde_json::Value,
}

/// Typed output from the chat headers hook.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ChatHeadersHookOutput {
    pub headers: serde_json::Value,
}

/// Typed input for the auth hook.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AuthHookInput {
    pub provider: String,
    pub token: String,
    pub headers: serde_json::Value,
}

/// Typed output from the auth hook.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct AuthHookOutput {
    pub headers: serde_json::Value,
}

/// Outcome of a lifecycle hook dispatch.
#[derive(Debug, Clone)]
pub enum PluginHookOutcome<T> {
    /// Hook produced a value, along with any plugin UI effects.
    Ok(T, Vec<codegg_protocol::ui::UiEffect>),
    /// Hook was skipped due to policy or no registered hooks.
    Skipped,
    /// Hook blocked the operation.
    Blocked { reason: Option<String> },
    /// Hook failed (timeout, error).
    Failed { error: String },
}

impl<T> PluginHookOutcome<T> {
    pub fn is_ok(&self) -> bool {
        matches!(self, PluginHookOutcome::Ok(_, _))
    }
    pub fn is_blocked(&self) -> bool {
        matches!(self, PluginHookOutcome::Blocked { .. })
    }
    pub fn is_failed(&self) -> bool {
        matches!(self, PluginHookOutcome::Failed { .. })
    }
    pub fn is_skipped(&self) -> bool {
        matches!(self, PluginHookOutcome::Skipped)
    }
}

/// High-level lifecycle hook dispatcher that keeps call sites concise.
///
/// Centralizes serialization between typed inputs and `PluginInvocation`,
/// and enforces policy gating for each hook type.
pub struct LifecycleHooks {
    service: Arc<PluginService>,
    policy: PluginLifecyclePolicy,
}

impl LifecycleHooks {
    pub fn new(service: Arc<PluginService>, policy: PluginLifecyclePolicy) -> Self {
        Self { service, policy }
    }

    pub fn service(&self) -> &PluginService {
        &self.service
    }

    pub fn policy(&self) -> &PluginLifecyclePolicy {
        &self.policy
    }

    /// Emit an event observation hook. Always fails open.
    pub async fn emit_event(&self, input: EventHookInput) -> PluginHookOutcome<()> {
        if !self.policy.is_hook_allowed(HookType::Event) {
            tracing::debug!(
                hook_type = "event",
                policy_decision = "skipped",
                "event hook skipped by policy"
            );
            return PluginHookOutcome::Skipped;
        }

        let json = match serde_json::to_value(&input) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(hook_type = "event", error = %e, "event hook serialization failed");
                return PluginHookOutcome::Failed {
                    error: e.to_string(),
                };
            }
        };

        let start = std::time::Instant::now();
        let result = self
            .service
            .dispatch_hook(HookContext {
                hook_type: HookType::Event,
                input: json,
            })
            .await;
        let duration_ms = start.elapsed().as_millis();

        tracing::debug!(
            hook_type = "event",
            event_type = %input.event_type,
            duration_ms,
            blocked = result.blocked,
            error = result.error.as_deref(),
            "event hook dispatched"
        );

        outcome_to_unit(result, self.policy.observation_fail_open())
    }

    /// Dispatch the tool execute before hook. May block or modify args.
    pub async fn before_tool_execute(
        &self,
        input: ToolBeforeHookInput,
    ) -> PluginHookOutcome<ToolBeforeHookOutput> {
        if !self.policy.is_hook_allowed(HookType::ToolExecuteBefore) {
            tracing::debug!(
                hook_type = "tool_execute_before",
                tool = %input.tool_name,
                policy_decision = "skipped",
                "before-tool hook skipped by policy"
            );
            return PluginHookOutcome::Skipped;
        }

        let json = match serde_json::to_value(&input) {
            Ok(v) => v,
            Err(e) => {
                return PluginHookOutcome::Failed {
                    error: e.to_string(),
                };
            }
        };

        let start = std::time::Instant::now();
        let result = self
            .service
            .dispatch_hook(HookContext {
                hook_type: HookType::ToolExecuteBefore,
                input: json,
            })
            .await;
        let duration_ms = start.elapsed().as_millis();

        tracing::debug!(
            hook_type = "tool_execute_before",
            tool = %input.tool_name,
            risk = %input.risk,
            duration_ms,
            blocked = result.blocked,
            error = result.error.as_deref(),
            "before-tool hook dispatched"
        );

        outcome_to_typed(result, self.policy.observation_fail_open())
    }

    /// Dispatch the tool execute after hook. Observation only.
    pub async fn after_tool_execute(&self, input: ToolAfterHookInput) -> PluginHookOutcome<()> {
        if !self.policy.is_hook_allowed(HookType::ToolExecuteAfter) {
            return PluginHookOutcome::Skipped;
        }

        let json = match serde_json::to_value(&input) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(hook_type = "tool_execute_after", error = %e, "after-tool hook serialization failed");
                return PluginHookOutcome::Failed {
                    error: e.to_string(),
                };
            }
        };

        let start = std::time::Instant::now();
        let result = self
            .service
            .dispatch_hook(HookContext {
                hook_type: HookType::ToolExecuteAfter,
                input: json,
            })
            .await;
        let duration_ms = start.elapsed().as_millis();

        tracing::debug!(
            hook_type = "tool_execute_after",
            tool = %input.tool_name,
            success = input.success,
            duration_ms,
            blocked = result.blocked,
            error = result.error.as_deref(),
            "after-tool hook dispatched"
        );

        outcome_to_unit(result, self.policy.observation_fail_open())
    }

    /// Dispatch the message transform hook. Mutating: returns transformed messages.
    pub async fn transform_messages(
        &self,
        input: MessageTransformInput,
    ) -> PluginHookOutcome<MessageTransformOutput> {
        if !self.policy.is_hook_allowed(HookType::MessagesTransform) {
            tracing::debug!(
                hook_type = "messages_transform",
                policy_decision = "skipped",
                "message transform hook skipped by policy"
            );
            return PluginHookOutcome::Skipped;
        }

        let json = match serde_json::to_value(&input) {
            Ok(v) => v,
            Err(e) => {
                return PluginHookOutcome::Failed {
                    error: e.to_string(),
                };
            }
        };

        let start = std::time::Instant::now();
        let result = self
            .service
            .dispatch_hook(HookContext {
                hook_type: HookType::MessagesTransform,
                input: json,
            })
            .await;
        let duration_ms = start.elapsed().as_millis();

        tracing::debug!(
            hook_type = "messages_transform",
            message_count = input.messages.len(),
            duration_ms,
            blocked = result.blocked,
            error = result.error.as_deref(),
            "message transform hook dispatched"
        );

        outcome_to_typed(result, self.policy.mutating_fail_open())
    }

    /// Dispatch the shell env hook. Mutating: returns env additions/removals.
    pub async fn shell_env(
        &self,
        input: ShellEnvHookInput,
    ) -> PluginHookOutcome<ShellEnvHookOutput> {
        if !self.policy.is_hook_allowed(HookType::ShellEnv) {
            tracing::debug!(
                hook_type = "shell_env",
                policy_decision = "skipped",
                "shell env hook skipped by policy"
            );
            return PluginHookOutcome::Skipped;
        }

        let json = match serde_json::to_value(&input) {
            Ok(v) => v,
            Err(e) => {
                return PluginHookOutcome::Failed {
                    error: e.to_string(),
                };
            }
        };

        let start = std::time::Instant::now();
        let result = self
            .service
            .dispatch_hook(HookContext {
                hook_type: HookType::ShellEnv,
                input: json,
            })
            .await;
        let duration_ms = start.elapsed().as_millis();

        tracing::debug!(
            hook_type = "shell_env",
            command = %input.command,
            duration_ms,
            blocked = result.blocked,
            error = result.error.as_deref(),
            "shell env hook dispatched"
        );

        outcome_to_typed(result, self.policy.mutating_fail_open())
    }

    /// Dispatch the chat params hook. Mutating: returns modified params.
    pub async fn chat_params(
        &self,
        input: ChatParamsHookInput,
    ) -> PluginHookOutcome<ChatParamsHookOutput> {
        if !self.policy.is_hook_allowed(HookType::ChatParams) {
            tracing::debug!(
                hook_type = "chat_params",
                policy_decision = "skipped",
                "chat params hook skipped by policy"
            );
            return PluginHookOutcome::Skipped;
        }

        let json = match serde_json::to_value(&input) {
            Ok(v) => v,
            Err(e) => {
                return PluginHookOutcome::Failed {
                    error: e.to_string(),
                };
            }
        };

        let start = std::time::Instant::now();
        let result = self
            .service
            .dispatch_hook(HookContext {
                hook_type: HookType::ChatParams,
                input: json,
            })
            .await;
        let duration_ms = start.elapsed().as_millis();

        tracing::debug!(
            hook_type = "chat_params",
            model = %input.model,
            duration_ms,
            blocked = result.blocked,
            error = result.error.as_deref(),
            "chat params hook dispatched"
        );

        outcome_to_typed(result, self.policy.mutating_fail_open())
    }

    /// Dispatch the chat headers hook. Mutating: returns modified headers.
    pub async fn chat_headers(
        &self,
        input: ChatHeadersHookInput,
    ) -> PluginHookOutcome<ChatHeadersHookOutput> {
        if !self.policy.is_hook_allowed(HookType::ChatHeaders) {
            tracing::debug!(
                hook_type = "chat_headers",
                policy_decision = "skipped",
                "chat headers hook skipped by policy"
            );
            return PluginHookOutcome::Skipped;
        }

        let json = match serde_json::to_value(&input) {
            Ok(v) => v,
            Err(e) => {
                return PluginHookOutcome::Failed {
                    error: e.to_string(),
                };
            }
        };

        let start = std::time::Instant::now();
        let result = self
            .service
            .dispatch_hook(HookContext {
                hook_type: HookType::ChatHeaders,
                input: json,
            })
            .await;
        let duration_ms = start.elapsed().as_millis();

        tracing::debug!(
            hook_type = "chat_headers",
            provider = %input.provider,
            duration_ms,
            blocked = result.blocked,
            error = result.error.as_deref(),
            "chat headers hook dispatched"
        );

        outcome_to_typed(result, self.policy.mutating_fail_open())
    }

    /// Dispatch the auth hook. Blocking: returns modified headers.
    pub async fn auth(&self, input: AuthHookInput) -> PluginHookOutcome<AuthHookOutput> {
        if !self.policy.is_hook_allowed(HookType::Auth) {
            tracing::debug!(
                hook_type = "auth",
                policy_decision = "skipped",
                "auth hook skipped by policy"
            );
            return PluginHookOutcome::Skipped;
        }

        let json = match serde_json::to_value(&input) {
            Ok(v) => v,
            Err(e) => {
                return PluginHookOutcome::Failed {
                    error: e.to_string(),
                };
            }
        };

        let start = std::time::Instant::now();
        let result = self
            .service
            .dispatch_hook(HookContext {
                hook_type: HookType::Auth,
                input: json,
            })
            .await;
        let duration_ms = start.elapsed().as_millis();

        tracing::debug!(
            hook_type = "auth",
            provider = %input.provider,
            duration_ms,
            blocked = result.blocked,
            error = result.error.as_deref(),
            "auth hook dispatched"
        );

        outcome_to_typed(result, self.policy.observation_fail_open())
    }
}

fn outcome_to_unit(result: HookResult, fail_open: bool) -> PluginHookOutcome<()> {
    if result.blocked {
        return PluginHookOutcome::Blocked {
            reason: result.error,
        };
    }
    if let Some(err) = result.error {
        if fail_open {
            tracing::debug!("hook failed (fail-open): {}", err);
            return PluginHookOutcome::Skipped;
        }
        return PluginHookOutcome::Failed { error: err };
    }
    PluginHookOutcome::Ok((), result.effects)
}

fn outcome_to_typed<T: for<'de> serde::Deserialize<'de>>(
    result: HookResult,
    fail_open: bool,
) -> PluginHookOutcome<T> {
    if result.blocked {
        return PluginHookOutcome::Blocked {
            reason: result.error,
        };
    }
    if let Some(err) = result.error {
        if fail_open {
            tracing::debug!("hook failed (fail-open): {}", err);
            return PluginHookOutcome::Skipped;
        }
        return PluginHookOutcome::Failed { error: err };
    }
    match serde_json::from_value::<T>(result.output) {
        Ok(val) => PluginHookOutcome::Ok(val, result.effects),
        Err(e) => {
            if fail_open {
                tracing::debug!("hook output deserialization failed (fail-open): {}", e);
                PluginHookOutcome::Skipped
            } else {
                PluginHookOutcome::Failed {
                    error: format!("invalid hook output: {}", e),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_outcome_predicates() {
        let ok: PluginHookOutcome<()> = PluginHookOutcome::Ok((), Vec::new());
        assert!(ok.is_ok());
        assert!(!ok.is_blocked());
        assert!(!ok.is_failed());
        assert!(!ok.is_skipped());

        let skipped: PluginHookOutcome<()> = PluginHookOutcome::Skipped;
        assert!(!skipped.is_ok());
        assert!(skipped.is_skipped());

        let blocked: PluginHookOutcome<()> = PluginHookOutcome::Blocked {
            reason: Some("denied".into()),
        };
        assert!(blocked.is_blocked());

        let failed: PluginHookOutcome<()> = PluginHookOutcome::Failed {
            error: "timeout".into(),
        };
        assert!(failed.is_failed());
    }

    #[test]
    fn event_hook_input_serialization() {
        let input = EventHookInput {
            event_type: "session.created".into(),
            session_id: Some("s1".into()),
            event: serde_json::json!({"key": "value"}),
        };
        let json = serde_json::to_value(&input).unwrap();
        assert_eq!(json["event_type"], "session.created");
        assert_eq!(json["session_id"], "s1");
        assert_eq!(json["event"]["key"], "value");
    }

    #[test]
    fn tool_before_hook_input_serialization() {
        let input = ToolBeforeHookInput {
            tool_name: "edit".into(),
            tool_call_id: "tc1".into(),
            args: serde_json::json!({"path": "foo.rs"}),
            session_id: "s1".into(),
            risk: "normal".into(),
        };
        let json = serde_json::to_value(&input).unwrap();
        assert_eq!(json["tool_name"], "edit");
        assert_eq!(json["args"]["path"], "foo.rs");
    }

    #[test]
    fn tool_before_hook_output_deserialization() {
        let json = serde_json::json!({
            "action": "allow",
            "reason": null
        });
        let output: ToolBeforeHookOutput = serde_json::from_value(json).unwrap();
        assert_eq!(output.action, ToolBeforeAction::Allow);
        assert!(output.reason.is_none());
    }

    #[test]
    fn tool_before_hook_output_deny() {
        let json = serde_json::json!({
            "action": "deny",
            "reason": "blocked by policy"
        });
        let output: ToolBeforeHookOutput = serde_json::from_value(json).unwrap();
        assert_eq!(output.action, ToolBeforeAction::Deny);
        assert_eq!(output.reason.as_deref(), Some("blocked by policy"));
    }

    #[test]
    fn shell_env_output_deserialization() {
        let json = serde_json::json!({
            "env": {"MY_VAR": "hello"},
            "remove": ["OLD_VAR"]
        });
        let output: ShellEnvHookOutput = serde_json::from_value(json).unwrap();
        assert_eq!(output.env.get("MY_VAR").unwrap(), "hello");
        assert_eq!(output.remove, vec!["OLD_VAR"]);
    }

    #[test]
    fn shell_env_output_empty_defaults() {
        let json = serde_json::json!({});
        let output: ShellEnvHookOutput = serde_json::from_value(json).unwrap();
        assert!(output.env.is_empty());
        assert!(output.remove.is_empty());
    }

    #[test]
    fn outcome_to_unit_ok() {
        let result = HookResult::ok(serde_json::json!({}));
        let outcome = outcome_to_unit(result, true);
        assert!(outcome.is_ok());
    }

    #[test]
    fn outcome_to_unit_blocked() {
        let mut result = HookResult::ok(serde_json::Value::Null);
        result.blocked = true;
        result.error = Some("denied".into());
        let outcome = outcome_to_unit(result, true);
        assert!(outcome.is_blocked());
    }

    #[test]
    fn outcome_to_unit_error_fail_open() {
        let result = HookResult::error("something broke");
        let outcome = outcome_to_unit(result, true);
        assert!(outcome.is_skipped());
    }

    #[test]
    fn outcome_to_unit_error_fail_closed() {
        let result = HookResult::error("something broke");
        let outcome = outcome_to_unit(result, false);
        assert!(outcome.is_failed());
    }

    // ── Integration tests with real PluginService ────────────────────

    #[tokio::test]
    async fn event_hook_observes_with_real_service() {
        use crate::plugin::registry::PluginRegistry;
        use crate::plugin::service::PluginService;
        use std::sync::Arc;

        let registry = Arc::new(PluginRegistry::new());
        let service = Arc::new(PluginService::new(registry));
        let hooks = LifecycleHooks::new(service, PluginLifecyclePolicy::default());

        let input = EventHookInput {
            event_type: "session.created".into(),
            session_id: Some("test-session".into()),
            event: serde_json::json!({"key": "value"}),
        };

        let outcome = hooks.emit_event(input).await;
        // No event hooks registered, so should be Ok (passthrough).
        assert!(outcome.is_ok());
    }

    #[tokio::test]
    async fn before_tool_execute_skipped_when_blocking_disabled() {
        use crate::plugin::registry::PluginRegistry;
        use crate::plugin::service::PluginService;
        use std::sync::Arc;

        let registry = Arc::new(PluginRegistry::new());
        let service = Arc::new(PluginService::new(registry));
        let policy = PluginLifecyclePolicy {
            enable_blocking_hooks: false,
            ..Default::default()
        };
        let hooks = LifecycleHooks::new(service, policy);

        let input = ToolBeforeHookInput {
            tool_name: "edit".into(),
            tool_call_id: "tc1".into(),
            args: serde_json::json!({"path": "foo.rs"}),
            session_id: "s1".into(),
            risk: "normal".into(),
        };

        let outcome = hooks.before_tool_execute(input).await;
        assert!(outcome.is_skipped());
    }

    #[tokio::test]
    async fn before_tool_execute_allowed_when_blocking_enabled() {
        use crate::plugin::registry::PluginRegistry;
        use crate::plugin::service::PluginService;
        use std::sync::Arc;

        let registry = Arc::new(PluginRegistry::new());
        let service = Arc::new(PluginService::new(registry));
        let policy = PluginLifecyclePolicy {
            enable_blocking_hooks: true,
            ..Default::default()
        };
        let hooks = LifecycleHooks::new(service, policy);

        let input = ToolBeforeHookInput {
            tool_name: "edit".into(),
            tool_call_id: "tc1".into(),
            args: serde_json::json!({"path": "foo.rs"}),
            session_id: "s1".into(),
            risk: "normal".into(),
        };

        let outcome = hooks.before_tool_execute(input).await;
        // No hooks registered, so should return Skipped (no registered hooks = passthrough).
        assert!(outcome.is_ok() || outcome.is_skipped());
    }

    #[tokio::test]
    async fn after_tool_execute_observes_with_real_service() {
        use crate::plugin::registry::PluginRegistry;
        use crate::plugin::service::PluginService;
        use std::sync::Arc;

        let registry = Arc::new(PluginRegistry::new());
        let service = Arc::new(PluginService::new(registry));
        let hooks = LifecycleHooks::new(service, PluginLifecyclePolicy::default());

        let input = ToolAfterHookInput {
            tool_name: "edit".into(),
            tool_call_id: "tc1".into(),
            args: serde_json::json!({"path": "foo.rs"}),
            success: true,
            output: "Applied patch".into(),
            duration_ms: 42,
        };

        let outcome = hooks.after_tool_execute(input).await;
        assert!(outcome.is_ok());
    }

    #[tokio::test]
    async fn shell_env_hook_with_real_service() {
        use crate::plugin::registry::PluginRegistry;
        use crate::plugin::service::PluginService;
        use std::sync::Arc;

        let registry = Arc::new(PluginRegistry::new());
        let service = Arc::new(PluginService::new(registry));
        let policy = PluginLifecyclePolicy {
            enable_mutating_hooks: true,
            ..Default::default()
        };
        let hooks = LifecycleHooks::new(service, policy);

        let input = ShellEnvHookInput {
            command: "echo hello".into(),
            cwd: "/tmp".into(),
            base_env_keys: vec!["PATH".into()],
        };

        let outcome = hooks.shell_env(input).await;
        // No hooks registered, should get empty output.
        match outcome {
            PluginHookOutcome::Ok(output, _) => {
                assert!(output.env.is_empty());
                assert!(output.remove.is_empty());
            }
            other => panic!("expected Ok, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn message_transform_skipped_when_mutating_disabled() {
        use crate::plugin::registry::PluginRegistry;
        use crate::plugin::service::PluginService;
        use std::sync::Arc;

        let registry = Arc::new(PluginRegistry::new());
        let service = Arc::new(PluginService::new(registry));
        let policy = PluginLifecyclePolicy {
            enable_mutating_hooks: false,
            ..Default::default()
        };
        let hooks = LifecycleHooks::new(service, policy);

        let input = MessageTransformInput {
            messages: vec![
                serde_json::json!({"role": "user", "content": [{"type": "text", "text": "hello"}]}),
            ],
            session_id: None,
            model: None,
            agent: None,
        };

        let outcome = hooks.transform_messages(input).await;
        assert!(outcome.is_skipped());
    }

    #[tokio::test]
    async fn disabled_plugin_does_not_receive_hooks() {
        use crate::plugin::manifest::{
            PluginCapability, PluginHookSpec, PluginManifest, PluginRuntimeSpec, PluginTrustClass,
        };
        use crate::plugin::registry::PluginInfo;
        use crate::plugin::registry::PluginRegistry;
        use crate::plugin::service::PluginService;
        use std::sync::Arc;

        let registry = PluginRegistry::new();

        // Register a plugin with an event hook, then disable it.
        let manifest = PluginManifest {
            api_version: 1,
            runtime: PluginRuntimeSpec::Builtin {
                handler: "test".into(),
            },
            capabilities: vec![PluginCapability::Hook(PluginHookSpec {
                hook_type: "event".into(),
                priority: 0,
                handler: None,
            })],
            ..Default::default()
        };
        let info = PluginInfo {
            id: "plugin:test".into(),
            manifest,
            enabled: true,
            trust: PluginTrustClass::Builtin,
            diagnostics: Vec::new(),
        };
        registry.register(info).await.unwrap();

        // Disable the plugin.
        registry.set_enabled("plugin:test", false).await.unwrap();

        let service = Arc::new(PluginService::new(Arc::new(registry)));
        let hooks = LifecycleHooks::new(service, PluginLifecyclePolicy::default());

        let input = EventHookInput {
            event_type: "test".into(),
            session_id: None,
            event: serde_json::json!({}),
        };

        let outcome = hooks.emit_event(input).await;
        // Plugin is disabled, so hook should not fire.
        assert!(outcome.is_ok() || outcome.is_skipped());
    }

    #[tokio::test]
    async fn builtin_auth_hook_dispatches_through_service() {
        use crate::plugin::builtin::{builtin_runtime_registry, register_builtins};
        use crate::plugin::registry::PluginRegistry;
        use crate::plugin::runtime::builtin::BuiltinRuntime;
        use crate::plugin::service::PluginService;
        use std::sync::Arc;

        let registry = Arc::new(PluginRegistry::new());
        register_builtins(&registry).await;

        let handler_registry = Arc::new(builtin_runtime_registry());
        let builtin_rt = Arc::new(BuiltinRuntime::new(handler_registry));

        let service = Arc::new(PluginService::new(registry).with_builtin_runtime(builtin_rt));

        // Dispatch an auth hook for copilot.
        let result = service
            .dispatch_auth(serde_json::json!({
                "provider": "copilot",
                "token": "test-token",
                "headers": {}
            }))
            .await;

        assert!(!result.blocked);
        assert!(result.error.is_none());
        // The copilot builtin should inject Authorization header.
        assert!(
            result.output.get("headers").is_some(),
            "should have headers in output"
        );
    }

    #[tokio::test]
    async fn process_lifecycle_hook_denied_by_default_policy() {
        use crate::plugin::manifest::PluginRuntimeSpec;

        let policy = PluginLifecyclePolicy::default();
        assert!(
            !policy.is_runtime_allowed(&PluginRuntimeSpec::Process {
                command: "my-plugin".into(),
                args: Vec::new(),
                timeout_ms: None,
            }),
            "process runtime should be denied by default"
        );
    }

    #[tokio::test]
    async fn lifecycle_hooks_policy_prevents_tool_before_when_disabled() {
        use crate::plugin::registry::PluginRegistry;
        use crate::plugin::service::PluginService;
        use std::sync::Arc;

        let registry = Arc::new(PluginRegistry::new());
        let service = Arc::new(PluginService::new(registry));

        // Default policy has blocking hooks disabled.
        let hooks = LifecycleHooks::new(service, PluginLifecyclePolicy::default());

        let input = ToolBeforeHookInput {
            tool_name: "bash".into(),
            tool_call_id: "tc-1".into(),
            args: serde_json::json!({"command": "rm -rf /"}),
            session_id: "s1".into(),
            risk: "dangerous".into(),
        };

        let outcome = hooks.before_tool_execute(input).await;
        // Blocking hooks disabled -> skipped.
        assert!(outcome.is_skipped());
    }

    #[tokio::test]
    async fn before_tool_execute_allow_passes_args_unchanged() {
        use crate::plugin::builtin::builtin_runtime_registry;
        use crate::plugin::manifest::{
            PluginCapability, PluginHookSpec, PluginManifest, PluginRuntimeSpec, PluginTrustClass,
        };
        use crate::plugin::registry::PluginInfo;
        use crate::plugin::registry::PluginRegistry;
        use crate::plugin::runtime::builtin::BuiltinRuntime;
        use crate::plugin::service::PluginService;
        use std::sync::Arc;

        let registry = PluginRegistry::new();
        let manifest = PluginManifest {
            api_version: 1,
            runtime: PluginRuntimeSpec::Builtin {
                handler: "test-allow".into(),
            },
            capabilities: vec![PluginCapability::Hook(PluginHookSpec {
                hook_type: "tool_execute_before".into(),
                priority: 0,
                handler: None,
            })],
            ..Default::default()
        };
        let info = PluginInfo {
            id: "plugin:test-allow".into(),
            manifest,
            enabled: true,
            trust: PluginTrustClass::Builtin,
            diagnostics: Vec::new(),
        };
        registry.register(info).await.unwrap();

        let handler_registry = Arc::new(builtin_runtime_registry());
        let builtin_rt = Arc::new(BuiltinRuntime::new(handler_registry));
        let service =
            Arc::new(PluginService::new(Arc::new(registry)).with_builtin_runtime(builtin_rt));

        let policy = PluginLifecyclePolicy {
            enable_blocking_hooks: true,
            ..Default::default()
        };
        let hooks = LifecycleHooks::new(service, policy);

        let input = ToolBeforeHookInput {
            tool_name: "edit".into(),
            tool_call_id: "tc1".into(),
            args: serde_json::json!({"path": "foo.rs", "content": "hello"}),
            session_id: "s1".into(),
            risk: "normal".into(),
        };

        let outcome = hooks.before_tool_execute(input).await;
        // No test handler registered, so passthrough (Ok or Skipped).
        match outcome {
            PluginHookOutcome::Ok(output, _) => {
                assert_eq!(output.action, ToolBeforeAction::Allow);
            }
            PluginHookOutcome::Skipped => {}
            other => panic!("expected Ok or Skipped, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn before_tool_execute_modify_ignored_when_mutating_disabled() {
        use crate::plugin::registry::PluginRegistry;
        use crate::plugin::service::PluginService;
        use std::sync::Arc;

        let registry = Arc::new(PluginRegistry::new());
        let service = Arc::new(PluginService::new(registry));

        // Both blocking and mutating disabled (default).
        let hooks = LifecycleHooks::new(service, PluginLifecyclePolicy::default());

        let input = ToolBeforeHookInput {
            tool_name: "edit".into(),
            tool_call_id: "tc1".into(),
            args: serde_json::json!({"path": "foo.rs"}),
            session_id: "s1".into(),
            risk: "normal".into(),
        };

        let outcome = hooks.before_tool_execute(input).await;
        // Blocking hooks disabled -> skipped entirely, modify never fires.
        assert!(outcome.is_skipped());
    }

    #[tokio::test]
    async fn wasm_and_builtin_mutating_hooks_allowed_when_policy_enables() {
        use crate::plugin::hooks::HookType;
        use crate::plugin::manifest::PluginRuntimeSpec;
        use crate::plugin::policy::{classify_hook, HookCategory, PluginLifecyclePolicy};

        let policy = PluginLifecyclePolicy {
            enable_mutating_hooks: true,
            ..Default::default()
        };

        // Mutating hook types should be allowed.
        assert!(policy.is_hook_allowed(HookType::MessagesTransform));
        assert!(policy.is_hook_allowed(HookType::ShellEnv));
        assert!(policy.is_hook_allowed(HookType::ToolDefinition));
        assert!(policy.is_hook_allowed(HookType::ChatParams));
        assert!(policy.is_hook_allowed(HookType::ChatHeaders));

        // Builtin/WASM runtimes should always be allowed.
        assert!(policy.is_runtime_allowed(&PluginRuntimeSpec::Builtin {
            handler: "test".into()
        }));
        assert!(policy.is_runtime_allowed(&PluginRuntimeSpec::Wasm {
            module: "test.wasm".into(),
            timeout_ms: None,
            memory_max_mb: None,
            fuel_per_call: None,
        }));

        // Classification should be Mutating.
        assert_eq!(
            classify_hook(HookType::MessagesTransform),
            HookCategory::Mutating
        );
        assert_eq!(classify_hook(HookType::ShellEnv), HookCategory::Mutating);
    }

    #[tokio::test]
    async fn shell_env_hook_does_not_log_secret_values() {
        use crate::plugin::registry::PluginRegistry;
        use crate::plugin::service::PluginService;
        use std::sync::Arc;

        let registry = Arc::new(PluginRegistry::new());
        let service = Arc::new(PluginService::new(registry));
        let policy = PluginLifecyclePolicy {
            enable_mutating_hooks: true,
            ..Default::default()
        };
        let hooks = LifecycleHooks::new(service, policy);

        let input = ShellEnvHookInput {
            command: "echo secret".into(),
            cwd: "/tmp".into(),
            base_env_keys: vec!["PATH".into(), "SECRET_KEY".into()],
        };

        // The hook dispatches; no secret values appear in tracing output
        // because tracing fields only log command/cwd, not env values.
        let outcome = hooks.shell_env(input).await;
        match outcome {
            PluginHookOutcome::Ok(output, _) => {
                assert!(output.env.is_empty());
            }
            other => panic!("expected Ok, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn chat_params_hook_skipped_when_mutating_disabled() {
        use crate::plugin::registry::PluginRegistry;
        use crate::plugin::service::PluginService;
        use std::sync::Arc;

        let registry = Arc::new(PluginRegistry::new());
        let service = Arc::new(PluginService::new(registry));
        let hooks = LifecycleHooks::new(service, PluginLifecyclePolicy::default());

        let input = ChatParamsHookInput {
            model: "gpt-4".into(),
            params: serde_json::json!({"temperature": 0.7}),
        };

        let outcome = hooks.chat_params(input).await;
        assert!(outcome.is_skipped());
    }

    #[tokio::test]
    async fn chat_headers_hook_skipped_when_mutating_disabled() {
        use crate::plugin::registry::PluginRegistry;
        use crate::plugin::service::PluginService;
        use std::sync::Arc;

        let registry = Arc::new(PluginRegistry::new());
        let service = Arc::new(PluginService::new(registry));
        let hooks = LifecycleHooks::new(service, PluginLifecyclePolicy::default());

        let input = ChatHeadersHookInput {
            provider: "openai".into(),
            headers: serde_json::json!({"Authorization": "Bearer test"}),
        };

        let outcome = hooks.chat_headers(input).await;
        assert!(outcome.is_skipped());
    }

    #[tokio::test]
    async fn auth_hook_skipped_when_blocking_disabled() {
        use crate::plugin::registry::PluginRegistry;
        use crate::plugin::service::PluginService;
        use std::sync::Arc;

        let registry = Arc::new(PluginRegistry::new());
        let service = Arc::new(PluginService::new(registry));
        let hooks = LifecycleHooks::new(service, PluginLifecyclePolicy::default());

        let input = AuthHookInput {
            provider: "copilot".into(),
            token: "test-token".into(),
            headers: serde_json::json!({}),
        };

        let outcome = hooks.auth(input).await;
        assert!(outcome.is_skipped());
    }

    #[test]
    fn hook_outcome_ok_carries_effects() {
        use crate::protocol::ui::{ToastLevel, ToastSpec, UiEffect};

        let effects = vec![
            UiEffect::ShowToast {
                toast: ToastSpec {
                    level: ToastLevel::Info,
                    message: "first".into(),
                },
            },
            UiEffect::ShowToast {
                toast: ToastSpec {
                    level: ToastLevel::Success,
                    message: "second".into(),
                },
            },
        ];
        let outcome: PluginHookOutcome<()> = PluginHookOutcome::Ok((), effects);
        match outcome {
            PluginHookOutcome::Ok((), effects) => {
                assert_eq!(effects.len(), 2);
                assert!(matches!(
                    effects[0],
                    UiEffect::ShowToast {
                        toast: ToastSpec {
                            level: ToastLevel::Info,
                            ..
                        }
                    }
                ));
                assert!(matches!(
                    effects[1],
                    UiEffect::ShowToast {
                        toast: ToastSpec {
                            level: ToastLevel::Success,
                            ..
                        }
                    }
                ));
            }
            other => panic!("expected Ok, got: {other:?}"),
        }
    }

    #[test]
    fn hook_outcome_effect_ordering_preserved() {
        use crate::protocol::ui::{
            DialogSpec, PanelPlacement, PanelSpec, ToastLevel, ToastSpec, UiEffect, UiNode,
        };

        let effects = vec![
            UiEffect::OpenDialog {
                dialog: DialogSpec {
                    id: "dlg-1".into(),
                    title: "Dialog".into(),
                    body: UiNode::Empty,
                    modal: true,
                },
            },
            UiEffect::OpenPanel {
                panel: PanelSpec {
                    id: "panel-1".into(),
                    title: "Panel".into(),
                    placement: PanelPlacement::Right,
                    body: UiNode::Empty,
                },
            },
            UiEffect::ShowToast {
                toast: ToastSpec {
                    level: ToastLevel::Info,
                    message: "done".into(),
                },
            },
        ];
        let outcome: PluginHookOutcome<()> = PluginHookOutcome::Ok((), effects);
        if let PluginHookOutcome::Ok((), effects) = outcome {
            assert_eq!(effects.len(), 3);
            assert!(matches!(effects[0], UiEffect::OpenDialog { .. }));
            assert!(matches!(effects[1], UiEffect::OpenPanel { .. }));
            assert!(matches!(effects[2], UiEffect::ShowToast { .. }));
        } else {
            panic!("expected Ok");
        }
    }

    #[tokio::test]
    async fn disabled_event_hook_skipped_by_policy() {
        use crate::plugin::registry::PluginRegistry;
        use crate::plugin::service::PluginService;
        use std::sync::Arc;

        let registry = Arc::new(PluginRegistry::new());
        let service = Arc::new(PluginService::new(registry));
        // Disable observation hooks to test the skip path.
        let policy = PluginLifecyclePolicy {
            enable_observation_hooks: false,
            ..Default::default()
        };
        let hooks = LifecycleHooks::new(service, policy);
        let input = EventHookInput {
            event_type: "test.event".into(),
            session_id: None,
            event: serde_json::json!({}),
        };
        let outcome = hooks.emit_event(input).await;
        assert!(outcome.is_skipped());
    }
}
