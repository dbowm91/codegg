//! Research handler functions for the TUI.

use crate::tui::app::App;
use crate::tui::app::TuiCommand;
use crate::tui::async_cmd::spawn_registered_tui_task;
use crate::tui::task_lifecycle::TuiTaskKind;

#[allow(dead_code)]
pub(crate) async fn handle_research_list_runs(app: &mut App) {
    let project_dir = app.session_state.project_dir.clone();
    let service =
        crate::research::service::ResearchService::new(std::path::PathBuf::from(&project_dir));
    match service.list_runs().await {
        Ok(runs) => {
            if let Some(ref mut browser) = app.dialog_state.research_browser {
                browser.loading = false;
                browser.set_runs(runs);
            } else {
                app.messages_state
                    .toasts
                    .info("No research browser dialog open");
            }
        }
        Err(e) => {
            app.messages_state
                .toasts
                .error(&format!("Failed to list research runs: {}", e));
            if let Some(ref mut browser) = app.dialog_state.research_browser {
                browser.loading = false;
            }
        }
    }
}

#[allow(dead_code)]
pub(crate) async fn handle_research_load_run(app: &mut App, run_id: String) {
    let project_dir = app.session_state.project_dir.clone();
    let service =
        crate::research::service::ResearchService::new(std::path::PathBuf::from(&project_dir));
    match service.load_run(&run_id).await {
        Ok(bundle) => {
            if let Some(ref mut browser) = app.dialog_state.research_browser {
                browser.loading = false;
                browser.set_bundle(bundle);
            } else {
                app.messages_state
                    .toasts
                    .info("No research browser dialog open");
            }
        }
        Err(e) => {
            app.messages_state
                .toasts
                .error(&format!("Failed to load research run: {}", e));
            if let Some(ref mut browser) = app.dialog_state.research_browser {
                browser.loading = false;
            }
        }
    }
}

#[allow(dead_code)]
pub(crate) async fn handle_research_load_section(app: &mut App, run_id: String, section: String) {
    let project_dir = app.session_state.project_dir.clone();
    let service =
        crate::research::service::ResearchService::new(std::path::PathBuf::from(&project_dir));

    let result = match section.as_str() {
        "Research Plan" => {
            if let Ok(bundle) = service.load_run(&run_id).await {
                if let Some(ref plan) = bundle.plan {
                    let content = format!(
                        "Scope: {}\n\nComparison Axes:\n{}\n\nSource Classes:\n{}\n\nExclusion Criteria:\n{}\n\nStopping Conditions:\n{}\n\nExpected Outputs:\n{}",
                        plan.scope,
                        plan.comparison_axes.iter().map(|s| format!("  - {}", s)).collect::<Vec<_>>().join("\n"),
                        plan.source_classes.iter().map(|s| format!("  - {}", s)).collect::<Vec<_>>().join("\n"),
                        plan.exclusion_criteria.iter().map(|s| format!("  - {}", s)).collect::<Vec<_>>().join("\n"),
                        plan.stopping_conditions.iter().map(|s| format!("  - {}", s)).collect::<Vec<_>>().join("\n"),
                        plan.expected_outputs.iter().map(|s| format!("  - {}", s)).collect::<Vec<_>>().join("\n"),
                    );
                    Some((
                        crate::tui::components::dialogs::research::ReportSection::Report,
                        content,
                    ))
                } else {
                    None
                }
            } else {
                None
            }
        }
        "Sources" => {
            if let Ok(bundle) = service.load_run(&run_id).await {
                if bundle.sources.is_empty() {
                    Some((
                        crate::tui::components::dialogs::research::ReportSection::Brief,
                        "No sources collected.".to_string(),
                    ))
                } else {
                    let lines: Vec<String> = bundle
                        .sources
                        .iter()
                        .enumerate()
                        .map(|(i, src)| {
                            let title = src.title.as_deref().unwrap_or(&src.uri);
                            format!(
                                "{}. {} [{:?}]\n   URI: {}",
                                i + 1,
                                title,
                                src.source_type,
                                src.uri
                            )
                        })
                        .collect();
                    Some((
                        crate::tui::components::dialogs::research::ReportSection::Brief,
                        lines.join("\n\n"),
                    ))
                }
            } else {
                None
            }
        }
        "Claims" => {
            if let Ok(bundle) = service.load_run(&run_id).await {
                if bundle.claims.is_empty() {
                    Some((
                        crate::tui::components::dialogs::research::ReportSection::AgentAnswer,
                        "No claims derived.".to_string(),
                    ))
                } else {
                    let lines: Vec<String> = bundle.claims.iter().map(|claim| {
                        format!("[{}] {} (confidence: {:?})\n   Evidence: {} sources\n   Caveats: {}",
                            claim.claim_type.as_str(), claim.text, claim.confidence,
                            claim.evidence_ids.len(),
                            if claim.caveats.is_empty() { "none".to_string() } else { claim.caveats.join("; ") })
                    }).collect();
                    Some((
                        crate::tui::components::dialogs::research::ReportSection::AgentAnswer,
                        lines.join("\n\n"),
                    ))
                }
            } else {
                None
            }
        }
        "Contradictions" => {
            if let Ok(bundle) = service.load_run(&run_id).await {
                if bundle.contradictions.is_empty() {
                    Some((
                        crate::tui::components::dialogs::research::ReportSection::Handoff,
                        "No contradictions detected.".to_string(),
                    ))
                } else {
                    let lines: Vec<String> = bundle
                        .contradictions
                        .iter()
                        .map(|c| {
                            format!(
                                "[{:?}] {}\n   Claims: {}",
                                c.severity,
                                c.description,
                                c.claim_ids.join(", ")
                            )
                        })
                        .collect();
                    Some((
                        crate::tui::components::dialogs::research::ReportSection::Handoff,
                        lines.join("\n\n"),
                    ))
                }
            } else {
                None
            }
        }
        _ => None,
    };

    if let Some(ref mut browser) = app.dialog_state.research_browser {
        if let Some((section, content)) = result {
            browser.set_report_content(section, content);
        } else {
            app.messages_state
                .toasts
                .warning("Could not load section content");
        }
    }
}

