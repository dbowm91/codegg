//! The one place a dialog's rect is decided: the host clamps what the dialog
//! states. A dialog states a preference
//! ([`Component::preferred_area`]); [`clamp_area`] is the only code that
//! applies the floor and the ceiling, so a dialog can neither paint outside
//! the frame nor honour a policy of its own.
//!
//! [`Component::preferred_area`]: crate::tui::components::component::Component::preferred_area

use ratatui::layout::Rect;

use crate::tui::components::component::centered_rect;

/// The smallest rect a dialog is given; below it a dialog is unusable, and
/// only a smaller frame is answered with something smaller.
pub const MIN_DIALOG_WIDTH: u16 = 20;
pub const MIN_DIALOG_HEIGHT: u16 = 6;

/// The most of the available rect a dialog may take, in percent.
pub const MAX_DIALOG_PERCENT: u16 = 90;

/// `preferred`'s size, clamped and centred inside `available`.
///
/// The stated origin is not honoured: the host places the box, so an
/// off-centre preference cannot land outside the frame.
pub fn clamp_area(preferred: Rect, available: Rect) -> Rect {
    let width = clamp_axis(preferred.width, available.width, MIN_DIALOG_WIDTH);
    let height = clamp_axis(preferred.height, available.height, MIN_DIALOG_HEIGHT);
    Rect {
        x: available.x + (available.width - width) / 2,
        y: available.y + (available.height - height) / 2,
        width,
        height,
    }
}

/// The legacy 60% width with `rows` of height, centred in `available`: a
/// dialog that adopts the seam changes its height only.
pub fn preferred_rows(available: Rect, rows: u16) -> Rect {
    let width = centered_rect(60, 50, available).width;
    Rect {
        x: available.x + available.width.saturating_sub(width) / 2,
        y: available.y + available.height.saturating_sub(rows) / 2,
        width,
        height: rows,
    }
}

/// The legacy 60×50 height, which a box with live content keeps as its floor.
pub fn legacy_height(available: Rect) -> u16 {
    centered_rect(60, 50, available).height
}

