//! One producer of dialog chrome: the outer edge, its title and its hint.
//!
//! `plans/implementation/tui-modularity-and-render-invariants/003-one-dialog-frame.md`
//! §6 asks for a `Section` a dialog hands its outer frame to, so the border,
//! the title and the focus treatment are decided once instead of once per dialog.
//!
//! ## The style policy is not restated here
//!
//! [`ChromeStyles::resolve`] is the one focus policy: border `theme.primary`
//! when focused and `theme.border` when not, title `theme.primary` /
//! `theme.muted`. `Section` calls it rather than spelling a second pair out,
//! so a panel edge and a dialog edge cannot drift apart. The hint is painted
//! with the same title style; it is text on the edge, and it is never marked,
//! because the marker names the focused surface and a hint names verbs.
//!
//! ## The marker is the host's, and only the host's
//!
//! A dialog is a surface, not a column: nothing competes with it for the
//! screen, so a `●` on its frame would name no choice a user can make. The
//! marker means "this column holds the keyboard", which is a fact about a
//! panel host that has just painted the dialog into a shared rect — so it is
//! the **host's** job and [`Section`] never paints one. `focused` therefore
//! decides only the border and title **colour**; a modal reads as the focused
//! surface (primary edge, primary title) without claiming a marker it does not
//! own. Conflating the two once made modals pass `focused: false` to avoid the
//! dot, and a muted title came with it.
//!
//! ## The title string is built here, not by the caller
//!
//! A caller supplies `"Files"`. `Section` trims it, drops a caller-supplied `●`
//! (leading or trailing), applies Title Case, and pads the result through
//! [`chrome_title`], so a dialog cannot reintroduce `" ● Files "` or
//! `" Job detail"`. A title that is *data* — a breadcrumb, a node name — uses
//! [`Section::verbatim`], which keeps the casing and still pads.
//!
//! ## The docked variant and the deliberate non-goals
//!
//! [`SectionFrame::Hosted`] writes **no cell at all** and answers the rect the
//! host's own edge leaves: that rect includes the border cells, so painting
//! even a backdrop would erase the edge, and the arm is what makes the docked
//! rule structural rather than remembered. The owned arm draws an edge at any
//! size, because a dialog's rect is chosen by `App::render_dialog` and
//! content-derived sizing is a later milestone. No content layout either:
//! `Section` answers an inner `Rect` and stops.

use ratatui::buffer::Buffer;
use ratatui::layout::{Margin, Rect};
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders, Widget};

use crate::tui::panels::chrome::{chrome_title, has_chrome, ChromeStyles, FOCUS_MARKER};
use crate::tui::theme::Theme;

/// Which surface owns the outer edge of a [`Section`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionFrame {
    /// The section is the whole surface — a modal dialog — so it draws the
    /// outer border, the title on it and the hint under it, and insets by one
    /// cell on each edge.
    Owned,
    /// A panel host has already painted the outer edge into the same rect, so
    /// this section draws no cell of it and answers the rect that edge leaves.
    Hosted,
}

/// A bordered block with an optional title and an optional hint on its edge.
#[derive(Debug, Clone, Copy)]
pub struct Section<'a> {
    title: Option<&'a str>,
    hint: Option<&'a str>,
    focused: bool,
    frame: SectionFrame,
    theme: &'a Theme,
    /// Whether `title` is data rather than a label. See [`Section::verbatim`].
    verbatim: bool,
}

impl<'a> Section<'a> {
    /// A bordered section titled `title`, for a surface that owns its edge.
    ///
    /// `title` is a plain label — no padding and no marker. [`Section::title`]
    /// adds both, and strips them if the caller supplied them anyway.
    pub fn new(title: &'a str, theme: &'a Theme) -> Self {
        Self {
            title: Some(title),
            hint: None,
            focused: false,
            frame: SectionFrame::Owned,
            theme,
            verbatim: false,
        }
    }

    /// A section with no title: a content or footer box that needs an edge.
    pub fn plain(theme: &'a Theme) -> Self {
        Self {
            title: None,
            hint: None,
            focused: false,
            frame: SectionFrame::Owned,
            theme,
            verbatim: false,
        }
    }

    /// A section whose title is **data** rather than a label.
    ///
    /// A label (`"Files"`, `"Job detail"`) is cased by [`Section::new`] so one
    /// spelling reaches every edge. Some titles are not labels: a file
    /// breadcrumb (`"Files · src/tui (3 shown)"`) and a caller-supplied node
    /// title keep their own casing because re-casing them would corrupt the
    /// path or the name they carry. This constructor still owns the padding
    /// and still strips a caller-supplied marker, so the two things the
    /// primitive exists to standardise hold here too; only the casing is the
    /// caller's.
    pub fn verbatim(title: &'a str, theme: &'a Theme) -> Self {
        Self {
            verbatim: true,
            ..Self::new(title, theme)
        }
    }

