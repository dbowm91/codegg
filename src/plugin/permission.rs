use crate::plugin::hooks::HookType;
use crate::plugin::manifest::{PluginManifest, PluginOutputSurface, PluginTrustClass};
use crate::plugin::policy::{classify_hook, PluginPolicy};
use crate::protocol::plugin::PluginCapabilityInvocation;
use crate::protocol::ui::UiEffect;

/// Result of a policy evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    /// The action is allowed.
    Allow,
    /// The action is denied with a reason.
    Deny(String),
    /// The action is degraded (e.g., UI effect stripped) with a reason.
    Degrade(String),
}

impl PolicyDecision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, PolicyDecision::Allow)
    }

    pub fn is_denied(&self) -> bool {
        matches!(self, PolicyDecision::Deny(_))
    }

    pub fn reason(&self) -> Option<&str> {
        match self {
            PolicyDecision::Allow => None,
            PolicyDecision::Deny(r) | PolicyDecision::Degrade(r) => Some(r),
        }
    }
}

/// Check whether a plugin command invocation is allowed by policy.
pub fn check_invocation_allowed(
    manifest: &PluginManifest,
    invocation: &PluginCapabilityInvocation,
    trust: &PluginTrustClass,
    policy: &PluginPolicy,
) -> PolicyDecision {
    let plugin_name = &manifest.name;
    let decision = match invocation {
        PluginCapabilityInvocation::Command { name } => {
            let has_command = manifest.capabilities.iter().any(|cap| {
                matches!(cap, crate::plugin::manifest::PluginCapability::Command(cmd) if cmd.name == *name)
            });
            if has_command {
                PolicyDecision::Allow
            } else if policy.runtime.deny_undeclared_capabilities {
                PolicyDecision::Deny(format!("command '{}' not declared in manifest", name))
            } else {
                PolicyDecision::Allow
            }
        }
        PluginCapabilityInvocation::Hook { hook_type } => {
            if let Some(ht) = HookType::parse(hook_type) {
                let category = classify_hook(ht);
                let allowed = policy.is_hook_allowed(ht);

                if !allowed {
                    PolicyDecision::Deny(format!("{:?} hook {:?} denied by policy", category, ht))
                } else if !policy.is_trust_allowed(trust) {
                    PolicyDecision::Deny(format!(
                        "trust class {:?} not allowed for lifecycle hooks",
                        trust
                    ))
                } else if matches!(ht, HookType::Auth | HookType::Provider)
                    && policy.permissions.require_high_trust_for_auth_hooks
                    && !matches!(trust, PluginTrustClass::Builtin)
                {
                    PolicyDecision::Deny(
                        "auth/provider hooks require Builtin trust class".to_string(),
                    )
                } else {
                    PolicyDecision::Allow
                }
            } else {
                PolicyDecision::Deny(format!("unknown hook type: {}", hook_type))
            }
        }
        PluginCapabilityInvocation::Panel { .. }
        | PluginCapabilityInvocation::StatusWidget { .. }
        | PluginCapabilityInvocation::Event { .. } => PolicyDecision::Deny(format!(
            "capability type {:?} is not invocable at runtime",
            std::mem::discriminant(invocation)
        )),
    };

    if !decision.is_allowed() {
        tracing::warn!(
            plugin = plugin_name,
            trust = ?trust,
            decision = %decision.reason().unwrap_or("unknown"),
            "policy denied invocation"
        );
    }
    decision
}

