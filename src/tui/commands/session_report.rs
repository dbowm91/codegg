//! Read-only session reporting: status, context, cost, usage, plan and state.

use crate::tui::app::App;
use crate::tui::app::Dialog;

pub(crate) fn dispatch_status(app: &mut App) {
    let mut status = format!(
        "status: {:?} | tokens: {}↑ {}↓ | model: {}",
        app.session_state.session_status,
        app.session_state.token_in,
        app.session_state.token_out,
        app.agent_state
            .current_model
            .split('/')
            .next_back()
            .unwrap_or(&app.agent_state.current_model)
    );
    // M007: daemon-resolved effective policy alongside the
    // legacy status line (observers can read policy here;
    // /policy itself stays control-gated).
    if let Some(line) = app.policy_ui.status_line() {
        status.push_str(&format!(" | {}", line));
    }
    app.messages_state.toasts.info(&status);
}

pub(crate) fn dispatch_context(app: &mut App) {
    app.ui_state.command_mode = false;
    app.open_dialog(Dialog::Context);
}

pub(crate) fn dispatch_cost(app: &mut App) {
    app.ui_state.command_mode = false;
    app.open_dialog(Dialog::Cost);
}

pub(crate) fn dispatch_usage(app: &mut App) {
    app.ui_state.command_mode = false;
    app.open_dialog(Dialog::Usage);
}

pub(crate) fn dispatch_plan(app: &mut App, argument: &str) {
    // M007-H (D3): the plan is the daemon's durable work-plan
    // state, so the daemon publishes it and this arm renders
    // that answer. The local checklist is **gone**: a second
    // copy of the plan is a second source of truth.
    if !argument.trim().is_empty() {
        app.messages_state
            .toasts
            .warning("The plan is the daemon's; /plan takes no argument");
        return;
    }
    let Some(plan) = app.session_state_derived.plan.clone() else {
        app.messages_state
            .toasts
            .info("No plan published by the daemon");
        return;
    };
    if plan.items.is_empty() {
        app.messages_state.toasts.info("Plan is empty");
        return;
    }
    let items: Vec<String> = plan
        .items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let icon = match item.status {
                crate::session::events::PlanItemStatus::Done => "[x]",
                crate::session::events::PlanItemStatus::InProgress => "[>]",
                crate::session::events::PlanItemStatus::Skipped => "[-]",
                crate::session::events::PlanItemStatus::Blocked => "[?]",
                crate::session::events::PlanItemStatus::Pending => "[ ]",
            };
            format!("{} {} {}", i, icon, item.text)
        })
        .collect();
    let mut plan_lines = vec![format!("Plan ({} items):", plan.items.len())];
    plan_lines.extend(items);
    app.show_short_or_info(
        crate::tui::components::dialogs::info::InfoType::Stats,
        plan_lines,
    );
}

pub(crate) fn dispatch_state(app: &mut App) {
    let state = &app.session_state_derived;
    let mut lines = Vec::new();

    if let Some(ref goal) = state.goal {
        lines.push(format!("Goal: {}", goal));
    }

    if let Some(ref plan) = state.plan {
        let done = plan
            .items
            .iter()
            .filter(|i| i.status == crate::session::events::PlanItemStatus::Done)
            .count();
        lines.push(format!("Plan: {}/{} done", done, plan.items.len()));
    }

    let model_short = app
        .agent_state
        .current_model
        .split('/')
        .next_back()
        .unwrap_or(&app.agent_state.current_model);
    lines.push(format!("Model: {}", model_short));

    let agent_name = &app.agent_state.agents[app.agent_state.current_agent].name;
    lines.push(format!("Agent: {}", agent_name));

    lines.push(format!("Files: {} changed", state.changed_files.len()));

    // `context_limit` is 0 until the agent resolves a window for the turn.
    // Dividing unconditionally here yielded NaN (rendered as a
    // bogus 0%) whenever the limit was still unknown.
    let context_line = if app.session_state.context_limit > 0 {
        format!(
            "Context: {}%",
            (app.session_state.context_tokens as f64 / app.session_state.context_limit as f64
                * 100.0) as u64
        )
    } else {
        "Context: unknown".to_string()
    };
    lines.push(context_line);

    app.show_short_or_info(
        crate::tui::components::dialogs::info::InfoType::Stats,
        lines,
    );
}
