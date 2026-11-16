//! The skill-promotion commands (M007-I, work packages B and C).
//!
//! The promotion workflow is **daemon-owned**: the durable record lives in
//! the daemon's session storage, keyed on the canonical `WorkspaceId`, and
//! every step of the workflow is a request against it. The frontend holds
//! no promotion state of its own, so this module never opens a store and
//! never writes a skill root — it sends the request and renders the
//! daemon's answer.
//!
//! The publication is mediated by the daemon's C004 checkout reservation
//! gate, so a reserved checkout refuses the publication before its first
//! effect and the refusal is recorded durably. The frontend reports that
//! refusal as the daemon returned it.

use crate::protocol::core::{CoreRequest, CoreResponse};
use crate::protocol::skill::{
    SkillPromotionActionDto, SkillPromotionRequestDto, SkillProposalDetailDto,
    SkillProposalSummaryDto,
};
use crate::tui::app::App;
use crate::tui::app::TuiCommand;
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;

/// Send one promotion request to the daemon.
pub(crate) fn start_skill_promotion(app: &mut App, request: SkillPromotionRequestDto) {
    let action = request.action;
    let core_client = app.core_client.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Memory,
        "skill_promotion",
        async move {
            let Some(core_client) = core_client else {
                return Some(TuiCommand::SkillPromotionFinished {
                    action,
                    response: None,
                    error: Some("Core unavailable — check daemon status with /doctor".to_string()),
                });
            };
            let envelope = crate::protocol::core::RequestEnvelope {
                protocol_version: crate::protocol::core::PROTOCOL_VERSION,
                request_id: uuid::Uuid::new_v4().to_string(),
                payload: CoreRequest::SkillPromotion { request },
            };
            match core_client.request(envelope).await {
                Ok(response) => Some(TuiCommand::SkillPromotionFinished {
                    action,
                    response: Some(response),
                    error: None,
                }),
                Err(error) => Some(TuiCommand::SkillPromotionFinished {
                    action,
                    response: None,
                    error: Some(format!("The promotion request failed: {error}")),
                }),
            }
        },
    );
}

/// Render the daemon's answer.
///
/// The daemon's typed refusal is rendered as it arrives. The frontend never
/// substitutes a local approximation of the workflow it could not perform.
pub(crate) fn apply_skill_promotion_finished(
    app: &mut App,
    action: SkillPromotionActionDto,
    response: Option<CoreResponse>,
    error: Option<String>,
) {
    if let Some(error) = error {
        app.messages_state.toasts.error(&error);
        return;
    }
    let Some(response) = response else {
        return;
    };
    let result = match response {
        CoreResponse::Error { code, message } => {
            app.messages_state
                .toasts
                .error(&format!("{code}: {message}"));
            return;
        }
        CoreResponse::SkillPromotion { result } => result,
        _ => {
            app.messages_state
                .toasts
                .warning("The promotion request returned an unexpected response");
            return;
        }
    };
    match action {
        SkillPromotionActionDto::Begin => {
            let Some(prompt) = result.draft_prompt else {
                app.messages_state
                    .toasts
                    .warning("The daemon staged no draft prompt");
                return;
            };
            app.prompt_state.prompt.set_text(prompt);
            app.send_prompt();
        }
        SkillPromotionActionDto::List => {
            let proposals = result.proposals.unwrap_or_default();
            if proposals.is_empty() {
                app.messages_state
                    .toasts
                    .info("No skill proposals yet. Use /skill-promote <habit-id>.");
                return;
            }
            let mut lines = vec![format!("Skill proposals ({}):", proposals.len())];
            for proposal in &proposals {
                lines.push(proposal_summary_line(proposal));
            }
            lines.push(
                "Preview with /skill-proposal <id>; reject with /skill-proposal reject <id>; request a new draft with /skill-promote <habit-id>."
                    .to_string(),
            );
            app.open_info_dialog(
                crate::tui::components::dialogs::info::InfoType::MemoryResults,
                lines,
            );
        }
        SkillPromotionActionDto::Get | SkillPromotionActionDto::Preview => {
            let Some(detail) = result.detail else {
                app.messages_state
                    .toasts
                    .warning("The daemon returned no proposal");
                return;
            };
            app.open_info_dialog(
                crate::tui::components::dialogs::info::InfoType::MemoryResults,
                proposal_detail_lines(&detail),
            );
        }
        SkillPromotionActionDto::Reject => {
            if result.changed {
                app.messages_state.toasts.info("Skill proposal rejected.");
            } else {
                app.messages_state
                    .toasts
                    .warning("Proposal not found or is not awaiting review.");
            }
        }
        SkillPromotionActionDto::Publish => {
            let Some(relative_path) = result.published_relative_path else {
                app.messages_state
                    .toasts
                    .warning("The daemon published no skill");
                return;
            };
            app.messages_state
                .toasts
                .success(&format!("Published skill: {relative_path}"));
            // The existing daemon-owned refresh path is the only authority
            // allowed to publish a new runtime asset generation.
            crate::tui::commands::agents::start_refresh_assets(app);
        }
    }
}

/// One proposal's listing line.
fn proposal_summary_line(proposal: &SkillProposalSummaryDto) -> String {
    format!(
        "- [{}] {} {} — {} (rev {}, digest {})",
        proposal.proposal_id.chars().take(8).collect::<String>(),
        proposal.status,
        proposal.name,
        proposal.description,
        proposal.revision,
        proposal.content_digest.chars().take(12).collect::<String>()
    )
}

