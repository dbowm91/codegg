//! Project Work Orders M001: bounded wire DTOs.
//!
//! This module owns only data contracts for the durable project-level
//! `WorkOrder` domain: creation/update/list requests, lane operations,
//! occurrence projections, summary counts, and capability negotiation.
//! It owns no runtime services, no I/O, and no authorization: the daemon
//! resolves every ID-only locator to its owning project server-side
//! before capability evaluation, and `codegg-core::work_order` owns all
//! validation, lifecycle, and CAS semantics.
//!
//! ## Compatibility
//!
//! Additive fields and variants are minor: old clients that ignore
//! unknown variants remain forward-compatible. Prompt bodies travel in
//! create/update DTOs (they are author-supplied intent, not secrets);
//! projections never carry trigger secrets, provider credentials, or
//! hidden reasoning (M001 introduces no trigger secrets and no hidden
//! reasoning field).

use serde::{Deserialize, Serialize};

// ── Protocol version and capability ──────────────────────────────────

/// Version of the project work-order protocol surface.
pub const WORK_ORDER_PROTOCOL_VERSION: u32 = 1;
/// Capability string advertised for the work-order surface.
pub const WORK_ORDER_CAPABILITY: &str = "work_order.v1";

// ── Gate description DTOs ────────────────────────────────────────────

/// Wire name for one release-gate kind. The closed M001 gate set is
/// `immediate`, `delay`, `not_before`, `sequence_ready`, and
/// `external_trigger`. Unknown values fail closed at the daemon
/// boundary (invalid gate, never a silent default).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkOrderGateDto {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay_secs: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_before_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lane_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_ref: Option<String>,
}

// ── Create / batch DTOs ──────────────────────────────────────────────

/// Bounded work-order creation request. `project_id` scopes the
/// operation directly; every ID-only locator below resolves server-side.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkOrderCreateRequest {
    pub project_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_approval: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_sandbox: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_policy: Option<String>,
    #[serde(default)]
    pub gates: Vec<WorkOrderGateDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate_join: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence_lane_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_work_order_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

/// One item of an atomic batch creation request. Items share the
/// enclosing batch's `project_id`; per-item lane placement is explicit.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkOrderBatchItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_approval: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_sandbox: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_policy: Option<String>,
    #[serde(default)]
    pub gates: Vec<WorkOrderGateDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate_join: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_work_order_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

/// Atomic bounded batch creation request. Either every item (and the
/// optional lane ordering) commits in one transaction or none does.
/// `batch_key` scopes retry convergence to `(project_id, key)`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkOrderBatchCreateRequest {
    pub project_id: String,
    pub items: Vec<WorkOrderBatchItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence_lane_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch_key: Option<String>,
}

/// Bounded work-order update request. `None` leaves a field unchanged;
/// `Some` replaces it after domain validation. Lane placement changes
/// through the lane reorder operation, not through this DTO.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkOrderUpdateRequest {
    pub work_order_id: String,
    pub expected_revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_approval: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_sandbox: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_policy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gates: Option<Vec<WorkOrderGateDto>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate_join: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat_count: Option<u32>,
}

// ── Lane DTOs ────────────────────────────────────────────────────────

/// Bounded sequence-lane creation request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkOrderLaneCreateRequest {
    pub project_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_policy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
}

/// CAS-guarded lane reorder request. `ordered_work_order_ids` is the
/// complete new order: every current member exactly once, no extras.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkOrderLaneReorderRequest {
    pub lane_id: String,
    pub expected_revision: u64,
    pub ordered_work_order_ids: Vec<String>,
}

/// CAS-guarded request attaching one existing waiting work order to a
/// lane, appended or inserted at `position`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkOrderLaneAttachRequest {
    pub lane_id: String,
    pub expected_revision: u64,
    pub work_order_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub position: Option<u64>,
}

// ── Projection DTOs ──────────────────────────────────────────────────

/// Wire shape of one durable work order. Structural metadata only for
/// gate/policy structures; the author-supplied prompt/title are
/// carried because they are the work description itself. Never carries
/// trigger secrets, credentials, or hidden reasoning.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkOrderDto {
    pub work_order_id: String,
    pub revision: u64,
    pub project_id: String,
    pub creator_principal: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_work_order_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_approval: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_sandbox: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_policy: Option<String>,
    #[serde(default)]
    pub gates: Vec<WorkOrderGateDto>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate_join: Option<String>,
    pub repeat_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence_lane_id: Option<String>,
    pub state: String,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancelled_at_ms: Option<i64>,
}

/// Wire shape of one work-order occurrence. Materialization references
/// (`session_id`, `job_id`, `workspace_id`, `worktree_id`) are absent
/// until the M002 coordinator claims the occurrence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkOrderOccurrenceDto {
    pub occurrence_id: String,
    pub work_order_id: String,
    pub project_id: String,
    pub occurrence_index: u64,
    pub state: String,
    #[serde(default)]
    pub gate_latches: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_before_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_check_at_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attention_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    /// Claim/materialization timestamps, set by the M002 coordinator.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_at_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_at_ms: Option<i64>,
}

