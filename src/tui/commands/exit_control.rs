//! The two-press exit confirmation.
//!
//! Leaving the program is destructive and one keystroke away from a stray
//! `q` or from a terminal that delivers `Ctrl+C` to a program it did not
//! mean to stop. Both exits are therefore gated behind a second press of
//! the **same** key inside [`EXIT_ARM_WINDOW`]: the first press clears the
//! input field and arms the exit, the second one leaves.
//!
//! The shape is the stop-agent confirmation's — an armed timestamp on the
//! prompt state, an expiry that runs on the render pump, and a line on the
//! row above the input that names the key to press again.

use std::time::Instant;

use crate::tui::app::state::prompt::{ExitArm, EXIT_ARM_WINDOW};
use crate::tui::app::App;

/// Arm the exit, or report that it was already armed inside the window.
///
/// `true` means the exit is **confirmed**: the caller exits now. `false`
/// means the arm was just raised and the caller should tell the user so.
pub(crate) fn request_exit(app: &mut App, arm: ExitArm) -> bool {
    let confirmed = app
        .prompt_state
        .exit_armed_at
        .is_some_and(|(armed, at)| armed == arm && at.elapsed() < EXIT_ARM_WINDOW);
    if confirmed {
        app.prompt_state.exit_armed_at = None;
        return true;
    }
    app.prompt_state.exit_armed_at = Some((arm, Instant::now()));
    false
}

/// The armed exit, if it is still inside its window.
///
/// A lapsed arm reads as disarmed: the confirmation is gone, and the next
/// press is a first press again.
pub(crate) fn exit_armed(app: &App) -> Option<ExitArm> {
    app.prompt_state
        .exit_armed_at
        .filter(|(_, at)| at.elapsed() < EXIT_ARM_WINDOW)
        .map(|(arm, _)| arm)
}

/// Drop a lapsed arm.
///
/// Called from the render pump, so a confirmation the user walked away from
/// clears itself instead of waiting to be overwritten by whatever they type
/// next.
pub(crate) fn expire_pending_exit_arm(app: &mut App) {
    let lapsed = app
        .prompt_state
        .exit_armed_at
        .is_some_and(|(_, at)| at.elapsed() >= EXIT_ARM_WINDOW);
    if lapsed {
        app.prompt_state.exit_armed_at = None;
    }
}

/// The line the row above the input shows while an exit is armed.
///
/// It names the key, because the two exits are different keys and a user
/// who pressed `q` should not be told to press `Ctrl+C`.
///
/// `q` and `Ctrl+Q` are both the **quit** arm, so the line says `q` for both:
/// `q` completes either, which is what the user is being told to do. Naming
/// the exact key would need the binding to carry it, and the arm is about
/// *which exit* rather than which spelling of the key reached it.
pub(crate) fn exit_arm_hint(app: &App) -> Option<String> {
    match exit_armed(app)? {
        ExitArm::Quit => Some("press q again to quit".to_string()),
        ExitArm::Force => Some("press ctrl+c again to exit".to_string()),
    }
}
