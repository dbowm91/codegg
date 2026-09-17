//! Project Work Orders M001: `work_orders` request family for `CoreDaemon`.
//!
//! Durable project work orders, occurrences, and sequence lanes over the
//! daemon-owned [`codegg_core::work_order::WorkOrderService`]. Operates on
//! the same daemon-owned state as the thin dispatcher; introduces no new
//! scheduler, executor, worktree allocator, or authority. M001 persists,
//! validates, and projects waiting work only: no work order executes yet
//! (release evaluation and materialization arrive in M002).
//!
//! The M003 gate has already enforced the project-scoped capability
//! before this runs. ID-only locators resolve to their owning project
//! server-side through the durable row; unknown or foreign ids report
//! the privacy-preserving not-found shape, never a project oracle.

use codegg_core::identity::{ProjectId, SequenceLaneId, WorkOrderId};
use codegg_core::work_order::{
    validate_gate_set, validate_idempotency_key, validate_lane_label, validate_model,
    validate_parent_ref, validate_prompt, validate_repeat_count, validate_title, ApprovalRequest,
    GateJoin, GateKind, GateSpec, LaneFailurePolicy, NewSequenceLane, NewWorkOrder, ReleaseGateSet,
    SandboxRequest, SequenceLane, WorkOrder, WorkOrderError, WorkOrderPatch, WorkOrderState,
    WorkspacePolicy,
};

use crate::error::AppError;
use crate::protocol::core::{CoreEvent, CoreRequest, CoreResponse};

use super::daemon::CoreDaemon;

macro_rules! ok_or_response {
    ($expr:expr) => {
        match $expr {
            Ok(value) => value,
            Err(response) => return Ok(*response),
        }
    };
}

impl CoreDaemon {
    /// `true` for the bounded work-order operation family, which
    /// dispatches through the boxed [`Self::handle_work_order_request`]
    /// helper instead of the giant dispatch match.
    pub(crate) fn is_work_order_request(request: &CoreRequest) -> bool {
        matches!(
            request,
            CoreRequest::WorkOrderCapabilities
                | CoreRequest::WorkOrderCreate { .. }
                | CoreRequest::WorkOrderBatchCreate { .. }
                | CoreRequest::WorkOrderList { .. }
                | CoreRequest::WorkOrderGet { .. }
                | CoreRequest::WorkOrderUpdate { .. }
                | CoreRequest::WorkOrderCancel { .. }
                | CoreRequest::WorkOrderPause { .. }
                | CoreRequest::WorkOrderResume { .. }
                | CoreRequest::WorkOrderLaneCreate { .. }
                | CoreRequest::WorkOrderLaneGet { .. }
                | CoreRequest::WorkOrderLaneList { .. }
                | CoreRequest::WorkOrderLaneReorder { .. }
                | CoreRequest::WorkOrderLaneAttach { .. }
                | CoreRequest::WorkOrderOccurrenceGet { .. }
                | CoreRequest::WorkOrderOccurrenceList { .. }
                | CoreRequest::WorkOrderSummary { .. }
        )
    }

    /// `true` for work-order operations that mutate durable state. They
    /// skip the pre-side-effect audit emit and are recorded
    /// post-mutation with their durable ids and revisions.
    pub(crate) fn is_work_order_mutation(request: &CoreRequest) -> bool {
        matches!(
            request,
            CoreRequest::WorkOrderCreate { .. }
                | CoreRequest::WorkOrderBatchCreate { .. }
                | CoreRequest::WorkOrderUpdate { .. }
                | CoreRequest::WorkOrderCancel { .. }
                | CoreRequest::WorkOrderPause { .. }
                | CoreRequest::WorkOrderResume { .. }
                | CoreRequest::WorkOrderLaneCreate { .. }
                | CoreRequest::WorkOrderLaneReorder { .. }
                | CoreRequest::WorkOrderLaneAttach { .. }
        )
    }

