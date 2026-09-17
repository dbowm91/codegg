//! Durable project-level work orders, occurrences, and sequence lanes
//! (Project Work Orders M001).
//!
//! A `WorkOrder` is project intent: it records that some future work
//! should happen in a project, under which release conditions, in which
//! sequence lane, and with which requested model/policy snapshot. It is
//! not scheduler execution authority, not a durable schedule rule, not
//! delegated child intent, not within-session completion state, and not
//! a conversation.
//!
//! ## Design notes
//!
//! - Project authorization guards every operation at the daemon boundary;
//!   this module performs no authorization itself so the daemon gate
//!   stays the single authority. Unknown IDs fail closed via
//!   [`work_order_project`] returning `None`.
//! - Work-order, occurrence, and lane IDs are distinct typed identities
//!   ([`WorkOrderId`], [`WorkOrderOccurrenceId`], [`SequenceLaneId`]).
//! - Waiting work orders exist without session rows. The store never
//!   executes: no job submission, no worktree allocation, no model call.
//! - Updates and lane reorders are CAS-guarded by revision; concurrent
//!   writers receive an explicit conflict, never last-writer-wins.
//! - Batch creation is all-or-nothing in one transaction with
//!   deterministic lane positions.
//! - Submission/batch keys are idempotent: retries converge on stored
//!   rows, while a reused key with a different payload is an explicit
//!   mismatch conflict.
//! - Prompt/title/gate/list/repeat fields are bounded before persistence
//!   and projection. No trigger secret material exists in M001 and no
//!   hidden reasoning field exists.
//! - Occurrence indexing is explicit and 0-based; repeat counts are
//!   finite and bounded.
//!
//! ## Transport contract
//!
//! Wire DTOs live in [`codegg_protocol::work_order`] (`WorkOrderDto`,
//! `WorkOrderOccurrenceDto`, `SequenceLaneDto`, request shapes,
//! capabilities). This module owns the domain state machine and converts
//! to those DTOs. Work-order storage stays separate from the append-only
//! audit store; [`audit_metadata_for_work_order`] exposes only structural
//! locators (never bodies) for audit linkage.

pub mod model;
pub mod store;

pub use model::{
    audit_metadata_for_work_order, can_edit_execution_fields, can_transition_occurrence,
    can_transition_work_order, validate_diagnostic, validate_gate_set, validate_idempotency_key,
    validate_lane_label, validate_list_limit, validate_model, validate_parent_ref, validate_prompt,
    validate_repeat_count, validate_title, ApprovalRequest, AttentionCode, GateJoin, GateKind,
    GateSpec, LaneFailurePolicy, NewSequenceLane, NewWorkOrder, OccurrenceState, ReleaseGateSet,
    SandboxRequest, SequenceLane, WorkOrder, WorkOrderError, WorkOrderOccurrence, WorkOrderPatch,
    WorkOrderState, WorkOrderSummary, WorkspacePolicy, DEFAULT_WORK_ORDER_LIST_LIMIT,
    MAX_DELAY_SECS, MAX_DIAGNOSTIC_CHARS, MAX_GATE_SPEC_BYTES, MAX_IDEMPOTENCY_KEY_LEN,
    MAX_LANES_PER_PROJECT, MAX_LANE_LABEL_CHARS, MAX_LANE_MEMBERS, MAX_MODEL_ID_CHARS,
    MAX_NOT_BEFORE_MS, MAX_PARENT_REF_CHARS, MAX_REPEAT_COUNT, MAX_TRIGGER_REF_CHARS,
    MAX_WORK_ORDER_BATCH_ITEMS, MAX_WORK_ORDER_LIST_LIMIT, MAX_WORK_ORDER_PROMPT_BYTES,
    MAX_WORK_ORDER_TITLE_CHARS,
};
pub use store::{
    ensure_work_order_tables, work_order_project, BatchOutcome, CreateOutcome, OccurrenceListPage,
    WorkOrderConfig, WorkOrderListPage, WorkOrderService, WORK_ORDER_SCHEMA_STATEMENTS,
};
