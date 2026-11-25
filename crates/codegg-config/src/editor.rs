use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::error::ConfigError;

pub use crate::paths::{find_project_editor_config_from, global_editor_config_path};

/// One minor mode's entries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MenuConfig {
    pub entries: BTreeMap<String, KeyBinding>,
}

impl MenuConfig {
    pub fn merge(&mut self, other: &MenuConfig) {
        for (key, binding) in &other.entries {
            match self.entries.entry(key.clone()) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(binding.clone());
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => {
                    match (entry.get_mut(), binding) {
                        (KeyBinding::Menu(existing), KeyBinding::Menu(incoming)) => {
                            existing.merge(incoming);
                        }
                        (_, replacement) => {
                            entry.insert(replacement.clone());
                        }
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyBinding {
    Command(String),
    Menu(MenuConfig),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditorSettings {
    pub completion_popup: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditorConfigFile {
    pub keys: BTreeMap<String, MenuConfig>,
    pub editor: EditorSettings,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoadedEditorConfig {
    pub config: EditorConfigFile,
    pub problems: Vec<String>,
}

/// Parse one editor configuration file.
pub fn parse_editor_config(content: &str) -> Result<EditorConfigFile, ConfigError> {
    let value = toml::from_str::<toml::Value>(content)
        .map_err(|error| ConfigError::Parse(error.to_string()))?;
    let root = value
        .as_table()
        .ok_or_else(|| ConfigError::Parse("editor configuration must be a TOML table".into()))?;

    let mut config = EditorConfigFile::default();
    if let Some(keys) = root.get("keys") {
        let modes = keys
            .as_table()
            .ok_or_else(|| ConfigError::Parse("keys must be a TOML table".into()))?;
        for (mode, menu) in modes {
            config
                .keys
                .insert(mode.clone(), parse_menu(menu, &format!("keys.{mode}"))?);
        }
    }

    if let Some(editor) = root.get("editor") {
        let settings: EditorSettingsFile =
            editor
                .clone()
                .try_into()
                .map_err(|error: toml::de::Error| {
                    ConfigError::Parse(format!("invalid editor section: {error}"))
                })?;
        config.editor.completion_popup = settings.completion_popup;
    }

    Ok(config)
}

fn parse_menu(value: &toml::Value, location: &str) -> Result<MenuConfig, ConfigError> {
    let table = value
        .as_table()
        .ok_or_else(|| ConfigError::Parse(format!("{location} must be a TOML table")))?;
    let mut menu = MenuConfig::default();

    for (key, value) in table {
        let binding = match value {
            toml::Value::String(command) => KeyBinding::Command(command.clone()),
            toml::Value::Table(_) => {
                KeyBinding::Menu(parse_menu(value, &format!("{location}.{key}"))?)
            }
            toml::Value::Datetime(_) => {
                return Err(ConfigError::Parse(format!(
                    "{location}.{key} is a datetime, not a key binding"
                )));
            }
            _ => {
                return Err(ConfigError::Parse(format!(
                    "{location}.{key} must be a string command or nested table"
                )));
            }
        };
        menu.entries.insert(key.clone(), binding);
    }

    Ok(menu)
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
struct EditorSettingsFile {
    #[serde(alias = "completion_popup")]
    completion_popup: Option<bool>,
}

pub fn load_editor_config(project_root: Option<&Path>) -> LoadedEditorConfig {
    let mut loaded = LoadedEditorConfig::default();

    if let Some(path) = global_editor_config_path().filter(|path| path.exists()) {
        load_layer(&path, &mut loaded);
    }
    if let Some(path) = project_root.and_then(find_project_editor_config_from) {
        load_layer(&path, &mut loaded);
    }

    loaded
}

fn load_layer(path: &Path, loaded: &mut LoadedEditorConfig) {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) => {
            loaded.problems.push(format!("{}: {error}", path.display()));
            return;
        }
    };

    match parse_editor_config(&content) {
        Ok(config) => merge_config(&mut loaded.config, config),
        Err(error) => loaded.problems.push(format!("{}: {error}", path.display())),
    }
}

fn merge_config(base: &mut EditorConfigFile, overlay: EditorConfigFile) {
    for (mode, menu) in overlay.keys {
        base.keys.entry(mode).or_default().merge(&menu);
    }
    if overlay.editor.completion_popup.is_some() {
        base.editor.completion_popup = overlay.editor.completion_popup;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::MutexGuard;

    fn expected_nested_menu() -> EditorConfigFile {
        let mut leaf = MenuConfig::default();
        leaf.entries
            .insert("a".into(), KeyBinding::Command("llm_insert".into()));
        let mut inner = MenuConfig::default();
        inner.entries.insert("q".into(), KeyBinding::Menu(leaf));
        let mut normal = MenuConfig::default();
        normal
            .entries
            .insert("space".into(), KeyBinding::Menu(inner));
        EditorConfigFile {
            keys: BTreeMap::from([("normal".into(), normal)]),
            ..Default::default()
        }
    }

    #[test]
    fn parses_nested_table_keymap() {
        let parsed = parse_editor_config(
            r#"
                [keys.normal.space.q]
                a = "llm_insert"
            "#,
        )
        .expect("valid nested keymap");

        assert_eq!(parsed, expected_nested_menu());
    }

    #[test]
    fn inline_and_nested_table_keymaps_are_equivalent() {
        let table_form = parse_editor_config(
            r#"
                [keys.normal.space.q]
                a = "llm_insert"
            "#,
        )
        .expect("valid nested table keymap");
        let inline_form = parse_editor_config(
            r#"
                [keys.normal.space]
                q = { a = "llm_insert" }
            "#,
        )
        .expect("valid inline keymap");

        assert_eq!(inline_form, table_form);
    }

    #[test]
    fn parses_plain_command_entries_and_unrecognized_modes() {
        let parsed = parse_editor_config(
            r#"
                [keys.custom]
                x = "noop"
            "#,
        )
        .expect("valid command binding");

        assert_eq!(
            parsed.keys["custom"].entries["x"],
            KeyBinding::Command("noop".into())
        );
    }

    #[test]
    fn a_project_editor_config_is_found_and_loaded() {
        let root = tempfile::tempdir().expect("tempdir");
        let project = root.path().join("repo");
        std::fs::create_dir_all(project.join(".codegg")).expect("the project config dir");
        std::fs::write(
            project.join(".codegg").join("editor.toml"),
            "[keys.normal.space.q]\nb = \"llm_insert\"\n",
        )
        .expect("the project config");

        assert_eq!(
            find_project_editor_config_from(&project),
            Some(project.join(".codegg").join("editor.toml"))
        );

        let loaded = load_editor_config(Some(&project));
        assert!(loaded.problems.is_empty(), "{:?}", loaded.problems);
        let menu = loaded.config.keys["normal"].entries["space"].clone();
        assert!(matches!(menu, KeyBinding::Menu(_)));
        let inner = match menu {
            KeyBinding::Menu(inner) => inner,
            other => panic!("the space menu is a menu, not {other:?}"),
        };
        let leaf = match inner.entries["q"].clone() {
            KeyBinding::Menu(leaf) => leaf,
            other => panic!("the q menu is a menu, not {other:?}"),
        };
        assert_eq!(leaf.entries["b"], KeyBinding::Command("llm_insert".into()));
    }

    #[test]
    fn a_global_editor_config_is_found_and_loaded() {
        let guard = crate::ENV_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let root = tempfile::tempdir().expect("tempdir");
        let isolated = root.path().join("config");
        let previous = (
            std::env::var_os("HOME"),
            std::env::var_os("XDG_CONFIG_HOME"),
            std::env::var_os("APPDATA"),
        );
        std::env::set_var("HOME", &isolated);
        std::env::set_var("XDG_CONFIG_HOME", &isolated);
        std::env::set_var("APPDATA", &isolated);
        let content = "[keys.normal.space.q]\ng = \"llm_insert\"\n";
        let mut bases = vec![isolated.clone()];
        if let Some(resolved) = dirs::config_dir() {
            bases.push(resolved);
        }
        for base in bases {
            let config_dir = base.join("codegg");
            std::fs::create_dir_all(&config_dir).expect("the global config dir");
            std::fs::write(config_dir.join("editor.toml"), content).expect("the global config");
        }
        let loaded = load_editor_config(None);
        for (name, value) in [
            ("HOME", previous.0),
            ("XDG_CONFIG_HOME", previous.1),
            ("APPDATA", previous.2),
        ] {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
        drop(guard);
        assert!(loaded.problems.is_empty(), "{:?}", loaded.problems);
        let KeyBinding::Menu(space) = loaded.config.keys["normal"].entries["space"].clone() else {
            panic!("the space menu is a menu");
        };
        let KeyBinding::Menu(leaf) = space.entries["q"].clone() else {
            panic!("the q menu is a menu");
        };
        assert_eq!(leaf.entries["g"], KeyBinding::Command("llm_insert".into()));
    }
    #[test]
    fn parses_completion_popup_values() {
        let disabled = parse_editor_config(
            "[editor]
completion-popup = false",
        )
        .unwrap();
        let enabled = parse_editor_config(
            "[editor]
completion-popup = true",
        )
        .unwrap();

        assert_eq!(disabled.editor.completion_popup, Some(false));
        assert_eq!(enabled.editor.completion_popup, Some(true));
    }

    #[test]
    fn ignores_unknown_editor_settings() {
        let parsed = parse_editor_config(
            "[editor]
unknown-setting = 'value'",
        )
        .unwrap();

        assert_eq!(parsed.editor, EditorSettings::default());
    }

    #[test]
    fn invalid_toml_returns_an_error() {
        assert!(parse_editor_config(
            "[keys.normal
invalid = "
        )
        .is_err());
    }

    #[test]
    fn menu_merge_replaces_entries_and_recursively_merges_menus() {
        let mut lower_leaf = MenuConfig::default();
        lower_leaf
            .entries
            .insert("a".into(), KeyBinding::Command("old".into()));
        lower_leaf
            .entries
            .insert("b".into(), KeyBinding::Command("keep".into()));
        let mut lower = MenuConfig::default();
        lower
            .entries
            .insert("q".into(), KeyBinding::Menu(lower_leaf));
        lower
            .entries
            .insert("x".into(), KeyBinding::Command("old".into()));

        let mut upper_leaf = MenuConfig::default();
        upper_leaf
            .entries
            .insert("a".into(), KeyBinding::Command("new".into()));
        let mut upper = MenuConfig::default();
        upper
            .entries
            .insert("q".into(), KeyBinding::Menu(upper_leaf));
        upper
            .entries
            .insert("x".into(), KeyBinding::Command("replacement".into()));

        lower.merge(&upper);

        let KeyBinding::Menu(leaf) = &lower.entries["q"] else {
            panic!("nested menu should remain a menu");
        };
        assert_eq!(leaf.entries["a"], KeyBinding::Command("new".into()));
        assert_eq!(leaf.entries["b"], KeyBinding::Command("keep".into()));
        assert_eq!(
            lower.entries["x"],
            KeyBinding::Command("replacement".into())
        );
    }

    #[test]
    fn no_editor_files_produces_empty_config_without_problems() {
        let _guard: MutexGuard<'_, ()> = crate::ENV_LOCK.lock().unwrap();
        let root = tempfile::tempdir().expect("temporary project root");
        let isolated_config = tempfile::tempdir().expect("temporary user config root");
        let old_home = std::env::var_os("HOME");
        let old_xdg = std::env::var_os("XDG_CONFIG_HOME");
        let old_appdata = std::env::var_os("APPDATA");
        std::env::set_var("HOME", isolated_config.path());
        std::env::set_var("XDG_CONFIG_HOME", isolated_config.path());
        std::env::set_var("APPDATA", isolated_config.path());

        let loaded = load_editor_config(Some(root.path()));

        if let Some(value) = old_home {
            std::env::set_var("HOME", value);
        } else {
            std::env::remove_var("HOME");
        }
        if let Some(value) = old_xdg {
            std::env::set_var("XDG_CONFIG_HOME", value);
        } else {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
        if let Some(value) = old_appdata {
            std::env::set_var("APPDATA", value);
        } else {
            std::env::remove_var("APPDATA");
        }

        assert_eq!(loaded, LoadedEditorConfig::default());
    }
}