/// One proposal's preview lines.
fn proposal_detail_lines(detail: &SkillProposalDetailDto) -> Vec<String> {
    let mut lines = vec![
        format!("Skill proposal {}", detail.proposal_id),
        format!("Status: {}", detail.status),
        format!("Name: {}", detail.name),
        format!("Description: {}", detail.description),
        format!("Target scope: {:?}", detail.target_scope),
        format!(
            "Habit: {} (fingerprint {}, candidate rev {})",
            detail.habit_id, detail.habit_fingerprint, detail.candidate_revision
        ),
        format!(
            "Content digest: {} (proposal rev {})",
            detail.content_digest, detail.revision
        ),
        match &detail.published_relative_path {
            Some(relative_path) => format!(
                "Published at {relative_path}; runtime activation is determined by the next asset refresh."
            ),
            None => "Not installed or effective yet.".to_string(),
        },
        String::new(),
        detail.skill_markdown.clone(),
    ];
    for diagnostic in &detail.diagnostics {
        lines.push(format!(
            "Diagnostic [{}]: {}",
            diagnostic.severity, diagnostic.reason
        ));
    }
    lines.push(
        "Request a revised draft with /skill-promote <habit-id>; rejection does not dismiss the habit."
            .to_string(),
    );
    lines
}

pub(crate) fn dispatch_skill_promote(app: &mut App, argument: &str) {
    if app.prompt_state.pending_send {
        app.messages_state
            .toasts
            .warning("Still waiting for previous prompt to finish");
        return;
    }
    let Some(habit_id) = crate::memory::habit::HabitId::parse(argument) else {
        app.messages_state
            .toasts
            .warning("Usage: /skill-promote <ready habit id>");
        return;
    };
    let Some(session_id) = app.session_state.session.as_ref().map(|s| s.id.clone()) else {
        app.messages_state
            .toasts
            .warning("Start or select a session before promoting a habit");
        return;
    };
    let Some(workspace_id) = app.active_workspace_id().map(str::to_owned) else {
        app.messages_state
            .toasts
            .warning("Select a workspace before promoting a habit");
        return;
    };
    // M007-I (work package B): the daemon owns the promotion
    // store, so the frontend stages nothing itself. It asks the
    // daemon, and the daemon records the request against the
    // canonical workspace.
    crate::tui::commands::promotion::start_skill_promotion(
        app,
        crate::protocol::skill::SkillPromotionRequestDto {
            workspace_id,
            session_id,
            action: crate::protocol::skill::SkillPromotionActionDto::Begin,
            habit_id: Some(habit_id.as_str().to_owned()),
            proposal_id: None,
            target_scope: None,
        },
    );
}

pub(crate) fn dispatch_skill_proposals(app: &mut App) {
    let Some(session_id) = app.session_state.session.as_ref().map(|s| s.id.clone()) else {
        app.messages_state
            .toasts
            .warning("Start or select a session before listing proposals");
        return;
    };
    let Some(workspace_id) = app.active_workspace_id().map(str::to_owned) else {
        app.messages_state
            .toasts
            .warning("Select a workspace before listing proposals");
        return;
    };
    crate::tui::commands::promotion::start_skill_promotion(
        app,
        crate::protocol::skill::SkillPromotionRequestDto {
            workspace_id,
            session_id,
            action: crate::protocol::skill::SkillPromotionActionDto::List,
            habit_id: None,
            proposal_id: None,
            target_scope: None,
        },
    );
}

pub(crate) fn dispatch_skill_proposal(app: &mut App, argument: &str) {
    let Some(session_id) = app.session_state.session.as_ref().map(|s| s.id.clone()) else {
        app.messages_state
            .toasts
            .warning("Start or select a session before acting on a proposal");
        return;
    };
    let Some(workspace_id) = app.active_workspace_id().map(str::to_owned) else {
        app.messages_state
            .toasts
            .warning("Select a workspace before acting on a proposal");
        return;
    };
    let mut action = crate::protocol::skill::SkillPromotionActionDto::Preview;
    let mut target_scope = None;
    let id_text = if let Some(publish_args) = argument.strip_prefix("publish") {
        let parts = publish_args.split_whitespace().collect::<Vec<_>>();
        if parts.len() != 2 {
            app.messages_state
                .toasts
                .warning("Usage: /skill-proposal publish <id> [project|global]");
            return;
        }
        target_scope = Some(match parts[1] {
            "project" => crate::protocol::skill::SkillTargetScopeDto::Project,
            "global" => crate::protocol::skill::SkillTargetScopeDto::Global,
            _ => {
                app.messages_state
                    .toasts
                    .warning("Publication scope must be project or global");
                return;
            }
        });
        action = crate::protocol::skill::SkillPromotionActionDto::Publish;
        parts[0].to_string()
    } else if let Some(reject_id) = argument.strip_prefix("reject ") {
        action = crate::protocol::skill::SkillPromotionActionDto::Reject;
        reject_id.trim().to_string()
    } else {
        argument.trim().to_string()
    };
    if crate::skills::promotion::SkillProposalId::parse(&id_text).is_none() {
        app.messages_state
            .toasts
            .warning("Usage: /skill-proposal <id> or /skill-proposal reject <id>");
        return;
    }
    crate::tui::commands::promotion::start_skill_promotion(
        app,
        crate::protocol::skill::SkillPromotionRequestDto {
            workspace_id,
            session_id,
            action,
            habit_id: None,
            proposal_id: Some(id_text),
            target_scope,
        },
    );
}
