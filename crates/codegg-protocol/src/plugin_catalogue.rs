//! M007-G (ADR-0057): the daemon's published plugin catalogue.
//!
//! Two ways the daemon's plugin registry reaches a frontend: the
//! **catalogue**, carried on the session projection, and the **manage
//! request**, the four mutating forms served under `plugin.manage`.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PluginCatalogueEntryDto {
    pub plugin_id: String,
    pub name: String,
    pub version: String,
    pub enabled: bool,
    pub trust: String,
    pub runtime_kind: String,
    pub command_count: usize,
    pub hook_count: usize,
    pub diagnostic_count: usize,
    pub has_error: bool,
    pub source_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct PluginCatalogueDto {
    pub entries: Vec<PluginCatalogueEntryDto>,
    pub truncated: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginManageActionDto {
    Enable,
    Disable,
    Remove,
    Install,
}

impl PluginManageActionDto {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Enable => "enable",
            Self::Disable => "disable",
            Self::Remove => "remove",
            Self::Install => "install",
        }
    }

    pub const fn command_id(self) -> &'static str {
        match self {
            Self::Enable => "plugin_enable",
            Self::Disable => "plugin_disable",
            Self::Remove => "plugin_remove",
            Self::Install => "plugin_install",
        }
    }
}

/// One request to change a plugin's state.
///
/// **Session-scoped**, like ADR-0056's `lsp.control`: the session locates the
/// project, and the project holds the `plugin.manage` grant.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginManageRequestDto {
    pub session_id: String,
    pub action: PluginManageActionDto,
    pub target: String,
}
/// The outcome of one plugin-management request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginManageResultDto {
    pub action: PluginManageActionDto,
    pub plugin_id: String,
    pub enabled: Option<bool>,
    pub removed_paths: Vec<String>,
    pub install_path: Option<String>,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_manage_actions_name_their_own_command() {
        assert_eq!(PluginManageActionDto::Enable.command_id(), "plugin_enable");
        assert_eq!(
            PluginManageActionDto::Disable.command_id(),
            "plugin_disable"
        );
        assert_eq!(PluginManageActionDto::Remove.command_id(), "plugin_remove");
        assert_eq!(
            PluginManageActionDto::Install.command_id(),
            "plugin_install"
        );
    }

    #[test]
    fn the_manage_actions_round_trip_through_the_wire() {
        for action in [
            PluginManageActionDto::Enable,
            PluginManageActionDto::Disable,
            PluginManageActionDto::Remove,
            PluginManageActionDto::Install,
        ] {
            let encoded = serde_json::to_string(&action).expect("encode");
            assert_eq!(encoded, format!("\"{}\"", action.as_str()));
            let decoded: PluginManageActionDto = serde_json::from_str(&encoded).expect("decode");
            assert_eq!(decoded, action);
        }
    }
}