    /// Work Orders M001: dedicated work-order request handler.
    ///
    /// Principals come from transport authority, never from the payload.
    /// Prompt bodies are author-supplied intent and are stored; trigger
    /// secrets, credentials, and hidden reasoning never enter this path
    /// (M001 introduces none of them).
    pub(crate) async fn handle_work_order_request(
        &self,
        _request_id: &str,
        request: CoreRequest,
        trusted_client_id: &str,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: &codegg_core::authorization::AuthorizationDecision,
    ) -> Result<CoreResponse, AppError> {
        let principal = self
            .request_authority_for_client(trusted_client_id)
            .principal()
            .principal_id()
            .clone();
        let now_ms = chrono::Utc::now().timestamp_millis();
        match request {
            CoreRequest::WorkOrderCapabilities => Ok(CoreResponse::WorkOrderCapabilities {
                capabilities: self.work_orders.capabilities_dto(),
            }),
            CoreRequest::WorkOrderCreate { request } => {
                let project = ok_or_response!(parse_project(&request.project_id));
                let input = ok_or_response!(new_work_order_from_create(&request));
                match self
                    .work_orders
                    .create_work_order(&project, &principal, input, now_ms)
                    .await
                {
                    Ok(outcome) => {
                        self.after_work_order_mutation(
                            authority,
                            authz_decision,
                            &outcome.work_order,
                            "create",
                            outcome.duplicate,
                        )
                        .await;
                        Ok(CoreResponse::WorkOrder {
                            work_order: outcome.work_order.to_dto(),
                            duplicate: outcome.duplicate,
                        })
                    }
                    Err(error) => Ok(work_order_error(error)),
                }
            }
            CoreRequest::WorkOrderBatchCreate { request } => {
                let project = ok_or_response!(parse_project(&request.project_id));
                let lane_id = ok_or_response!(request
                    .sequence_lane_id
                    .as_deref()
                    .map(SequenceLaneId::parse)
                    .transpose()
                    .map_err(|error| Box::new(work_order_error(WorkOrderError::invalid(
                        "sequence_lane_id",
                        error.to_string()
                    )))));
                let mut items = Vec::with_capacity(request.items.len());
                for item in &request.items {
                    items.push(ok_or_response!(new_work_order_from_batch_item(item)));
                }
                match self
                    .work_orders
                    .batch_create_work_orders(
                        &project,
                        &principal,
                        items,
                        lane_id,
                        request.batch_key.clone(),
                        now_ms,
                    )
                    .await
                {
                    Ok(outcome) => {
                        for work_order in &outcome.work_orders {
                            self.after_work_order_mutation(
                                authority,
                                authz_decision,
                                work_order,
                                "batch_create",
                                outcome.duplicate,
                            )
                            .await;
                        }
                        if !outcome.duplicate {
                            if let Some(ref lane_id) = request.sequence_lane_id {
                                self.publish_lane_changed(&project, lane_id, None).await;
                            }
                        }
                        Ok(CoreResponse::WorkOrderBatch {
                            work_orders: outcome
                                .work_orders
                                .iter()
                                .map(WorkOrder::to_dto)
                                .collect(),
                            duplicate: outcome.duplicate,
                        })
                    }
                    Err(error) => Ok(work_order_error(error)),
                }
            }
            CoreRequest::WorkOrderList {
                project_id,
                state_filter,
                cursor,
                limit,
            } => {
                let project = ok_or_response!(parse_project(&project_id));
                let filter = ok_or_response!(state_filter
                    .as_deref()
                    .map(|raw| WorkOrderState::parse(raw).ok_or_else(|| {
                        Box::new(work_order_error(WorkOrderError::invalid(
                            "state_filter",
                            "unknown work order state",
                        )))
                    }))
                    .transpose());
                match self
                    .work_orders
                    .list_work_orders(&project, filter, cursor.as_deref(), limit)
                    .await
                {
                    Ok(page) => Ok(CoreResponse::WorkOrderList {
                        work_orders: page.work_orders.iter().map(WorkOrder::to_dto).collect(),
                        next_cursor: page.next_cursor,
                        truncated: page.truncated,
                    }),
                    Err(error) => Ok(work_order_error(error)),
                }
            }
            CoreRequest::WorkOrderGet { work_order_id } => {
                match self.resolve_work_order(&work_order_id).await {
                    Err(response) => Ok(*response),
                    Ok((id, project)) => match self.work_orders.get_work_order(&project, &id).await
                    {
                        Ok(Some(work_order)) => Ok(CoreResponse::WorkOrder {
                            work_order: work_order.to_dto(),
                            duplicate: false,
                        }),
                        Ok(None) | Err(_) => Ok(work_order_not_found()),
                    },
                }
            }
            CoreRequest::WorkOrderUpdate { request } => {
                let (id, project) = match self.resolve_work_order(&request.work_order_id).await {
                    Err(response) => return Ok(*response),
                    Ok(pair) => pair,
                };
                let patch = ok_or_response!(patch_from_update(&request));
                match self
                    .work_orders
                    .update_work_order(&project, &id, patch, request.expected_revision, now_ms)
                    .await
                {
                    Ok(work_order) => {
                        self.after_work_order_mutation(
                            authority,
                            authz_decision,
                            &work_order,
                            "update",
                            false,
                        )
                        .await;
                        Ok(CoreResponse::WorkOrder {
                            work_order: work_order.to_dto(),
                            duplicate: false,
                        })
                    }
                    Err(error) => Ok(work_order_error(error)),
                }
            }
            CoreRequest::WorkOrderCancel { work_order_id } => {
                self.transition_work_order(
                    &work_order_id,
                    WorkOrderState::Cancelled,
                    "cancel",
                    authority,
                    authz_decision,
                    now_ms,
                )
                .await
            }
            CoreRequest::WorkOrderPause { work_order_id } => {
                self.transition_work_order(
                    &work_order_id,
                    WorkOrderState::Paused,
                    "pause",
                    authority,
                    authz_decision,
                    now_ms,
                )
                .await
            }
            CoreRequest::WorkOrderResume { work_order_id } => {
                self.transition_work_order(
                    &work_order_id,
                    WorkOrderState::Active,
                    "resume",
                    authority,
                    authz_decision,
                    now_ms,
                )
                .await
            }
            CoreRequest::WorkOrderLaneCreate { request } => {
                let project = ok_or_response!(parse_project(&request.project_id));
                let input = ok_or_response!(lane_input_from_create(&request));
                match self.work_orders.create_lane(&project, input, now_ms).await {
                    Ok(lane) => {
                        self.after_lane_mutation(
                            authority,
                            authz_decision,
                            &project,
                            &lane,
                            "lane_create",
                        )
                        .await;
                        Ok(CoreResponse::WorkOrderLane {
                            lane: lane.to_dto(),
                        })
                    }
                    Err(error) => Ok(work_order_error(error)),
                }
            }
            CoreRequest::WorkOrderLaneGet { lane_id } => match self.resolve_lane(&lane_id).await {
                Err(response) => Ok(*response),
                Ok((id, project)) => match self.work_orders.get_lane(&project, &id).await {
                    Ok(Some(lane)) => Ok(CoreResponse::WorkOrderLane {
                        lane: lane.to_dto(),
                    }),
                    Ok(None) | Err(_) => Ok(work_order_not_found()),
                },
            },
            CoreRequest::WorkOrderLaneList { project_id, limit } => {
                let project = ok_or_response!(parse_project(&project_id));
                match self.work_orders.list_lanes(&project, limit).await {
                    Ok((lanes, truncated)) => Ok(CoreResponse::WorkOrderLaneList {
                        lanes: lanes.iter().map(SequenceLane::to_dto).collect(),
                        truncated,
                    }),
                    Err(error) => Ok(work_order_error(error)),
                }
            }
            CoreRequest::WorkOrderLaneReorder { request } => {
                let (lane_id, project) = match self.resolve_lane(&request.lane_id).await {
                    Err(response) => return Ok(*response),
                    Ok(pair) => pair,
                };
                let ordered =
                    ok_or_response!(parse_work_order_ids(&request.ordered_work_order_ids));
                match self
                    .work_orders
                    .reorder_lane(
                        &project,
                        &lane_id,
                        request.expected_revision,
                        ordered,
                        now_ms,
                    )
                    .await
                {
                    Ok(lane) => {
                        self.after_lane_mutation(
                            authority,
                            authz_decision,
                            &project,
                            &lane,
                            "lane_reorder",
                        )
                        .await;
                        Ok(CoreResponse::WorkOrderLane {
                            lane: lane.to_dto(),
                        })
                    }
                    Err(error) => Ok(work_order_error(error)),
                }
            }
            CoreRequest::WorkOrderLaneAttach { request } => {
                let (lane_id, project) = match self.resolve_lane(&request.lane_id).await {
                    Err(response) => return Ok(*response),
                    Ok(pair) => pair,
                };
                let work_order_id = ok_or_response!(WorkOrderId::parse(&request.work_order_id)
                    .map_err(|error| Box::new(work_order_error(WorkOrderError::invalid(
                        "work_order_id",
                        error.to_string()
                    )))));
                match self
                    .work_orders
                    .attach_to_lane(
                        &project,
                        &lane_id,
                        request.expected_revision,
                        &work_order_id,
                        request.position,
                        now_ms,
                    )
                    .await
                {
                    Ok(lane) => {
                        self.after_lane_mutation(
                            authority,
                            authz_decision,
                            &project,
                            &lane,
                            "lane_attach",
                        )
                        .await;
                        Ok(CoreResponse::WorkOrderLane {
                            lane: lane.to_dto(),
                        })
                    }
                    Err(error) => Ok(work_order_error(error)),
                }
            }
            CoreRequest::WorkOrderOccurrenceGet { occurrence_id } => {
                match self.resolve_occurrence(&occurrence_id).await {
                    Err(response) => Ok(*response),
                    Ok((id, project)) => {
                        match self.work_orders.get_occurrence(&project, &id).await {
                            Ok(Some(occurrence)) => Ok(CoreResponse::WorkOrderOccurrence {
                                occurrence: occurrence.to_dto(),
                            }),
                            Ok(None) | Err(_) => Ok(work_order_not_found()),
                        }
                    }
                }
            }
            CoreRequest::WorkOrderOccurrenceList {
                work_order_id,
                limit,
            } => {
                let (id, project) = match self.resolve_work_order(&work_order_id).await {
                    Err(response) => return Ok(*response),
                    Ok(pair) => pair,
                };
                match self
                    .work_orders
                    .list_occurrences(&project, &id, limit)
                    .await
                {
                    Ok(page) => Ok(CoreResponse::WorkOrderOccurrenceList {
                        occurrences: page
                            .occurrences
                            .iter()
                            .map(|occurrence| occurrence.to_dto())
                            .collect(),
                        truncated: page.truncated,
                    }),
                    Err(error) => Ok(work_order_error(error)),
                }
            }
            CoreRequest::WorkOrderSummary { project_id } => {
                let project = ok_or_response!(parse_project(&project_id));
                match self.work_orders.summary_counts(&project).await {
                    Ok(summary) => Ok(CoreResponse::WorkOrderSummary {
                        summary: summary.to_dto(),
                    }),
                    Err(error) => Ok(work_order_error(error)),
                }
            }
            _ => Ok(CoreResponse::Error {
                code: "unimplemented".to_string(),
                message: "This request type is not yet implemented".to_string(),
            }),
        }
    }

