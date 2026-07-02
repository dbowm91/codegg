use crate::plugin::hooks::HookType;
use crate::plugin::manifest::{PluginRuntimeSpec, PluginTrustClass};

/// Controls which lifecycle hooks are allowed to run and how failures are handled.
///
/// Defaults are conservative: observation hooks enabled, mutating/blocking hooks
/// disabled, process lifecycle hooks disabled.
#[derive(Debug, Clone)]
pub struct PluginLifecyclePolicy {
    /// Allow observation-only hooks (Event, TextComplete, Config).
    pub enable_observation_hooks: bool,
    /// Allow mutating hooks that can alter inputs (MessagesTransform, ShellEnv, ChatParams).
    pub enable_mutating_hooks: bool,
    /// Allow blocking hooks that can deny operations (ToolExecuteBefore deny/modify).
    pub enable_blocking_hooks: bool,
    /// Allow process-runtime plugins to participate in lifecycle hooks.
    /// Disabled by default because process plugins are local executables and should
    /// not silently intercept core lifecycle paths without explicit trust.
    pub allow_process_lifecycle_hooks: bool,
    /// Fail-open behavior for observation hooks. When true, a failing observation
    /// hook is silently ignored. When false, a failing observation hook propagates
    /// an error.
    pub observation_fail_open: bool,
    /// Fail-open behavior for mutating hooks. When true, a failing mutating hook
    /// falls back to the original input. When false, the mutation is aborted.
    pub mutating_fail_open: bool,
}

impl Default for PluginLifecyclePolicy {
    fn default() -> Self {
        Self {
            enable_observation_hooks: true,
            enable_mutating_hooks: false,
            enable_blocking_hooks: false,
            allow_process_lifecycle_hooks: false,
            observation_fail_open: true,
            mutating_fail_open: true,
        }
    }
}

impl PluginLifecyclePolicy {
    /// Check whether a specific hook type is allowed by this policy.
    pub fn is_hook_allowed(&self, hook_type: HookType) -> bool {
        match hook_type {
            // Observation hooks
            HookType::Event | HookType::TextComplete | HookType::Config => {
                self.enable_observation_hooks
            }
            // Mutating hooks
            HookType::MessagesTransform
            | HookType::ShellEnv
            | HookType::ChatParams
            | HookType::ChatHeaders
            | HookType::Provider => self.enable_mutating_hooks,
            // Blocking hooks
            HookType::ToolExecuteBefore | HookType::Auth => self.enable_blocking_hooks,
            // Post-action observation hooks
            HookType::ToolExecuteAfter | HookType::SessionCompacting => {
                self.enable_observation_hooks
            }
            // Definition hooks (treated as mutating)
            HookType::ToolDefinition => self.enable_mutating_hooks,
        }
    }

    /// Check whether a specific runtime is allowed to participate in lifecycle hooks.
    pub fn is_runtime_allowed(&self, runtime: &PluginRuntimeSpec) -> bool {
        match runtime {
            PluginRuntimeSpec::Builtin { .. } => true,
            PluginRuntimeSpec::Wasm { .. } => true,
            PluginRuntimeSpec::Process { .. } => self.allow_process_lifecycle_hooks,
        }
    }

    /// Check whether a specific trust class is allowed.
    pub fn is_trust_allowed(&self, trust: &PluginTrustClass) -> bool {
        match trust {
            PluginTrustClass::Builtin => true,
            PluginTrustClass::SandboxedWasm => true,
            PluginTrustClass::TrustedLocal => self.allow_process_lifecycle_hooks,
            PluginTrustClass::LocalProcess => self.allow_process_lifecycle_hooks,
        }
    }

    /// Whether observation hook failures should be silently ignored.
    pub fn observation_fail_open(&self) -> bool {
        self.observation_fail_open
    }

    /// Whether mutating hook failures should fall back to original input.
    pub fn mutating_fail_open(&self) -> bool {
        self.mutating_fail_open
    }
}

/// Describes the category of a hook for policy evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookCategory {
    /// Read-only observation; output is ignored or logged.
    Observation,
    /// Can transform input data (messages, env, params).
    Mutating,
    /// Can block or deny operations.
    Blocking,
}

