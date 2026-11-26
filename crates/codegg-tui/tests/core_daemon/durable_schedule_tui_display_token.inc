use std::path::Path;

use crate::common;
use codegg::core::daemon::CoreDaemon;
use codegg::protocol::core::{CoreRequest, CoreResponse};
use codegg_tui::commands::test_support::{
    resolve_schedule_id, schedule_display_id, schedule_label,
};

async fn test_daemon() -> CoreDaemon {
    CoreDaemon::new(Some(common::pool::isolated_pool().await), None, None)
}

async fn seed_test_context(daemon: &CoreDaemon, root: &Path) -> (String, String) {
    let workspace = daemon
        .workspaces
        .get_or_register(root)
        .await
        .expect("test workspace registration");
    let project = codegg_core::project_catalog::ProjectCatalog::new(
        daemon.pool.clone().expect("test daemon pool"),
    )
    .register_local_project(
        codegg_core::project_catalog::RegisterLocalProject {
            display_name: "Daemon test project".to_string(),
            description: None,
            tags: Vec::new(),
            primary_repository_id: None,
        },
        &workspace.id,
        "daemon-test",
    )
    .await
    .expect("test project registration");
    (
        project.project_id.as_str().to_string(),
        workspace.id.as_str().to_string(),
    )
}

#[tokio::test]
async fn durable_schedule_tui_display_token_resolves_and_deletes_in_workspace() {
    let daemon = test_daemon().await;
    let root = tempfile::tempdir().unwrap();
    let (_project_id, workspace_id) = seed_test_context(&daemon, root.path()).await;
    let prompt = "check the build from the TUI";
    let session_id = "tui-schedule-session".to_string();
    let kind = codegg_core::jobs::ScheduleKind::Interval {
        every: std::time::Duration::from_secs(300),
        anchor: chrono::Utc::now(),
    };
    let template = codegg_core::jobs::schedule::JobTemplate::for_subagent(
        codegg_core::jobs::JobKind::Subagent,
        prompt.to_string(),
        "build".to_string(),
        Some(session_id.clone()),
    );
    let spec = codegg::protocol::dto::ScheduleCreateDto {
        workspace_id: workspace_id.clone(),
        session_id: Some(session_id),
        kind: serde_json::to_value(kind).unwrap(),
        job_template: serde_json::to_value(template).unwrap(),
        overlap_policy: "skip_if_running".to_string(),
        missed_run_policy: serde_json::to_value(codegg_core::jobs::MissedRunPolicy::RunOnceNow)
            .unwrap(),
        labels: std::collections::HashMap::new(),
    };

    let schedule_id = match daemon
        .handle_request(codegg::core::new_request(
            "tui-schedule-create".into(),
            CoreRequest::ScheduleCreate { spec },
        ))
        .await
        .unwrap()
    {
        CoreResponse::ScheduleCreated { schedule_id } => schedule_id,
        other => panic!("unexpected schedule creation response: {other:?}"),
    };

    let schedules = match daemon
        .handle_request(codegg::core::new_request(
            "tui-schedule-list".into(),
            CoreRequest::ScheduleList {
                workspace_id: Some(workspace_id.clone()),
                include_archived: false,
            },
        ))
        .await
        .unwrap()
    {
        CoreResponse::ScheduleList { schedules } => schedules,
        other => panic!("unexpected schedule list response: {other:?}"),
    };
    let summary = schedules
        .iter()
        .find(|schedule| schedule.schedule_id == schedule_id)
        .expect("created schedule is listed in its workspace");

    let detail = match daemon
        .handle_request(codegg::core::new_request(
            "tui-schedule-get".into(),
            CoreRequest::ScheduleGet {
                schedule_id: schedule_id.clone(),
            },
        ))
        .await
        .unwrap()
    {
        CoreResponse::ScheduleGet { schedule } => schedule,
        other => panic!("unexpected schedule get response: {other:?}"),
    };
    assert_eq!(schedule_label(summary, Some(&detail)), prompt);

    let displayed_token = schedule_display_id(&schedule_id);
    let resolved_id = resolve_schedule_id(&displayed_token, &schedules).unwrap();
    assert_eq!(resolved_id, schedule_id);

    let deleted = daemon
        .handle_request(codegg::core::new_request(
            "tui-schedule-delete".into(),
            CoreRequest::ScheduleDelete {
                schedule_id: resolved_id,
            },
        ))
        .await
        .unwrap();
    assert!(matches!(
        deleted,
        CoreResponse::ScheduleDeleted { schedule_id: deleted_id }
            if deleted_id == schedule_id
    ));

    let remaining = daemon
        .handle_request(codegg::core::new_request(
            "tui-schedule-list-after-delete".into(),
            CoreRequest::ScheduleList {
                workspace_id: Some(workspace_id),
                include_archived: false,
            },
        ))
        .await
        .unwrap();
    assert!(matches!(
        remaining,
        CoreResponse::ScheduleList { schedules }
            if schedules.iter().all(|schedule| schedule.schedule_id != schedule_id)
    ));
}