    /// Move one work order along its template lifecycle with
    /// post-mutation audit/event evidence.
    async fn transition_work_order(
        &self,
        work_order_id: &str,
        target: WorkOrderState,
        operation: &str,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: &codegg_core::authorization::AuthorizationDecision,
        now_ms: i64,
    ) -> Result<CoreResponse, AppError> {
        let (id, project) = match self.resolve_work_order(work_order_id).await {
            Err(response) => return Ok(*response),
            Ok(pair) => pair,
        };
        match self
            .work_orders
            .transition_work_order(&project, &id, target, now_ms)
            .await
        {
            Ok(work_order) => {
                self.after_work_order_mutation(
                    authority,
                    authz_decision,
                    &work_order,
                    operation,
                    false,
                )
                .await;
                Ok(CoreResponse::WorkOrder {
                    work_order: work_order.to_dto(),
                    duplicate: false,
                })
            }
            Err(error) => Ok(work_order_error(error)),
        }
    }

    /// Post-mutation evidence for one work-order write: immutable origin
    /// attribution on creation (first write wins), structural audit with
    /// the authorizing decision id and durable revision, and a
    /// structural liveness event. Retried (duplicate) submissions emit
    /// no new event or audit row: they converged on existing truth.
    async fn after_work_order_mutation(
        &self,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: &codegg_core::authorization::AuthorizationDecision,
        work_order: &WorkOrder,
        operation: &str,
        duplicate: bool,
    ) {
        if duplicate {
            return;
        }
        if operation == "create" || operation == "batch_create" {
            Box::pin(self.record_origin_with_decision(
                authority,
                authz_decision,
                "work_order",
                work_order.id.as_str(),
            ))
            .await;
        }
        let provenance = codegg_core::authorization::audit_provenance(authz_decision);
        let mut chain = codegg_core::audit_instrumentation::AuditChainContext::new();
        chain.project = authz_decision.project_id.clone();
        let builder = codegg_core::audit_instrumentation::work_order_lifecycle_event(
            authority.principal(),
            &provenance,
            &chain,
            work_order.id.as_str(),
            work_order.revision,
            operation,
            work_order.state.as_str(),
            "allow",
        );
        Box::pin(self.append_audit_event(builder)).await;
        let change = match operation {
            "create" | "batch_create" => "created",
            "update" => "updated",
            "cancel" => "cancelled",
            "pause" => "paused",
            "resume" => "resumed",
            _ => "updated",
        };
        self.event_log
            .publish(
                None,
                None,
                CoreEvent::WorkOrderChanged {
                    project_id: work_order.project_id.as_str().to_owned(),
                    work_order_id: work_order.id.as_str().to_owned(),
                    change: change.to_owned(),
                    revision: work_order.revision,
                },
            )
            .await;
    }

