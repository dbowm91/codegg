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
    },
}
