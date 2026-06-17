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

    /// Pass 8 — Validate and convert to the domain
    /// [`LspRestartPolicy`]. Returns a `LspError::InvalidConfig`
    /// on any of the following conditions:
    ///
    /// - mode is `OnUnexpectedExit` AND `max_attempts == 0`
    /// - `initial_backoff_ms > max_backoff_ms`
    /// - any duration overflows `Duration::MAX`
    pub fn try_to_domain(
        &self,
        base: &LspRestartPolicy,
    ) -> Result<LspRestartPolicy, crate::error::LspError> {
        use crate::error::LspError;
        let mode = self
            .mode
            .as_ref()
            .map(Into::into)
            .unwrap_or(base.mode.clone());
        let max_attempts = self.max_attempts.unwrap_or(base.max_attempts);
        // Initial backoff must be <= max backoff when the
        // policy is enabled. `0` is valid for both.
        let initial_backoff_ms = self
            .initial_backoff_ms
            .unwrap_or(base.initial_backoff.as_millis() as u64);
        let max_backoff_ms = self
            .max_backoff_ms
            .unwrap_or(base.max_backoff.as_millis() as u64);
        let reset_after_healthy_secs = self
            .reset_after_healthy_secs
            .unwrap_or(base.reset_after_healthy.as_secs());

        if matches!(mode, LspRestartMode::OnUnexpectedExit) && max_attempts == 0 {
            return Err(LspError::InvalidConfig(
                "restart mode OnUnexpectedExit requires max_attempts > 0".to_string(),
            ));
        }
        if initial_backoff_ms > max_backoff_ms {
            return Err(LspError::InvalidConfig(format!(
                "initial_backoff_ms ({initial_backoff_ms}) must be <= max_backoff_ms ({max_backoff_ms})"
            )));
        }
        let initial_backoff = Duration::from_millis(initial_backoff_ms);
        let max_backoff = Duration::from_millis(max_backoff_ms);
        let reset_after_healthy = Duration::from_secs(reset_after_healthy_secs);
        // Overflow guard: Duration::from_millis / from_secs
        // can overflow for very large inputs. We catch
        // explicit overflow cases (the conversion panics
        // otherwise).
        if initial_backoff.as_millis() as u64 != initial_backoff_ms
            || max_backoff.as_millis() as u64 != max_backoff_ms
            || reset_after_healthy.as_secs() != reset_after_healthy_secs
        {
            return Err(LspError::InvalidConfig(
                "duration value overflows Duration::MAX".to_string(),
            ));
        }
        Ok(LspRestartPolicy {
            mode,
            max_attempts,
            initial_backoff,
            max_backoff,
            reset_after_healthy,
        })
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

    /// Pass 8 — \`try_to_domain\` rejects a config with
    /// \`mode = OnUnexpectedExit\` and \`max_attempts = 0\`.
    /// The combination is meaningless: an enabled mode with
    /// zero retries does nothing.
    #[test]
    fn try_to_domain_rejects_enabled_with_zero_max_attempts() {
        let base = LspRestartPolicy::default();
        let cfg = LspRestartPolicyConfig {
            mode: Some(LspRestartModeConfig::OnUnexpectedExit),
            max_attempts: Some(0),
            initial_backoff_ms: None,
            max_backoff_ms: None,
            reset_after_healthy_secs: None,
        };
        let err = cfg.try_to_domain(&base).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("max_attempts"),
            "expected max_attempts in error, got: {msg}"
        );
    }

    /// Pass 8 — \`try_to_domain\` rejects a config where
    /// \`initial_backoff_ms > max_backoff_ms\`.
    #[test]
    fn try_to_domain_rejects_initial_greater_than_max() {
        let base = LspRestartPolicy::default();
        let cfg = LspRestartPolicyConfig {
            mode: Some(LspRestartModeConfig::OnUnexpectedExit),
            max_attempts: Some(3),
            initial_backoff_ms: Some(5000),
            max_backoff_ms: Some(1000),
            reset_after_healthy_secs: None,
        };
        let err = cfg.try_to_domain(&base).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("initial_backoff_ms"),
            "expected initial_backoff_ms in error, got: {msg}"
        );
    }

    /// Pass 8 — \`try_to_domain\` accepts a valid config and
    /// returns a domain policy.
    #[test]
    fn try_to_domain_accepts_valid_config() {
        let base = LspRestartPolicy::default();
        let cfg = LspRestartPolicyConfig {
            mode: Some(LspRestartModeConfig::OnUnexpectedExit),
            max_attempts: Some(5),
            initial_backoff_ms: Some(500),
            max_backoff_ms: Some(2000),
            reset_after_healthy_secs: Some(120),
        };
        let policy = cfg.try_to_domain(&base).expect("valid config");
        assert_eq!(policy.max_attempts, 5);
        assert_eq!(policy.initial_backoff, Duration::from_millis(500));
        assert_eq!(policy.max_backoff, Duration::from_millis(2000));
        assert_eq!(policy.reset_after_healthy, Duration::from_secs(120));
    }

    /// Pass 8 — `try_to_domain` falls back to the base
    /// policy for missing fields.
    #[test]
    fn try_to_domain_falls_back_to_base() {
        let base = LspRestartPolicy {
            max_attempts: 7,
            ..LspRestartPolicy::default()
        };
        let cfg = LspRestartPolicyConfig::default();
        let policy = cfg.try_to_domain(&base).expect("empty config");
        assert_eq!(policy.max_attempts, 7);
    }

    /// Pass 8 — cold start and restart use the same
    /// descriptor. Two calls to \`from_profile\` with the
    /// same inputs produce equal descriptors.
    #[test]
    fn cold_start_and_restart_receive_identical_descriptor() {
        use crate::{LspClientDescriptor, LspLaunchSpec};
        use std::path::PathBuf;
        let launch_spec = LspLaunchSpec::default_for_test();
        let d1 = LspClientDescriptor::from_profile(
            "k".to_string(),
            "rust-analyzer",
            PathBuf::from("/tmp"),
            launch_spec.clone(),
            Some(PathBuf::from("/tmp/src/lib.rs")),
            None,
            None,
        );
        let d2 = LspClientDescriptor::from_profile(
            "k".to_string(),
            "rust-analyzer",
            PathBuf::from("/tmp"),
            launch_spec,
            Some(PathBuf::from("/tmp/src/lib.rs")),
            None,
            None,
        );
        assert_eq!(d1.initialization_options, d2.initialization_options);
        assert_eq!(d1.workspace_configuration, d2.workspace_configuration);
        assert_eq!(d1.readiness_policy, d2.readiness_policy);
        assert_eq!(d1.restart_policy, d2.restart_policy);
    }
}