    /// Post-mutation evidence for one lane write: structural audit plus
    /// a structural lane liveness event.
    async fn after_lane_mutation(
        &self,
        authority: &codegg_core::transport_auth::RequestAuthorityContext,
        authz_decision: &codegg_core::authorization::AuthorizationDecision,
        project: &ProjectId,
        lane: &SequenceLane,
        operation: &str,
    ) {
        let provenance = codegg_core::authorization::audit_provenance(authz_decision);
        let mut chain = codegg_core::audit_instrumentation::AuditChainContext::new();
        chain.project = authz_decision.project_id.clone();
        let builder = codegg_core::audit::AuditEventBuilder::new(
            codegg_core::audit::AuditAction::WorkOrderLifecycle,
            authority.principal(),
            &provenance,
        )
        .with_visibility(codegg_core::audit::AuditVisibility::Project)
        .with_metadata("project.id", project.as_str())
        .with_metadata("sequence_lane.id", lane.id.as_str())
        .with_metadata("sequence_lane.revision", lane.revision.to_string().as_str())
        .with_metadata("work_order.op", operation)
        .with_metadata("decision.id", provenance.decision_id())
        .with_metadata("decision.outcome", "allow");
        let builder = codegg_core::audit_instrumentation::apply_chain(
            builder,
            &chain,
            chain.project.as_ref(),
        );
        Box::pin(self.append_audit_event(builder)).await;
        self.event_log
            .publish(
                None,
                None,
                CoreEvent::WorkOrderLaneChanged {
                    project_id: project.as_str().to_owned(),
                    lane_id: lane.id.as_str().to_owned(),
                    revision: lane.revision,
                },
            )
            .await;
    }