/// Check whether a UI effect is allowed for a plugin with given capabilities.
pub fn check_ui_effect_allowed(
    manifest: &PluginManifest,
    effect: &UiEffect,
    policy: &PluginPolicy,
) -> PolicyDecision {
    let decision = match effect {
        UiEffect::EmitChat { .. } => {
            let has_chat = manifest.capabilities.iter().any(|cap| {
                matches!(cap, crate::plugin::manifest::PluginCapability::Command(cmd)
                    if cmd.output.contains(&PluginOutputSurface::Chat))
            }) || !manifest.hooks.is_empty(); // hooks implicitly allow chat

            if has_chat || policy.ui.allow_chat {
                PolicyDecision::Allow
            } else {
                PolicyDecision::Degrade(
                    "EmitChat denied: plugin does not declare chat output surface".to_string(),
                )
            }
        }
        UiEffect::ShowToast { .. } => {
            if policy.ui.allow_toast {
                PolicyDecision::Allow
            } else {
                PolicyDecision::Degrade("ShowToast denied by policy".to_string())
            }
        }
        UiEffect::OpenDialog { .. } | UiEffect::CloseDialog { .. } => {
            if policy.ui.allow_dialog {
                PolicyDecision::Allow
            } else {
                PolicyDecision::Degrade("Dialog effects denied by policy".to_string())
            }
        }
        UiEffect::OpenPanel { .. } | UiEffect::UpdatePanel { .. } | UiEffect::ClosePanel { .. } => {
            let has_panel = manifest
                .capabilities
                .iter()
                .any(|cap| matches!(cap, crate::plugin::manifest::PluginCapability::Panel(_)));

            if has_panel && policy.ui.allow_panel {
                PolicyDecision::Allow
            } else if !policy.ui.allow_panel {
                PolicyDecision::Degrade("Panel effects denied by policy".to_string())
            } else {
                PolicyDecision::Degrade(
                    "Panel effects denied: plugin does not declare panel capability".to_string(),
                )
            }
        }
        UiEffect::AddStatusItem { .. }
        | UiEffect::UpdateStatusItem { .. }
        | UiEffect::RemoveStatusItem { .. } => {
            let has_status = manifest.capabilities.iter().any(|cap| {
                matches!(
                    cap,
                    crate::plugin::manifest::PluginCapability::StatusWidget(_)
                )
            });

            if has_status && policy.ui.allow_status {
                PolicyDecision::Allow
            } else if !policy.ui.allow_status {
                PolicyDecision::Degrade("Status effects denied by policy".to_string())
            } else {
                PolicyDecision::Degrade(
                    "Status effects denied: plugin does not declare status widget capability"
                        .to_string(),
                )
            }
        }
    };

    if !decision.is_allowed() {
        tracing::debug!(
            plugin = manifest.name,
            effect = ?std::mem::discriminant(effect),
            decision = %decision.reason().unwrap_or("unknown"),
            "policy denied UI effect"
        );
    }
    decision
}

/// Check whether a lifecycle hook is allowed by policy.
pub fn check_lifecycle_hook_allowed(
    hook_type: HookType,
    trust: &PluginTrustClass,
    policy: &PluginPolicy,
) -> PolicyDecision {
    let category = classify_hook(hook_type);

    let decision = if !policy.is_hook_allowed(hook_type) {
        PolicyDecision::Deny(format!(
            "{:?} hook {:?} denied by lifecycle policy",
            category, hook_type
        ))
    } else if !policy.is_trust_allowed(trust) {
        PolicyDecision::Deny(format!(
            "trust class {:?} denied for {:?} hooks",
            trust, category
        ))
    } else if matches!(hook_type, HookType::Auth | HookType::Provider)
        && policy.permissions.require_high_trust_for_auth_hooks
        && !matches!(trust, PluginTrustClass::Builtin)
    {
        PolicyDecision::Deny("auth/provider hooks require Builtin trust class".to_string())
    } else {
        PolicyDecision::Allow
    };

    if !decision.is_allowed() {
        tracing::warn!(
            hook_type = hook_type.as_str(),
            trust = ?trust,
            decision = %decision.reason().unwrap_or("unknown"),
            "policy denied lifecycle hook"
        );
    }
    decision
}

