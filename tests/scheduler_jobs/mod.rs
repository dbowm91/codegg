//! Consolidated the scheduler, durable jobs, managed processes, and background work integration test family (M004).
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

#[path = "async_background_c003_idle_lifecycle.inc"]
mod async_background_c003_idle_lifecycle;

#[path = "async_background_c004_live_attention.inc"]
mod async_background_c004_live_attention;

#[path = "async_background_c005_idle_resume.inc"]
mod async_background_c005_idle_resume;

#[path = "async_background_c006_tool_to_idle_ci.inc"]
mod async_background_c006_tool_to_idle_ci;

#[path = "bash_background_m002_routes.inc"]
mod bash_background_m002_routes;

#[path = "ci_watch_c001_idle_driver.inc"]
mod ci_watch_c001_idle_driver;

#[path = "ci_watch_m004_completion.inc"]
mod ci_watch_m004_completion;

#[path = "completion_signal_broadcast.inc"]
mod completion_signal_broadcast;

#[path = "durable_jobs_phase4.inc"]
mod durable_jobs_phase4;

#[path = "eggwork_node_telemetry.inc"]
mod eggwork_node_telemetry;

#[path = "eggwork_remote_execution.inc"]
mod eggwork_remote_execution;

#[path = "eggwork_remote_limits.inc"]
mod eggwork_remote_limits;

#[path = "eggwork_target_boundary.inc"]
mod eggwork_target_boundary;

#[path = "job_background_test_slice.inc"]
mod job_background_test_slice;

#[path = "job_continuation_c002_idle_resume.inc"]
mod job_continuation_c002_idle_resume;

#[path = "job_continuation_m003_visibility.inc"]
mod job_continuation_m003_visibility;

#[path = "job_notification_m001_contract.inc"]
mod job_notification_m001_contract;

#[path = "managed_process_descendants.inc"]
mod managed_process_descendants;

#[path = "managed_scratch_job_admission.inc"]
mod managed_scratch_job_admission;

#[path = "schedule_materialization.inc"]
mod schedule_materialization;

#[path = "scheduler_authority_matrix.inc"]
mod scheduler_authority_matrix;

#[path = "scheduler_contention.inc"]
mod scheduler_contention;

#[path = "scheduler_executor_coverage.inc"]
mod scheduler_executor_coverage;

#[path = "scheduler_permit_lifecycle.inc"]
mod scheduler_permit_lifecycle;

#[path = "scheduler_phase5.inc"]
mod scheduler_phase5;

#[path = "scheduler_protocol_consistency.inc"]
mod scheduler_protocol_consistency;

#[path = "scheduler_resource_profiles.inc"]
mod scheduler_resource_profiles;

#[path = "scheduler_restart_recovery.inc"]
mod scheduler_restart_recovery;

#[path = "scheduler_submission_idempotency.inc"]
mod scheduler_submission_idempotency;

#[path = "scratch_cache_root_contract.inc"]
mod scratch_cache_root_contract;

#[path = "scratch_retention_sweep.inc"]
mod scratch_retention_sweep;

#[path = "scratch_usage_census.inc"]
mod scratch_usage_census;