    /// Publish a lane liveness event when the lane revision is already
    /// known (batch placement path resolves the lane afterwards).
    async fn publish_lane_changed(
        &self,
        project: &ProjectId,
        lane_id: &str,
        revision: Option<u64>,
    ) {
        let revision = match revision {
            Some(revision) => revision,
            None => match SequenceLaneId::parse(lane_id) {
                Err(_) => return,
                Ok(id) => match self.work_orders.get_lane(project, &id).await {
                    Ok(Some(lane)) => lane.revision,
                    _ => return,
                },
            },
        };
        self.event_log
            .publish(
                None,
                None,
                CoreEvent::WorkOrderLaneChanged {
                    project_id: project.as_str().to_owned(),
                    lane_id: lane_id.to_owned(),
                    revision,
                },
            )
            .await;
    }

    /// Resolve one work-order locator to its `(WorkOrderId, ProjectId)`
    /// pair through the durable row. Unknown ids report the
    /// privacy-preserving not-found shape; malformed ids report invalid
    /// input. Neither leaks which projects exist.
    async fn resolve_work_order(
        &self,
        work_order_id: &str,
    ) -> Result<(WorkOrderId, ProjectId), Box<CoreResponse>> {
        let id = WorkOrderId::parse(work_order_id).map_err(|error| {
            Box::new(CoreResponse::Error {
                code: "work_order_invalid_input".to_owned(),
                message: error.to_string(),
            })
        })?;
        let Some(pool) = self.pool.clone() else {
            return Err(Box::new(CoreResponse::Error {
                code: "work_order_unavailable".to_owned(),
                message: "project work orders require a durable database pool".to_owned(),
            }));
        };
        match codegg_core::work_order::work_order_project(&pool, id.as_str()).await {
            Some(project) => Ok((id, project)),
            None => Err(Box::new(work_order_not_found())),
        }
    }

