//! Crate-local `LspConfig` and `LspRule` types.
//!
//! Codegg converts its own config into these at the boundary. The shape
//! mirrors the prior `crate::config::schema::LspConfig` / `LspRule` so
//! that no model-facing tool schema has to change.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

use crate::compatibility::{LspRestartMode, LspRestartPolicy};

/// Restart mode as serialized in the config layer.
///
/// Maps to [`crate::compatibility::LspRestartMode`].
#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LspRestartModeConfig {
    Disabled,
    OnUnexpectedExit,
}

impl From<&LspRestartModeConfig> for LspRestartMode {
    fn from(config: &LspRestartModeConfig) -> Self {
        match config {
            LspRestartModeConfig::Disabled => LspRestartMode::Disabled,
            LspRestartModeConfig::OnUnexpectedExit => LspRestartMode::OnUnexpectedExit,
        }
    }
}

/// Restart policy as serialized in the config layer.
///
/// Optional fields use defaults from [`LspRestartPolicy::default`].
#[derive(Deserialize, Serialize, Debug, Clone, Default, PartialEq, Eq)]
#[serde(default)]
pub struct LspRestartPolicyConfig {
    /// `"disabled"` or `"on_unexpected_exit"`.
    pub mode: Option<LspRestartModeConfig>,
    /// Max consecutive restart attempts before marking the server failed.
    pub max_attempts: Option<u32>,
    /// Initial backoff in milliseconds.
    pub initial_backoff_ms: Option<u64>,
    /// Maximum backoff in milliseconds.
    pub max_backoff_ms: Option<u64>,
    /// Seconds of health required before resetting the attempt counter.
    pub reset_after_healthy_secs: Option<u64>,
}

impl LspRestartPolicyConfig {
    /// Merge non-None fields from `other` into `self`.
    pub fn merge_with_profile(&mut self, other: &LspRestartPolicyConfig) {
        if other.mode.is_some() {
            self.mode.clone_from(&other.mode);
        }
        if other.max_attempts.is_some() {
            self.max_attempts.clone_from(&other.max_attempts);
        }
        if other.initial_backoff_ms.is_some() {
            self.initial_backoff_ms
                .clone_from(&other.initial_backoff_ms);
        }
        if other.max_backoff_ms.is_some() {
            self.max_backoff_ms.clone_from(&other.max_backoff_ms);
        }
        if other.reset_after_healthy_secs.is_some() {
            self.reset_after_healthy_secs
                .clone_from(&other.reset_after_healthy_secs);
        }
    }

    /// Convert to the domain [`LspRestartPolicy`] using defaults for
    /// missing fields.
    pub fn to_domain(&self) -> LspRestartPolicy {
        LspRestartPolicy {
            mode: self
                .mode
                .as_ref()
                .map(Into::into)
                .unwrap_or(LspRestartMode::Disabled),
            max_attempts: self.max_attempts.unwrap_or(3),
            initial_backoff: Duration::from_millis(self.initial_backoff_ms.unwrap_or(1000)),
            max_backoff: Duration::from_millis(self.max_backoff_ms.unwrap_or(30000)),
            reset_after_healthy: Duration::from_secs(self.reset_after_healthy_secs.unwrap_or(300)),
        }
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub enum LspConfig {
    Disabled(bool),
    Rules(HashMap<String, LspRule>),
}

impl Default for LspConfig {
    fn default() -> Self {
        LspConfig::Rules(HashMap::new())
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
#[serde(untagged)]
pub enum LspRule {
    Disabled {
        disabled: bool,
    },
    Active {
        command: Vec<String>,
        extensions: Option<Vec<String>>,
        disabled: Option<bool>,
        env: Option<HashMap<String, String>>,
        initialization: Option<HashMap<String, serde_json::Value>>,
        workspace_configuration: Option<HashMap<String, serde_json::Value>>,
        restart: Option<LspRestartPolicyConfig>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_active_without_workspace_configuration() {
        let json = r#"{"command": ["rust-analyzer"], "initialization": {"checkOnSave": true}}"#;
        let rule: LspRule = serde_json::from_str(json).unwrap();
        match rule {
            LspRule::Active {
                command,
                workspace_configuration,
                restart,
                ..
            } => {
                assert_eq!(command, vec!["rust-analyzer"]);
                assert!(workspace_configuration.is_none());
                assert!(restart.is_none());
            }
            _ => panic!("expected Active variant"),
        }
    }

    #[test]
    fn deserialize_active_with_workspace_configuration() {
        let json = r#"{
            "command": ["rust-analyzer"],
            "initialization": {"checkOnSave": true},
            "workspace_configuration": {"rust-analyzer": {"checkOnSave": false}}
        }"#;
        let rule: LspRule = serde_json::from_str(json).unwrap();
        match rule {
            LspRule::Active {
                initialization,
                workspace_configuration,
                ..
            } => {
                assert!(initialization.is_some());
                let wc = workspace_configuration.unwrap();
                assert_eq!(wc["rust-analyzer"]["checkOnSave"], false);
            }
            _ => panic!("expected Active variant"),
        }
    }

    #[test]
    fn deserialize_disabled_variant() {
        let json = r#"{"disabled": true}"#;
        let rule: LspRule = serde_json::from_str(json).unwrap();
        assert!(matches!(rule, LspRule::Disabled { disabled: true }));
    }

    #[test]
    fn roundtrip_active_with_workspace_configuration() {
        let mut ws_config = HashMap::new();
        ws_config.insert(
            "pyright".to_string(),
            serde_json::json!({"typeCheckingMode": "basic"}),
        );
        let rule = LspRule::Active {
            command: vec!["pyright-langserver".into(), "--stdio".into()],
            extensions: Some(vec!["py".into()]),
            disabled: None,
            env: None,
            initialization: None,
            workspace_configuration: Some(ws_config),
            restart: None,
        };
        let serialized = serde_json::to_string(&rule).unwrap();
        let deserialized: LspRule = serde_json::from_str(&serialized).unwrap();
        assert_eq!(rule, deserialized);
    }
}
