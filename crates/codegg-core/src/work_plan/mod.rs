//! Durable WorkPlan foundation with M003 projection and arbitration.
//!
//! See `model` for bounds/transitions, `store` for CAS persistence,
//! `assessment` for the pure completion arbiter input, `projection` for the
//! bounded model read surface, `evidence` for host-evidence correlation, and
//! `todo_projection` for the one-way WorkPlan-to-Todo contract.

pub mod assessment;
pub mod evidence;
pub mod model;
pub mod projection;
pub mod store;
pub mod todo_projection;

pub use assessment::{
    assess_work_plan, WorkPlanCompletionAssessment, MAX_ASSESSMENT_REASON_CHARS,
    MAX_ASSESSMENT_TEXT_CHARS,
};
pub use evidence::{
    item_has_failed_evidence, item_has_in_progress_evidence, item_has_unavailable_evidence,
    item_is_satisfied, item_is_user_judgment_only, HostEvidenceStatus, WorkPlanEvidenceSnapshot,
};
pub use model::{
    actionable_items, can_transition_item, can_transition_plan, work_plan_origin_digest,
    NewWorkItem, NewWorkPlan, WorkAcceptance, WorkAcceptanceDisposition, WorkEvidenceKind,
    WorkEvidenceRef, WorkItem, WorkItemId, WorkItemPatch, WorkItemStatus, WorkPlan, WorkPlanError,
    WorkPlanId, WorkPlanStatus, MAX_ACCEPTANCE_CHARS, MAX_ACCEPTANCE_NOTE_CHARS,
    MAX_ACCEPTANCE_PER_ITEM, MAX_BLOCKER_CHARS, MAX_DEPENDENCIES_PER_ITEM,
    MAX_EVIDENCE_DETAIL_CHARS, MAX_EVIDENCE_PER_ITEM, MAX_EVIDENCE_REF_CHARS, MAX_ITEMS_PER_PLAN,
    MAX_ITEM_DESCRIPTION_CHARS, MAX_NEXT_ACTION_CHARS, MAX_OBJECTIVE_CHARS,
    MAX_ORIGIN_PROVENANCE_CHARS, MAX_OWNER_REF_CHARS, MAX_PHASE_CHARS, MAX_SCOPE_ID_CHARS,
};
pub use projection::{
    lookup_item_summary, project_work_plan, WorkPlanItemSummary, WorkPlanProjection,
    WorkPlanProjectionParams, MAX_PROJECTION_BYTES, MAX_PROJECTION_ITEMS,
    MAX_PROJECTION_OBJECTIVE_CHARS, MAX_PROJECTION_TEXT_CHARS,
};
pub use store::{WorkPlanStore, WORK_PLAN_SCHEMA_STATEMENTS};
pub use todo_projection::{
    parse_todo_work_ref, project_to_todo_items, todo_id_for_work_item, validate_todo_feedback,
    TodoFeedbackError, MAX_PROJECTED_TODO_CONTENT_CHARS, TODO_WORK_REF_SEPARATOR,
};