fn clamp_axis(preferred: u16, available: u16, minimum: u16) -> u16 {
    let floor = minimum.min(available);
    // Widened to the floor when the two cross: a frame narrower than the
    // minimum has no smaller answer to give.
    let ceiling = ((available as u32 * MAX_DIALOG_PERCENT as u32 / 100) as u16)
        .max(floor)
        .min(available);
    preferred.max(floor).min(ceiling)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::{Buffer, Cell};
    use ratatui::style::{Color, Modifier, Style};
    use ratatui::widgets::{Block, Borders, Widget};
    use ratatui::Terminal;
    use std::sync::Arc;

    use crate::tui::app::TuiMsg;
    use crate::tui::components::component::{Component, DialogType};
    use crate::tui::components::dialogs::confirm::ConfirmDialog;
    use crate::tui::components::dialogs::goto::GotoDialog;
    use crate::tui::components::dialogs::import::ImportDialog;
    use crate::tui::components::dialogs::info::{InfoDialog, InfoType};
    use crate::tui::components::dialogs::project_picker::ProjectPickerDialog;
    use crate::tui::components::dialogs::question::{QuestionDialog, QuestionSpec};
    use crate::tui::theme::Theme;

    const SENTINEL: &str = "¤";
    const SENTINEL_FG: Color = Color::Rgb(0xff, 0x00, 0xff);
    const SENTINEL_BG: Color = Color::Rgb(0x00, 0xff, 0xff);

    struct NoPreference;

    impl Component for NoPreference {
        fn handle_key(&mut self, _key: crossterm::event::KeyEvent) -> Option<TuiMsg> {
            None
        }

        fn update(&mut self, _msg: TuiMsg) -> Option<TuiMsg> {
            None
        }

        fn render(&mut self, _frame: &mut ratatui::Frame, _area: Rect, _theme: &Arc<Theme>) {}

        fn dialog_type(&self) -> DialogType {
            DialogType::Confirm
        }
    }

    struct ClaimsTheWholeFrame;

    impl Component for ClaimsTheWholeFrame {
        fn handle_key(&mut self, _key: crossterm::event::KeyEvent) -> Option<TuiMsg> {
            None
        }

        fn update(&mut self, _msg: TuiMsg) -> Option<TuiMsg> {
            None
        }

        fn render(&mut self, frame: &mut ratatui::Frame, area: Rect, _theme: &Arc<Theme>) {
            Block::default()
                .borders(Borders::ALL)
                .render(area, frame.buffer_mut());
        }

        fn preferred_area(&self, _available: Rect) -> Rect {
            Rect::new(0, 0, 400, 400)
        }

        fn dialog_type(&self) -> DialogType {
            DialogType::Confirm
        }
    }

    fn theme() -> Arc<Theme> {
        Arc::new(Theme::dark())
    }

    fn fill_sentinel(buf: &mut Buffer) {
        for cell in buf.content.iter_mut() {
            cell.set_symbol(SENTINEL);
            cell.set_style(
                Style::default()
                    .fg(SENTINEL_FG)
                    .bg(SENTINEL_BG)
                    .add_modifier(Modifier::BOLD),
            );
        }
    }

    fn is_sentinel(cell: &Cell) -> bool {
        cell.symbol() == SENTINEL
            && cell.fg == SENTINEL_FG
            && cell.bg == SENTINEL_BG
            && cell.modifier == Modifier::BOLD
    }

    fn holds(outer: Rect, x: u16, y: u16) -> bool {
        outer.width > 0
            && outer.height > 0
            && x >= outer.x
            && y >= outer.y
            && x < outer.x.saturating_add(outer.width)
            && y < outer.y.saturating_add(outer.height)
    }

    fn inside(inner: Rect, outer: Rect) -> bool {
        holds(outer, inner.x, inner.y)
            && holds(
                outer,
                inner.x.saturating_add(inner.width).saturating_sub(1),
                inner.y.saturating_add(inner.height).saturating_sub(1),
            )
    }

    fn painted_outside(buf: &Buffer, rect: Rect) -> Option<(u16, u16)> {
        for (index, cell) in buf.content.iter().enumerate() {
            let x = buf.area.x + (index as u16 % buf.area.width);
            let y = buf.area.y + (index as u16 / buf.area.width);
            if !holds(rect, x, y) && !is_sentinel(cell) {
                return Some((x, y));
            }
        }
        None
    }

    fn painted_inside(buf: &Buffer, rect: Rect) -> usize {
        buf.content
            .iter()
            .enumerate()
            .filter(|(index, cell)| {
                let x = buf.area.x + (*index as u16 % buf.area.width);
                let y = buf.area.y + (*index as u16 / buf.area.width);
                holds(rect, x, y) && !cell.symbol().trim().is_empty()
            })
            .count()
    }

    fn render_clamped(
        component: &mut dyn Component,
        screen: Rect,
        theme: &Arc<Theme>,
    ) -> (Rect, Buffer) {
        let rect = clamp_area(component.preferred_area(screen), screen);
        let mut terminal = Terminal::new(TestBackend::new(screen.width, screen.height))
            .expect("a test terminal is constructible");
        fill_sentinel(terminal.current_buffer_mut());
        let completed = terminal
            .draw(|frame| component.render(frame, rect, theme))
            .expect("the test backend is infallible");
        (rect, (*completed.buffer).clone())
    }

    #[test]
    fn the_clamp_keeps_a_stated_preference_inside_the_frame() {
        for (width, height) in [(120, 40), (80, 24), (40, 12), (20, 6), (10, 3), (1, 1)] {
            let available = Rect::new(0, 0, width, height);
            let rect = clamp_area(Rect::new(0, 0, 400, 400), available);
            assert!(
                inside(rect, available),
                "a 400x400 claim in {width}x{height} must stay inside the frame, got {rect:?}"
            );
            assert!(
                rect.width >= 1 && rect.height >= 1,
                "the clamp must never answer an empty rect at {width}x{height}: {rect:?}"
            );
        }
        assert_eq!(
            clamp_area(Rect::new(0, 0, 400, 400), Rect::new(0, 0, 80, 24)),
            Rect::new(4, 1, 72, 21),
            "80x24 caps a claim at the 90% ceiling"
        );
    }

    #[test]
    fn a_preference_below_the_minimum_gets_the_minimum_not_an_empty_rect() {
        let rect = clamp_area(Rect::new(0, 0, 0, 0), Rect::new(0, 0, 80, 24));
        assert_eq!(
            rect,
            Rect::new(30, 9, MIN_DIALOG_WIDTH, MIN_DIALOG_HEIGHT),
            "a zero-sized claim must be lifted to the floor and centred"
        );
    }

    #[test]
    fn a_frame_smaller_than_the_minimum_gets_the_whole_frame() {
        let tiny = Rect::new(0, 0, 10, 3);
        assert_eq!(
            clamp_area(Rect::new(0, 0, 0, 0), tiny),
            tiny,
            "when the frame cannot hold the floor, the frame is the answer"
        );
    }

    #[test]
    fn the_default_preference_is_the_legacy_percentage_unchanged() {
        let dialog = NoPreference;
        for (width, height) in [(80, 24), (120, 40), (40, 12)] {
            let available = Rect::new(0, 0, width, height);
            let legacy = centered_rect(60, 50, available);
            assert_eq!(
                dialog.preferred_area(available),
                legacy,
                "the default preference at {width}x{height} must be the legacy 60x50 popup"
            );
            assert_eq!(
                clamp_area(dialog.preferred_area(available), available),
                legacy,
                "the clamp must not move a dialog that keeps the default at {width}x{height}"
            );
        }
        assert_eq!(
            dialog.preferred_area(Rect::new(0, 0, 80, 24)),
            Rect::new(16, 6, 48, 12),
            "60% of 80 columns and 50% of 24 rows, centred"
        );
    }

    #[test]
    fn a_stated_preference_is_idempotent_for_the_same_frame() {
        let available = Rect::new(0, 0, 80, 24);
        let dialog = ConfirmDialog::new("Confirm".to_string(), "Are you sure?".to_string());
        let first = dialog.preferred_area(available);
        assert_eq!(
            first,
            dialog.preferred_area(available),
            "the seam must not read the rect it was given, or it feeds back on itself"
        );
        assert_eq!(
            first.height,
            crate::tui::components::section::Section::owned_height(2)
                + crate::tui::components::section::Section::owned_height(2)
                + crate::tui::components::section::Section::owned_height(3),
            "Confirm's preference is its three sections' heights, border counted once each"
        );
    }

    #[test]
    fn every_adopting_dialog_renders_inside_its_clamped_preference() {
        let theme = theme();
        let mut dialogs: Vec<(&str, Box<dyn Component>)> = vec![
            (
                "Confirm",
                Box::new(ConfirmDialog::new(
                    "Confirm".to_string(),
                    "Are you sure?".to_string(),
                )),
            ),
            ("Goto", Box::new(GotoDialog::new(12))),
            ("Import", Box::new(ImportDialog::new(Arc::clone(&theme)))),
            (
                "Question",
                Box::new(QuestionDialog::new(vec![QuestionSpec {
                    question: "Continue?".to_string(),
                    options: None,
                    initial: None,
                }])),
            ),
            (
                "Info",
                Box::new(InfoDialog::new(
                    Arc::clone(&theme),
                    InfoType::Cost,
                    (1..=40).map(|n| format!("row {n}")).collect(),
                )),
            ),
            (
                "ProjectPicker",
                Box::new(ProjectPickerDialog::new(Arc::clone(&theme))),
            ),
        ];
        for (surface, dialog) in dialogs.iter_mut() {
            for (width, height) in [(80, 24), (120, 40), (40, 12)] {
                let screen = Rect::new(0, 0, width, height);
                let (rect, buf) = render_clamped(dialog.as_mut(), screen, &theme);
                assert!(
                    inside(rect, screen),
                    "{surface} claimed a rect outside {width}x{height}: {rect:?}"
                );
                assert_eq!(
                    painted_outside(&buf, rect),
                    None,
                    "{surface} painted outside the rect the host clamped at {width}x{height}"
                );
                assert!(
                    painted_inside(&buf, rect) > 0,
                    "{surface} painted nothing at {width}x{height}"
                );
            }
        }
    }

    #[test]
    fn an_oversized_preference_is_clamped_rather_than_cropped() {
        let theme = theme();
        for (width, height) in [(80, 24), (120, 40), (40, 12)] {
            let screen = Rect::new(0, 0, width, height);
            let mut dialog = ClaimsTheWholeFrame;
            let (rect, buf) = render_clamped(&mut dialog, screen, &theme);
            assert_eq!(
                rect,
                clamp_area(dialog.preferred_area(screen), screen),
                "the host must place the clamped preference, not the claim, at {width}x{height}"
            );
            assert!(
                inside(rect, screen),
                "the clamped rect must be inside {width}x{height}: {rect:?}"
            );
            assert_eq!(
                painted_outside(&buf, rect),
                None,
                "a 400x400 claim must not paint outside the clamped rect at {width}x{height}"
            );
        }
    }
}