    /// Resolve one lane locator to its `(SequenceLaneId, ProjectId)` pair.
    async fn resolve_lane(
        &self,
        lane_id: &str,
    ) -> Result<(SequenceLaneId, ProjectId), Box<CoreResponse>> {
        let id = SequenceLaneId::parse(lane_id).map_err(|error| {
            Box::new(CoreResponse::Error {
                code: "work_order_invalid_input".to_owned(),
                message: error.to_string(),
            })
        })?;
        let Some(pool) = self.pool.clone() else {
            return Err(Box::new(CoreResponse::Error {
                code: "work_order_unavailable".to_owned(),
                message: "project work orders require a durable database pool".to_owned(),
            }));
        };
        match codegg_core::work_order::work_order_project(&pool, id.as_str()).await {
            Some(project) => Ok((id, project)),
            None => Err(Box::new(work_order_not_found())),
        }
    }

    /// Resolve one occurrence locator to its `(OccurrenceId, ProjectId)` pair.
    async fn resolve_occurrence(
        &self,
        occurrence_id: &str,
    ) -> Result<(codegg_core::identity::WorkOrderOccurrenceId, ProjectId), Box<CoreResponse>> {
        let id = codegg_core::identity::WorkOrderOccurrenceId::parse(occurrence_id).map_err(
            |error| {
                Box::new(CoreResponse::Error {
                    code: "work_order_invalid_input".to_owned(),
                    message: error.to_string(),
                })
            },
        )?;
        let Some(pool) = self.pool.clone() else {
            return Err(Box::new(CoreResponse::Error {
                code: "work_order_unavailable".to_owned(),
                message: "project work orders require a durable database pool".to_owned(),
            }));
        };
        match codegg_core::work_order::work_order_project(&pool, id.as_str()).await {
            Some(project) => Ok((id, project)),
            None => Err(Box::new(work_order_not_found())),
        }
    }
}

// ── Request conversion ─────────────────────────────────────────────────

fn parse_project(raw: &str) -> Result<ProjectId, Box<CoreResponse>> {
    ProjectId::parse(raw).map_err(|error| {
        Box::new(CoreResponse::Error {
            code: "work_order_invalid_input".to_owned(),
            message: error.to_string(),
        })
    })
}

fn parse_work_order_ids(raw: &[String]) -> Result<Vec<WorkOrderId>, Box<CoreResponse>> {
    raw.iter()
        .map(|id| {
            WorkOrderId::parse(id).map_err(|error| {
                Box::new(CoreResponse::Error {
                    code: "work_order_invalid_input".to_owned(),
                    message: error.to_string(),
                })
            })
        })
        .collect()
}

fn work_order_error(error: WorkOrderError) -> CoreResponse {
    CoreResponse::Error {
        code: error.code().to_owned(),
        message: error.to_string(),
    }
}

fn boxed_work_order_error(error: WorkOrderError) -> Box<CoreResponse> {
    Box::new(work_order_error(error))
}

/// Privacy-preserving not-found shape: identical for absent and foreign
/// rows so opaque ids never oracle project existence.
fn work_order_not_found() -> CoreResponse {
    CoreResponse::Error {
        code: "work_order_not_found".to_owned(),
        message: "work order not found".to_owned(),
    }
}

fn gate_spec_from_dto(
    raw: &codegg_protocol::work_order::WorkOrderGateDto,
) -> Result<GateSpec, WorkOrderError> {
    let kind = GateKind::parse(&raw.kind).ok_or_else(|| {
        WorkOrderError::invalid("gates", format!("unknown release-gate kind {:?}", raw.kind))
    })?;
    Ok(GateSpec {
        kind,
        delay_secs: raw.delay_secs,
        not_before_ms: raw.not_before_ms,
        lane_id: raw
            .lane_id
            .as_deref()
            .map(SequenceLaneId::parse)
            .transpose()
            .map_err(|error| WorkOrderError::invalid("gates", error.to_string()))?,
        trigger_ref: raw.trigger_ref.clone(),
    })
}

