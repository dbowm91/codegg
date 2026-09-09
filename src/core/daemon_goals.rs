//! M002: `goals` request family for `CoreDaemon`.
//!
//! Session goals, todos, edit checkpoints, and LSP preview apply over daemon-owned domain stores.
//! Operates on the same daemon-owned state as the thin dispatcher;
//! introduces no new store, scheduler, state machine, or authority.

use crate::error::AppError;
use crate::protocol::core::{CoreRequest, CoreResponse};

use super::daemon::CoreDaemon;

impl CoreDaemon {
    pub(crate) async fn handle_goals_request(
        &self,
        request: CoreRequest,
        request_id: &str,
        trusted_client_id: &str,
        authority: codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: codegg_core::authorization::AuthorizationDecision,
    ) -> Result<CoreResponse, AppError> {
        let _ = (request_id, trusted_client_id, &authority, &authz_decision);
        match request {
            CoreRequest::GoalSet {
                session_id,
                project_id,
                objective,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let goal_store = crate::goal::GoalStore::new(pool.clone());
                let title = objective
                    .lines()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or(&objective)
                    .chars()
                    .take(80)
                    .collect::<String>();
                let completion_criteria = vec![
                    "Implementation satisfies the stated objective.".to_string(),
                    "Relevant tests or checks have been run, or skipped with justification."
                        .to_string(),
                    "Checkpoint/progress state is updated.".to_string(),
                ];
                match goal_store
                    .create_active(
                        &session_id,
                        &project_id,
                        &title,
                        &objective,
                        None,
                        None,
                        completion_criteria,
                    )
                    .await
                {
                    Ok(goal) => {
                        let project_path = std::path::PathBuf::from(&project_id);
                        let checkpoint_path = match crate::goal::checkpoint::create_checkpoint_file(
                            &project_path,
                            &goal,
                            None,
                        )
                        .await
                        {
                            Ok(path) => Some(path.to_string_lossy().to_string()),
                            Err(_) => None,
                        };
                        if let Some(ref cp) = checkpoint_path {
                            if let Err(error) =
                                sqlx::query("UPDATE goal SET checkpoint_path = ? WHERE id = ?")
                                    .bind(cp)
                                    .bind(&goal.id)
                                    .execute(&pool)
                                    .await
                            {
                                tracing::error!(
                                    goal_id = %goal.id,
                                    checkpoint_path = %cp,
                                    ?error,
                                    "failed to record goal checkpoint path"
                                );
                                return Ok(CoreResponse::Error {
                                    code: "goal_checkpoint_failed".to_string(),
                                    message: format!("failed to record checkpoint path: {error}"),
                                });
                            }
                        }
                        let updated = match goal_store.get(&goal.id).await {
                            Ok(Some(g)) => Some(g),
                            Ok(None) => None,
                            Err(e) => {
                                tracing::warn!(error = %e, id = %goal.id, "failed to load goal after creation");
                                None
                            }
                        };
                        super::publish_goal_updated(&session_id, updated);
                        Ok(CoreResponse::Json {
                            data: serde_json::json!({
                                "status": "active",
                                "id": goal.id,
                                "title": title,
                                "checkpoint_path": checkpoint_path,
                            }),
                        })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "goal_create_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::GoalFromFile {
                session_id,
                project_id,
                path,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let file_path = if std::path::Path::new(&path).is_absolute() {
                    std::path::PathBuf::from(&path)
                } else {
                    std::path::PathBuf::from(&project_id).join(&path)
                };
                let content = match tokio::fs::read_to_string(&file_path).await {
                    Ok(c) => c,
                    Err(e) => {
                        return Ok(CoreResponse::Error {
                            code: "file_read_failed".to_string(),
                            message: format!("Failed to read {}: {}", path, e),
                        })
                    }
                };
                let title = content
                    .lines()
                    .find(|l| l.starts_with('#'))
                    .map(|l| {
                        l.trim_start_matches('#')
                            .trim()
                            .chars()
                            .take(80)
                            .collect::<String>()
                    })
                    .unwrap_or_else(|| {
                        std::path::Path::new(&path)
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_else(|| "Goal from file".to_string())
                    });
                let objective = format!("Follow implementation plan from {}", path);
                let completion_criteria = vec![
                    "All phases in the plan file that are in scope are completed.".to_string(),
                    "Tests/checks specified in the plan have been run.".to_string(),
                    "Goal checkpoint is updated with completed/remaining work.".to_string(),
                ];
                let plan_excerpt = if content.len() > 4000 {
                    Some(crate::util::truncate_prefix(&content, 4000))
                } else {
                    Some(content.as_str())
                };
                let goal_store = crate::goal::GoalStore::new(pool.clone());
                match goal_store
                    .create_active(
                        &session_id,
                        &project_id,
                        &title,
                        &objective,
                        Some(path),
                        None,
                        completion_criteria,
                    )
                    .await
                {
                    Ok(goal) => {
                        let project_path = std::path::PathBuf::from(&project_id);
                        let checkpoint_path = match crate::goal::checkpoint::create_checkpoint_file(
                            &project_path,
                            &goal,
                            plan_excerpt,
                        )
                        .await
                        {
                            Ok(path) => Some(path.to_string_lossy().to_string()),
                            Err(_) => None,
                        };
                        if let Some(ref cp) = checkpoint_path {
                            if let Err(error) =
                                sqlx::query("UPDATE goal SET checkpoint_path = ? WHERE id = ?")
                                    .bind(cp)
                                    .bind(&goal.id)
                                    .execute(&pool)
                                    .await
                            {
                                tracing::error!(
                                    goal_id = %goal.id,
                                    checkpoint_path = %cp,
                                    ?error,
                                    "failed to record goal checkpoint path"
                                );
                                return Ok(CoreResponse::Error {
                                    code: "goal_checkpoint_failed".to_string(),
                                    message: format!("failed to record checkpoint path: {error}"),
                                });
                            }
                        }
                        let updated = match goal_store.get(&goal.id).await {
                            Ok(Some(g)) => Some(g),
                            Ok(None) => None,
                            Err(e) => {
                                tracing::warn!(error = %e, id = %goal.id, "failed to load goal after creation");
                                None
                            }
                        };
                        super::publish_goal_updated(&session_id, updated);
                        Ok(CoreResponse::Json {
                            data: serde_json::json!({
                                "status": "active",
                                "id": goal.id,
                                "title": goal.title,
                                "checkpoint_path": checkpoint_path,
                            }),
                        })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "goal_create_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::GoalShow { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let goal_store = crate::goal::GoalStore::new(pool);
                match goal_store.active_for_session(&session_id).await {
                    Ok(Some(goal)) => {
                        let checkpoint_excerpt = if let Some(ref path) = goal.checkpoint_path {
                            crate::goal::checkpoint::read_checkpoint_excerpt(path, 4000)
                                .await
                                .ok()
                                .flatten()
                        } else {
                            None
                        };
                        let rendered = crate::goal::render::render_goal_status(&goal);
                        Ok(CoreResponse::Json {
                            data: serde_json::json!({
                                "goal": serde_json::to_value(&goal).unwrap_or_default(),
                                "rendered": rendered,
                                "checkpoint_excerpt": checkpoint_excerpt,
                            }),
                        })
                    }
                    Ok(None) => Ok(CoreResponse::Json {
                        data: serde_json::json!({ "active": false }),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "goal_show_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::GoalPause { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let goal_store = crate::goal::GoalStore::new(pool);
                match goal_store.active_for_session(&session_id).await {
                    Ok(Some(goal)) => {
                        match goal_store
                            .update_status(&goal.id, crate::goal::GoalStatus::Paused)
                            .await
                        {
                            Ok(Some(updated)) => {
                                super::publish_goal_updated(&session_id, Some(updated));
                                Ok(CoreResponse::Json {
                                    data: serde_json::json!({ "status": "paused", "id": goal.id }),
                                })
                            }
                            Ok(None) => {
                                super::publish_goal_updated(&session_id, None);
                                Ok(CoreResponse::Json {
                                    data: serde_json::json!({ "status": "paused", "id": goal.id }),
                                })
                            }
                            Err(e) => Ok(CoreResponse::Error {
                                code: "goal_pause_failed".to_string(),
                                message: e.to_string(),
                            }),
                        }
                    }
                    Ok(None) => Ok(CoreResponse::Error {
                        code: "no_active_goal".to_string(),
                        message: "No active goal to pause".to_string(),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "goal_pause_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::GoalResume { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let goal_store = crate::goal::GoalStore::new(pool);
                match goal_store.latest_paused_for_session(&session_id).await {
                    Ok(Some(goal)) => {
                        match goal_store
                            .update_status(&goal.id, crate::goal::GoalStatus::Active)
                            .await
                        {
                            Ok(Some(updated)) => {
                                super::publish_goal_updated(&session_id, Some(updated));
                                Ok(CoreResponse::Json {
                                    data: serde_json::json!({ "status": "active", "id": goal.id }),
                                })
                            }
                            Ok(None) => {
                                super::publish_goal_updated(&session_id, None);
                                Ok(CoreResponse::Json {
                                    data: serde_json::json!({ "status": "active", "id": goal.id }),
                                })
                            }
                            Err(e) => Ok(CoreResponse::Error {
                                code: "goal_resume_failed".to_string(),
                                message: e.to_string(),
                            }),
                        }
                    }
                    Ok(None) => Ok(CoreResponse::Error {
                        code: "no_paused_goal".to_string(),
                        message: "No paused goal to resume".to_string(),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "goal_resume_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::GoalClear { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let goal_store = crate::goal::GoalStore::new(pool);
                match goal_store.clear_active_for_session(&session_id).await {
                    Ok(()) => {
                        super::publish_goal_updated(&session_id, None);
                        Ok(CoreResponse::Json {
                            data: serde_json::json!({ "cleared": true }),
                        })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "goal_clear_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::GoalDone { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let goal_store = crate::goal::GoalStore::new(pool);
                match goal_store.active_for_session(&session_id).await {
                    Ok(Some(goal)) => {
                        match goal_store.complete_if_active(&goal.id, goal.revision).await {
                            Ok(Some(updated)) => {
                                super::publish_goal_updated(&session_id, Some(updated.clone()));
                                crate::bus::global::GlobalEventBus::publish(
                                    crate::bus::events::AppEvent::GoalCompleted {
                                        session_id: session_id.clone(),
                                        goal_id: goal.id.clone(),
                                        evidence: "marked complete via /goal done".to_string(),
                                    },
                                );
                                Ok(CoreResponse::Json {
                                    data: serde_json::json!({ "status": "complete", "id": goal.id }),
                                })
                            }
                            Ok(None) => {
                                super::publish_goal_updated(&session_id, None);
                                Ok(CoreResponse::Json {
                                    data: serde_json::json!({ "status": "complete", "id": goal.id }),
                                })
                            }
                            Err(e) => Ok(CoreResponse::Error {
                                code: "goal_done_failed".to_string(),
                                message: e.to_string(),
                            }),
                        }
                    }
                    Ok(None) => Ok(CoreResponse::Error {
                        code: "no_active_goal".to_string(),
                        message: "No active goal to mark done".to_string(),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "goal_done_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::GoalCheckpoint {
                session_id,
                project_id,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let goal_store = crate::goal::GoalStore::new(pool);
                match goal_store.active_for_session(&session_id).await {
                    Ok(Some(goal)) => {
                        if let Some(ref cp_path) = goal.checkpoint_path {
                            let update = crate::goal::GoalProgressUpdate {
                                current_phase: goal.current_phase.clone(),
                                progress_summary: Some(goal.progress_summary.clone()),
                                next_action: goal.next_action.clone(),
                                completed_items: vec![],
                                remaining_items: vec![],
                                open_questions: goal.open_questions.clone(),
                            };
                            match crate::goal::checkpoint::append_checkpoint_update(
                                cp_path, &update,
                            )
                            .await
                            {
                                Ok(()) => Ok(CoreResponse::Json {
                                    data: serde_json::json!({ "checkpoint_path": cp_path, "appended": true }),
                                }),
                                Err(error) => {
                                    tracing::warn!(error = %error, checkpoint_path = %cp_path, "failed to append goal checkpoint update");
                                    Ok(CoreResponse::Error {
                                        code: "goal_checkpoint_update_failed".to_string(),
                                        message: error.to_string(),
                                    })
                                }
                            }
                        } else {
                            let project_path = std::path::PathBuf::from(&project_id);
                            match crate::goal::checkpoint::create_checkpoint_file(
                                &project_path,
                                &goal,
                                None,
                            )
                            .await
                            {
                                Ok(path) => {
                                    let path_str = path.to_string_lossy().to_string();
                                    if let Err(error) = sqlx::query(
                                        "UPDATE goal SET checkpoint_path = ? WHERE id = ?",
                                    )
                                    .bind(&path_str)
                                    .bind(&goal.id)
                                    .execute(&goal_store.pool)
                                    .await
                                    {
                                        tracing::error!(
                                            goal_id = %goal.id,
                                            checkpoint_path = %path_str,
                                            ?error,
                                            "failed to record goal checkpoint path"
                                        );
                                        return Ok(CoreResponse::Error {
                                            code: "goal_checkpoint_failed".to_string(),
                                            message: format!(
                                                "failed to record checkpoint path: {error}"
                                            ),
                                        });
                                    }
                                    Ok(CoreResponse::Json {
                                        data: serde_json::json!({ "checkpoint_path": path_str, "created": true }),
                                    })
                                }
                                Err(e) => Ok(CoreResponse::Error {
                                    code: "goal_checkpoint_failed".to_string(),
                                    message: e.to_string(),
                                }),
                            }
                        }
                    }
                    Ok(None) => Ok(CoreResponse::Error {
                        code: "no_active_goal".to_string(),
                        message: "No active goal".to_string(),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "goal_checkpoint_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::TodoList { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::store::TodoStore::new(pool);
                match store.list(&session_id).await {
                    Ok(items) => {
                        let snapshots: Vec<crate::bus::events::TodoItemSnapshot> = items
                            .iter()
                            .enumerate()
                            .map(|(i, item)| {
                                use crate::bus::events::TodoItemSnapshot;
                                TodoItemSnapshot {
                                    id: format!("pos-{}", i),
                                    content: item.content.clone(),
                                    status: item.status.clone(),
                                    priority: item.priority.clone(),
                                }
                            })
                            .collect();
                        Ok(CoreResponse::Json {
                            data: serde_json::json!({
                                "items": serde_json::to_value(&snapshots)
                                    .unwrap_or(serde_json::Value::Array(vec![])),
                            }),
                        })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "todo_list_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::ActiveGoalLoad { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let goal_store = crate::goal::GoalStore::new(pool);
                match goal_store.active_for_session(&session_id).await {
                    Ok(Some(goal)) => Ok(CoreResponse::Json {
                        data: serde_json::json!({
                            "active": true,
                            "goal": serde_json::to_value(goal.to_snapshot())
                                .unwrap_or(serde_json::Value::Null),
                        }),
                    }),
                    Ok(None) => Ok(CoreResponse::Json {
                        data: serde_json::json!({ "active": false }),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "active_goal_load_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::GoalSetBudget {
                session_id,
                max_turns,
                max_model_tokens,
                max_tool_calls,
                max_wallclock_secs,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let goal_store = crate::goal::GoalStore::new(pool);
                match goal_store.active_for_session(&session_id).await {
                    Ok(Some(goal)) => {
                        let new_budget = crate::goal::model::GoalBudget {
                            max_turns,
                            max_model_tokens,
                            max_tool_calls,
                            max_wallclock_secs,
                        };
                        match goal_store.set_budget(&goal.id, new_budget).await {
                            Ok(Some(updated)) => {
                                super::publish_goal_updated(&session_id, Some(updated));
                                Ok(CoreResponse::Json {
                                    data: serde_json::json!({ "status": "ok", "id": goal.id }),
                                })
                            }
                            Ok(None) => Ok(CoreResponse::Json {
                                data: serde_json::json!({ "status": "ok", "id": goal.id }),
                            }),
                            Err(e) => Ok(CoreResponse::Error {
                                code: "goal_set_budget_failed".to_string(),
                                message: e.to_string(),
                            }),
                        }
                    }
                    Ok(None) => Ok(CoreResponse::Error {
                        code: "no_active_goal".to_string(),
                        message: "No active goal to update budget".to_string(),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "goal_set_budget_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::EditCheckpointList {
                workspace_id,
                session_id,
                limit,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".into(),
                        message: "no database pool".into(),
                    });
                };
                let wid = codegg_core::workspace::WorkspaceId::new_unchecked(workspace_id.clone());
                let Some(record) = self.workspaces.resolve(&wid).await else {
                    return Ok(CoreResponse::Error {
                        code: "workspace_not_found".into(),
                        message: format!("workspace {} not found", workspace_id),
                    });
                };
                let mgr = codegg_core::snapshot::checkpoint::EditCheckpointManager::new(
                    pool,
                    record.canonical_root.clone(),
                );
                match mgr.summaries_for_session(&session_id).await {
                    Ok(mut summaries) => {
                        if let Some(lim) = limit {
                            summaries.truncate(lim);
                        } else if summaries.len() > 100 {
                            summaries.truncate(100);
                        }
                        let dtos = summaries
                            .iter()
                            .map(codegg_core::protocol_conversions::checkpoint_summary_to_dto)
                            .collect();
                        Ok(CoreResponse::EditCheckpointList { checkpoints: dtos })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "edit_checkpoint_list_failed".into(),
                        message: e,
                    }),
                }
            }
            CoreRequest::EditCheckpointGet {
                checkpoint_id,
                workspace_id,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".into(),
                        message: "no database pool".into(),
                    });
                };
                let wid = codegg_core::workspace::WorkspaceId::new_unchecked(workspace_id.clone());
                let Some(record) = self.workspaces.resolve(&wid).await else {
                    return Ok(CoreResponse::Error {
                        code: "workspace_not_found".into(),
                        message: format!("workspace {} not found", workspace_id),
                    });
                };
                let mgr = codegg_core::snapshot::checkpoint::EditCheckpointManager::new(
                    pool,
                    record.canonical_root.clone(),
                );
                match mgr.get(&checkpoint_id).await {
                    Ok(Some(cp)) => {
                        if cp.workspace_id != workspace_id {
                            return Ok(CoreResponse::Error {
                                code: "wrong_workspace".into(),
                                message: format!(
                                    "checkpoint workspace {} does not match requested {}",
                                    cp.workspace_id, workspace_id
                                ),
                            });
                        }
                        let dto = codegg_core::protocol_conversions::checkpoint_to_detail_dto(&cp);
                        Ok(CoreResponse::EditCheckpointDetail {
                            checkpoint: Some(dto),
                        })
                    }
                    Ok(None) => Ok(CoreResponse::EditCheckpointDetail { checkpoint: None }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "edit_checkpoint_get_failed".into(),
                        message: e,
                    }),
                }
            }
            CoreRequest::EditCheckpointUndo {
                checkpoint_id,
                workspace_id,
                session_id,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".into(),
                        message: "no database pool".into(),
                    });
                };
                let wid = codegg_core::workspace::WorkspaceId::new_unchecked(workspace_id.clone());
                let Some(record) = self.workspaces.resolve(&wid).await else {
                    return Ok(CoreResponse::Error {
                        code: "workspace_not_found".into(),
                        message: format!("workspace {} not found", workspace_id),
                    });
                };
                let lease = match self.workspace_services.acquire(&wid).await {
                    Ok(l) => l,
                    Err(e) => {
                        return Ok(CoreResponse::Error {
                            code: "workspace_not_active".into(),
                            message: e.to_string(),
                        })
                    }
                };
                let _guard = lease
                    .locks()
                    .acquire_repository(&record.canonical_root)
                    .await;
                let mgr = codegg_core::snapshot::checkpoint::EditCheckpointManager::new(
                    pool,
                    record.canonical_root.clone(),
                );
                match mgr
                    .checked_undo(&checkpoint_id, &workspace_id, Some(&session_id))
                    .await
                {
                    Ok(outcome) => {
                        let dto = codegg_core::protocol_conversions::checked_restore_outcome_to_dto(
                            &outcome,
                        );
                        Ok(CoreResponse::EditCheckpointUndoResult { result: dto })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "edit_undo_failed".into(),
                        message: e,
                    }),
                }
            }
            CoreRequest::EditCheckpointUndoLatest {
                workspace_id,
                session_id,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".into(),
                        message: "no database pool".into(),
                    });
                };
                let wid = codegg_core::workspace::WorkspaceId::new_unchecked(workspace_id.clone());
                let Some(record) = self.workspaces.resolve(&wid).await else {
                    return Ok(CoreResponse::Error {
                        code: "workspace_not_found".into(),
                        message: format!("workspace {} not found", workspace_id),
                    });
                };
                let lease = match self.workspace_services.acquire(&wid).await {
                    Ok(l) => l,
                    Err(e) => {
                        return Ok(CoreResponse::Error {
                            code: "workspace_not_active".into(),
                            message: e.to_string(),
                        })
                    }
                };
                let _guard = lease
                    .locks()
                    .acquire_repository(&record.canonical_root)
                    .await;
                let mgr = codegg_core::snapshot::checkpoint::EditCheckpointManager::new(
                    pool,
                    record.canonical_root.clone(),
                );
                match mgr
                    .undo_latest_for_session(&session_id, &workspace_id)
                    .await
                {
                    Ok(outcome) => {
                        let dto = codegg_core::protocol_conversions::checked_restore_outcome_to_dto(
                            &outcome,
                        );
                        Ok(CoreResponse::EditCheckpointUndoResult { result: dto })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "edit_undo_failed".into(),
                        message: e,
                    }),
                }
            }
            CoreRequest::EditCheckpointReapply {
                checkpoint_id,
                workspace_id,
                session_id,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".into(),
                        message: "no database pool".into(),
                    });
                };
                let wid = codegg_core::workspace::WorkspaceId::new_unchecked(workspace_id.clone());
                let Some(record) = self.workspaces.resolve(&wid).await else {
                    return Ok(CoreResponse::Error {
                        code: "workspace_not_found".into(),
                        message: format!("workspace {} not found", workspace_id),
                    });
                };
                let lease = match self.workspace_services.acquire(&wid).await {
                    Ok(l) => l,
                    Err(e) => {
                        return Ok(CoreResponse::Error {
                            code: "workspace_not_active".into(),
                            message: e.to_string(),
                        })
                    }
                };
                let _guard = lease
                    .locks()
                    .acquire_repository(&record.canonical_root)
                    .await;
                let mgr = codegg_core::snapshot::checkpoint::EditCheckpointManager::new(
                    pool,
                    record.canonical_root.clone(),
                );
                match mgr
                    .checked_reapply(&checkpoint_id, &workspace_id, Some(&session_id))
                    .await
                {
                    Ok(outcome) => {
                        let dto = codegg_core::protocol_conversions::checked_restore_outcome_to_dto(
                            &outcome,
                        );
                        Ok(CoreResponse::EditCheckpointReapplyResult { result: dto })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "edit_reapply_failed".into(),
                        message: e,
                    }),
                }
            }
            CoreRequest::EditCheckpointReapplyLatest {
                workspace_id,
                session_id,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".into(),
                        message: "no database pool".into(),
                    });
                };
                let wid = codegg_core::workspace::WorkspaceId::new_unchecked(workspace_id.clone());
                let Some(record) = self.workspaces.resolve(&wid).await else {
                    return Ok(CoreResponse::Error {
                        code: "workspace_not_found".into(),
                        message: format!("workspace {} not found", workspace_id),
                    });
                };
                let lease = match self.workspace_services.acquire(&wid).await {
                    Ok(l) => l,
                    Err(e) => {
                        return Ok(CoreResponse::Error {
                            code: "workspace_not_active".into(),
                            message: e.to_string(),
                        })
                    }
                };
                let _guard = lease
                    .locks()
                    .acquire_repository(&record.canonical_root)
                    .await;
                let mgr = codegg_core::snapshot::checkpoint::EditCheckpointManager::new(
                    pool,
                    record.canonical_root.clone(),
                );
                match mgr
                    .reapply_latest_undone_for_session(&session_id, &workspace_id)
                    .await
                {
                    Ok(outcome) => {
                        let dto = codegg_core::protocol_conversions::checked_restore_outcome_to_dto(
                            &outcome,
                        );
                        Ok(CoreResponse::EditCheckpointReapplyResult { result: dto })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "edit_reapply_failed".into(),
                        message: e,
                    }),
                }
            }
            CoreRequest::LspPreviewApply { request } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".into(),
                        message: "LSP preview apply requires a durable database pool".into(),
                    });
                };
                if request.workspace_id.is_empty() || request.session_id.is_empty() {
                    return Ok(CoreResponse::Error {
                        code: "invalid_lsp_preview_apply".into(),
                        message: "workspace_id and session_id are required".into(),
                    });
                }
                let workspace_id = codegg_core::workspace::WorkspaceId::new_unchecked(
                    request.workspace_id.clone(),
                );
                let Some(record) = self.workspaces.resolve(&workspace_id).await else {
                    return Ok(CoreResponse::Error {
                        code: "workspace_not_found".into(),
                        message: format!("workspace {} not found", request.workspace_id),
                    });
                };
                let session_workspace = sqlx::query_scalar::<_, Option<String>>(
                    "SELECT workspace_id FROM session WHERE id = ?",
                )
                .bind(&request.session_id)
                .fetch_optional(&pool)
                .await
                .map_err(|error| AppError::Other(anyhow::anyhow!(error)))?;
                if session_workspace.as_ref().and_then(|id| id.as_deref())
                    != Some(request.workspace_id.as_str())
                {
                    return Ok(CoreResponse::Error {
                        code: "session_workspace_mismatch".into(),
                        message: "session is not bound to the requested workspace".into(),
                    });
                }
                let lease = match self.workspace_services.acquire(&workspace_id).await {
                    Ok(lease) => lease,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "workspace_not_active".into(),
                            message: error.to_string(),
                        })
                    }
                };
                match crate::lsp::mutation::apply_preview(
                    request,
                    record.canonical_root.clone(),
                    lease.locks(),
                    pool,
                    self.deps.lsp_service.clone(),
                )
                .await
                {
                    Ok(result) => Ok(CoreResponse::LspPreviewApplyResult { result }),
                    Err(error) => Ok(CoreResponse::Error {
                        code: "lsp_preview_apply_failed".into(),
                        message: error.to_string(),
                    }),
                }
            }
            _ => {
                tracing::warn!("Unhandled CoreRequest variant");
                Ok(CoreResponse::Error {
                    code: "unimplemented".to_string(),
                    message: "This request type is not yet implemented".to_string(),
                })
            }
        }
    }
}
