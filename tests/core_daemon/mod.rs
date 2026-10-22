//! Consolidated the daemon core, session lifecycle, identity authority, and work orders integration test family (M004).
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

#[path = "approval_router.inc"]
mod approval_router;

#[path = "checked_restore_integration.inc"]
mod checked_restore_integration;

#[path = "collaboration_m001_chat.inc"]
mod collaboration_m001_chat;

#[path = "collaboration_m002_chat_policy.inc"]
mod collaboration_m002_chat_policy;

#[path = "collaboration_m003_chat_actions.inc"]
mod collaboration_m003_chat_actions;

#[path = "directory_listing_c005.inc"]
mod directory_listing_c005;

#[path = "directory_listing_c006.inc"]
mod directory_listing_c006;

#[path = "edit_checkpoint_integration.inc"]
mod edit_checkpoint_integration;

#[path = "headless_projection_consumer.inc"]
mod headless_projection_consumer;

#[path = "identity_live_execution_audit.inc"]
mod identity_live_execution_audit;

#[path = "identity_m002_live_audit_hooks.inc"]
mod identity_m002_live_audit_hooks;

#[path = "identity_m003_daemon_authorization.inc"]
mod identity_m003_daemon_authorization;

#[path = "identity_m003_scheduler_job_complete.inc"]
mod identity_m003_scheduler_job_complete;

#[path = "identity_m004_audit_foundation.inc"]
mod identity_m004_audit_foundation;

#[path = "identity_m005_audit_instrumentation.inc"]
mod identity_m005_audit_instrumentation;

#[path = "launch_directory_bootstrap.inc"]
mod launch_directory_bootstrap;

#[path = "model_preference_convergence.inc"]
mod model_preference_convergence;

#[path = "presence_m001_leases.inc"]
mod presence_m001_leases;

#[path = "presence_m003_observation.inc"]
mod presence_m003_observation;

#[path = "project_activation.inc"]
mod project_activation;

#[path = "projection_artifact_handles.inc"]
mod projection_artifact_handles;

#[path = "projection_disclosure_invariants.inc"]
mod projection_disclosure_invariants;

#[path = "provider_connections_lifecycle.inc"]
mod provider_connections_lifecycle;

#[path = "provider_reconnect_e2e.inc"]
mod provider_reconnect_e2e;

#[path = "shell_projection_harness.inc"]
mod shell_projection_harness;

#[path = "shell_projection_phase10.inc"]
mod shell_projection_phase10;

#[path = "single_daemon_lifecycle.inc"]
mod single_daemon_lifecycle;

#[path = "snapshot.inc"]
mod snapshot;

#[path = "storage_migrations.inc"]
mod storage_migrations;

#[path = "team_m003_membership_admin.inc"]
mod team_m003_membership_admin;

#[path = "work_orders_m001_foundation.inc"]
mod work_orders_m001_foundation;

#[path = "work_orders_m002_materialization.inc"]
mod work_orders_m002_materialization;

#[path = "work_orders_m004_dashboard.inc"]
mod work_orders_m004_dashboard;

#[path = "work_orders_m005_trigger.inc"]
mod work_orders_m005_trigger;

#[path = "work_orders_m006_agent_tool.inc"]
mod work_orders_m006_agent_tool;

#[path = "work_orders_m007_trajectory.inc"]
mod work_orders_m007_trajectory;

#[path = "workstream_claim_m003.inc"]
mod workstream_claim_m003;

#[path = "ws_rate_limit.inc"]
mod ws_rate_limit;