/// Check whether a plugin is allowed to access a secret.
pub fn check_secret_access_allowed(
    manifest: &PluginManifest,
    secret_name: &str,
    policy: &PluginPolicy,
) -> PolicyDecision {
    let decision = if !policy.permissions.deny_secrets_by_default
        || manifest
            .permissions
            .secrets
            .iter()
            .any(|s| s == secret_name)
    {
        PolicyDecision::Allow
    } else {
        PolicyDecision::Deny(format!(
            "secret '{}' not declared in plugin permissions",
            secret_name
        ))
    };

    if !decision.is_allowed() {
        tracing::warn!(
            plugin = manifest.name,
            secret = secret_name,
            decision = %decision.reason().unwrap_or("unknown"),
            "policy denied secret access"
        );
    }
    decision
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::manifest::{
        PluginCapability, PluginCommandSpec, PluginOutputSurface, PluginPermissionSet,
        PluginRuntimeSpec, PluginTrustClass,
    };
    use crate::plugin::policy::PluginPolicy;
    use crate::protocol::plugin::PluginCapabilityInvocation;
    use crate::protocol::ui::{
        ChatBlock, ChatFormat, PanelPlacement, PanelSpec, ToastLevel, ToastSpec, UiEffect, UiNode,
    };

    fn builtin_manifest() -> PluginManifest {
        PluginManifest {
            name: "test-plugin".into(),
            version: "1.0.0".into(),
            api_version: 1,
            runtime: PluginRuntimeSpec::Builtin {
                handler: "test".into(),
            },
            capabilities: vec![PluginCapability::Command(PluginCommandSpec {
                name: "test-cmd".into(),
                description: None,
                handler: None,
                aliases: vec![],
                output: vec![PluginOutputSurface::Chat, PluginOutputSurface::Toast],
            })],
            permissions: PluginPermissionSet::default(),
            description: None,
            author: None,
            homepage: None,
            license: None,
            hooks: vec![],
            config: Default::default(),
        }
    }

    fn process_manifest() -> PluginManifest {
        PluginManifest {
            name: "proc-plugin".into(),
            version: "1.0.0".into(),
            api_version: 1,
            runtime: PluginRuntimeSpec::Process {
                command: "my-cmd".into(),
                args: vec![],
                timeout_ms: None,
            },
            capabilities: vec![],
            permissions: PluginPermissionSet {
                secrets: vec!["MY_SECRET".into()],
                ..Default::default()
            },
            description: None,
            author: None,
            homepage: None,
            license: None,
            hooks: vec![],
            config: Default::default(),
        }
    }

    #[test]
    fn check_invocation_allowed_for_declared_command() {
        let manifest = builtin_manifest();
        let invocation = PluginCapabilityInvocation::Command {
            name: "test-cmd".into(),
        };
        let decision = check_invocation_allowed(
            &manifest,
            &invocation,
            &PluginTrustClass::Builtin,
            &PluginPolicy::default(),
        );
        assert_eq!(decision, PolicyDecision::Allow);
    }

    #[test]
    fn check_invocation_denied_for_undeclared_command() {
        let manifest = builtin_manifest();
        let invocation = PluginCapabilityInvocation::Command {
            name: "unknown-cmd".into(),
        };
        let decision = check_invocation_allowed(
            &manifest,
            &invocation,
            &PluginTrustClass::Builtin,
            &PluginPolicy::default(),
        );
        assert!(decision.is_denied());
        assert!(decision.reason().unwrap().contains("not declared"));
    }

    #[test]
    fn check_invocation_allows_undeclared_when_policy_disabled() {
        let manifest = builtin_manifest();
        let invocation = PluginCapabilityInvocation::Command {
            name: "unknown-cmd".into(),
        };
        let mut policy = PluginPolicy::default();
        policy.runtime.deny_undeclared_capabilities = false;
        let decision =
            check_invocation_allowed(&manifest, &invocation, &PluginTrustClass::Builtin, &policy);
        assert_eq!(decision, PolicyDecision::Allow);
    }

    #[test]
    fn check_ui_effect_chat_allowed() {
        let manifest = builtin_manifest();
        let effect = UiEffect::EmitChat {
            block: ChatBlock {
                content: "hello".into(),
                format: ChatFormat::Plain,
            },
        };
        let decision = check_ui_effect_allowed(&manifest, &effect, &PluginPolicy::default());
        assert_eq!(decision, PolicyDecision::Allow);
    }

    #[test]
    fn check_ui_effect_chat_denied_when_no_surface() {
        let manifest = PluginManifest {
            name: "no-chat".into(),
            version: "1.0.0".into(),
            api_version: 1,
            runtime: PluginRuntimeSpec::Builtin {
                handler: "x".into(),
            },
            capabilities: vec![PluginCapability::Command(PluginCommandSpec {
                name: "cmd".into(),
                description: None,
                handler: None,
                aliases: vec![],
                output: vec![PluginOutputSurface::Toast], // no Chat
            })],
            permissions: PluginPermissionSet::default(),
            description: None,
            author: None,
            homepage: None,
            license: None,
            hooks: vec![],
            config: Default::default(),
        };
        let effect = UiEffect::EmitChat {
            block: ChatBlock {
                content: "hello".into(),
                format: ChatFormat::Plain,
            },
        };
        let mut policy = PluginPolicy::default();
        policy.ui.allow_chat = false;
        let decision = check_ui_effect_allowed(&manifest, &effect, &policy);
        assert!(decision.is_denied() || matches!(decision, PolicyDecision::Degrade(_)));
    }

    #[test]
    fn check_ui_effect_toast_allowed_by_default() {
        let manifest = builtin_manifest();
        let effect = UiEffect::ShowToast {
            toast: ToastSpec {
                level: ToastLevel::Info,
                message: "hi".into(),
            },
        };
        let decision = check_ui_effect_allowed(&manifest, &effect, &PluginPolicy::default());
        assert_eq!(decision, PolicyDecision::Allow);
    }

    #[test]
    fn check_ui_effect_panel_denied_by_default() {
        let manifest = builtin_manifest();
        let effect = UiEffect::OpenPanel {
            panel: PanelSpec {
                id: "p".into(),
                title: "t".into(),
                placement: PanelPlacement::Left,
                body: UiNode::Empty,
            },
        };
        let decision = check_ui_effect_allowed(&manifest, &effect, &PluginPolicy::default());
        assert!(matches!(decision, PolicyDecision::Degrade(_)));
    }

    #[test]
    fn check_ui_effect_panel_allowed_when_declared_and_policy_enabled() {
        let manifest = PluginManifest {
            name: "panel-plugin".into(),
            version: "1.0.0".into(),
            api_version: 1,
            runtime: PluginRuntimeSpec::Builtin {
                handler: "x".into(),
            },
            capabilities: vec![PluginCapability::Panel(
                crate::plugin::manifest::PluginPanelContribution {
                    id: "p".into(),
                    title: "t".into(),
                    placement: "left".into(),
                    handler: None,
                },
            )],
            permissions: PluginPermissionSet::default(),
            description: None,
            author: None,
            homepage: None,
            license: None,
            hooks: vec![],
            config: Default::default(),
        };
        let mut policy = PluginPolicy::default();
        policy.ui.allow_panel = true;
        let effect = UiEffect::OpenPanel {
            panel: PanelSpec {
                id: "p".into(),
                title: "t".into(),
                placement: PanelPlacement::Left,
                body: UiNode::Empty,
            },
        };
        let decision = check_ui_effect_allowed(&manifest, &effect, &policy);
        assert_eq!(decision, PolicyDecision::Allow);
    }

    #[test]
    fn check_lifecycle_hook_observation_allowed() {
        let decision = check_lifecycle_hook_allowed(
            HookType::Event,
            &PluginTrustClass::Builtin,
            &PluginPolicy::default(),
        );
        assert_eq!(decision, PolicyDecision::Allow);
    }

    #[test]
    fn check_lifecycle_hook_blocking_denied() {
        let decision = check_lifecycle_hook_allowed(
            HookType::ToolExecuteBefore,
            &PluginTrustClass::Builtin,
            &PluginPolicy::default(),
        );
        assert!(decision.is_denied());
    }

    #[test]
    fn check_lifecycle_hook_mutating_denied() {
        let decision = check_lifecycle_hook_allowed(
            HookType::ShellEnv,
            &PluginTrustClass::Builtin,
            &PluginPolicy::default(),
        );
        assert!(decision.is_denied());
    }

    #[test]
    fn check_lifecycle_hook_process_denied() {
        let decision = check_lifecycle_hook_allowed(
            HookType::Event,
            &PluginTrustClass::LocalProcess,
            &PluginPolicy::default(),
        );
        assert!(decision.is_denied());
    }

    #[test]
    fn check_lifecycle_hook_process_allowed_when_enabled() {
        let mut policy = PluginPolicy::default();
        policy.lifecycle.allow_process_lifecycle_hooks = true;
        let decision =
            check_lifecycle_hook_allowed(HookType::Event, &PluginTrustClass::LocalProcess, &policy);
        assert_eq!(decision, PolicyDecision::Allow);
    }

    #[test]
    fn check_lifecycle_auth_requires_builtin_trust() {
        let mut policy = PluginPolicy::default();
        policy.lifecycle.enable_blocking_hooks = true;
        let decision =
            check_lifecycle_hook_allowed(HookType::Auth, &PluginTrustClass::SandboxedWasm, &policy);
        assert!(decision.is_denied());
        assert!(decision.reason().unwrap().contains("Builtin trust"));
    }

    #[test]
    fn check_lifecycle_auth_allowed_for_builtin() {
        let mut policy = PluginPolicy::default();
        policy.lifecycle.enable_blocking_hooks = true;
        let decision =
            check_lifecycle_hook_allowed(HookType::Auth, &PluginTrustClass::Builtin, &policy);
        assert_eq!(decision, PolicyDecision::Allow);
    }

    #[test]
    fn check_secret_access_denied_without_declaration() {
        let manifest = process_manifest();
        let decision =
            check_secret_access_allowed(&manifest, "UNDECLARED", &PluginPolicy::default());
        assert!(decision.is_denied());
    }

    #[test]
    fn check_secret_access_allowed_when_declared() {
        let manifest = process_manifest();
        let decision =
            check_secret_access_allowed(&manifest, "MY_SECRET", &PluginPolicy::default());
        assert_eq!(decision, PolicyDecision::Allow);
    }

    #[test]
    fn check_secret_access_allowed_when_policy_disabled() {
        let manifest = process_manifest();
        let mut policy = PluginPolicy::default();
        policy.permissions.deny_secrets_by_default = false;
        let decision = check_secret_access_allowed(&manifest, "ANYTHING", &policy);
        assert_eq!(decision, PolicyDecision::Allow);
    }

    #[test]
    fn policy_decision_reason_for_deny() {
        let d = PolicyDecision::Deny("test".into());
        assert_eq!(d.reason(), Some("test"));
    }

    #[test]
    fn policy_decision_reason_for_allow() {
        let d = PolicyDecision::Allow;
        assert_eq!(d.reason(), None);
    }

    #[test]
    fn policy_decision_is_denied() {
        assert!(PolicyDecision::Deny("x".into()).is_denied());
        assert!(!PolicyDecision::Allow.is_denied());
        assert!(!PolicyDecision::Degrade("x".into()).is_denied());
    }
}
