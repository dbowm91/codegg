use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Default)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub homepage: Option<String>,
    pub license: Option<String>,
    #[serde(default)]
    pub hooks: Vec<HookSpec>,
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq)]
pub struct HookSpec {
    #[serde(rename = "type")]
    pub hook_type: String,
    pub priority: Option<i32>,
}
