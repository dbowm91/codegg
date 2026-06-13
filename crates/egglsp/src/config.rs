//! Crate-local `LspConfig` and `LspRule` types.
//!
//! Codegg converts its own config into these at the boundary. The shape
//! mirrors the prior `crate::config::schema::LspConfig` / `LspRule` so
//! that no model-facing tool schema has to change.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
                ..
            } => {
                assert_eq!(command, vec!["rust-analyzer"]);
                assert!(workspace_configuration.is_none());
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
        };
        let serialized = serde_json::to_string(&rule).unwrap();
        let deserialized: LspRule = serde_json::from_str(&serialized).unwrap();
        assert_eq!(rule, deserialized);
    }
}
