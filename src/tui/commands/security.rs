//! Security review command handlers.

use crate::tui::app::App;

/// Legacy/back-compat security review dispatch. The slash command
/// path (`App::execute_command`'s `/security-review` branch) already
/// runs the review in a background task and posts the result via
/// `TuiCommand::SecurityReviewFinished`, so production never reaches
/// this handler. Convert it to fire-and-forget so any legacy caller
/// cannot block the TUI dispatch loop on a long-running security
/// review.
pub(crate) fn handle_security_review_run(
    app: &mut App,
    id: String,
    root: std::path::PathBuf,
    args: crate::security::workflow::SecurityReviewCommandArgs,
    lsp_tool: Option<std::sync::Arc<crate::tool::lsp::LspTool>>,
) {
    use crate::tui::app::TuiCommand;
    use crate::tui::async_cmd::spawn_tui_task;

    if app.core_client.is_none() {
        app.messages_state.toasts.warning("Core client unavailable");
        return;
    }

    let tx = app.tui_cmd_tx.clone();
    spawn_tui_task(tx, "security_review_legacy", async move {
        let result =
            crate::security::workflow::run_security_review_background(root, args, lsp_tool).await;
        match result {
            Ok(receipt) => Some(TuiCommand::SecurityReviewFinished {
                id,
                receipt: Some(Box::new(receipt)),
                error: None,
            }),
            Err(e) => Some(TuiCommand::SecurityReviewFinished {
                id,
                receipt: None,
                error: Some(format!("Security review failed: {e}")),
            }),
        }
    });
}

/// Apply a completed security review to the App: store the latest
/// receipt, push the rendered report into the message timeline, and
/// surface a success toast. Shared by the inline `SecurityReviewRun`
/// handler and the `SecurityReviewFinished` completion arm.
pub(crate) fn apply_security_review_receipt(
    app: &mut App,
    receipt: crate::security::workflow::SecurityReviewReceipt,
) {
    use crate::tui::components::messages::{MessageRole, MsgPart, UIMessage};
    let open_panel = receipt.args.open_panel_on_complete;
    let labeled = format!("[Security Review]\n{}", receipt.rendered_report);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    app.messages_state.messages.messages.push(UIMessage {
        role: MessageRole::Assistant,
        parts: vec![MsgPart::Text { content: labeled }],
        timestamp: Some(timestamp),
        is_plan_mode: None,
    });
    app.messages_state.messages.scroll_to_bottom();
    app.set_latest_security_review(receipt);
    if open_panel {
        app.open_dialog(crate::tui::Dialog::SecurityReview);
        app.messages_state
            .toasts
            .success("Security review complete — result panel opened.");
    } else {
        app.messages_state.toasts.success(
            "Security review complete — run /security-review-show to open the result panel.",
        );
    }
}

pub(crate) fn handle_security_review_finished(
    app: &mut App,
    id: String,
    receipt: Option<Box<crate::security::workflow::SecurityReviewReceipt>>,
    error: Option<String>,
) {
    // Stale completion: a different (or cancelled) run is now active.
    if app.security_review_run_id() != Some(id.as_str()) {
        return;
    }
    app.security_review_running = None;
    match (receipt, error) {
        (Some(receipt), None) => {
            apply_security_review_receipt(app, *receipt);
        }
        (_, Some(e)) => {
            app.messages_state
                .toasts
                .error(&format!("Security review failed: {e}"));
        }
        _ => {
            app.messages_state
                .toasts
                .error("Security review failed: no result returned");
        }
    }
}
