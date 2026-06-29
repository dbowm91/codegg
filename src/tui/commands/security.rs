//! Security review command handlers.

use crate::tui::app::App;

pub(crate) async fn handle_security_review_run(
    app: &mut App,
    id: String,
    root: std::path::PathBuf,
    args: crate::security::workflow::SecurityReviewCommandArgs,
    lsp_tool: Option<std::sync::Arc<crate::tool::lsp::LspTool>>,
) {
    let result =
        crate::security::workflow::run_security_review_background(root, args, lsp_tool).await;

    // Always clear the reentrancy guard, even on failure.
    if app.security_review_run_id() == Some(id.as_str()) {
        app.security_review_running = None;
    }

    match result {
        Ok(receipt) => apply_security_review_receipt(app, receipt),
        Err(e) => {
            app.messages_state
                .toasts
                .error(&format!("Security review failed: {e}"));
        }
    }
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