    /// Put `hint` on the bottom edge.
    pub fn hint(mut self, hint: &'a str) -> Self {
        self.hint = Some(hint);
        self
    }

    /// Mark this section's **chrome** as focused.
    ///
    /// This selects the primary edge and title — it does not add a marker. A
    /// modal passes `true` because it is the surface holding the interaction;
    /// the `●` belongs to the panel host that framed it, never to this frame.
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }

    /// Choose which surface owns the outer edge.
    pub fn frame(mut self, frame: SectionFrame) -> Self {
        self.frame = frame;
        self
    }

    /// The area height an owned section needs for `content_rows` rows of body.
    ///
    /// The border is counted here and only here: [`Section::draw`] spends the
    /// two rows this adds, so a caller that sums one height per box and a
    /// caller that splits the same rows into layout chunks cannot disagree
    /// about it.
    pub const fn owned_height(content_rows: u16) -> u16 {
        content_rows.saturating_add(2)
    }

    /// The styles this section paints with, resolved through the shared policy.
    pub fn styles(&self) -> ChromeStyles {
        ChromeStyles::resolve(self.theme, self.focused)
    }

    /// The rendered title, or `None` when this section carries no title.
    ///
    /// Never marked, whatever `focused` says: the marker names the column the
    /// host framed, and a `Section` is not a column. Passing `false` here is
    /// the whole of that decision, kept in one call rather than restated at
    /// every site that builds a title.
    pub fn title(&self) -> Option<String> {
        let label = self.title?;
        let label = if self.verbatim {
            label
                .trim()
                .trim_matches(|c: char| FOCUS_MARKER.contains(c))
                .trim()
                .to_string()
        } else {
            normalise_title(label)
        };
        if label.is_empty() {
            // A caller that passed only a marker asked for no title; painting
            // `"  "` would reserve an invisible run of border and push the
            // real edge glyphs aside.
            return None;
        }
        Some(chrome_title(&label, false))
    }

    /// The area left for content, without painting.
    ///
    /// The hosted arm answers the rect the host's own edge leaves, derived
    /// from [`has_chrome`] rather than restated: the seam hands a docked
    /// dialog the **whole column**, border cells included, so a body painted
    /// into that rect would overwrite the edge. Below the chrome threshold the
    /// host drew no edge, and then there is nothing to inset for.
    pub fn inner(&self, area: Rect) -> Rect {
        match self.frame {
            SectionFrame::Owned => Block::default().borders(Borders::ALL).inner(area),
            SectionFrame::Hosted if has_chrome(area) => area.inner(Margin::new(1, 1)),
            SectionFrame::Hosted => area,
        }
    }

    /// Paint the chrome and answer the area left for content.
    pub fn draw(&self, area: Rect, buf: &mut Buffer) -> Rect {
        if self.frame == SectionFrame::Hosted {
            // No cell is written: `area` includes the host's border cells, so
            // even a background fill would overwrite the edge this arm exists
            // to leave alone.
            return self.inner(area);
        }
        let styles = self.styles();
        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_style(styles.border)
            .title_style(styles.title)
            .style(Style::default().bg(self.theme.background));
        if let Some(title) = self.title() {
            block = block.title(title);
        }
        if let Some(hint) = self.hint {
            // The hint is verb vocabulary, so its casing is the caller's; only
            // the padding is shared with the title.
            block = block.title_bottom(chrome_title(hint.trim(), false));
        }
        let inner = block.inner(area);
        block.render(area, buf);
        inner
    }
}