pub(crate) fn start_research_list_runs(app: &mut App) {
    let request_id = app.dialog_state.research_request.begin();

    if let Some(ref mut browser) = app.dialog_state.research_browser {
        browser.loading = true;
    }

    let project_dir = app.session_state.project_dir.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Research,
        "research_list_runs",
        async move {
            let service = crate::research::service::ResearchService::new(std::path::PathBuf::from(
                &project_dir,
            ));
            match service.list_runs().await {
                Ok(runs) => Some(TuiCommand::ResearchRunsLoaded {
                    request_id,
                    runs,
                    error: None,
                }),
                Err(e) => Some(TuiCommand::ResearchRunsLoaded {
                    request_id,
                    runs: Vec::new(),
                    error: Some(format!("Failed to list research runs: {}", e)),
                }),
            }
        },
    );
}

pub(crate) fn apply_research_runs_loaded(
    app: &mut App,
    request_id: u64,
    runs: Vec<crate::research::service::ResearchRunSummary>,
    error: Option<String>,
) {
    if !app.dialog_state.research_request.is_current(request_id) {
        return;
    }
    if let Some(ref mut browser) = app.dialog_state.research_browser {
        browser.loading = false;
        if let Some(err) = error {
            app.messages_state.toasts.error(&err);
        } else {
            browser.set_runs(runs);
        }
    }
}

pub(crate) fn start_research_load_run(app: &mut App, run_id: String) {
    let request_id = app.dialog_state.research_request.begin();

    if let Some(ref mut browser) = app.dialog_state.research_browser {
        browser.loading = true;
    }

    let project_dir = app.session_state.project_dir.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Research,
        "research_load_run",
        async move {
            let service = crate::research::service::ResearchService::new(std::path::PathBuf::from(
                &project_dir,
            ));
            match service.load_run(&run_id).await {
                Ok(bundle) => Some(TuiCommand::ResearchRunLoaded {
                    request_id,
                    run_id,
                    bundle: Some(Box::new(bundle)),
                    error: None,
                }),
                Err(e) => Some(TuiCommand::ResearchRunLoaded {
                    request_id,
                    run_id,
                    bundle: None,
                    error: Some(format!("Failed to load research run: {}", e)),
                }),
            }
        },
    );
}

pub(crate) fn apply_research_run_loaded(
    app: &mut App,
    request_id: u64,
    _run_id: String,
    bundle: Option<Box<crate::research::types::ResearchBundle>>,
    error: Option<String>,
) {
    if !app.dialog_state.research_request.is_current(request_id) {
        return;
    }
    if let Some(ref mut browser) = app.dialog_state.research_browser {
        browser.loading = false;
        if let Some(err) = error {
            app.messages_state.toasts.error(&err);
        } else if let Some(bundle) = bundle {
            browser.set_bundle(*bundle);
        }
    }
}

