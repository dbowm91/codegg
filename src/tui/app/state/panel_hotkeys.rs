//! Saved panel hotkeys.
//!
//! A tiny user-saved map from digit to panel type, persisted through
//! `codegg.json`. It exists because the panel list is short but the *switch*
//! is frequent, and the alternative — re-opening a picker — costs two keys
//! every time.
//!
//! ## Why `Ctrl`, not `Cmd`
//!
//! The obvious spelling for this is `Cmd+1`, and it cannot work in a
//! terminal. Crossterm reports no Super/Meta modifier here, `parse_key` only
//! understands CONTROL/SHIFT/ALT, and terminals that do pass a meta byte
//! through disagree about the encoding — the same class of hazard that made
//! every `Ctrl+Shift+X` chord undecodable in this codebase. `Ctrl` is the
//! family that is reliably delivered, so `Ctrl+1…Ctrl+0` is what ships.
//!
//! ## Why this is not in the binding table
//!
//! The bindings map is static and configurable by key name. These slots are
//! *user-assigned at runtime* and stored by digit, so they cannot be
//! `ActionKey`s: a `Ctrl+2` that means "chat" today may mean "terminal" after
//! the user reassigns it, and the table has no way to express that. Keeping
//! them here makes the assignment the user made the only source of truth.

use std::collections::BTreeMap;

use crate::tui::panels::SidePanelId;

/// Digits that can hold a panel. `0` is the tenth slot, after `1`-`9`.
const SLOTS: [char; 10] = ['1', '2', '3', '4', '5', '6', '7', '8', '9', '0'];

/// Which panel each digit switches to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PanelHotkeys {
    slots: BTreeMap<char, SidePanelId>,
}

impl PanelHotkeys {
    /// The panel bound to `digit`, if any.
    pub fn get(&self, digit: char) -> Option<SidePanelId> {
        self.slots.get(&digit).copied()
    }

    /// Bind `digit` to `panel`, replacing whatever was there.
    ///
    /// Reassignment is silent rather than refused: a user who reuses slot 2
    /// for a different panel meant to, and the old binding is exactly the
    /// thing they asked to replace.
    pub fn set(&mut self, digit: char, panel: SidePanelId) {
        self.slots.insert(digit, panel);
    }

    /// Forget `digit`. Returns what it pointed at, for the confirm toast.
    pub fn clear(&mut self, digit: char) -> Option<SidePanelId> {
        self.slots.remove(&digit)
    }

    /// True when nothing is saved.
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Digit bound to `panel`, if any. Used by the manager to show what is
    /// already saved for the highlighted row.
    pub fn digit_for(&self, panel: SidePanelId) -> Option<char> {
        self.slots
            .iter()
            .find(|(_, bound)| **bound == panel)
            .map(|(digit, _)| *digit)
    }

    /// The raw map, for persisting into config.
    pub fn to_map(&self) -> BTreeMap<String, String> {
        self.slots
            .iter()
            .map(|(digit, panel)| (digit.to_string(), panel_key(*panel)))
            .collect()
    }

    /// Rebuild from a config map, skipping anything unrecognised.
    ///
    /// A bad value is dropped rather than fatal: a config typo must not stop
    /// the TUI from starting, and a hotkey is not worth a boot failure.
    pub fn from_map(map: &BTreeMap<String, String>) -> Self {
        let mut slots = BTreeMap::new();
        for (digit, panel) in map {
            let Some(digit) = digit.chars().next().filter(|d| SLOTS.contains(d)) else {
                continue;
            };
            if let Some(panel) = panel_from_key(panel) {
                slots.insert(digit, panel);
            }
        }
        Self { slots }
    }
}

/// Stable config spelling for a panel type.
///
/// A label (`"Chat"`) would drift the moment the title bar is reworded, so
/// the persisted form is the enum name in snake_case — the same vocabulary
/// `SidePanelId` already uses.
fn panel_key(panel: SidePanelId) -> String {
    // Derived from the registry so a new panel is persisted without touching
    // this file; `SidePanelId::config_key` owns the stable spelling.
    panel.config_key().to_string()
}

/// Inverse of [`panel_key`], tolerant of casing and of the plural.
fn panel_from_key(key: &str) -> Option<SidePanelId> {
    let needle = key.trim().to_ascii_lowercase();
    // Legacy spellings kept verbatim: `sessions` has always meant the
    // sidebar panel and still does, so the `/sessions` box panel uses
    // `session_switcher` rather than stealing it.
    match needle.as_str() {
        "sessions" | "session" => return Some(SidePanelId::Session),
        _ => {}
    }
    SidePanelId::ALL
        .iter()
        .copied()
        .find(|panel| panel.config_key() == needle)
}

/// Map a `Ctrl`+digit key event onto its slot, if it is one.
///
/// Returns the *digit*, not the panel: the caller looks the panel up in its
/// own store, so this stays a pure key-shape test with no view of which
/// bindings exist. Separate from the bindings table for the reason in the
/// module docs — these are user-assigned at runtime.
pub fn digit_slot(key: crossterm::event::KeyEvent) -> Option<char> {
    use crossterm::event::{KeyCode, KeyModifiers};
    if key.modifiers != KeyModifiers::CONTROL {
        return None;
    }
    let KeyCode::Char(c) = key.code else {
        return None;
    };
    SLOTS.contains(&c).then_some(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_the_config_spelling() {
        let mut hotkeys = PanelHotkeys::default();
        hotkeys.set('1', SidePanelId::Chat);
        hotkeys.set('0', SidePanelId::Terminal);
        let map = hotkeys.to_map();
        assert_eq!(map.get("1").map(String::as_str), Some("chat"));
        assert_eq!(PanelHotkeys::from_map(&map), hotkeys);
    }

    #[test]
    fn reassigning_a_slot_replaces_rather_than_refuses() {
        let mut hotkeys = PanelHotkeys::default();
        hotkeys.set('2', SidePanelId::Chat);
        hotkeys.set('2', SidePanelId::Help);
        assert_eq!(hotkeys.get('2'), Some(SidePanelId::Help));
        assert_eq!(hotkeys.digit_for(SidePanelId::Chat), None);
    }

    #[test]
    fn an_unknown_panel_name_is_dropped_rather_than_fatal() {
        let map = BTreeMap::from([
            ("1".to_string(), "chat".to_string()),
            ("2".to_string(), "nonsense".to_string()),
            ("x".to_string(), "help".to_string()),
        ]);
        let hotkeys = PanelHotkeys::from_map(&map);
        assert_eq!(hotkeys.get('1'), Some(SidePanelId::Chat));
        assert_eq!(hotkeys.get('2'), None, "unknown panel dropped");
        assert_eq!(hotkeys.get('x'), None, "non-slot key dropped");
    }

    #[test]
    fn zero_is_the_tenth_slot_after_nine() {
        let mut hotkeys = PanelHotkeys::default();
        hotkeys.set('0', SidePanelId::Help);
        assert_eq!(hotkeys.digit_for(SidePanelId::Help), Some('0'));
    }
}
