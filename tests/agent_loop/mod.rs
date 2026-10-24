//! Consolidated the agent loop, context and compaction, subagents, and skills integration test family (M004).
//!
//! History: each member lived as its own `tests/<NAME>.rs` file and was
//! therefore its own Cargo test executable — N executables for N files.
//! The `.inc` extension is what stops Cargo's `tests/<dir>/*.rs`
//! auto-discovery from generating one executable per file, and
//! `tests/<dir>/mod.rs` is the single integration target. Cargo does not
//! auto-discover `tests/<dir>/mod.rs` either, so the target is declared
//! explicitly in the root `Cargo.toml`. Each member is included with
//! `#[path]`, the pattern the existing `tests/projection_replay/` and
//! `tests/session_family/` families already use.
//!
//! Consolidation replaces N separate Cargo test executables with 1,
//! preserving every test body and every assertion. The only visible change
//! is the test id, which becomes `<family>::<member>::<test>`.
//!
//! `common` — the shared integration-test helper module — lives one
//! directory up under `tests/common/mod.rs`, so it is bound here with an
//! explicit `#[path]`; the members reach it with `use super::common;`.

#[path = "../common/mod.rs"]
pub mod common;

#[path = "agent_convergence.inc"]
pub mod agent_convergence;

#[path = "agent_loop_harness.inc"]
pub mod agent_loop_harness;

#[path = "asset_snapshot.inc"]
pub mod asset_snapshot;

#[path = "blackboard_economics_m008.inc"]
pub mod blackboard_economics_m008;

#[path = "blackboard_trajectory_m008.inc"]
pub mod blackboard_trajectory_m008;

#[path = "causal_active_m005.inc"]
pub mod causal_active_m005;

#[path = "compaction.inc"]
pub mod compaction;

#[path = "context_continuity_m004.inc"]
pub mod context_continuity_m004;

#[path = "context_epoch_handoff.inc"]
pub mod context_epoch_handoff;

#[path = "delegated_transcript.inc"]
pub mod delegated_transcript;

#[path = "context_plan_convergence.inc"]
pub mod context_plan_convergence;

#[path = "delegated_notice_c001.inc"]
pub mod delegated_notice_c001;

#[path = "event_processor.inc"]
pub mod event_processor;

#[path = "goal_continuation_progress.inc"]
pub mod goal_continuation_progress;

#[path = "habit_skill_promotion.inc"]
pub mod habit_skill_promotion;

#[path = "long_horizon_trajectory_qualification.inc"]
pub mod long_horizon_trajectory_qualification;

#[path = "memory.inc"]
pub mod memory;

#[path = "opencode_go_connection_trajectory.inc"]
pub mod opencode_go_connection_trajectory;

#[path = "plugin.inc"]
pub mod plugin;

#[path = "plugin_contributions.inc"]
pub mod plugin_contributions;

#[path = "provider_transcripts.inc"]
pub mod provider_transcripts;

#[path = "reliability_qualification_m008.inc"]
pub mod reliability_qualification_m008;

#[path = "retry_budget_reconciliation.inc"]
pub mod retry_budget_reconciliation;

#[path = "skill_publication.inc"]
pub mod skill_publication;

#[path = "skills.inc"]
pub mod skills;

#[path = "skills_registry.inc"]
pub mod skills_registry;

#[path = "subagent.inc"]
pub mod subagent;

#[path = "tool_surface_minimization.inc"]
pub mod tool_surface_minimization;

#[path = "work_plan_eggplan_differential.inc"]
pub mod work_plan_eggplan_differential;

#[path = "work_plan_projection_arbiter.inc"]
pub mod work_plan_projection_arbiter;

#[path = "work_plan_repository_binding.inc"]
pub mod work_plan_repository_binding;

#[path = "work_plan_resolved_evidence.inc"]
pub mod work_plan_resolved_evidence;

#[path = "workspace_isolation.inc"]
pub mod workspace_isolation;

#[path = "workspace_root_execution_authority.inc"]
pub mod workspace_root_execution_authority;

#[path = "workspace_services_isolation.inc"]
pub mod workspace_services_isolation;