pub(crate) fn start_research_load_section(app: &mut App, run_id: String, section: String) {
    let request_id = app.dialog_state.research_request.begin();

    let project_dir = app.session_state.project_dir.clone();
    let tx = app.tui_cmd_tx.clone();

    spawn_registered_tui_task(
        tx,
        &mut app.task_registry,
        TuiTaskKind::Research,
        "research_load_section",
        async move {
            let service = crate::research::service::ResearchService::new(std::path::PathBuf::from(
                &project_dir,
            ));

            let result = match section.as_str() {
                "Research Plan" => {
                    if let Ok(bundle) = service.load_run(&run_id).await {
                        if let Some(ref plan) = bundle.plan {
                            let content = format!(
                            "Scope: {}\n\nComparison Axes:\n{}\n\nSource Classes:\n{}\n\nExclusion Criteria:\n{}\n\nStopping Conditions:\n{}\n\nExpected Outputs:\n{}",
                            plan.scope,
                            plan.comparison_axes.iter().map(|s| format!("  - {}", s)).collect::<Vec<_>>().join("\n"),
                            plan.source_classes.iter().map(|s| format!("  - {}", s)).collect::<Vec<_>>().join("\n"),
                            plan.exclusion_criteria.iter().map(|s| format!("  - {}", s)).collect::<Vec<_>>().join("\n"),
                            plan.stopping_conditions.iter().map(|s| format!("  - {}", s)).collect::<Vec<_>>().join("\n"),
                            plan.expected_outputs.iter().map(|s| format!("  - {}", s)).collect::<Vec<_>>().join("\n"),
                        );
                            Some((
                                crate::tui::components::dialogs::research::ReportSection::Report,
                                content,
                            ))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                }
                "Sources" => {
                    if let Ok(bundle) = service.load_run(&run_id).await {
                        if bundle.sources.is_empty() {
                            Some((
                                crate::tui::components::dialogs::research::ReportSection::Brief,
                                "No sources collected.".to_string(),
                            ))
                        } else {
                            let lines: Vec<String> = bundle
                                .sources
                                .iter()
                                .enumerate()
                                .map(|(i, src)| {
                                    let title = src.title.as_deref().unwrap_or(&src.uri);
                                    format!(
                                        "{}. {} [{:?}]\n   URI: {}",
                                        i + 1,
                                        title,
                                        src.source_type,
                                        src.uri
                                    )
                                })
                                .collect();
                            Some((
                                crate::tui::components::dialogs::research::ReportSection::Brief,
                                lines.join("\n\n"),
                            ))
                        }
                    } else {
                        None
                    }
                }
                "Claims" => {
                    if let Ok(bundle) = service.load_run(&run_id).await {
                        if bundle.claims.is_empty() {
                            Some((
                            crate::tui::components::dialogs::research::ReportSection::AgentAnswer,
                            "No claims derived.".to_string(),
                        ))
                        } else {
                            let lines: Vec<String> = bundle.claims.iter().map(|claim| {
                            format!("[{}] {} (confidence: {:?})\n   Evidence: {} sources\n   Caveats: {}",
                                claim.claim_type.as_str(), claim.text, claim.confidence,
                                claim.evidence_ids.len(),
                                if claim.caveats.is_empty() { "none".to_string() } else { claim.caveats.join("; ") })
                        }).collect();
                            Some((
                            crate::tui::components::dialogs::research::ReportSection::AgentAnswer,
                            lines.join("\n\n"),
                        ))
                        }
                    } else {
                        None
                    }
                }
                "Contradictions" => {
                    if let Ok(bundle) = service.load_run(&run_id).await {
                        if bundle.contradictions.is_empty() {
                            Some((
                                crate::tui::components::dialogs::research::ReportSection::Handoff,
                                "No contradictions detected.".to_string(),
                            ))
                        } else {
                            let lines: Vec<String> = bundle
                                .contradictions
                                .iter()
                                .map(|c| {
                                    format!(
                                        "[{:?}] {}\n   Claims: {}",
                                        c.severity,
                                        c.description,
                                        c.claim_ids.join(", ")
                                    )
                                })
                                .collect();
                            Some((
                                crate::tui::components::dialogs::research::ReportSection::Handoff,
                                lines.join("\n\n"),
                            ))
                        }
                    } else {
                        None
                    }
                }
                _ => None,
            };

            Some(TuiCommand::ResearchSectionLoaded {
                request_id,
                section,
                content: result,
                error: None,
            })
        },
    );
}

pub(crate) fn apply_research_section_loaded(
    app: &mut App,
    request_id: u64,
    _section: String,
    content: Option<(
        crate::tui::components::dialogs::research::ReportSection,
        String,
    )>,
    error: Option<String>,
) {
    if !app.dialog_state.research_request.is_current(request_id) {
        return;
    }
    if let Some(ref mut browser) = app.dialog_state.research_browser {
        if let Some(err) = error {
            app.messages_state.toasts.warning(&err);
        } else if let Some((section_type, content)) = content {
            browser.set_report_content(section_type, content);
        } else {
            app.messages_state
                .toasts
                .warning("Could not load section content");
        }
    }
}