/// Classify a hook type into its policy category.
pub fn classify_hook(hook_type: HookType) -> HookCategory {
    match hook_type {
        HookType::Event | HookType::TextComplete | HookType::Config => HookCategory::Observation,
        HookType::ToolExecuteAfter | HookType::SessionCompacting => HookCategory::Observation,
        HookType::MessagesTransform
        | HookType::ShellEnv
        | HookType::ChatParams
        | HookType::ChatHeaders
        | HookType::Provider
        | HookType::ToolDefinition => HookCategory::Mutating,
        HookType::ToolExecuteBefore | HookType::Auth => HookCategory::Blocking,
    }
}

/// Output surfaces that a plugin is allowed to emit UI effects to.
#[derive(Debug, Clone)]
pub struct PluginUiPolicy {
    /// Allow EmitChat effects. Default: true for commands, false for hooks.
    pub allow_chat: bool,
    /// Allow ShowToast effects. Default: true.
    pub allow_toast: bool,
    /// Allow OpenDialog/CloseDialog effects. Default: true.
    pub allow_dialog: bool,
    /// Allow OpenPanel/UpdatePanel/ClosePanel effects. Default: false.
    pub allow_panel: bool,
    /// Allow AddStatusItem/UpdateStatusItem/RemoveStatusItem effects. Default: false.
    pub allow_status: bool,
}

impl Default for PluginUiPolicy {
    fn default() -> Self {
        Self {
            allow_chat: true,
            allow_toast: true,
            allow_dialog: true,
            allow_panel: false,
            allow_status: false,
        }
    }
}

/// Controls plugin permission evaluation.
#[derive(Debug, Clone)]
pub struct PluginPermissionPolicy {
    /// Deny all secret access by default. Plugins must explicitly declare
    /// and be granted secret access.
    pub deny_secrets_by_default: bool,
    /// Deny environment variable passthrough by default. Process plugins
    /// only receive explicitly configured env entries.
    pub deny_env_passthrough_by_default: bool,
    /// Require explicit high-trust setting for auth/provider hooks.
    pub require_high_trust_for_auth_hooks: bool,
}

impl Default for PluginPermissionPolicy {
    fn default() -> Self {
        Self {
            deny_secrets_by_default: true,
            deny_env_passthrough_by_default: true,
            require_high_trust_for_auth_hooks: true,
        }
    }
}

/// Controls plugin installation and removal safety.
#[derive(Debug, Clone)]
pub struct PluginInstallPolicy {
    /// Reject path traversal in install sources. Default: true.
    pub reject_path_traversal: bool,
    /// Refuse to remove paths outside the plugin install dir. Default: true.
    pub refuse_outside_install_dir: bool,
    /// Warn when WASM module path is outside the plugin install dir. Default: true.
    pub warn_wasm_outside_plugin_dir: bool,
}

impl Default for PluginInstallPolicy {
    fn default() -> Self {
        Self {
            reject_path_traversal: true,
            refuse_outside_install_dir: true,
            warn_wasm_outside_plugin_dir: true,
        }
    }
}

/// Controls runtime-specific enforcement.
#[derive(Debug, Clone)]
pub struct PluginRuntimePolicy {
    /// Deny process lifecycle hooks unless explicitly allowed. Default: true.
    pub deny_process_lifecycle_hooks: bool,
    /// Deny undeclared capabilities. Default: true.
    pub deny_undeclared_capabilities: bool,
    /// Deny unknown output surfaces (degrade instead). Default: true.
    pub deny_unknown_surfaces: bool,
}

impl Default for PluginRuntimePolicy {
    fn default() -> Self {
        Self {
            deny_process_lifecycle_hooks: true,
            deny_undeclared_capabilities: true,
            deny_unknown_surfaces: true,
        }
    }
}

