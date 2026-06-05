use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::error::AppError;
use crate::protocol::core::{
    CoreEvent, CoreRequest, CoreResponse, EventEnvelope, RequestEnvelope, PROTOCOL_VERSION,
};
use crate::provider::{ChatRequest, ProviderRegistry};

pub mod transport;

#[async_trait]
pub trait CoreClient: Send + Sync {
    async fn request(
        &self,
        request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError>;
    fn subscribe(&self) -> mpsc::UnboundedReceiver<EventEnvelope<CoreEvent>>;
}

#[derive(Clone, Default)]
pub struct InprocCoreClient {
    pub subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
    pub memory_store: Option<Arc<crate::memory::MemoryStore>>,
    pub bg_scheduler: Option<Arc<crate::agent::task::BackgroundScheduler>>,
    pub pool: Option<sqlx::SqlitePool>,
}

impl InprocCoreClient {
    pub fn new(
        subagent_pool: Option<Arc<crate::agent::worker::SubAgentPool>>,
        memory_store: Option<Arc<crate::memory::MemoryStore>>,
        bg_scheduler: Option<Arc<crate::agent::task::BackgroundScheduler>>,
        pool: Option<sqlx::SqlitePool>,
    ) -> Self {
        Self {
            subagent_pool,
            memory_store,
            bg_scheduler,
            pool,
        }
    }
}

/// Publish a `GoalUpdated` bus event so the TUI (and any remote
/// subscribers) can reflect the latest goal state. Always pair with a
/// successful goal store write.
fn publish_goal_updated(session_id: &str, goal: Option<crate::goal::model::Goal>) {
    let snap = goal.map(|g| g.to_snapshot());
    crate::bus::global::GlobalEventBus::publish(
        crate::bus::events::AppEvent::GoalUpdated {
            session_id: session_id.to_string(),
            goal: snap,
        },
    );
}

#[async_trait]
impl CoreClient for InprocCoreClient {
    async fn request(
        &self,
        request: RequestEnvelope<CoreRequest>,
    ) -> Result<CoreResponse, AppError> {
        match request.payload {
            CoreRequest::TurnSubmit {
                session_id,
                model,
                agents,
                current_agent_idx,
                messages,
                plan_mode,
                ..
            } => {
                if current_agent_idx >= agents.len() {
                    crate::bus::global::GlobalEventBus::publish(
                        crate::bus::events::AppEvent::Error {
                            message: format!(
                                "Invalid agent index {} for {} agents",
                                current_agent_idx,
                                agents.len()
                            ),
                        },
                    );
                    return Ok(CoreResponse::Error {
                        code: "invalid_agent_index".to_string(),
                        message: "Invalid agent index".to_string(),
                    });
                }
                let mut registry = ProviderRegistry::new();
                let config = crate::config::schema::Config::load().unwrap_or_default();
                crate::provider::register_builtin_with_config(&mut registry, &config);
                let provider_name = model.split('/').next().unwrap_or("openai").to_string();
                let model_name = model.split('/').next_back().unwrap_or(&model).to_string();
                let Some(base_provider) = registry.get(&provider_name) else {
                    crate::bus::global::GlobalEventBus::publish(
                        crate::bus::events::AppEvent::Error {
                            message: format!(
                                "Provider '{}' not found. Please check your configuration.",
                                provider_name
                            ),
                        },
                    );
                    return Ok(CoreResponse::Error {
                        code: "provider_not_found".to_string(),
                        message: format!("Provider not found: {}", provider_name),
                    });
                };
                let provider = base_provider.clone_box();
                // Resolve model profile early for task state policy
                let model_profile = crate::model_profile::ModelProfileResolver::new(&config)
                    .resolve(&model_name);
                let task_state_policy = model_profile.task_state_policy;
                let todo_state = std::sync::Arc::new(tokio::sync::Mutex::new(
                    crate::task_state::TodoState::new(),
                ));
                let mut tool_registry = crate::tool::ToolRegistry::with_session_defaults(
                    todo_state.clone(),
                    task_state_policy.clone(),
                    self.pool.clone(),
                    Some(session_id.clone()),
                );
                if let Some(pool) = self.subagent_pool.clone() {
                    let task_tool = crate::tool::task::TaskTool::new(
                        pool.task_store(),
                        Some(pool.spawner()),
                        Some(session_id.clone()),
                        Vec::new(),
                    );
                    tool_registry.register(task_tool);
                }
                let permission_checker =
                    crate::permission::PermissionChecker::new(Some(&config), None)
                        .with_active_mode(&config);
                let memory_context = self
                    .memory_store
                    .as_ref()
                    .map(|store| {
                        let all_memories = store.list("user/preferences");
                        if all_memories.is_empty() {
                            String::new()
                        } else {
                            let summary: String = all_memories
                                .iter()
                                .take(10)
                                .map(|m| {
                                    format!(
                                        "- [{}] {}",
                                        m.id,
                                        m.title.as_deref().unwrap_or("(untitled)")
                                    )
                                })
                                .collect::<Vec<_>>()
                                .join("\n");
                            format!("\n\n## Learned Preferences\n{}\n", summary)
                        }
                    })
                    .unwrap_or_default();
                let mut system = crate::agent::prompt::load_agent_prompt(
                    &agents[current_agent_idx],
                    &config,
                    &model_name,
                );
                system.push_str(&memory_context);

                // Inject active goal context into system prompt
                let goal_context = if let Some(pool) = self.pool.clone() {
                    let goal_store = crate::goal::GoalStore::new(pool.clone());
                    match goal_store.active_for_session(&session_id).await {
                        Ok(Some(goal)) if goal.status == crate::goal::GoalStatus::Active => {
                            let checkpoint_excerpt = if let Some(ref path) = goal.checkpoint_path {
                                crate::goal::checkpoint::read_checkpoint_excerpt(path, 4000)
                                    .await
                                    .ok()
                                    .flatten()
                            } else {
                                None
                            };
                            crate::goal::render::render_goal_context(
                                &goal,
                                checkpoint_excerpt.as_deref(),
                            )
                        }
                        _ => String::new(),
                    }
                } else {
                    String::new()
                };
                system.push_str(&goal_context);

                // Inject plan mode contract if the user submitted this turn
                // with plan_mode enabled. The agent loop's `filter_tools_for_model`
                // also hides mutating tools, but the model needs explicit text
                // awareness so it doesn't try to use shell commands or
                // write tools that don't exist in its schema.
                if plan_mode {
                    system.push_str("\n\n");
                    system.push_str(crate::agent::prompt::plan_mode_contract());
                }

                // Register session-scoped goal tools
                if let Some(pool) = self.pool.clone() {
                    tool_registry.register(crate::goal::tool::GoalGetTool::new(
                        pool.clone(),
                        session_id.clone(),
                    ));
                    tool_registry.register(crate::goal::tool::GoalUpdateProgressTool::new(
                        pool.clone(),
                        session_id.clone(),
                    ));
                    tool_registry.register(crate::goal::tool::GoalRequestCompletionTool::new(
                        pool,
                        session_id.clone(),
                    ));
                }
                let mut agent_loop = crate::agent::r#loop::AgentLoop::new(
                    agents,
                    provider,
                    permission_checker,
                    tool_registry,
                    config,
                    None,
                    self.pool.clone(),
                );
                agent_loop.set_session_id(&session_id);
                if let Some(ref pool) = self.subagent_pool {
                    agent_loop.set_subagent_pool(Arc::clone(pool));
                }
                agent_loop.set_task_state_policy(task_state_policy);
                agent_loop.load_persisted_todos().await;
                let request = ChatRequest {
                    messages,
                    model: model_name,
                    tools: None,
                    system: Some(system),
                    temperature: None,
                    top_p: None,
                    max_tokens: None,
                    response_format: None,
                    thinking_budget: None,
                    reasoning_effort: None,
                };
                tokio::spawn(async move {
                    if let Err(e) = agent_loop.run(request).await {
                        tracing::error!("Agent loop error: {}", e);
                        crate::bus::global::GlobalEventBus::publish(
                            crate::bus::events::AppEvent::Error {
                                message: format!("Agent error: {}", e),
                            },
                        );
                    } else {
                        crate::bus::global::GlobalEventBus::publish(
                            crate::bus::events::AppEvent::AgentFinished {
                                session_id,
                                stop_reason: "completed".to_string(),
                                input_tokens: None,
                                output_tokens: None,
                                cached_tokens: None,
                                reasoning_tokens: None,
                            },
                        );
                    }
                });
                Ok(CoreResponse::Ack)
            }
            CoreRequest::SessionMessagesLoad { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::MessageStore::new(pool);
                match store.list(&session_id).await {
                    Ok(messages) => Ok(CoreResponse::SessionMessages {
                        session_id,
                        messages,
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_messages_load_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionMessageCounts { session_ids } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                match store.message_counts(&session_ids).await {
                    Ok(counts) => Ok(CoreResponse::SessionMessageCounts { counts }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_message_counts_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionCreate { directory, title } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                match store
                    .create(crate::session::CreateSession {
                        project_id: directory.clone(),
                        directory,
                        title,
                        parent_id: None,
                        workspace_id: None,
                        agent: None,
                        model: None,
                        tags: None,
                    })
                    .await
                {
                    Ok(session) => Ok(CoreResponse::Session { session }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_create_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionLoad { session_id } | CoreRequest::SessionAttach { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                match store.get(&session_id).await {
                    Ok(Some(session)) => Ok(CoreResponse::Session { session }),
                    Ok(None) => Ok(CoreResponse::Error {
                        code: "session_not_found".to_string(),
                        message: format!("Session not found: {}", session_id),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_load_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionList {
                project_id,
                show_archived,
                limit,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                let sessions = if show_archived {
                    store.list_all(&project_id, None).await
                } else {
                    store.list(&project_id, limit).await
                };
                match sessions {
                    Ok(sessions) => Ok(CoreResponse::SessionList { sessions }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_list_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionFork { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                match store.fork(&session_id).await {
                    Ok(_) => Ok(CoreResponse::Ack),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_fork_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionDelete {
                session_id,
                permanent,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                let result = if permanent {
                    store.delete(&session_id).await.map(|_| ())
                } else {
                    store.soft_delete(&session_id).await.map(|_| ())
                };
                match result {
                    Ok(()) => Ok(CoreResponse::Ack),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_delete_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionArchive {
                session_id,
                unarchive,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                let result = if unarchive {
                    store.unarchive(&session_id).await
                } else {
                    store.archive(&session_id).await
                };
                match result {
                    Ok(_) => Ok(CoreResponse::Ack),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_archive_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionRestore { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                match store.restore(&session_id).await {
                    Ok(session) => Ok(CoreResponse::Session { session }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_restore_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionShare { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                match store.share_session(&session_id).await {
                    Ok(session) => Ok(CoreResponse::Session { session }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_share_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionUnshare { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                match store.unshare_session(&session_id).await {
                    Ok(session) => Ok(CoreResponse::Session { session }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_unshare_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionRename {
                session_id,
                new_title,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                match store
                    .update(
                        &session_id,
                        crate::session::UpdateSession {
                            title: Some(new_title),
                            share_url: None,
                            summary_additions: None,
                            summary_deletions: None,
                            summary_files: None,
                            summary_diffs: None,
                            revert: None,
                            permission: None,
                            tags: None,
                            time_compacting: None,
                            time_archived: None,
                        },
                    )
                    .await
                {
                    Ok(session) => Ok(CoreResponse::Session { session }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_rename_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionExport { session_id } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                match store.export_session(&session_id).await {
                    Ok(data) => Ok(CoreResponse::Json { data }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_export_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionImportData { data } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                match store.import_session(data, None).await {
                    Ok(session) => Ok(CoreResponse::Session { session }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_import_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SessionCreateFromTemplate {
                template,
                project_id,
                directory,
            } => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let store = crate::session::SessionStore::new(pool);
                match store
                    .create_from_template(&template, &project_id, &directory)
                    .await
                {
                    Ok(session) => Ok(CoreResponse::Session { session }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "session_create_from_template_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::PermissionRespond { id, choice } => {
                let parsed = match choice.as_str() {
                    "allow" => crate::permission::PermissionChoice::AllowOnce,
                    "always_allow" => crate::permission::PermissionChoice::AlwaysAllow,
                    "deny" => crate::permission::PermissionChoice::DenyOnce,
                    "always_deny" => crate::permission::PermissionChoice::AlwaysDeny,
                    _ => {
                        return Ok(CoreResponse::Error {
                            code: "invalid_permission_choice".to_string(),
                            message: format!("Invalid permission choice: {}", choice),
                        });
                    }
                };
                let sent = crate::bus::PermissionRegistry::respond(id, parsed);
                if sent {
                    Ok(CoreResponse::Ack)
                } else {
                    Ok(CoreResponse::Error {
                        code: "permission_response_failed".to_string(),
                        message: "No pending permission request found".to_string(),
                    })
                }
            }
            CoreRequest::QuestionRespond { id, answers } => {
                let sent = crate::bus::QuestionRegistry::answer_question(id, answers.to_string());
                if sent {
                    Ok(CoreResponse::Ack)
                } else {
                    Ok(CoreResponse::Error {
                        code: "question_response_failed".to_string(),
                        message: "No pending question found".to_string(),
                    })
                }
            }
            CoreRequest::ModelsRefresh => {
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_pool".to_string(),
                        message: "Core client missing database pool".to_string(),
                    });
                };
                let config = crate::config::schema::Config::load().unwrap_or_default();
                let mut registry = crate::provider::ProviderRegistry::new();
                crate::provider::register_builtin_with_config(&mut registry, &config);
                let discovery = crate::provider::discovery::ModelDiscoveryService::new(
                    std::path::PathBuf::new(),
                )
                .with_pool(pool);
                let models = discovery.refresh(&registry).await;
                let model_ids: Vec<String> = models
                    .iter()
                    .map(|m| format!("{}/{}", m.provider, m.id))
                    .collect();
                Ok(CoreResponse::Json {
                    data: serde_json::json!({ "models": model_ids }),
                })
            }
            CoreRequest::TaskList => {
                let Some(scheduler) = self.bg_scheduler.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_scheduler".to_string(),
                        message: "Core client missing background scheduler".to_string(),
                    });
                };
                let tasks = scheduler.list().await;
                Ok(CoreResponse::Json {
                    data: serde_json::json!({
                        "tasks": tasks.iter().map(|t| serde_json::json!({
                            "id": t.id,
                            "message": t.message,
                            "interval_secs": t.interval.as_secs(),
                            "session_id": t.session_id,
                            "created_at": t.created_at,
                            "last_run": t.last_run,
                        })).collect::<Vec<_>>()
                    }),
                })
            }
            CoreRequest::TaskDelete { id } => {
                let Some(scheduler) = self.bg_scheduler.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_scheduler".to_string(),
                        message: "Core client missing background scheduler".to_string(),
                    });
                };
                let removed = scheduler.remove(&id.to_string()).await;
                if removed {
                    Ok(CoreResponse::Ack)
                } else {
                    Ok(CoreResponse::Error {
                        code: "task_not_found".to_string(),
                        message: format!("Task not found: {}", id),
                    })
                }
            }
            CoreRequest::TaskSchedule {
                session_id,
                interval_secs,
                message,
            } => {
                let Some(scheduler) = self.bg_scheduler.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_scheduler".to_string(),
                        message: "Core client missing background scheduler".to_string(),
                    });
                };
                let task = crate::agent::task::BackgroundTask::new(
                    session_id.clone(),
                    std::time::Duration::from_secs(interval_secs),
                    message.clone(),
                );
                let task_id = task.id.clone();
                match scheduler.add(task).await {
                    Ok(_) => {
                        if let Some(pool) = self.subagent_pool.clone() {
                            let request = crate::agent::worker::SubAgentRequest {
                                task_id: 0,
                                prompt: format!("[Background] {}", message),
                                agent: "build".to_string(),
                                parent_id: Some(session_id),
                                denied_tools: Vec::new(),
                                allowed_paths: Vec::new(),
                                description: "Background loop task".to_string(),
                                depth: 1,
                                max_tool_calls: None,
                            };
                            let _ = pool.spawner().send(request).await;
                        }
                        Ok(CoreResponse::Json {
                            data: serde_json::json!({ "task_id": task_id, "interval_secs": interval_secs }),
                        })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "task_schedule_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::WorktreeList { project_dir } => {
                let git_root = std::path::PathBuf::from(&project_dir);
                let Some(root) = crate::worktree::find_git_root(&git_root) else {
                    return Ok(CoreResponse::Json {
                        data: serde_json::json!({ "worktrees": [] }),
                    });
                };
                match crate::worktree::list_worktrees(&root) {
                    Ok(trees) => Ok(CoreResponse::Json {
                        data: serde_json::json!({
                            "worktrees": trees.iter().map(|t| serde_json::json!({
                                "path": t.path,
                                "branch": t.branch
                            })).collect::<Vec<_>>()
                        }),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "worktree_list_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::MemoryList { namespace } => {
                let Some(memory_store) = self.memory_store.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_memory_store".to_string(),
                        message: "Core client missing memory store".to_string(),
                    });
                };
                let memories = memory_store.list(&namespace);
                Ok(CoreResponse::Json {
                    data: serde_json::json!({ "memories": memories }),
                })
            }
            CoreRequest::MemorySearch { query } => {
                let Some(memory_store) = self.memory_store.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_memory_store".to_string(),
                        message: "Core client missing memory store".to_string(),
                    });
                };
                let memories = memory_store.search(&query);
                Ok(CoreResponse::Json {
                    data: serde_json::json!({ "memories": memories }),
                })
            }
            CoreRequest::MemoryRemember { text, namespace } => {
                let Some(memory_store) = self.memory_store.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_memory_store".to_string(),
                        message: "Core client missing memory store".to_string(),
                    });
                };
                let ns = namespace.unwrap_or_else(|| "user/preferences".to_string());
                let memory = crate::memory::Memory::new(ns, text);
                memory_store.add(memory.clone());
                Ok(CoreResponse::Json {
                    data: serde_json::json!({ "memory": memory }),
                })
            }
            CoreRequest::MemoryForget { id } => {
                let Some(memory_store) = self.memory_store.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "missing_memory_store".to_string(),
                        message: "Core client missing memory store".to_string(),
                    });
                };
                let deleted = memory_store.delete(&id).is_some();
                Ok(CoreResponse::Json {
                    data: serde_json::json!({ "deleted": deleted }),
                })
            }
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
                            let _ = sqlx::query("UPDATE goal SET checkpoint_path = ? WHERE id = ?")
                                .bind(cp)
                                .bind(&goal.id)
                                .execute(&pool)
                                .await;
                        }
                        let updated = goal_store.get(&goal.id).await.ok().flatten();
                        publish_goal_updated(&session_id, updated);
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
                    Some(&content[..4000])
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
                            let _ = sqlx::query("UPDATE goal SET checkpoint_path = ? WHERE id = ?")
                                .bind(cp)
                                .bind(&goal.id)
                                .execute(&pool)
                                .await;
                        }
                        let updated = goal_store.get(&goal.id).await.ok().flatten();
                        publish_goal_updated(&session_id, updated);
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
                                publish_goal_updated(&session_id, Some(updated));
                                Ok(CoreResponse::Json {
                                    data: serde_json::json!({ "status": "paused", "id": goal.id }),
                                })
                            }
                            Ok(None) => {
                                publish_goal_updated(&session_id, None);
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
                                publish_goal_updated(&session_id, Some(updated));
                                Ok(CoreResponse::Json {
                                    data: serde_json::json!({ "status": "active", "id": goal.id }),
                                })
                            }
                            Ok(None) => {
                                publish_goal_updated(&session_id, None);
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
                        publish_goal_updated(&session_id, None);
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
                        match goal_store
                            .update_status(&goal.id, crate::goal::GoalStatus::Complete)
                            .await
                        {
                            Ok(Some(updated)) => {
                                publish_goal_updated(&session_id, Some(updated.clone()));
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
                                publish_goal_updated(&session_id, None);
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
                            let _ =
                                crate::goal::checkpoint::append_checkpoint_update(cp_path, &update)
                                    .await;
                            Ok(CoreResponse::Json {
                                data: serde_json::json!({ "checkpoint_path": cp_path, "appended": true }),
                            })
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
                                    let _ = sqlx::query(
                                        "UPDATE goal SET checkpoint_path = ? WHERE id = ?",
                                    )
                                    .bind(&path_str)
                                    .bind(&goal.id)
                                    .execute(&goal_store.pool)
                                    .await;
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
                            "goal": serde_json::to_value(&goal.to_snapshot())
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
                                publish_goal_updated(&session_id, Some(updated));
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
            _ => Ok(CoreResponse::Ack),
        }
    }

    fn subscribe(&self) -> mpsc::UnboundedReceiver<EventEnvelope<CoreEvent>> {
        let (tx, rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let mut bus_rx = crate::bus::global::GlobalEventBus::subscribe();
            let mut seq: u64 = 1;
            loop {
                match bus_rx.recv().await {
                    Ok(event) => {
                        if let Some(core_event) = map_app_event_to_core_event(event) {
                            let envelope = EventEnvelope {
                                protocol_version: PROTOCOL_VERSION,
                                event_seq: seq,
                                timestamp_ms: chrono::Utc::now().timestamp_millis(),
                                session_id: None,
                                turn_id: None,
                                payload: core_event,
                            };
                            seq = seq.saturating_add(1);
                            if tx.send(envelope).is_err() {
                                break;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        tracing::warn!("Core event bus lagged, {} events dropped", n);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }
        });
        rx
    }
}

fn map_app_event_to_core_event(event: crate::bus::events::AppEvent) -> Option<CoreEvent> {
    match event {
        crate::bus::events::AppEvent::TextDelta { delta, session_id } => {
            Some(CoreEvent::TurnTextDelta {
                session_id: session_id.to_string(),
                turn_id: String::new(),
                delta: delta.to_string(),
            })
        }
        crate::bus::events::AppEvent::ReasoningDelta { delta, session_id } => {
            Some(CoreEvent::TurnReasoningDelta {
                session_id: session_id.to_string(),
                turn_id: String::new(),
                delta,
            })
        }
        crate::bus::events::AppEvent::ToolCallStarted {
            session_id,
            tool_name,
            tool_id,
            arguments,
        } => Some(CoreEvent::ToolStarted {
            session_id,
            turn_id: None,
            tool_name,
            tool_id,
            arguments,
        }),
        crate::bus::events::AppEvent::ToolResult {
            session_id,
            tool_id,
            output,
            success,
            ..
        } => Some(CoreEvent::ToolCompleted {
            session_id,
            turn_id: None,
            tool_id,
            output,
            success,
        }),
        crate::bus::events::AppEvent::PermissionPending {
            perm_id,
            tool,
            path,
            ..
        } => Some(CoreEvent::PermissionPending {
            id: perm_id,
            tool,
            path,
        }),
        crate::bus::events::AppEvent::QuestionPending {
            session_id,
            questions,
        } => Some(CoreEvent::QuestionPending {
            id: session_id,
            questions: serde_json::from_str(&questions).unwrap_or(serde_json::Value::Null),
        }),
        crate::bus::events::AppEvent::AgentFinished {
            session_id,
            stop_reason,
            input_tokens: _,
            output_tokens: _,
            cached_tokens: _,
            reasoning_tokens: _,
        } => Some(CoreEvent::TurnCompleted {
            session_id,
            turn_id: String::new(),
            stop_reason,
        }),
        crate::bus::events::AppEvent::Error { message } => Some(CoreEvent::Error {
            code: "agent_error".to_string(),
            message,
        }),
        crate::bus::events::AppEvent::SubagentStarted {
            session_id,
            task_id,
            agent,
            description,
        } => Some(CoreEvent::SubagentStarted {
            session_id,
            task_id,
            agent,
            description,
        }),
        crate::bus::events::AppEvent::SubagentProgress {
            session_id,
            task_id,
            agent,
            message,
        } => Some(CoreEvent::SubagentProgress {
            session_id,
            task_id,
            agent,
            message,
        }),
        crate::bus::events::AppEvent::SubagentCompleted {
            session_id,
            task_id,
            agent,
            result_summary,
        } => Some(CoreEvent::SubagentCompleted {
            session_id,
            task_id,
            agent,
            result_summary,
        }),
        crate::bus::events::AppEvent::SubagentFailed {
            session_id,
            task_id,
            agent,
            error,
        } => Some(CoreEvent::SubagentFailed {
            session_id,
            task_id,
            agent,
            error,
        }),
        _ => None,
    }
}

pub fn new_request<T>(request_id: String, payload: T) -> RequestEnvelope<T> {
    RequestEnvelope {
        protocol_version: PROTOCOL_VERSION,
        request_id,
        payload,
    }
}
