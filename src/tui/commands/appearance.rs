//! Appearance and display-mode commands: themes, fullscreen, TUI stats and TTS.

use crate::tui::app::App;

pub(crate) fn dispatch_themes(app: &mut App, argument: &str) {
    app.handle_theme_command(argument);
}

pub(crate) fn dispatch_tui(app: &mut App) {
    app.toggle_fullscreen();
}

pub(crate) fn dispatch_tui_stats(app: &mut App) {
    app.open_stats_dialog();
}

pub(crate) fn dispatch_tts(app: &mut App) {
    app.toggle_tts();
}