/// Composite plugin policy combining all sub-policies.
///
/// Defaults are conservative: observation hooks allowed, mutating/blocking
/// hooks denied, process lifecycle hooks denied, secrets denied, env
/// passthrough denied, undeclared capabilities denied, and UI effects
/// filtered by declared output surfaces.
#[derive(Debug, Clone, Default)]
pub struct PluginPolicy {
    pub lifecycle: PluginLifecyclePolicy,
    pub ui: PluginUiPolicy,
    pub permissions: PluginPermissionPolicy,
    pub install: PluginInstallPolicy,
    pub runtime: PluginRuntimePolicy,
}

impl PluginPolicy {
    /// Check whether a specific hook type is allowed (delegates to lifecycle policy).
    pub fn is_hook_allowed(&self, hook_type: HookType) -> bool {
        self.lifecycle.is_hook_allowed(hook_type)
    }

    /// Check whether a runtime is allowed for lifecycle hooks.
    pub fn is_runtime_allowed(&self, runtime: &PluginRuntimeSpec) -> bool {
        self.lifecycle.is_runtime_allowed(runtime)
    }

    /// Check whether a trust class is allowed for lifecycle hooks.
    pub fn is_trust_allowed(&self, trust: &PluginTrustClass) -> bool {
        self.lifecycle.is_trust_allowed(trust)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_policy_allows_observation_hooks() {
        let policy = PluginLifecyclePolicy::default();
        assert!(policy.is_hook_allowed(HookType::Event));
        assert!(policy.is_hook_allowed(HookType::TextComplete));
        assert!(policy.is_hook_allowed(HookType::Config));
        assert!(policy.is_hook_allowed(HookType::ToolExecuteAfter));
        assert!(policy.is_hook_allowed(HookType::SessionCompacting));
    }

    #[test]
    fn default_policy_disables_mutating_hooks() {
        let policy = PluginLifecyclePolicy::default();
        assert!(!policy.is_hook_allowed(HookType::MessagesTransform));
        assert!(!policy.is_hook_allowed(HookType::ShellEnv));
        assert!(!policy.is_hook_allowed(HookType::ChatParams));
        assert!(!policy.is_hook_allowed(HookType::ChatHeaders));
        assert!(!policy.is_hook_allowed(HookType::Provider));
        assert!(!policy.is_hook_allowed(HookType::ToolDefinition));
    }

    #[test]
    fn default_policy_disables_blocking_hooks() {
        let policy = PluginLifecyclePolicy::default();
        assert!(!policy.is_hook_allowed(HookType::ToolExecuteBefore));
        assert!(!policy.is_hook_allowed(HookType::Auth));
    }

    #[test]
    fn enabling_mutating_allows_mutating_hooks() {
        let policy = PluginLifecyclePolicy {
            enable_mutating_hooks: true,
            ..Default::default()
        };
        assert!(policy.is_hook_allowed(HookType::MessagesTransform));
        assert!(policy.is_hook_allowed(HookType::ShellEnv));
        assert!(policy.is_hook_allowed(HookType::ToolDefinition));
    }

    #[test]
    fn enabling_blocking_allows_blocking_hooks() {
        let policy = PluginLifecyclePolicy {
            enable_blocking_hooks: true,
            ..Default::default()
        };
        assert!(policy.is_hook_allowed(HookType::ToolExecuteBefore));
        assert!(policy.is_hook_allowed(HookType::Auth));
    }

    #[test]
    fn builtin_runtime_always_allowed() {
        let policy = PluginLifecyclePolicy::default();
        assert!(policy.is_runtime_allowed(&PluginRuntimeSpec::Builtin {
            handler: "test".into()
        }));
    }

    #[test]
    fn wasm_runtime_always_allowed() {
        let policy = PluginLifecyclePolicy::default();
        assert!(policy.is_runtime_allowed(&PluginRuntimeSpec::Wasm {
            module: "test.wasm".into(),
            timeout_ms: None,
            memory_max_mb: None,
            fuel_per_call: None,
        }));
    }

    #[test]
    fn process_runtime_denied_by_default() {
        let policy = PluginLifecyclePolicy::default();
        assert!(!policy.is_runtime_allowed(&PluginRuntimeSpec::Process {
            command: "test".into(),
            args: Vec::new(),
            timeout_ms: None,
        }));
    }

    #[test]
    fn process_runtime_allowed_when_enabled() {
        let policy = PluginLifecyclePolicy {
            allow_process_lifecycle_hooks: true,
            ..Default::default()
        };
        assert!(policy.is_runtime_allowed(&PluginRuntimeSpec::Process {
            command: "test".into(),
            args: Vec::new(),
            timeout_ms: None,
        }));
    }

    #[test]
    fn trust_class_classification() {
        let policy = PluginLifecyclePolicy::default();
        assert!(policy.is_trust_allowed(&PluginTrustClass::Builtin));
        assert!(policy.is_trust_allowed(&PluginTrustClass::SandboxedWasm));
        assert!(!policy.is_trust_allowed(&PluginTrustClass::TrustedLocal));
        assert!(!policy.is_trust_allowed(&PluginTrustClass::LocalProcess));
    }

    #[test]
    fn hook_category_classification() {
        assert_eq!(classify_hook(HookType::Event), HookCategory::Observation);
        assert_eq!(
            classify_hook(HookType::TextComplete),
            HookCategory::Observation
        );
        assert_eq!(classify_hook(HookType::Config), HookCategory::Observation);
        assert_eq!(
            classify_hook(HookType::ToolExecuteAfter),
            HookCategory::Observation
        );
        assert_eq!(
            classify_hook(HookType::SessionCompacting),
            HookCategory::Observation
        );
        assert_eq!(
            classify_hook(HookType::MessagesTransform),
            HookCategory::Mutating
        );
        assert_eq!(classify_hook(HookType::ShellEnv), HookCategory::Mutating);
        assert_eq!(classify_hook(HookType::ChatParams), HookCategory::Mutating);
        assert_eq!(
            classify_hook(HookType::ToolDefinition),
            HookCategory::Mutating
        );
        assert_eq!(
            classify_hook(HookType::ToolExecuteBefore),
            HookCategory::Blocking
        );
        assert_eq!(classify_hook(HookType::Auth), HookCategory::Blocking);
    }

    #[test]
    fn plugin_policy_default_has_conservative_ui() {
        let policy = PluginPolicy::default();
        assert!(policy.ui.allow_chat);
        assert!(policy.ui.allow_toast);
        assert!(policy.ui.allow_dialog);
        assert!(!policy.ui.allow_panel);
        assert!(!policy.ui.allow_status);
    }

    #[test]
    fn plugin_policy_default_denies_secrets() {
        let policy = PluginPolicy::default();
        assert!(policy.permissions.deny_secrets_by_default);
        assert!(policy.permissions.deny_env_passthrough_by_default);
        assert!(policy.permissions.require_high_trust_for_auth_hooks);
    }

    #[test]
    fn plugin_policy_default_enforces_runtime() {
        let policy = PluginPolicy::default();
        assert!(policy.runtime.deny_process_lifecycle_hooks);
        assert!(policy.runtime.deny_undeclared_capabilities);
        assert!(policy.runtime.deny_unknown_surfaces);
    }

    #[test]
    fn plugin_policy_default_rejects_path_traversal() {
        let policy = PluginPolicy::default();
        assert!(policy.install.reject_path_traversal);
        assert!(policy.install.refuse_outside_install_dir);
        assert!(policy.install.warn_wasm_outside_plugin_dir);
    }

    #[test]
    fn plugin_policy_delegates_to_lifecycle() {
        let policy = PluginPolicy::default();
        assert!(policy.is_hook_allowed(HookType::Event));
        assert!(!policy.is_hook_allowed(HookType::ToolExecuteBefore));
        assert!(policy.is_runtime_allowed(&PluginRuntimeSpec::Builtin {
            handler: "x".into()
        }));
        assert!(!policy.is_runtime_allowed(&PluginRuntimeSpec::Process {
            command: "x".into(),
            args: vec![],
            timeout_ms: None
        }));
        assert!(policy.is_trust_allowed(&PluginTrustClass::Builtin));
        assert!(!policy.is_trust_allowed(&PluginTrustClass::LocalProcess));
    }
}
