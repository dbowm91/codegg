//! The editor's command palette: a centred, filterable list of the
//! editor's commands, with the editor's own key bindings beneath them.
//!
//! Helix's palette is a centred box with a filter bar and a match count; this
//! is the same shape for the same reason — the reader wants to *find* a
//! command, and a list they must scroll is a list they cannot search.

use crate::editor::{EditorCommandName, MAX_EDITOR_COMMAND_LINE};

/// The editor's own keys, as the palette lists them.
///
/// The editor's **own** verbs, not the whole application's: the list answers
/// "what can I press *in here*", and a list that also carried the composer's
/// and the panels' keys would answer a different question.
pub const EDITOR_KEYMAP: &[(&str, &str)] = &[
    ("e / b", "word end / start — selects the word"),
    ("w", "next word start"),
    (
        "3w / 5j",
        "a count repeats the motion; 0 alone is line start",
    ),
    ("v", "select mode — movement extends"),
    (
        "x / X / Alt-x",
        "select the line / widen to lines / narrow back",
    ),
    (
        "d / Alt-d / y",
        "delete into the unnamed register / delete without yanking / yank it and the clipboard",
    ),
    ("c / Alt-c", "change — delete the selection, then type"),
    ("_", "trim the selection to whole lines"),
    ("\" a", "select register a — then y / p / P / R"),
    ("J / Alt-J", "join the lines covered"),
    ("> / <", "indent / unindent the covered lines"),
    ("space c / C", "toggle line / block comment"),
    ("u / Ctrl-r", "undo / redo"),
    (". / Alt-.", "repeat the last insert / the last motion"),
    (":", "command line — matches as you type"),
    ("/ / ?", "regex search / the same backwards"),
    ("*", "search for the selected text"),
    ("n / N", "next / previous match"),
    ("Ctrl-d / Ctrl-u", "half a page down / up"),
    ("PgDn / PgUp", "a full page down / up"),
    ("z c / t / b", "view: centre / top / bottom"),
    ("g g / g G", "start / end of the buffer"),
    ("g t / c / b", "top / middle / bottom of screen"),
    ("space q a", "ask the model for an insertion at the cursor"),
    (
        "Ctrl+Alt+u",
        "accept the generated region as one undoable edit",
    ),
    ("Ctrl+Alt+v", "discard the generated region"),
    (
        "[a] [d] [e]",
        "review the generated region: accept, deny, edit",
    ),
    ("space j", "jumplist — back to a visited position"),
    ("space ?", "the command palette — this box"),
    ("Esc", "leave select mode / cancel"),
];

/// One row of the palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteRow {
    Command(EditorCommandName),
    Key {
        keys: &'static str,
        description: &'static str,
    },
}

/// The palette's open state: the filter text and the highlighted row.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditorCommandPalette {
    pub query: String,
    pub selected: usize,
}

impl EditorCommandPalette {
    /// The rows the filter matches, commands first.
    pub fn rows(&self) -> Vec<PaletteRow> {
        let mut rows: Vec<PaletteRow> = self
            .matching_commands()
            .into_iter()
            .map(PaletteRow::Command)
            .collect();
        for (keys, description) in EDITOR_KEYMAP {
            if self.key_matches(keys, description) {
                rows.push(PaletteRow::Key { keys, description });
            }
        }
        rows
    }

    /// The commands the filter matches, by their long name.
    ///
    /// The **name** is the filter's subject, not the `:` spelling.
    fn matching_commands(&self) -> Vec<EditorCommandName> {
        let query = self.query.trim();
        if query.is_empty() {
            return EditorCommandName::ALL.to_vec();
        }
        let mut scored: Vec<(usize, EditorCommandName)> = EditorCommandName::ALL
            .iter()
            .filter_map(|command| {
                let name = command.config_name();
                let score = if name.starts_with(query) {
                    0
                } else {
                    let fuzzy = codegg::util::fuzzy::fuzzy_score(query, name);
                    if fuzzy == 0 {
                        return None;
                    }
                    fuzzy
                };
                Some((score, *command))
            })
            .collect();
        scored.sort_by_key(|(score, command)| (*score, command.config_name()));
        scored.into_iter().map(|(_, command)| command).collect()
    }

    /// How many rows the palette could show without a filter.
    pub fn total(&self) -> usize {
        EditorCommandName::ALL.len() + EDITOR_KEYMAP.len()
    }

    fn key_matches(&self, keys: &str, description: &str) -> bool {
        let query = self.query.trim().to_lowercase();
        if query.is_empty() {
            return true;
        }
        keys.to_lowercase().contains(&query) || description.to_lowercase().contains(&query)
    }

    pub fn push(&mut self, ch: char) -> bool {
        if self.query.chars().count() >= MAX_EDITOR_COMMAND_LINE {
            return false;
        }
        self.query.push(ch);
        self.selected = 0;
        true
    }

    pub fn pop(&mut self) {
        self.query.pop();
        self.selected = 0;
    }

    pub fn move_selection(&mut self, delta: isize) {
        let count = self.rows().len();
        if count == 0 {
            self.selected = 0;
            return;
        }
        let next = if delta.is_negative() {
            self.selected.saturating_sub(delta.unsigned_abs())
        } else {
            self.selected
                .saturating_add(delta.unsigned_abs())
                .min(count - 1)
        };
        self.selected = next;
    }

    pub fn selected_row(&self) -> Option<PaletteRow> {
        self.rows().get(self.selected).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_filter_lists_every_command_and_key() {
        let palette = EditorCommandPalette::default();
        assert_eq!(palette.rows().len(), palette.total());
    }

    #[test]
    fn a_filter_narrows_to_the_matching_command() {
        let mut palette = EditorCommandPalette::default();
        palette.push('w');
        let rows = palette.rows();
        assert!(rows.contains(&PaletteRow::Command(EditorCommandName::Write)));
        assert!(!rows.contains(&PaletteRow::Command(EditorCommandName::Quit)));
    }

    #[test]
    fn a_key_row_matches_its_description_and_not_a_loose_subsequence() {
        let mut palette = EditorCommandPalette::default();
        for ch in ['g', 'o', 't', 'o'] {
            palette.push(ch);
        }
        let rows = palette.rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0], PaletteRow::Command(EditorCommandName::GotoLine));

        let mut palette = EditorCommandPalette::default();
        for ch in ['w', 'o', 'r', 'd'] {
            palette.push(ch);
        }
        assert!(palette.rows().iter().any(|row| matches!(
            row,
            PaletteRow::Key { description, .. } if description.contains("next word start")
        )));
    }

    #[test]
    fn the_palette_selection_stays_inside_its_matching_rows() {
        let mut palette = EditorCommandPalette::default();
        palette.push('w');
        palette.move_selection(50);
        assert_eq!(palette.selected, palette.rows().len() - 1);
    }
}