fn gate_set_from_dto(
    gates: &[codegg_protocol::work_order::WorkOrderGateDto],
    join: Option<&str>,
) -> Result<ReleaseGateSet, WorkOrderError> {
    let join = join
        .map(|raw| {
            GateJoin::parse(raw)
                .ok_or_else(|| WorkOrderError::invalid("gate_join", "unknown gate join policy"))
        })
        .transpose()?
        .unwrap_or(GateJoin::All);
    let mut specs = Vec::with_capacity(gates.len());
    for raw in gates {
        specs.push(gate_spec_from_dto(raw)?);
    }
    validate_gate_set(&specs, join)
}

fn approval_from_dto(value: Option<&str>) -> Result<Option<ApprovalRequest>, WorkOrderError> {
    value
        .map(|raw| {
            ApprovalRequest::parse(raw).ok_or_else(|| {
                WorkOrderError::invalid("requested_approval", "unknown approval mode")
            })
        })
        .transpose()
}

fn sandbox_from_dto(value: Option<&str>) -> Result<Option<SandboxRequest>, WorkOrderError> {
    value
        .map(|raw| {
            SandboxRequest::parse(raw).ok_or_else(|| {
                WorkOrderError::invalid("requested_sandbox", "unknown sandbox profile")
            })
        })
        .transpose()
}

fn workspace_policy_from_dto(
    value: Option<&str>,
) -> Result<Option<WorkspacePolicy>, WorkOrderError> {
    value
        .map(|raw| {
            WorkspacePolicy::parse(raw).ok_or_else(|| {
                WorkOrderError::invalid("workspace_policy", "unknown workspace policy")
            })
        })
        .transpose()
}

fn new_work_order_from_create(
    request: &codegg_protocol::work_order::WorkOrderCreateRequest,
) -> Result<NewWorkOrder, Box<CoreResponse>> {
    build_new_work_order(
        request.title.as_deref(),
        &request.prompt,
        request.requested_model.as_deref(),
        request.requested_approval.as_deref(),
        request.requested_sandbox.as_deref(),
        request.workspace_policy.as_deref(),
        &request.gates,
        request.gate_join.as_deref(),
        request.repeat_count,
        request.sequence_lane_id.as_deref(),
        request.parent_session_id.as_deref(),
        request.parent_turn_id.as_deref(),
        request.parent_work_order_id.as_deref(),
        request.idempotency_key.as_deref(),
    )
}

fn new_work_order_from_batch_item(
    item: &codegg_protocol::work_order::WorkOrderBatchItem,
) -> Result<NewWorkOrder, Box<CoreResponse>> {
    build_new_work_order(
        item.title.as_deref(),
        &item.prompt,
        item.requested_model.as_deref(),
        item.requested_approval.as_deref(),
        item.requested_sandbox.as_deref(),
        item.workspace_policy.as_deref(),
        &item.gates,
        item.gate_join.as_deref(),
        item.repeat_count,
        None,
        item.parent_session_id.as_deref(),
        item.parent_turn_id.as_deref(),
        item.parent_work_order_id.as_deref(),
        item.idempotency_key.as_deref(),
    )
}

#[allow(clippy::too_many_arguments)]
fn build_new_work_order(
    title: Option<&str>,
    prompt: &str,
    requested_model: Option<&str>,
    requested_approval: Option<&str>,
    requested_sandbox: Option<&str>,
    workspace_policy: Option<&str>,
    gates: &[codegg_protocol::work_order::WorkOrderGateDto],
    gate_join: Option<&str>,
    repeat_count: Option<u32>,
    sequence_lane_id: Option<&str>,
    parent_session_id: Option<&str>,
    parent_turn_id: Option<&str>,
    parent_work_order_id: Option<&str>,
    idempotency_key: Option<&str>,
) -> Result<NewWorkOrder, Box<CoreResponse>> {
    let mapped = |error: WorkOrderError| boxed_work_order_error(error);
    Ok(NewWorkOrder {
        title: validate_title(title).map_err(mapped)?,
        prompt: validate_prompt(prompt).map_err(mapped)?,
        requested_model: validate_model(requested_model).map_err(mapped)?,
        requested_approval: approval_from_dto(requested_approval)
            .map_err(boxed_work_order_error)?,
        requested_sandbox: sandbox_from_dto(requested_sandbox).map_err(boxed_work_order_error)?,
        workspace_policy: workspace_policy_from_dto(workspace_policy)
            .map_err(boxed_work_order_error)?,
        gates: gate_set_from_dto(gates, gate_join).map_err(boxed_work_order_error)?,
        repeat_count: validate_repeat_count(repeat_count).map_err(mapped)?,
        sequence_lane_id: sequence_lane_id
            .map(SequenceLaneId::parse)
            .transpose()
            .map_err(|error| {
                boxed_work_order_error(WorkOrderError::invalid(
                    "sequence_lane_id",
                    error.to_string(),
                ))
            })?,
        parent_session_id: validate_parent_ref(parent_session_id).map_err(mapped)?,
        parent_turn_id: validate_parent_ref(parent_turn_id).map_err(mapped)?,
        parent_work_order_id: parent_work_order_id
            .map(WorkOrderId::parse)
            .transpose()
            .map_err(|error| {
                boxed_work_order_error(WorkOrderError::invalid(
                    "parent_work_order_id",
                    error.to_string(),
                ))
            })?,
        idempotency_key: validate_idempotency_key(idempotency_key).map_err(mapped)?,
    })
}

