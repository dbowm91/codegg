//! Transcript controls: search, undo/redo, timestamps, reasoning visibility, disclosure expansion and the timeline.

use crate::tui::app::App;

pub(crate) fn dispatch_timeline(app: &mut App) {
    app.show_timeline();
}

pub(crate) fn dispatch_undo(app: &mut App) {
    if app.messages_state.messages.undo() {
        app.messages_state.toasts.info("Undid last message");
    } else {
        app.messages_state.toasts.info("Nothing to undo");
    }
}

pub(crate) fn dispatch_redo(app: &mut App) {
    if app.messages_state.messages.redo() {
        app.messages_state.toasts.info("Redid message");
    } else {
        app.messages_state.toasts.info("Nothing to redo");
    }
}

pub(crate) fn dispatch_timestamps(app: &mut App) {
    app.ui_state.show_timestamps = !app.ui_state.show_timestamps;
    let msg = if app.ui_state.show_timestamps {
        "timestamps shown"
    } else {
        "timestamps hidden"
    };
    app.messages_state.toasts.info(msg);
}

pub(crate) fn dispatch_thinking(app: &mut App) {
    app.ui_state.show_thinking = !app.ui_state.show_thinking;
    let msg = if app.ui_state.show_thinking {
        "thinking shown"
    } else {
        "thinking hidden"
    };
    app.messages_state.toasts.info(msg);
}

pub(crate) fn dispatch_expand_all_disclosures(app: &mut App) {
    // The palette twin of the `Ctrl+Alt+O` chord: the same
    // gesture, the same toast.
    app.expand_all_disclosures();
}

pub(crate) fn dispatch_search(app: &mut App, argument: &str) {
    let query = argument.to_string();
    if query.is_empty() {
        app.messages_state.toasts.warning("Usage: /search <query>");
    } else {
        app.messages_state.messages.search(&query);
        let count = app.messages_state.messages.search_matches.len();
        if count == 0 {
            app.messages_state
                .toasts
                .info(&format!("No matches for \"{}\"", query));
        } else {
            app.messages_state.toasts.info(&format!(
                "{} match{} for \"{}\"",
                count,
                if count == 1 { "" } else { "es" },
                query
            ));
        }
    }
}
