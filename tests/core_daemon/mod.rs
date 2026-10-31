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

// M007-D: the daemon's request future grew a layer when the daemon took
// ownership of the command catalog (ADR-0052), and proving that future is
// `Send` inside this crate now costs more than the default recursion
// budget. The limit is raised here rather than in the library, because the
// overflow is in *this* crate's obligation proof.
#![recursion_limit = "512"]

#[path = "../common/mod.rs"]
pub mod common;

#[path = "approval_router.inc"]
pub mod approval_router;

#[path = "checked_restore_integration.inc"]
pub mod checked_restore_integration;

#[path = "checkout_reservation_c004.inc"]
pub mod checkout_reservation_c004;

#[path = "checkout_mediation_authority_c008.inc"]
pub mod checkout_mediation_authority_c008;

#[path = "c002_daemon_compaction.inc"]
pub mod c002_daemon_compaction;

#[path = "command_catalog_m007d.inc"]
pub mod command_catalog_m007d;

#[path = "plugin_catalogue_m007g.inc"]
pub mod plugin_catalogue_m007g;

#[path = "collaboration_m001_chat.inc"]
pub mod collaboration_m001_chat;

#[path = "collaboration_m002_chat_policy.inc"]
pub mod collaboration_m002_chat_policy;

#[path = "collaboration_m003_chat_actions.inc"]
pub mod collaboration_m003_chat_actions;

#[path = "directory_listing_c005.inc"]
pub mod directory_listing_c005;

#[path = "directory_listing_c006.inc"]
pub mod directory_listing_c006;

#[path = "edit_checkpoint_integration.inc"]
pub mod edit_checkpoint_integration;

#[path = "headless_projection_consumer.inc"]
pub mod headless_projection_consumer;

#[path = "identity_live_execution_audit.inc"]
pub mod identity_live_execution_audit;

#[path = "identity_m002_live_audit_hooks.inc"]
pub mod identity_m002_live_audit_hooks;

#[path = "delegated_transcript_m002.inc"]
pub mod delegated_transcript_m002;

#[path = "identity_m003_daemon_authorization.inc"]
pub mod identity_m003_daemon_authorization;

#[path = "identity_m003_scheduler_job_complete.inc"]
pub mod identity_m003_scheduler_job_complete;

#[path = "identity_m004_audit_foundation.inc"]
pub mod identity_m004_audit_foundation;

#[path = "identity_m005_audit_instrumentation.inc"]
pub mod identity_m005_audit_instrumentation;

#[path = "job_state_terminality_c004.inc"]
pub mod job_state_terminality_c004;

#[path = "launch_directory_bootstrap.inc"]
pub mod launch_directory_bootstrap;

#[path = "m007h_daemon_read_surfaces.inc"]
pub mod m007h_daemon_read_surfaces;

#[path = "m007i_daemon_diff.inc"]
pub mod m007i_daemon_diff;
#[path = "m007i_daemon_promotion.inc"]
pub mod m007i_daemon_promotion;

#[path = "model_preference_convergence.inc"]
pub mod model_preference_convergence;

#[path = "presence_m001_leases.inc"]
pub mod presence_m001_leases;

#[path = "presence_m003_observation.inc"]
pub mod presence_m003_observation;

#[path = "project_activation.inc"]
pub mod project_activation;

#[path = "projection_artifact_handles.inc"]
pub mod projection_artifact_handles;

#[path = "projection_disclosure_invariants.inc"]
pub mod projection_disclosure_invariants;

#[path = "provider_connections_lifecycle.inc"]
pub mod provider_connections_lifecycle;

#[path = "provider_reconnect_e2e.inc"]
pub mod provider_reconnect_e2e;

#[path = "session_launch_service.inc"]
pub mod session_launch_service;

#[path = "shell_projection_harness.inc"]
pub mod shell_projection_harness;

#[path = "shell_projection_phase10.inc"]
pub mod shell_projection_phase10;

#[path = "single_daemon_lifecycle.inc"]
pub mod single_daemon_lifecycle;

#[path = "snapshot.inc"]
pub mod snapshot;

#[path = "storage_migrations.inc"]
pub mod storage_migrations;

#[path = "team_m003_membership_admin.inc"]
pub mod team_m003_membership_admin;

#[path = "work_orders_m001_foundation.inc"]
pub mod work_orders_m001_foundation;

#[path = "work_orders_m002_materialization.inc"]
pub mod work_orders_m002_materialization;

#[path = "work_orders_m004_dashboard.inc"]
pub mod work_orders_m004_dashboard;

#[path = "work_orders_m005_trigger.inc"]
pub mod work_orders_m005_trigger;

#[path = "work_orders_m006_agent_tool.inc"]
pub mod work_orders_m006_agent_tool;

#[path = "work_orders_m007_trajectory.inc"]
pub mod work_orders_m007_trajectory;

#[path = "workstream_claim_m003.inc"]
pub mod workstream_claim_m003;

#[path = "writer_fence_c003.inc"]
pub mod writer_fence_c003;

#[path = "writer_permit_service.inc"]
pub mod writer_permit_service;

#[path = "ws_rate_limit.inc"]
pub mod ws_rate_limit;
