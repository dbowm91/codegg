//! M002: `jobs` request family for `CoreDaemon`.
//!
//! Durable jobs, schedules, run records, tool-program inspection, and legacy task shims over the scheduler/job stores.
//! Operates on the same daemon-owned state as the thin dispatcher;
//! introduces no new store, scheduler, state machine, or authority.

use crate::error::AppError;
use crate::protocol::core::{CoreRequest, CoreResponse};

use super::daemon::CoreDaemon;
use super::daemon::{
    ensure_session_row, tool_program_detail_from_job, tool_program_summary_from_job,
};

impl CoreDaemon {
    pub(crate) async fn handle_jobs_request(
        &self,
        request: CoreRequest,
        request_id: &str,
        trusted_client_id: &str,
        authority: codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: codegg_core::authorization::AuthorizationDecision,
    ) -> Result<CoreResponse, AppError> {
        let _ = (request_id, trusted_client_id, &authority, &authz_decision);
        match request {
            CoreRequest::TaskList => Ok(CoreResponse::Error {
                code: "legacy_task_compatibility_disabled".to_string(),
                message: "use durable ScheduleList protocol".to_string(),
            }),
            CoreRequest::TaskDelete { id } => {
                let _ = id;
                Ok(CoreResponse::Error {
                    code: "legacy_task_compatibility_disabled".to_string(),
                    message: "use durable ScheduleDelete protocol".to_string(),
                })
            }
            CoreRequest::TaskSchedule {
                session_id,
                interval_secs,
                message,
            } => {
                let _ = (session_id, interval_secs, message);
                Ok(CoreResponse::Error {
                    code: "legacy_task_compatibility_disabled".to_string(),
                    message: "use durable ScheduleCreate protocol".to_string(),
                })
            }
            // ── Phase 4: Durable Jobs and Schedules ──────────────────────
            CoreRequest::JobSubmit { spec } => {
                let submission_key = spec
                    .submission_key
                    .clone()
                    .map(crate::scheduler::SubmissionKey::new)
                    .transpose()
                    .map_err(|e| e.to_string());
                let submission_key = match submission_key {
                    Ok(key) => key,
                    Err(message) => {
                        return Ok(CoreResponse::Error {
                            code: "invalid_job_submit".to_string(),
                            message,
                        });
                    }
                };
                let new_job = match crate::protocol_conversions::job_submit_from_dto(spec) {
                    Ok(j) => j,
                    Err(e) => {
                        return Ok(CoreResponse::Error {
                            code: "invalid_job_submit".to_string(),
                            message: e,
                        });
                    }
                };
                // Tool Programs are admitted only through the model-facing
                // `tool_program` tool. That path receives the daemon-owned
                // accepted permission decision and freezes the active
                // ToolBroker catalog. A generic protocol client must not be
                // able to supply either authority object in arbitrary JSON.
                if new_job.kind == codegg_core::jobs::JobKind::ToolProgram
                    && !crate::test_failpoint::recovery_fixture_enabled()
                {
                    return Ok(CoreResponse::Error {
                        code: "invalid_job_submit".to_string(),
                        message: "tool_program jobs must be submitted through the authorized tool_program invocation boundary".to_string(),
                    });
                }
                let Some(submission) = self.deps.submission.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "scheduler_unavailable".to_string(),
                        message: "daemon has no job submission service".to_string(),
                    });
                };
                match submission.submit(submission_key, new_job).await {
                    Ok(submitted) => {
                        let job_id = submitted.job_id.as_str().to_string();
                        let record = match self.deps.job_store.get_job(&submitted.job_id).await {
                            Ok(r) => r,
                            Err(e) => {
                                tracing::warn!(error = %e, job_id = %submitted.job_id, "failed to load job after submission");
                                None
                            }
                        };
                        self.event_log
                            .publish(
                                record.as_ref().and_then(|r| r.session_id.clone()),
                                None,
                                crate::protocol::core::CoreEvent::JobCreated {
                                    job_id: job_id.clone(),
                                    workspace_id: submitted.workspace_id.to_string(),
                                    kind: record
                                        .as_ref()
                                        .map(|r| r.kind.as_str())
                                        .unwrap_or("unknown")
                                        .to_string(),
                                    session_id: record.as_ref().and_then(|r| r.session_id.clone()),
                                    turn_id: record.as_ref().and_then(|r| r.turn_id.clone()),
                                },
                            )
                            .await;
                        // M003: capture originating-principal attribution.
                        self.record_origin_with_decision(
                            &authority,
                            &authz_decision,
                            "job",
                            job_id.as_str(),
                        )
                        .await;
                        // M005: structural job-submit event with
                        // causation back to the submitting session/turn.
                        {
                            let provenance =
                                codegg_core::authorization::audit_provenance(&authz_decision);
                            let mut chain =
                                codegg_core::audit_instrumentation::AuditChainContext::new();
                            chain.project = authz_decision.project_id.clone();
                            chain.session_id = record.as_ref().and_then(|r| r.session_id.clone());
                            chain.turn_id = record.as_ref().and_then(|r| r.turn_id.clone());
                            chain.job_id = Some(job_id.clone());
                            if let Some(run) = record.as_ref().and_then(|r| r.session_id.clone()) {
                                let _ = run;
                            }
                            let builder = codegg_core::audit_instrumentation::job_submit_event(
                                authority.principal(),
                                &provenance,
                                &chain,
                                job_id.as_str(),
                                "allow",
                            );
                            self.append_audit_event(builder).await;
                        }
                        Ok(CoreResponse::JobSubmitted { job_id })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "job_submit_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::JobGet { job_id } => {
                let id = codegg_core::jobs::JobId::new_unchecked(job_id);
                match self.deps.job_store.get_job(&id).await {
                    Ok(record) => Ok(CoreResponse::JobGet {
                        job: record
                            .as_ref()
                            .map(crate::protocol_conversions::job_record_to_dto),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "job_get_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::JobWait { job_id, timeout_ms } => {
                let Some(scheduler) = self.deps.scheduler.as_ref() else {
                    return Ok(CoreResponse::Error {
                        code: "job_wait_failed".to_string(),
                        message: "scheduler unavailable".to_string(),
                    });
                };
                let id = codegg_core::jobs::JobId::new_unchecked(job_id.clone());
                let timeout =
                    std::time::Duration::from_millis(timeout_ms.unwrap_or(900_000).min(3_600_000));
                match scheduler.wait_for_completion(&id, timeout).await {
                    Ok(completion) => Ok(CoreResponse::JobWaited {
                        job_id,
                        status: format!("{:?}", completion.status).to_lowercase(),
                        summary: completion.summary,
                        run_id: completion.run_id.map(|id| id.as_str().to_string()),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "job_wait_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::JobList { query } => {
                let mut q = codegg_core::jobs::store::JobStoreQuery::default();
                if let Some(w) = query.workspace_id {
                    q.workspace_id = Some(codegg_core::workspace::WorkspaceId::new_unchecked(w));
                }
                if !query.states.is_empty() {
                    q.states = query
                        .states
                        .iter()
                        .map(|s| crate::protocol_conversions::job_state_from_str(s))
                        .collect();
                }
                if !query.kinds.is_empty() {
                    q.kinds = query
                        .kinds
                        .iter()
                        .map(|k| crate::protocol_conversions::job_kind_from_str(k))
                        .collect();
                }
                q.session_id = query.session_id;
                if query.limit > 0 {
                    q.limit = Some(query.limit);
                }
                match self.deps.job_store.list_jobs(q).await {
                    Ok(summaries) => {
                        let dtos = summaries
                            .iter()
                            .map(crate::protocol_conversions::job_summary_to_dto)
                            .collect();
                        Ok(CoreResponse::JobList { jobs: dtos })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "job_list_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::JobAttempts { job_id } => {
                let id = codegg_core::jobs::JobId::new_unchecked(job_id.clone());
                match self.deps.job_store.list_attempts(&id).await {
                    Ok(attempts) => {
                        let dtos = attempts
                            .iter()
                            .map(crate::protocol_conversions::job_attempt_to_dto)
                            .collect();
                        Ok(CoreResponse::JobAttempts {
                            job_id,
                            attempts: dtos,
                        })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "job_attempts_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::JobCancel { job_id, reason } => {
                let id = codegg_core::jobs::JobId::new_unchecked(job_id);
                let Some(scheduler) = self.deps.scheduler.as_ref() else {
                    return Ok(CoreResponse::Error {
                        code: "job_cancel_failed".to_string(),
                        message: "scheduler unavailable".to_string(),
                    });
                };
                match scheduler
                    .request_cancel(&id, &reason.unwrap_or_else(|| "user requested".to_string()))
                    .await
                {
                    Ok(result) => {
                        let outcome_str =
                            crate::protocol_conversions::cancel_outcome_to_str(result.state)
                                .to_string();
                        self.event_log
                            .publish(
                                None,
                                None,
                                crate::protocol::core::CoreEvent::JobCancelRequested {
                                    job_id: result.job_id.as_str().to_string(),
                                    reason: outcome_str,
                                },
                            )
                            .await;
                        Ok(CoreResponse::JobCancelResult {
                            result: crate::protocol_conversions::cancel_result_to_dto(&result),
                        })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "job_cancel_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::JobRetry { job_id } => {
                let id = codegg_core::jobs::JobId::new_unchecked(&job_id);
                let gen = self.deps.daemon_generation.clone();
                match self.deps.job_store.get_job(&id).await {
                    Ok(Some(record)) => {
                        let prior_attempt_id = match &record.current_attempt_id {
                            Some(aid) => aid.clone(),
                            None => {
                                // Find the last attempt from history
                                match self.deps.job_store.list_attempts(&id).await {
                                    Ok(attempts) => match attempts.last() {
                                        Some(a) => a.attempt_id.clone(),
                                        None => {
                                            return Ok(CoreResponse::Error {
                                                code: "no_prior_attempt".to_string(),
                                                message: "job has no attempts to retry".to_string(),
                                            });
                                        }
                                    },
                                    Err(e) => {
                                        return Ok(CoreResponse::Error {
                                            code: "retry_failed".to_string(),
                                            message: e.to_string(),
                                        });
                                    }
                                }
                            }
                        };
                        match self
                            .deps
                            .job_store
                            .retry_job(&id, &gen, &prior_attempt_id)
                            .await
                        {
                            Ok(new_attempt) => {
                                self.event_log
                                    .publish(
                                        record.session_id.clone(),
                                        None,
                                        crate::protocol::core::CoreEvent::JobRetried {
                                            job_id: job_id.clone(),
                                            new_attempt_id: new_attempt.attempt_id.to_string(),
                                            prior_attempt_id: prior_attempt_id.to_string(),
                                        },
                                    )
                                    .await;
                                Ok(CoreResponse::JobRetryStarted {
                                    job_id,
                                    attempt_id: new_attempt.attempt_id.to_string(),
                                })
                            }
                            Err(e) => Ok(CoreResponse::Error {
                                code: "job_retry_failed".to_string(),
                                message: e.to_string(),
                            }),
                        }
                    }
                    Ok(None) => Ok(CoreResponse::Error {
                        code: "job_not_found".to_string(),
                        message: format!("job '{job_id}' not found"),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "job_retry_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::ScheduleCreate { spec } => {
                let template = match crate::protocol_conversions::schedule_create_from_dto(spec) {
                    Ok(t) => t,
                    Err(e) => {
                        return Ok(CoreResponse::Error {
                            code: "invalid_schedule_create".to_string(),
                            message: e,
                        });
                    }
                };
                match self.deps.schedule_store.create(template).await {
                    Ok(record) => {
                        let schedule_id = record.schedule_id.as_str().to_string();
                        self.event_log
                            .publish(
                                None,
                                None,
                                crate::protocol::core::CoreEvent::ScheduleCreated {
                                    schedule_id: schedule_id.clone(),
                                    workspace_id: record.workspace_id.to_string(),
                                    kind_summary: record.kind.tag().to_string(),
                                },
                            )
                            .await;
                        Ok(CoreResponse::ScheduleCreated { schedule_id })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "schedule_create_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::ScheduleList {
                workspace_id,
                include_archived,
            } => {
                let mut query = codegg_core::jobs::ScheduleQuery::default();
                if let Some(w) = workspace_id {
                    query.workspace_id =
                        Some(codegg_core::workspace::WorkspaceId::new_unchecked(w));
                }
                query.include_archived = include_archived;
                match self.deps.schedule_store.list(query).await {
                    Ok(summaries) => {
                        let dtos = summaries
                            .iter()
                            .map(crate::protocol_conversions::schedule_summary_to_dto)
                            .collect();
                        Ok(CoreResponse::ScheduleList { schedules: dtos })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "schedule_list_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::ScheduleGet { schedule_id } => {
                let id = codegg_core::jobs::ScheduleId::new_unchecked(&schedule_id);
                match self.deps.schedule_store.get(&id).await {
                    Ok(Some(record)) => Ok(CoreResponse::ScheduleGet {
                        schedule: crate::protocol_conversions::schedule_record_to_dto(&record),
                    }),
                    Ok(None) => Ok(CoreResponse::Error {
                        code: "schedule_not_found".to_string(),
                        message: format!("schedule '{schedule_id}' not found"),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "schedule_get_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SchedulePause { schedule_id } => {
                let id = codegg_core::jobs::ScheduleId::new_unchecked(schedule_id.clone());
                match self
                    .deps
                    .schedule_store
                    .set_state(&id, codegg_core::jobs::ScheduleState::Paused)
                    .await
                {
                    Ok(_record) => {
                        self.event_log
                            .publish(
                                None,
                                None,
                                crate::protocol::core::CoreEvent::SchedulePaused {
                                    schedule_id: schedule_id.clone(),
                                },
                            )
                            .await;
                        Ok(CoreResponse::SchedulePaused { schedule_id })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "schedule_pause_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::ScheduleResume { schedule_id } => {
                let id = codegg_core::jobs::ScheduleId::new_unchecked(schedule_id.clone());
                match self
                    .deps
                    .schedule_store
                    .set_state(&id, codegg_core::jobs::ScheduleState::Active)
                    .await
                {
                    Ok(_record) => {
                        self.event_log
                            .publish(
                                None,
                                None,
                                crate::protocol::core::CoreEvent::ScheduleResumed {
                                    schedule_id: schedule_id.clone(),
                                },
                            )
                            .await;
                        Ok(CoreResponse::ScheduleResumed { schedule_id })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "schedule_resume_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::ScheduleDelete { schedule_id } => {
                let id = codegg_core::jobs::ScheduleId::new_unchecked(schedule_id.clone());
                match self.deps.schedule_store.delete(&id).await {
                    Ok(()) => {
                        self.event_log
                            .publish(
                                None,
                                None,
                                crate::protocol::core::CoreEvent::ScheduleDeleted {
                                    schedule_id: schedule_id.clone(),
                                },
                            )
                            .await;
                        Ok(CoreResponse::ScheduleDeleted { schedule_id })
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "schedule_delete_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::JobRecoveryReport => {
                // The `recover_generation` API parameter is the
                // *new* daemon generation; the store finds any
                // non-terminal attempt whose generation does not
                // match. Pass our current generation so the report
                // reflects what would happen at startup.
                let current_gen = self.deps.daemon_generation.clone();
                let policy = self.deps.recovery_policy.clone();
                match self
                    .deps
                    .job_store
                    .recover_generation(&current_gen, &policy)
                    .await
                {
                    Ok(report) => Ok(CoreResponse::JobRecoveryReport {
                        report: crate::protocol_conversions::recovery_report_to_dto(&report),
                    }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "recovery_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::SchedulerSnapshot => {
                let Some(scheduler) = self.deps.scheduler.as_ref() else {
                    return Ok(CoreResponse::Error {
                        code: "scheduler_unavailable".to_string(),
                        message: "scheduler unavailable".to_string(),
                    });
                };
                match serde_json::to_value(scheduler.snapshot().await) {
                    Ok(snapshot) => Ok(CoreResponse::SchedulerSnapshot { snapshot }),
                    Err(e) => Ok(CoreResponse::Error {
                        code: "scheduler_snapshot_failed".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::RunList {
                workspace_id,
                query,
            } => {
                let id = codegg_core::workspace::WorkspaceId::new_unchecked(workspace_id);
                match self.workspace_services.acquire(&id).await {
                    Ok(lease) => {
                        let run_query =
                            codegg_core::protocol_conversions::run_query_from_dto(query);
                        let workspace_id_str = lease.workspace_id().to_string();
                        match lease.run_store().list_runs(run_query).await {
                            Ok(summaries) => {
                                let dtos = summaries
                                    .iter()
                                    .map(|s| {
                                        codegg_core::protocol_conversions::run_summary_to_dto(
                                            s,
                                            Some(&workspace_id_str),
                                        )
                                    })
                                    .collect();
                                drop(lease);
                                Ok(CoreResponse::RunList {
                                    workspace_id: workspace_id_str,
                                    runs: dtos,
                                })
                            }
                            Err(e) => Ok(CoreResponse::Error {
                                code: "run_list_failed".to_string(),
                                message: e.to_string(),
                            }),
                        }
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "workspace_not_active".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::RunGet {
                workspace_id,
                run_id,
            } => {
                let id = codegg_core::workspace::WorkspaceId::new_unchecked(workspace_id);
                match self.workspace_services.acquire(&id).await {
                    Ok(lease) => {
                        let workspace_id_str = lease.workspace_id().to_string();
                        let run_id_typed = codegg_core::run_store::RunId::new_unchecked(run_id);
                        match lease.run_store().get_run(&run_id_typed).await {
                            Ok(Some(manifest)) => {
                                let dto = codegg_core::protocol_conversions::run_manifest_to_dto(
                                    &manifest,
                                    Some(&workspace_id_str),
                                );
                                drop(lease);
                                Ok(CoreResponse::RunGet {
                                    workspace_id: workspace_id_str,
                                    run: Some(dto),
                                })
                            }
                            Ok(None) => Ok(CoreResponse::RunGet {
                                workspace_id: workspace_id_str,
                                run: None,
                            }),
                            Err(e) => Ok(CoreResponse::Error {
                                code: "run_get_failed".to_string(),
                                message: e.to_string(),
                            }),
                        }
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "workspace_not_active".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::RunArtifactRead {
                workspace_id,
                artifact_id,
                start,
                end,
            } => {
                let id = codegg_core::workspace::WorkspaceId::new_unchecked(workspace_id);
                match self.workspace_services.acquire(&id).await {
                    Ok(lease) => {
                        let workspace_id_str = lease.workspace_id().to_string();
                        let artifact_id_typed =
                            codegg_core::run_store::ArtifactId::new_unchecked(artifact_id);
                        let range = if end <= start {
                            None
                        } else {
                            Some(codegg_core::run_store::ByteRange { start, end })
                        };
                        match lease
                            .run_store()
                            .read_artifact(&artifact_id_typed, range)
                            .await
                        {
                            Ok(chunk) => {
                                drop(lease);
                                let data_b64 = base64::Engine::encode(
                                    &base64::engine::general_purpose::STANDARD,
                                    &chunk.data,
                                );
                                Ok(CoreResponse::RunArtifactChunk {
                                    workspace_id: workspace_id_str,
                                    artifact_id: artifact_id_typed.to_string(),
                                    data_b64,
                                    byte_offset: chunk.byte_offset,
                                    total_bytes: chunk.total_bytes,
                                })
                            }
                            Err(e) => Ok(CoreResponse::Error {
                                code: "run_artifact_read_failed".to_string(),
                                message: e.to_string(),
                            }),
                        }
                    }
                    Err(e) => Ok(CoreResponse::Error {
                        code: "workspace_not_active".to_string(),
                        message: e.to_string(),
                    }),
                }
            }
            CoreRequest::RunRerun {
                workspace_id,
                parent_run_id,
                session_id,
            } => {
                let workspace = codegg_core::workspace::WorkspaceId::new_unchecked(workspace_id);
                let Some(current_session_id) = session_id.as_deref() else {
                    return Ok(CoreResponse::Error {
                        code: "ineligible_authority_changed".to_string(),
                        message: "rerun requires a current bound session".to_string(),
                    });
                };
                let Some(runtime) = self.sessions.get(current_session_id) else {
                    return Ok(CoreResponse::Error {
                        code: "ineligible_authority_changed".to_string(),
                        message: format!("current session is not bound: {current_session_id}"),
                    });
                };
                if runtime.workspace_id != workspace {
                    return Ok(CoreResponse::Error {
                        code: "ineligible_authority_changed".to_string(),
                        message: "current session is bound to a different workspace".to_string(),
                    });
                }
                let lease = match self.workspace_services.acquire(&workspace).await {
                    Ok(lease) => lease,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "ineligible_missing_or_invalid_base".to_string(),
                            message: format!("workspace unavailable for rerun: {error}"),
                        });
                    }
                };
                let parent_id = codegg_core::run_store::RunId::new_unchecked(parent_run_id.clone());
                let parent = match lease.run_store().get_run(&parent_id).await {
                    Ok(Some(parent)) => parent,
                    Ok(None) => {
                        return Ok(CoreResponse::Error {
                            code: "ineligible_missing_spec".to_string(),
                            message: format!("historical run not found: {parent_run_id}"),
                        });
                    }
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "ineligible_missing_spec".to_string(),
                            message: format!("failed to load historical run: {error}"),
                        });
                    }
                };
                let validated = match crate::run_rerun::validate(
                    &parent,
                    &lease.path_policy().canonical_root,
                    Some(current_session_id),
                ) {
                    Ok(validated) => validated,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: error.code().to_string(),
                            message: error.to_string(),
                        });
                    }
                };
                let Some(submission) = self.deps.submission.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "scheduler_denied".to_string(),
                        message: "daemon scheduler is unavailable for rerun".to_string(),
                    });
                };
                let child_job = crate::run_rerun::to_job(validated, workspace.clone());
                match submission.submit(None, child_job).await {
                    Ok(submitted) => {
                        // M003: the rerun child job carries the requesting
                        // principal's origin, not the parent run's.
                        self.record_origin_with_decision(
                            &authority,
                            &authz_decision,
                            "job",
                            submitted.job_id.as_str(),
                        )
                        .await;
                        Ok(CoreResponse::RunRerunAccepted {
                            workspace_id: workspace.to_string(),
                            parent_run_id,
                            child_job_id: submitted.job_id.to_string(),
                        })
                    }
                    Err(error) => Ok(CoreResponse::Error {
                        code: "scheduler_denied".to_string(),
                        message: error.to_string(),
                    }),
                }
            }
            CoreRequest::ToolProgramList {
                session_id,
                state_filter,
            } => {
                let query = codegg_core::jobs::store::JobStoreQuery {
                    kinds: vec![codegg_core::jobs::JobKind::ToolProgram],
                    session_id: Some(session_id),
                    limit: Some(
                        codegg_protocol::projection::limits::MAX_PROJECTION_TOOL_PROGRAMS as u32,
                    ),
                    ..Default::default()
                };
                let summaries = match self.deps.job_store.list_jobs(query).await {
                    Ok(summaries) => summaries,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "tool_program_list_failed".into(),
                            message: error.to_string(),
                        });
                    }
                };
                let mut programs = Vec::new();
                for summary in summaries {
                    let job = match self.deps.job_store.get_job(&summary.job_id).await {
                        Ok(job) => job,
                        Err(error) => {
                            return Ok(CoreResponse::Error {
                                code: "tool_program_list_failed".into(),
                                message: error.to_string(),
                            });
                        }
                    };
                    if let Some(job) = job {
                        let program = tool_program_summary_from_job(&job, &[], 0);
                        if state_filter
                            .as_deref()
                            .map(|state| program.state == state)
                            .unwrap_or(true)
                        {
                            programs.push(program);
                        }
                    }
                }
                Ok(CoreResponse::ToolProgramList { programs })
            }
            CoreRequest::ToolProgramInspect { program_id } => {
                let job = match self.find_tool_program_job(&program_id).await {
                    Ok(job) => job,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "tool_program_inspect_failed".into(),
                            message: error,
                        });
                    }
                };
                let Some(job) = job else {
                    return Ok(CoreResponse::ToolProgramInspect { detail: None });
                };
                let attempts = match self.deps.job_store.list_attempts(&job.job_id).await {
                    Ok(attempts) => attempts,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "tool_program_inspect_failed".into(),
                            message: error.to_string(),
                        });
                    }
                };
                let call_page = match self.tool_program_call_page_for_job(&job, 0).await {
                    Ok(page) => page,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "tool_program_inspect_failed".into(),
                            message: error,
                        });
                    }
                };
                let mut detail = tool_program_detail_from_job(&job, &attempts, call_page);
                detail.normalise();
                Ok(CoreResponse::ToolProgramInspect {
                    detail: Some(detail),
                })
            }
            CoreRequest::ToolProgramCallPage { program_id, offset } => {
                let job = match self.find_tool_program_job(&program_id).await {
                    Ok(job) => job,
                    Err(error) => {
                        return Ok(CoreResponse::Error {
                            code: "tool_program_call_page_failed".into(),
                            message: error,
                        });
                    }
                };
                let Some(job) = job else {
                    return Ok(CoreResponse::ToolProgramCallPage { page: None });
                };
                match self.tool_program_call_page_for_job(&job, offset).await {
                    Ok(page) => Ok(CoreResponse::ToolProgramCallPage { page: Some(page) }),
                    Err(error) => Ok(CoreResponse::Error {
                        code: "tool_program_call_page_failed".into(),
                        message: error,
                    }),
                }
            }
            CoreRequest::ToolProgramNotificationReinject { session_id } => {
                if !crate::test_failpoint::recovery_fixture_enabled() {
                    return Ok(CoreResponse::Error {
                        code: "not_recovery_fixture".into(),
                        message:
                            "ToolProgramNotificationReinject is only available in recovery-fixture mode"
                                .into(),
                    });
                }
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "no_durable_pool".into(),
                        message: "daemon has no SQLite pool; cannot drive recovery".into(),
                    });
                };
                // The session_events row is FK-constrained to
                // session(id). A notification can exist for a session
                // that has not yet been created via SessionStore (for
                // example, a process that died before the agent loop
                // ran a turn). The recovery-fixture path ensures the
                // session row exists so the production inject loop's
                // FK-constrained append can succeed.
                if let Err(error) = ensure_session_row(&pool, &session_id).await {
                    return Ok(CoreResponse::Error {
                        code: "recovery_session_seed_failed".into(),
                        message: error,
                    });
                }
                let event_store = codegg_core::session::EventStore::new(pool.clone());
                let notification_service =
                    crate::scheduler::tool_program_notifications::ToolProgramNotificationService::with_pool(
                        pool,
                    );
                let report = crate::agent::tool_program_recovery::inject_recoverable_notifications(
                    Some(&event_store),
                    &notification_service,
                    &session_id,
                    |_| {},
                )
                .await;
                Ok(CoreResponse::ToolProgramNotificationReinjectReport {
                    considered: report.considered,
                    injected: report.injected,
                    recovered_via_event: report.recovered_via_event,
                    already_injected: report.already_injected,
                    leased: report.leased,
                    skipped: report.skipped,
                    errors: report.errors,
                })
            }
            CoreRequest::ToolProgramRecoveryDebugInspect {
                session_id,
                notification_id,
            } => {
                if !crate::test_failpoint::recovery_fixture_enabled() {
                    return Ok(CoreResponse::Error {
                        code: "not_recovery_fixture".into(),
                        message: "ToolProgramRecoveryDebugInspect is only available in recovery-fixture mode".into(),
                    });
                }
                let Some(pool) = self.pool.clone() else {
                    return Ok(CoreResponse::Error {
                        code: "no_durable_pool".into(),
                        message: "daemon has no SQLite pool; cannot inspect durable state".into(),
                    });
                };
                let event_store = codegg_core::session::EventStore::new(pool.clone());
                let notification_service =
                    crate::scheduler::tool_program_notifications::ToolProgramNotificationService::with_pool(
                        pool,
                    );
                let events = match event_store.list_for_session(&session_id).await {
                    Ok(events) => events,
                    Err(e) => {
                        return Ok(CoreResponse::Error {
                            code: "event_store_query_failed".into(),
                            message: format!("failed to list events: {e}"),
                        });
                    }
                };
                let event_ids: Vec<String> = events.iter().map(|e| e.meta().id.clone()).collect();
                let notification = match notification_service.get(&notification_id).await {
                    Ok(n) => n,
                    Err(e) => {
                        return Ok(CoreResponse::Error {
                            code: "notification_query_failed".into(),
                            message: format!("failed to load notification: {e}"),
                        });
                    }
                };
                match notification {
                    Some(n) => Ok(CoreResponse::ToolProgramRecoveryDebugInspectReport {
                        event_count: events.len(),
                        event_ids,
                        notification_state: format!("{:?}", n.state).to_lowercase(),
                        injected_event_id: n.injected_event_id,
                        delivered_at: n.delivered_at,
                        claim_owner: n.claim_owner,
                        claim_lease_until: n.claim_lease_until,
                    }),
                    None => Ok(CoreResponse::Error {
                        code: "notification_not_found".into(),
                        message: format!("notification {notification_id} not found"),
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
