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
mod common;

#[path = "agent_convergence.inc"]
mod agent_convergence;

#[path = "agent_loop_harness.inc"]
mod agent_loop_harness;

#[path = "asset_snapshot.inc"]
mod asset_snapshot;

#[path = "causal_active_m005.inc"]
mod causal_active_m005;

#[path = "compaction.inc"]
mod compaction;

#[path = "context_continuity_m004.inc"]
mod context_continuity_m004;

#[path = "context_epoch_handoff.inc"]
mod context_epoch_handoff;

#[path = "context_plan_convergence.inc"]
mod context_plan_convergence;

#[path = "event_processor.inc"]
mod event_processor;

#[path = "goal_continuation_progress.inc"]
mod goal_continuation_progress;

#[path = "habit_skill_promotion.inc"]
mod habit_skill_promotion;

#[path = "long_horizon_trajectory_qualification.inc"]
mod long_horizon_trajectory_qualification;

#[path = "memory.inc"]
mod memory;

#[path = "opencode_go_connection_trajectory.inc"]
mod opencode_go_connection_trajectory;

#[path = "plugin.inc"]
mod plugin;

#[path = "plugin_contributions.inc"]
mod plugin_contributions;

#[path = "provider_transcripts.inc"]
mod provider_transcripts;

#[path = "reliability_qualification_m008.inc"]
mod reliability_qualification_m008;

#[path = "retry_budget_reconciliation.inc"]
mod retry_budget_reconciliation;

#[path = "skill_publication.inc"]
mod skill_publication;

#[path = "skills.inc"]
mod skills;

#[path = "skills_registry.inc"]
mod skills_registry;

#[path = "subagent.inc"]
mod subagent;

#[path = "tool_surface_minimization.inc"]
mod tool_surface_minimization;

#[path = "work_plan_eggplan_differential.inc"]
mod work_plan_eggplan_differential;

#[path = "work_plan_projection_arbiter.inc"]
mod work_plan_projection_arbiter;

#[path = "work_plan_repository_binding.inc"]
mod work_plan_repository_binding;

#[path = "work_plan_resolved_evidence.inc"]
mod work_plan_resolved_evidence;

#[path = "workspace_isolation.inc"]
mod workspace_isolation;

#[path = "workspace_root_execution_authority.inc"]
mod workspace_root_execution_authority;

#[path = "workspace_services_isolation.inc"]
mod workspace_services_isolation;