/// `label` as a section title: trimmed, unmarked, Title Case.
fn normalise_title(label: &str) -> String {
    let unmarked = label
        .trim()
        .trim_matches(|c: char| FOCUS_MARKER.contains(c))
        .trim();
    unmarked
        .split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    fn theme() -> Theme {
        Theme::dark()
    }

    /// The first cell outside `rect` that no longer holds `sentinel`.
    ///
    /// This is I1 in one function: a widget may paint `rect` and must leave
    /// every other cell alone.
    fn first_overflow(buf: &Buffer, rect: Rect, sentinel: &str) -> Option<(u16, u16)> {
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                let inside = x >= rect.x
                    && y >= rect.y
                    && x < rect.x.saturating_add(rect.width)
                    && y < rect.y.saturating_add(rect.height);
                if !inside {
                    let cell = buf.cell((x, y))?;
                    if cell.symbol() != sentinel {
                        return Some((x, y));
                    }
                }
            }
        }
        None
    }

    fn sentinel_buffer(area: Rect) -> Buffer {
        let mut buf = Buffer::empty(area);
        for cell in buf.content.iter_mut() {
            cell.set_symbol("¤");
        }
        buf
    }

    #[test]
    fn a_title_is_padded_and_cased_here_and_never_marked() {
        // The three drifts the primitive exists to remove: no padding, a
        // sentence-case label, and a marker the caller placed itself. The
        // fourth row is the owner's decision: `focused` styles the chrome and
        // never marks it, because the marker names the host's column.
        let theme = theme();
        assert_eq!(
            Section::new("Files", &theme).title().as_deref(),
            Some(" Files ")
        );
        assert_eq!(
            Section::new("Files", &theme)
                .focused(true)
                .title()
                .as_deref(),
            Some(" Files "),
            "a focused section must style its edge and title, not paint a marker"
        );
        assert_eq!(
            Section::new("Job detail", &theme).title().as_deref(),
            Some(" Job Detail "),
            "a sentence-case label must be Title Cased, not passed through"
        );
        assert_eq!(
            Section::new("TUI Stats", &theme).title().as_deref(),
            Some(" TUI Stats "),
            "Title Case capitalises the first letter of a word and leaves the rest alone"
        );
    }

    #[test]
    fn a_caller_supplied_padding_or_marker_is_stripped() {
        // §10 requires the primitive to reject or strip a pre-padded title, so
        // the ten unpadded `run_detail` titles cannot reintroduce the drift by
        // being pasted into a caller.
        let theme = theme();
        assert_eq!(
            Section::new(" ● Files ", &theme).title().as_deref(),
            Some(" Files "),
            "a caller-supplied leading marker must be stripped, not moved"
        );
        assert_eq!(
            Section::new("Plan ●", &theme).title().as_deref(),
            Some(" Plan "),
            "a caller-supplied trailing marker must not survive either"
        );
        assert_eq!(
            Section::new("Plan ●", &theme)
                .focused(true)
                .title()
                .as_deref(),
            Some(" Plan "),
            "focus is not a licence to paint the marker the caller was denied"
        );
        assert_eq!(
            Section::new(" ● ", &theme).title(),
            None,
            "a marker with no label is no title at all"
        );
    }

    #[test]
    fn a_verbatim_title_keeps_its_data_and_still_owns_the_padding() {
        // A breadcrumb and a caller-supplied node title are data. Re-casing
        // them would corrupt the path or the name, so the primitive pads and
        // unmarks them but leaves their letters alone.
        let theme = theme();
        assert_eq!(
            Section::verbatim("Files · src/tui (3 shown)", &theme)
                .title()
                .as_deref(),
            Some(" Files · src/tui (3 shown) ")
        );
        assert_eq!(
            Section::verbatim(" ● My Node ", &theme)
                .focused(true)
                .title()
                .as_deref(),
            Some(" My Node "),
            "a data title must not carry a marker either"
        );
    }

    #[test]
    fn the_owned_frame_insets_by_one_on_every_edge() {
        let theme = theme();
        let area = Rect::new(2, 1, 20, 8);
        let mut buf = sentinel_buffer(Rect::new(0, 0, 26, 12));
        let section = Section::new("Files", &theme);
        let inner = section.draw(area, &mut buf);

        assert_eq!(inner, Rect::new(3, 2, 18, 6));
        assert_eq!(inner, section.inner(area), "draw and inner must agree");
        assert_eq!(buf.cell((2, 1)).map(|cell| cell.symbol()), Some("┌"));
        assert_eq!(
            buf.cell((21, 8)).map(|cell| cell.symbol()),
            Some("┘"),
            "the bottom-right corner must be the last cell of the area"
        );
        assert_eq!(
            buf.cell((22, 1)).map(|cell| cell.symbol()),
            Some("¤"),
            "one cell past the right edge must stay untouched"
        );
    }

    #[test]
    fn the_hosted_frame_paints_nothing_and_answers_the_hosts_inner_rect() {
        // The docked dialog's whole requirement: the host's border cells are
        // inside `area`, so a section that filled even its own background
        // would erase the edge it is meant to leave alone, and a body painted
        // into `area` would erase it too. So the arm writes no cell and
        // answers the rect the host's edge leaves.
        let theme = theme();
        let mut buf = sentinel_buffer(Rect::new(0, 0, 40, 20));
        let section = Section::plain(&theme)
            .frame(SectionFrame::Hosted)
            .focused(true);

        let chrome_sized = Rect::new(3, 2, 30, 12);
        let inner = section.draw(chrome_sized, &mut buf);
        assert_eq!(
            inner,
            chrome_sized.inner(Margin::new(1, 1)),
            "the hosted frame must answer the host's inner rect, or the body \
             would paint over the host's border"
        );
        assert_eq!(
            inner,
            section.inner(chrome_sized),
            "draw and inner must agree"
        );
        assert_eq!(
            first_overflow(&buf, Rect::ZERO, "¤"),
            None,
            "the hosted frame painted a cell"
        );

        // Below the chrome threshold the host drew no edge, so there is
        // nothing to inset for and the whole area is content.
        let tiny = Rect::new(3, 2, 10, 3);
        assert_eq!(section.draw(tiny, &mut buf), tiny);
        assert_eq!(
            first_overflow(&buf, Rect::ZERO, "¤"),
            None,
            "the sub-threshold hosted frame painted a cell"
        );
    }

    #[test]
    fn the_height_helper_counts_the_border_exactly_once() {
        // A dialog that derives its height from its sections must not subtract
        // the border a second time: at 72x24 a list chunk of 3 rows whose
        // border was taken twice left a page size of zero, and scrolling
        // stopped working with nothing failing.
        let theme = theme();
        let section = Section::plain(&theme);
        let mut buf = sentinel_buffer(Rect::new(0, 0, 8, 8));
        for content_rows in 0..=5u16 {
            let area = Rect::new(0, 0, 6, Section::owned_height(content_rows));
            assert_eq!(
                section.draw(area, &mut buf).height,
                content_rows,
                "an owned section given its own stated height must leave exactly \
                 {content_rows} content rows"
            );
        }
        assert_eq!(
            Section::owned_height(3),
            5,
            "three content rows plus one border"
        );
    }

    #[test]
    fn the_border_and_title_use_the_shared_focus_policy() {
        // The one outcome that would defeat the milestone is a second style
        // pair. Both halves are asserted against the shared resolution, so a
        // local literal cannot pass.
        let theme = theme();
        for focused in [true, false] {
            let section = Section::new("Files", &theme).focused(focused);
            assert_eq!(section.styles(), ChromeStyles::resolve(&theme, focused));
        }
        let focused = Section::new("Files", &theme).focused(true).styles();
        let unfocused = Section::new("Files", &theme).styles();
        assert_eq!(focused.border.fg, Some(theme.primary));
        assert_eq!(unfocused.border.fg, Some(theme.border));
        assert_ne!(
            focused.border.fg, unfocused.border.fg,
            "focus must change the edge, or the policy marks nothing"
        );
        assert_ne!(theme.primary, Color::Reset);
    }

    #[test]
    fn a_hint_sits_on_the_bottom_edge_padded_and_unmarked() {
        let theme = theme();
        let area = Rect::new(0, 0, 24, 6);
        let mut buf = Buffer::empty(area);
        let section = Section::plain(&theme).hint("j/k scroll").focused(true);
        section.draw(area, &mut buf);

        let bottom: String = (0..area.width)
            .map(|x| buf.cell((x, 5)).map(|c| c.symbol()).unwrap_or(" "))
            .collect();
        assert!(
            bottom.contains(" j/k scroll "),
            "the hint must be on the bottom edge, padded: {bottom:?}"
        );
        assert!(
            !bottom.contains('●'),
            "a hint names verbs; the marker names the focused surface: {bottom:?}"
        );
    }

    #[test]
    fn a_section_that_paints_one_cell_wider_fails_the_containment_check() {
        // I1's acceptance evidence for this primitive. The overflow fixture is
        // local to the test because it is not a defect to fix: it exists so the
        // assertion can be shown to fail on a real overflow.
        let theme = theme();
        let area = Rect::new(1, 1, 10, 4);
        let mut buf = sentinel_buffer(Rect::new(0, 0, 16, 8));

        let section = Section::new("Files", &theme);
        section.draw(area, &mut buf);
        assert_eq!(
            first_overflow(&buf, area, "¤"),
            None,
            "the real section must stay inside its area"
        );

        let mut overflow = sentinel_buffer(Rect::new(0, 0, 16, 8));
        let too_wide = Rect {
            width: area.width + 1,
            ..area
        };
        Block::default()
            .borders(Borders::ALL)
            .render(too_wide, &mut overflow);
        let caught = first_overflow(&overflow, area, "¤");
        assert!(
            caught.is_some(),
            "a section painting one cell too wide must fail the containment check"
        );
        assert_eq!(
            caught,
            Some((area.x + area.width, area.y)),
            "the report must localise the overflowing cell"
        );
    }
}
