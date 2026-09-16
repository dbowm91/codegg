//! Durable WorkPlan foundation.
//!
//! See `model` for bounds/transitions and `store` for CAS persistence.

pub mod model;
pub mod store;

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
pub use store::{WorkPlanStore, WORK_PLAN_SCHEMA_STATEMENTS};
