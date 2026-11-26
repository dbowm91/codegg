//! The editor's configurable keymap: Helix-style minor modes.
//!
//! The format, the layering, the command names and the shipped defaults
//! are documented in `architecture/tui.md`.

use std::collections::BTreeMap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use codegg::config::editor::{EditorConfigFile, KeyBinding, MenuConfig};

use crate::editor::{EditorCommandName, EditorPrefix};

/// How deep a minor mode may nest.
pub const MAX_MENU_DEPTH: usize = 4;

const DEFAULT_EDITOR_CONFIG: &str = include_str!("../assets/editor.toml");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamedKey {
    Space,
    Esc,
    Ret,
    Tab,
    Backspace,
    Del,
    Ins,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
}

impl NamedKey {
    pub fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "space" => Self::Space,
            "esc" => Self::Esc,
            "ret" => Self::Ret,
            "tab" => Self::Tab,
            "backspace" => Self::Backspace,
            "del" => Self::Del,
            "ins" => Self::Ins,
            "left" => Self::Left,
            "right" => Self::Right,
            "up" => Self::Up,
            "down" => Self::Down,
            "home" => Self::Home,
            "end" => Self::End,
            "pageup" => Self::PageUp,
            "pagedown" => Self::PageDown,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Space => "space",
            Self::Esc => "esc",
            Self::Ret => "ret",
            Self::Tab => "tab",
            Self::Backspace => "backspace",
            Self::Del => "del",
            Self::Ins => "ins",
            Self::Left => "left",
            Self::Right => "right",
            Self::Up => "up",
            Self::Down => "down",
            Self::Home => "home",
            Self::End => "end",
            Self::PageUp => "pageup",
            Self::PageDown => "pagedown",
        }
    }

    pub fn from_code(code: KeyCode) -> Option<Self> {
        Some(match code {
            KeyCode::Char(' ') => Self::Space,
            KeyCode::Esc => Self::Esc,
            KeyCode::Enter => Self::Ret,
            KeyCode::Tab => Self::Tab,
            KeyCode::Backspace => Self::Backspace,
            KeyCode::Delete => Self::Del,
            KeyCode::Insert => Self::Ins,
            KeyCode::Left => Self::Left,
            KeyCode::Right => Self::Right,
            KeyCode::Up => Self::Up,
            KeyCode::Down => Self::Down,
            KeyCode::Home => Self::Home,
            KeyCode::End => Self::End,
            KeyCode::PageUp => Self::PageUp,
            KeyCode::PageDown => Self::PageDown,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers {
    pub control: bool,
    pub alt: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyPattern {
    pub glyph: Option<char>,
    pub named: Option<NamedKey>,
    pub modifiers: Modifiers,
}

impl KeyPattern {
    pub fn parse(text: &str) -> Option<Self> {
        let mut rest = text;
        let mut modifiers = Modifiers::default();
        let mut shift = false;
        while let Some((prefix, tail)) = rest.split_once('-') {
            match prefix {
                "C" => modifiers.control = true,
                "A" => modifiers.alt = true,
                "S" => shift = true,
                _ => return None,
            }
            rest = tail;
            if rest.is_empty() {
                return None;
            }
        }
        if rest.is_empty() {
            return None;
        }
        if let Some(named) = NamedKey::parse(rest) {
            return Some(Self {
                glyph: None,
                named: Some(named),
                modifiers,
            });
        }
        let mut chars = rest.chars();
        let first = chars.next()?;
        if chars.next().is_some() {
            return None;
        }
        Some(Self {
            glyph: Some(if shift {
                first.to_ascii_uppercase()
            } else {
                first
            }),
            named: None,
            modifiers,
        })
    }

    pub fn label(self) -> String {
        let mut out = String::new();
        if self.modifiers.control {
            out.push_str("C-");
        }
        if self.modifiers.alt {
            out.push_str("A-");
        }
        match (self.glyph, self.named) {
            (Some(glyph), _) => out.push(glyph),
            (None, Some(named)) => out.push_str(named.label()),
            (None, None) => {}
        }
        out
    }

    pub fn matches(self, key: KeyEvent) -> bool {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        if control != self.modifiers.control || alt != self.modifiers.alt {
            return false;
        }
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        match (self.glyph, self.named) {
            (Some(glyph), None) => match key.code {
                KeyCode::Char(c) => {
                    let normalized = if shift { c.to_ascii_uppercase() } else { c };
                    normalized == glyph
                }
                _ => false,
            },
            (None, Some(named)) => NamedKey::from_code(key.code) == Some(named),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MenuPath {
    keys: [KeyPattern; MAX_MENU_DEPTH],
    len: u8,
}

impl MenuPath {
    pub fn root() -> Self {
        Self::default()
    }

    pub fn child(self, key: KeyPattern) -> Option<Self> {
        if self.len as usize >= MAX_MENU_DEPTH {
            return None;
        }
        let mut next = self;
        next.keys[next.len as usize] = key;
        next.len += 1;
        Some(next)
    }

    pub fn is_root(self) -> bool {
        self.len == 0
    }

    pub fn id(self) -> String {
        let mut parts = vec!["space".to_string()];
        for key in &self.keys[..self.len as usize] {
            parts.push(key.label());
        }
        parts.join(".")
    }

    pub fn label(self) -> String {
        let mut parts = vec!["space".to_string()];
        for key in &self.keys[..self.len as usize] {
            parts.push(key.label());
        }
        parts.join(" ")
    }

    pub fn title(self) -> String {
        let mut parts = vec!["Space".to_string()];
        for key in &self.keys[..self.len as usize] {
            parts.push(key.label());
        }
        parts.join(" ")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuAction {
    Command(EditorCommandName),
    Menu(MenuPath),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuEntry {
    pub key: KeyPattern,
    pub action: MenuAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MinorMode {
    pub id: String,
    pub entries: Vec<MenuEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorMenuEntry {
    pub key: String,
    pub label: String,
    pub is_menu: bool,
    pub available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorMenuView {
    pub path: String,
    pub entries: Vec<EditorMenuEntry>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditorKeymap {
    menus: BTreeMap<String, MinorMode>,
    completion_popup: Option<bool>,
    pub problems: Vec<String>,
}

impl EditorPrefix {
    /// The minor mode this prefix is waiting in, when it is waiting in one.
    ///
    /// The space menu and its nested menus are one grammar, so the path is
    /// what tells the resolver which menu's entries the next key is
    /// matched against.
    pub fn space_menu_path(self) -> Option<MenuPath> {
        match self {
            EditorPrefix::Space => Some(MenuPath::root()),
            EditorPrefix::SpaceMenu(path) => Some(path),
            _ => None,
        }
    }
}

impl EditorCommandName {
    /// The snake_case name a configuration file binds this command by.
    ///
    /// Helix names its commands in snake_case, so the configuration's
    /// vocabulary is this one and not the colon line's short spellings.
    pub fn config_name(self) -> &'static str {
        match self {
            Self::Write => "write",
            Self::Quit => "quit",
            Self::WriteQuit => "write_quit",
            Self::ForceQuit => "force_quit",
            Self::Reload => "reload",
            Self::WriteAll => "write_all",
            Self::New => "new",
            Self::Open => "open",
            Self::Complete => "complete",
            Self::SplitRegex => "split_regex",
            Self::KeepRegex => "keep_regex",
            Self::RemoveRegex => "remove_regex",
            Self::SelectRegex => "select_regex",
            Self::RenamePreview => "rename_preview",
            Self::GotoLine => "goto_line",
            Self::LlmInsert => "llm_insert",
        }
    }

    /// The command a configuration file's name binds.
    ///
    /// One list, the registry's own: a command that is not in
    /// the registry's ALL list has no name here either.
    pub fn from_config_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|command| command.config_name() == name)
    }
}

impl EditorKeymap {
    pub fn defaults() -> Self {
        let mut keymap = Self::default();
        match codegg::config::editor::parse_editor_config(DEFAULT_EDITOR_CONFIG) {
            Ok(config) => keymap.apply(&config),
            Err(error) => keymap.problems.push(format!(
                "the shipped editor configuration is unreadable: {error}"
            )),
        }
        keymap
    }

    pub fn load(project_root: Option<&std::path::Path>) -> Self {
        let mut keymap = Self::defaults();
        let loaded = codegg::config::editor::load_editor_config(project_root);
        keymap.apply(&loaded.config);
        keymap.problems.extend(loaded.problems);
        keymap
    }

    pub fn apply(&mut self, config: &EditorConfigFile) {
        if let Some(flag) = config.editor.completion_popup {
            self.completion_popup = Some(flag);
        }
        let Some(mode) = config.keys.get("normal") else {
            return;
        };
        for (key_text, binding) in &mode.entries {
            let Some(key) = KeyPattern::parse(key_text) else {
                self.problems
                    .push(format!("`{key_text}` is not a key this editor can press"));
                continue;
            };
            match binding {
                KeyBinding::Command(name) => self.problems.push(format!(
                    "`{key_text}` binds `{name}`, and the editor's own Normal-mode \
                     chords are not configurable yet"
                )),
                KeyBinding::Menu(sub) => {
                    if key.label() != "space" {
                        self.problems.push(format!(
                            "`{key_text}` opens a menu, and only the space menu is \
                             reachable today"
                        ));
                        continue;
                    }
                    self.fill_menu(MenuPath::root(), sub);
                }
            }
        }
    }

    fn fill_menu(&mut self, path: MenuPath, menu: &MenuConfig) {
        let mut resolved: Vec<(KeyPattern, MenuAction)> = Vec::new();
        for (key_text, binding) in &menu.entries {
            let Some(key) = KeyPattern::parse(key_text) else {
                self.problems
                    .push(format!("`{key_text}` is not a key this editor can press"));
                continue;
            };
            match binding {
                KeyBinding::Command(name) => match EditorCommandName::from_config_name(name) {
                    Some(command) => resolved.push((key, MenuAction::Command(command))),
                    None => self
                        .problems
                        .push(format!("`{name}` is not a command this editor knows")),
                },
                KeyBinding::Menu(sub) => {
                    let Some(child) = path.child(key) else {
                        self.problems.push(format!(
                            "`{}` nests deeper than the editor allows",
                            path.label()
                        ));
                        continue;
                    };
                    self.fill_menu(child, sub);
                    resolved.push((key, MenuAction::Menu(child)));
                }
            }
        }
        let id = path.id();
        let mode = self.menus.entry(id.clone()).or_insert_with(|| MinorMode {
            id,
            entries: Vec::new(),
        });
        for (key, action) in resolved {
            match mode
                .entries
                .iter_mut()
                .find(|entry| entry.key.label() == key.label())
            {
                Some(entry) => entry.action = action,
                None => {
                    mode.entries.push(MenuEntry { key, action });
                    mode.entries.sort_by_key(|entry| entry.key.label());
                }
            }
        }
    }

    pub fn menu(&self, id: &str) -> Option<&MinorMode> {
        self.menus.get(id)
    }

    pub fn completion_popup_enabled(&self) -> bool {
        self.completion_popup.unwrap_or(true)
    }

    pub fn menu_view(&self, path: MenuPath) -> Option<EditorMenuView> {
        let mut entries: Vec<EditorMenuEntry> = Vec::new();
        if path.is_root() {
            for leaf in crate::editor::SPACE_LEAVES {
                let Some((_, label)) = crate::editor::SPACE_LEAF_LABELS
                    .iter()
                    .find(|(glyph, _)| *glyph == leaf.glyph)
                else {
                    continue;
                };
                let available = !matches!(
                    leaf.action,
                    crate::editor::PrefixAction::Effect(
                        crate::editor_effect::EditorPendingEffect::Unavailable(_)
                    )
                );
                entries.push(EditorMenuEntry {
                    key: leaf.glyph.to_string(),
                    label: (*label).to_string(),
                    is_menu: false,
                    available,
                });
            }
        }
        if let Some(mode) = self.menus.get(&path.id()) {
            for entry in &mode.entries {
                let key = entry.key.label();
                let is_menu = matches!(entry.action, MenuAction::Menu(_));
                let label = match entry.action {
                    MenuAction::Command(command) => command.help().to_string(),
                    MenuAction::Menu(_) => format!("the {key} menu"),
                };
                match entries.iter_mut().find(|slot| slot.key == key) {
                    Some(slot) => {
                        slot.label = label;
                        slot.is_menu = is_menu;
                        slot.available = true;
                    }
                    None => entries.push(EditorMenuEntry {
                        key,
                        label,
                        is_menu,
                        available: true,
                    }),
                }
            }
        }
        if entries.is_empty() {
            return None;
        }
        Some(EditorMenuView {
            path: path.title(),
            entries,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg::config::editor::parse_editor_config;

    fn key(text: &str) -> KeyPattern {
        KeyPattern::parse(text).expect("a key this editor can press")
    }

    fn space_q() -> MenuPath {
        MenuPath::root().child(key("q")).expect("one level deep")
    }

    #[test]
    fn the_shipped_defaults_bind_the_space_menu_to_llm_insert() {
        let keymap = EditorKeymap::defaults();
        assert!(keymap.problems.is_empty(), "{:?}", keymap.problems);
        let menu = keymap.menu(&space_q().id()).expect("the q menu");
        assert_eq!(menu.entries.len(), 1);
        let entry = menu.entries[0];
        assert_eq!(entry.key.label(), "a");
        assert_eq!(
            entry.action,
            MenuAction::Command(EditorCommandName::LlmInsert)
        );
        assert_eq!(
            keymap.menu(&MenuPath::root().id()).map(|m| m.entries.len()),
            Some(1)
        );
    }

    #[test]
    fn the_defaults_leave_the_completion_popup_on() {
        assert!(EditorKeymap::defaults().completion_popup_enabled());
    }

    #[test]
    fn a_configured_command_binds_by_its_snake_case_name() {
        let config = parse_editor_config("[keys.normal.space.q]\na = \"llm_insert\"\n")
            .expect("a readable configuration");
        let mut keymap = EditorKeymap::defaults();
        keymap.apply(&config);
        let menu = keymap.menu(&space_q().id()).expect("the q menu");
        assert_eq!(
            menu.entries[0].action,
            MenuAction::Command(EditorCommandName::LlmInsert)
        );
    }

    #[test]
    fn an_unknown_command_is_reported_and_not_bound() {
        let config = parse_editor_config("[keys.normal.space.q]\na = \"not_a_command\"\n")
            .expect("readable");
        let mut keymap = EditorKeymap::defaults();
        keymap.apply(&config);
        assert!(keymap
            .problems
            .iter()
            .any(|problem| problem.contains("not_a_command")));
        let menu = keymap.menu(&space_q().id()).expect("the q menu");
        assert_eq!(
            menu.entries.len(),
            1,
            "an unusable binding leaves the shipped default in place"
        );
        assert_eq!(
            menu.entries[0].action,
            MenuAction::Command(EditorCommandName::LlmInsert)
        );
    }

    #[test]
    fn a_menu_that_is_not_the_space_menu_is_reported() {
        let config =
            parse_editor_config("[keys.normal.x]\na = \"llm_insert\"\n").expect("readable");
        let mut keymap = EditorKeymap::defaults();
        keymap.apply(&config);
        assert!(keymap
            .problems
            .iter()
            .any(|problem| problem.contains("only the space menu")));
    }

    #[test]
    fn the_editors_own_normal_chords_are_reported_as_not_configurable() {
        let config = parse_editor_config("[keys.normal]\nx = \"llm_insert\"\n").expect("readable");
        let mut keymap = EditorKeymap::defaults();
        keymap.apply(&config);
        assert!(keymap
            .problems
            .iter()
            .any(|problem| problem.contains("not configurable yet")));
    }

    #[test]
    fn the_completion_popup_can_be_switched_off() {
        let config = parse_editor_config("[editor]\ncompletion-popup = false\n").expect("readable");
        let mut keymap = EditorKeymap::defaults();
        keymap.apply(&config);
        assert!(!keymap.completion_popup_enabled());
    }

    #[test]
    fn key_patterns_parse_helix_spellings() {
        assert_eq!(key("a").glyph, Some('a'));
        assert_eq!(key("A").glyph, Some('A'));
        assert_eq!(key("space").named, Some(NamedKey::Space));
        assert_eq!(key("esc").named, Some(NamedKey::Esc));
        assert!(key("C-x").modifiers.control);
        assert!(key("A-x").modifiers.alt);
        assert_eq!(key("S-x").glyph, Some('X'));
        assert!(KeyPattern::parse("").is_none());
        assert!(KeyPattern::parse("ab").is_none());
        assert!(KeyPattern::parse("nope").is_none());
    }

    #[test]
    fn a_key_pattern_matches_the_key_it_names() {
        let plain = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        assert!(key("a").matches(plain));
        assert!(!key("A").matches(plain));
        assert!(key("A").matches(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::SHIFT)));
        assert!(key("A").matches(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::NONE)));
        assert!(!key("a").matches(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::SHIFT)));
        assert!(key("C-x").matches(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL)));
        assert!(!key("C-x").matches(plain));
        assert!(key("space").matches(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE)));
        assert!(key("esc").matches(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)));
    }

    #[test]
    fn a_project_config_extends_the_shipped_defaults() {
        let root = std::env::temp_dir().join(format!("codegg-keymap-{}", std::process::id()));
        let project = root.join("repo");
        std::fs::create_dir_all(project.join(".codegg")).expect("the project config dir");
        std::fs::write(
            project.join(".codegg").join("editor.toml"),
            "[keys.normal.space.q]\nb = \"llm_insert\"\n",
        )
        .expect("the project config");
        let keymap = EditorKeymap::load(Some(&project));
        std::fs::remove_dir_all(&root).ok();
        let menu = keymap.menu(&space_q().id()).expect("the q menu");
        let keys: Vec<String> = menu.entries.iter().map(|entry| entry.key.label()).collect();
        assert!(
            keys.contains(&"b".to_string()),
            "the project binding is loaded: {keys:?}"
        );
        assert!(
            keys.contains(&"a".to_string()),
            "the shipped default survives: {keys:?}"
        );
    }
    #[test]
    fn the_root_menu_lists_the_built_in_space_leaves() {
        let keymap = EditorKeymap::defaults();
        let menu = keymap.menu_view(MenuPath::root()).expect("the root menu");
        assert!(
            menu.entries.iter().any(|entry| entry.key == "q"),
            "the configured submenu is listed"
        );
        assert!(
            menu.entries.iter().any(|entry| entry.key == "f"),
            "a built-in leaf is listed"
        );
        assert!(
            menu.entries.iter().any(|entry| entry.key == "?"),
            "the palette entry is listed"
        );
    }

    #[test]
    fn a_nested_menu_lists_only_its_own_entries() {
        let keymap = EditorKeymap::defaults();
        let menu = keymap.menu_view(space_q()).expect("the q menu");
        assert_eq!(menu.entries.len(), 1);
        assert_eq!(menu.entries[0].key, "a");
    }

    #[test]
    fn a_menu_path_names_its_menu() {
        let root = MenuPath::root();
        assert_eq!(root.id(), "space");
        assert_eq!(root.label(), "space");
        let nested = root.child(key("q")).expect("one level deep");
        assert_eq!(nested.id(), "space.q");
        assert_eq!(nested.label(), "space q");
        assert!(root
            .child(key("r"))
            .and_then(|p| p.child(key("s")))
            .is_some());
        let mut deep = root;
        for _ in 0..MAX_MENU_DEPTH {
            deep = deep.child(key("z")).expect("room to nest");
        }
        assert!(deep.child(key("z")).is_none());
    }
}