/// Wire shape of one sequence lane with its deterministic member order.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SequenceLaneDto {
    pub lane_id: String,
    pub project_id: String,
    pub revision: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub failure_policy: String,
    #[serde(default)]
    pub ordered_work_order_ids: Vec<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// Bounded per-project work-order summary counts for later projections.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkOrderSummaryDto {
    pub project_id: String,
    pub active: u64,
    pub paused: u64,
    pub completed: u64,
    pub cancelled: u64,
    pub archived: u64,
    pub waiting_occurrences: u64,
    pub attention_occurrences: u64,
    pub lane_count: u64,
}

/// Capability negotiation for the work-order surface. Clients MUST
/// clamp list/batch payloads to these bounds before sending.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkOrderCapabilitiesDto {
    pub supported: bool,
    pub protocol_version: u32,
    pub max_title_chars: usize,
    pub max_prompt_bytes: usize,
    pub max_batch_items: usize,
    pub max_list_limit: u32,
    pub max_repeat_count: u32,
    pub max_lanes_per_project: usize,
    pub max_lane_members: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_constants_are_set() {
        assert_eq!(WORK_ORDER_PROTOCOL_VERSION, 1);
        assert_eq!(WORK_ORDER_CAPABILITY, "work_order.v1");
    }

    #[test]
    fn create_request_round_trips() {
        let request = WorkOrderCreateRequest {
            project_id: "project-1".to_owned(),
            title: Some("Implement feature".to_owned()),
            prompt: "Do the thing".to_owned(),
            requested_model: None,
            requested_approval: None,
            requested_sandbox: None,
            workspace_policy: None,
            gates: vec![WorkOrderGateDto {
                kind: "immediate".to_owned(),
                delay_secs: None,
                not_before_ms: None,
                lane_id: None,
                trigger_ref: None,
            }],
            gate_join: Some("all".to_owned()),
            repeat_count: Some(1),
            sequence_lane_id: None,
            parent_session_id: None,
            parent_turn_id: None,
            parent_work_order_id: None,
            idempotency_key: Some("key-1".to_owned()),
        };
        let json = serde_json::to_string(&request).expect("serialize");
        let back: WorkOrderCreateRequest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, request);
    }

    #[test]
    fn legacy_create_fixture_decodes_with_optional_fields() {
        let request: WorkOrderCreateRequest =
            serde_json::from_str(r#"{"project_id":"project-1","prompt":"Do the thing"}"#)
                .expect("decode minimal create");
        assert_eq!(request.project_id, "project-1");
        assert!(request.title.is_none());
        assert!(request.gates.is_empty());
        assert!(request.repeat_count.is_none());
    }

    #[test]
    fn work_order_dto_round_trips() {
        let dto = WorkOrderDto {
            work_order_id: "wo-1".to_owned(),
            revision: 1,
            project_id: "project-1".to_owned(),
            creator_principal: "local-owner".to_owned(),
            parent_session_id: None,
            parent_turn_id: None,
            parent_work_order_id: None,
            title: Some("Title".to_owned()),
            prompt: "Prompt".to_owned(),
            requested_model: None,
            requested_approval: None,
            requested_sandbox: None,
            workspace_policy: None,
            gates: Vec::new(),
            gate_join: None,
            repeat_count: 1,
            sequence_lane_id: None,
            state: "active".to_owned(),
            created_at_ms: 1,
            updated_at_ms: 2,
            cancelled_at_ms: None,
        };
        let json = serde_json::to_string(&dto).expect("serialize");
        let back: WorkOrderDto = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, dto);
    }

    #[test]
    fn lane_reorder_round_trips() {
        let request = WorkOrderLaneReorderRequest {
            lane_id: "lane-1".to_owned(),
            expected_revision: 3,
            ordered_work_order_ids: vec!["wo-b".to_owned(), "wo-a".to_owned()],
        };
        let json = serde_json::to_string(&request).expect("serialize");
        let back: WorkOrderLaneReorderRequest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, request);
    }

    #[test]
    fn occurrence_dto_carries_no_materialization_by_default() {
        let dto = WorkOrderOccurrenceDto {
            occurrence_id: "occ-1".to_owned(),
            work_order_id: "wo-1".to_owned(),
            project_id: "project-1".to_owned(),
            occurrence_index: 0,
            state: "waiting".to_owned(),
            gate_latches: Vec::new(),
            not_before_ms: None,
            next_check_at_ms: None,
            session_id: None,
            job_id: None,
            workspace_id: None,
            worktree_id: None,
            attention_code: None,
            diagnostic: None,
            claimed_at_ms: None,
            started_at_ms: None,
            terminal_at_ms: None,
            created_at_ms: 1,
            updated_at_ms: 1,
        };
        let json = serde_json::to_string(&dto).expect("serialize");
        assert!(!json.contains("secret"));
        assert!(!json.contains("reasoning"));
        let back: WorkOrderOccurrenceDto = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.session_id, None);
        assert_eq!(back.job_id, None);
    }
}
