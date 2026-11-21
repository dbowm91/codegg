//! One hint vocabulary: `(key, verb)` items joined by ` · `.
//!
//! The builder owns the separator, the casing rule and the whole-part
//! truncation, so a surface spells items and nothing else. A hint describes a
//! key; it never decides which stage claims a chord, which is ADR-0042's.
//!
//! ## The items are the advertisement, the string is one rendering of it
//!
//! [`HintLine::items`] exposes the `(key, verb)` pairs this row was built from.
//! They are the structured form the I4 invariant measures: it presses each
//! item's key. The rendered string is what a surface paints, and it is derived
//! from the same items, so the two cannot describe different advertisements.

/// The one item separator: space, U+00B7, space.
pub const HINT_SEPARATOR: &str = " · ";

/// One `(key, verb)` pair of a [`HintLine`].
///
/// A hint describes a key; it never says which stage owns it (ADR-0042), so
/// this type carries no claim and no `handles_key`-shaped question can be asked
/// of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HintItem {
    key: String,
    verb: String,
    required: bool,
}

/// An ordered row of hint items, rendered through one vocabulary.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HintLine {
    items: Vec<HintItem>,
}

impl HintLine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a row from `(key, verb)` pairs, in the order given.
    ///
    /// For a surface whose advertisement is a table rather than a builder
    /// chain — a footer written as one literal. No item is required: a footer
    /// is painted whole, and `render(0)` never truncates it.
    pub fn from_items(items: &[(&str, &str)]) -> Self {
        let mut line = Self::new();
        for (key, verb) in items {
            line.push(key, verb);
        }
        line
    }

    /// The items, in the order [`HintLine::render`] paints them.
    ///
    /// Required items first, then the rest in push order. That is the order a
    /// reader sees, so a measurement that walks `items()` walks the row.
    pub fn items(&self) -> impl Iterator<Item = &HintItem> {
        self.items
            .iter()
            .filter(|item| item.required)
            .chain(self.items.iter().filter(|item| !item.required))
    }

    /// Add an item. The verb is lowercased here; an empty verb renders the
    /// key alone, which is how a row carries a value rather than a verb.
    pub fn push(&mut self, key: &str, verb: &str) -> &mut Self {
        self.items.push(HintItem {
            key: key.to_string(),
            verb: verb.to_lowercase(),
            required: false,
        });
        self
    }

    /// Add the item that must survive truncation. Required items render first.
    pub fn push_required(&mut self, key: &str, verb: &str) -> &mut Self {
        self.push(key, verb);
        if let Some(last) = self.items.last_mut() {
            last.required = true;
        }
        self
    }

    pub fn push_line(&mut self, line: &HintLine) -> &mut Self {
        self.items.extend(line.items().cloned());
        self
    }

    /// Render into at most `budget` display columns; `0` is unbounded.
    pub fn render(&self, budget: usize) -> String {
        let ordered: Vec<String> = self.items().map(HintItem::text).collect();
        let parts: Vec<&str> = ordered.iter().map(String::as_str).collect();
        fit_parts(&parts, budget)
    }
}

impl HintItem {
    /// The key spelling, exactly as the surface wrote it.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The verb, lowercased by [`HintLine::push`].
    ///
    /// Empty when the item names a value rather than a verb — the truncation
    /// rule then renders the key alone.
    pub fn verb(&self) -> &str {
        &self.verb
    }

    /// Whether this item survives truncation before the others.
    pub fn is_required(&self) -> bool {
        self.required
    }

    fn text(&self) -> String {
        if self.verb.is_empty() {
            self.key.clone()
        } else {
            format!("{} {}", self.key, self.verb)
        }
    }
}

