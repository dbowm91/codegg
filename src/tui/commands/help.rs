//! The `/help` command: the app's own key list, opened as a dialog.

use crate::tui::app::App;
use crate::tui::app::Dialog;
use crate::tui::components::dialogs::help::HelpRoute;

pub(crate) fn dispatch_help(app: &mut App) {
    app.ui_state.command_mode = false;
    // The app's help, not the composer's chord: this box lists the
    // keys and never copies. See
    // `crate::tui::components::dialogs::help::HelpRoute`.
    app.open_dialog(Dialog::Help(HelpRoute::Command));
}
