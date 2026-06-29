//! Render panic recovery helpers.

use super::super::app::App;

pub const MAX_RENDER_PANICS: usize = 3;

pub fn clear_render_error(app: &mut App) {
    app.ui_state.render_panic_count = 0;
    app.ui_state.last_render_error = None;
}

pub fn handle_render_panic(app: &mut App, panic_err: Box<dyn std::any::Any + Send>) -> String {
    let msg = if let Some(s) = panic_err.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = panic_err.downcast_ref::<String>() {
        s.clone()
    } else {
        "Unknown render panic".to_string()
    };
    tracing::error!("Render panic: {}", msg);
    app.ui_state.render_panic_count += 1;
    app.ui_state.diagnostics.render_panic_count = app.ui_state.render_panic_count as u64;
    app.ui_state.last_render_error = Some(msg.clone());
    app.ui_state.diagnostics.last_render_error = Some(msg.clone());
    msg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_render_panics_is_three() {
        assert_eq!(MAX_RENDER_PANICS, 3);
    }

    #[test]
    fn clear_render_error_resets_state() {
        let mut app = App::new_for_testing("/tmp".into());
        app.ui_state.render_panic_count = 5;
        app.ui_state.last_render_error = Some("old error".to_string());
        clear_render_error(&mut app);
        assert_eq!(app.ui_state.render_panic_count, 0);
        assert!(app.ui_state.last_render_error.is_none());
    }
}