/// Join `parts` with the one separator, dropping whole parts that do not fit.
///
/// The **first** part is required: a budget that shows any part shows it, and a
/// budget too small for it renders an empty row rather than a partial key. A
/// half-key names a key that does not exist, so whole parts are the only unit
/// this function may drop.
pub fn fit_parts(parts: &[&str], budget: usize) -> String {
    use unicode_width::UnicodeWidthStr;
    let Some(first) = parts.first() else {
        return String::new();
    };
    let mut used = UnicodeWidthStr::width(*first);
    if budget > 0 && used > budget {
        return String::new();
    }
    let mut out = String::from(*first);
    let separator_width = UnicodeWidthStr::width(HINT_SEPARATOR);
    for part in &parts[1..] {
        let part_width = UnicodeWidthStr::width(*part);
        let extra = separator_width + part_width;
        if budget > 0 && used + extra > budget {
            break;
        }
        out.push_str(HINT_SEPARATOR);
        out.push_str(part);
        used += extra;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn composer_row() -> HintLine {
        let mut line = HintLine::new();
        line.push("type", "");
        line.push_required("Esc", "Normal");
        line
    }

    #[test]
    fn the_builder_owns_the_separator_and_the_casing() {
        let mut line = HintLine::new();
        line.push("j/k", "Move")
            .push("Enter", "SELECT")
            .push("Esc", "close");
        assert_eq!(HINT_SEPARATOR, " · ");
        assert_eq!(line.render(0), "j/k move · Enter select · Esc close");
    }

    #[test]
    fn a_required_item_renders_before_an_item_pushed_before_it() {
        assert_eq!(composer_row().render(0), "Esc normal · type");
    }

    #[test]
    fn an_empty_verb_renders_the_key_alone() {
        let mut line = HintLine::new();
        line.push("30s/5min/2h/1d", "").push("Esc", "back");
        assert_eq!(line.render(0), "30s/5min/2h/1d · Esc back");
    }

    #[test]
    fn a_budget_that_shows_any_part_shows_the_required_one() {
        let row = composer_row();
        for budget in [0, 1, 5, 12, 20, 80] {
            let rendered = row.render(budget);
            assert!(
                rendered.is_empty() || rendered.starts_with("Esc normal"),
                "budget {budget} dropped the required part: {rendered:?}"
            );
        }
    }

    #[test]
    fn a_budget_below_the_required_item_renders_an_empty_row() {
        let row = composer_row();
        assert_eq!(row.render(9), "", "a half-key must never be rendered");
        assert_eq!(row.render(10), "Esc normal");
        assert_eq!(row.render(16), "Esc normal");
        assert_eq!(row.render(17), "Esc normal · type");
    }

    #[test]
    fn no_budget_ends_in_the_middle_of_an_item() {
        let row = composer_row();
        let full = row.render(0);
        for budget in 0..=64 {
            let rendered = row.render(budget);
            assert!(
                full.starts_with(&rendered),
                "budget {budget} invented a part"
            );
            assert!(
                rendered.is_empty()
                    || rendered == full
                    || full[rendered.len()..].starts_with(HINT_SEPARATOR),
                "budget {budget} ended mid-item: {rendered:?}"
            );
        }
    }

    #[test]
    fn twenty_verbs_render_within_every_budget_without_a_partial_item() {
        let mut line = HintLine::new();
        for index in 0..20 {
            line.push(&format!("k{index}"), "verb");
        }
        let full = line.render(0);
        assert_eq!(full.matches(HINT_SEPARATOR).count(), 19);
        for budget in 0..=120 {
            let rendered = line.render(budget);
            assert!(
                rendered.is_empty()
                    || rendered == full
                    || full[rendered.len()..].starts_with(HINT_SEPARATOR),
                "budget {budget} ended mid-item: {rendered:?}"
            );
        }
    }

    #[test]
    fn the_items_are_the_pairs_the_render_joins_in_the_order_it_paints_them() {
        // The structured form and the painted form are one advertisement: the
        // I4 invariant presses `items()` and a reader sees `render(0)`, so a
        // row whose pairs disagree with its text is the drift the structure
        // exists to prevent.
        let mut line = HintLine::new();
        line.push("j/k", "Move")
            .push("Enter", "")
            .push_required("Esc", "Normal");
        let items: Vec<(&str, &str, bool)> = line
            .items()
            .map(|item| (item.key(), item.verb(), item.is_required()))
            .collect();
        assert_eq!(
            items,
            vec![
                ("Esc", "normal", true),
                ("j/k", "move", false),
                ("Enter", "", false),
            ],
            "items() must yield the painted order, the lowercased verb and the empty verb"
        );
        let joined: String = line
            .items()
            .map(|item| item.text())
            .collect::<Vec<_>>()
            .join(HINT_SEPARATOR);
        assert_eq!(
            joined,
            line.render(0),
            "the items and the render must be the same advertisement"
        );
    }

    #[test]
    fn from_items_builds_the_row_its_pairs_name() {
        // A surface whose advertisement is one footer literal builds it here
        // rather than spelling a second dialect. Nothing is required, because a
        // footer is never truncated.
        let line = HintLine::from_items(&[("j/k", "Move"), ("Esc", "")]);
        assert_eq!(line.render(0), "j/k move · Esc");
        let pairs: Vec<(&str, &str)> = line.items().map(|item| (item.key(), item.verb())).collect();
        assert_eq!(pairs, vec![("j/k", "move"), ("Esc", "")]);
        assert!(
            line.items().all(|item| !item.is_required()),
            "a static row has no item that must survive truncation"
        );
        assert_eq!(
            HintLine::from_items(&pairs).render(0),
            line.render(0),
            "a row built from items must round-trip through its own items"
        );
    }

    #[test]
    fn a_pushed_line_contributes_its_items_and_not_its_rendering() {
        let fragment = composer_row();
        let mut host = HintLine::new();
        host.push("CHAT", "")
            .push_line(&fragment)
            .push("⇧S", "panel");

        let items: Vec<(&str, &str)> = host.items().map(|i| (i.key(), i.verb())).collect();
        assert_eq!(
            items,
            vec![
                ("Esc", "normal"),
                ("CHAT", ""),
                ("type", ""),
                ("⇧S", "panel"),
            ],
            "the fragment's required item leads the composed row"
        );
        assert_eq!(
            host.items().count(),
            1 + fragment.items().count() + 1,
            "the composed row is the sum of the items, not three opaque strings"
        );
        assert_eq!(host.render(0), "Esc normal · CHAT · type · ⇧S panel");
        assert_eq!(host.render(20), "Esc normal · CHAT");
    }
}