fn lane_input_from_create(
    request: &codegg_protocol::work_order::WorkOrderLaneCreateRequest,
) -> Result<NewSequenceLane, Box<CoreResponse>> {
    let failure_policy = request
        .failure_policy
        .as_deref()
        .map(|raw| {
            LaneFailurePolicy::parse(raw).ok_or_else(|| {
                boxed_work_order_error(WorkOrderError::invalid(
                    "failure_policy",
                    "unknown lane failure policy",
                ))
            })
        })
        .transpose()?
        .unwrap_or(LaneFailurePolicy::HoldLane);
    Ok(NewSequenceLane {
        label: validate_lane_label(request.label.as_deref()).map_err(boxed_work_order_error)?,
        failure_policy,
        idempotency_key: validate_idempotency_key(request.idempotency_key.as_deref())
            .map_err(boxed_work_order_error)?,
    })
}

fn patch_from_update(
    request: &codegg_protocol::work_order::WorkOrderUpdateRequest,
) -> Result<WorkOrderPatch, Box<CoreResponse>> {
    let mapped = |error: WorkOrderError| boxed_work_order_error(error);
    // Tri-state text fields: absent leaves the value unchanged, an empty
    // string clears it, otherwise the value replaces after validation.
    let title = match request.title.as_deref() {
        None => None,
        Some(raw) if raw.trim().is_empty() => Some(None),
        Some(raw) => Some(validate_title(Some(raw)).map_err(mapped)?),
    };
    let requested_model = match request.requested_model.as_deref() {
        None => None,
        Some(raw) if raw.trim().is_empty() => Some(None),
        Some(raw) => Some(validate_model(Some(raw)).map_err(mapped)?),
    };
    let requested_approval = match request.requested_approval.as_deref() {
        None => None,
        Some(raw) if raw.trim().is_empty() => Some(None),
        Some(raw) => Some(approval_from_dto(Some(raw)).map_err(boxed_work_order_error)?),
    };
    let requested_sandbox = match request.requested_sandbox.as_deref() {
        None => None,
        Some(raw) if raw.trim().is_empty() => Some(None),
        Some(raw) => Some(sandbox_from_dto(Some(raw)).map_err(boxed_work_order_error)?),
    };
    let workspace_policy = match request.workspace_policy.as_deref() {
        None => None,
        Some(raw) if raw.trim().is_empty() => Some(None),
        Some(raw) => Some(workspace_policy_from_dto(Some(raw)).map_err(boxed_work_order_error)?),
    };
    let gates = request
        .gates
        .as_deref()
        .map(|gates| gate_set_from_dto(gates, request.gate_join.as_deref()))
        .transpose()
        .map_err(boxed_work_order_error)?;
    let repeat_count = request
        .repeat_count
        .map(|count| validate_repeat_count(Some(count)))
        .transpose()
        .map_err(mapped)?;
    Ok(WorkOrderPatch {
        title,
        prompt: request
            .prompt
            .as_deref()
            .map(validate_prompt)
            .transpose()
            .map_err(mapped)?,
        requested_model,
        requested_approval,
        requested_sandbox,
        workspace_policy,
        gates,
        repeat_count,
    })
}
