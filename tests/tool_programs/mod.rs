//! Consolidated tool program execution, the tool broker, and the tool contract integration test family (M004).
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

#[path = "eggsact_adapter.inc"]
mod eggsact_adapter;

#[path = "eggsact_deterministic_tools.inc"]
mod eggsact_deterministic_tools;

#[path = "hosted_tool_program_adapter.inc"]
mod hosted_tool_program_adapter;

#[path = "hosted_tool_program_contention.inc"]
mod hosted_tool_program_contention;

#[path = "hosted_tool_program_equivalence.inc"]
mod hosted_tool_program_equivalence;

#[path = "hosted_tool_program_recovery.inc"]
mod hosted_tool_program_recovery;

#[path = "hosted_tool_program_security.inc"]
mod hosted_tool_program_security;

#[path = "path_validation.inc"]
mod path_validation;

#[path = "python_sandbox_adversarial.inc"]
mod python_sandbox_adversarial;

#[path = "python_scheduler_execution.inc"]
mod python_scheduler_execution;

#[path = "python_script_completion_wakeup.inc"]
mod python_script_completion_wakeup;

#[path = "tool_broker_integration.inc"]
mod tool_broker_integration;

#[path = "tool_contract_guards.inc"]
mod tool_contract_guards;

#[path = "tool_execution.inc"]
mod tool_execution;

#[path = "tool_program_background.inc"]
mod tool_program_background;

#[path = "tool_program_build_test_matrix.inc"]
mod tool_program_build_test_matrix;

#[path = "tool_program_cache.inc"]
mod tool_program_cache;

#[path = "tool_program_chaos.inc"]
mod tool_program_chaos;

#[path = "tool_program_child_jobs.inc"]
mod tool_program_child_jobs;

#[path = "tool_program_child_recovery.inc"]
mod tool_program_child_recovery;

#[path = "tool_program_concurrent.inc"]
mod tool_program_concurrent;

#[path = "tool_program_context_artifacts.inc"]
mod tool_program_context_artifacts;

#[path = "tool_program_diff_palette.inc"]
mod tool_program_diff_palette;

#[path = "tool_program_fault_injection.inc"]
mod tool_program_fault_injection;

#[path = "tool_program_git_lsp_palette.inc"]
mod tool_program_git_lsp_palette;

#[path = "tool_program_ir.inc"]
mod tool_program_ir;

#[path = "tool_program_language.inc"]
mod tool_program_language;

#[path = "tool_program_lifecycle.inc"]
mod tool_program_lifecycle;

#[path = "tool_program_m011_correctness.inc"]
mod tool_program_m011_correctness;

#[path = "tool_program_m012_authority.inc"]
mod tool_program_m012_authority;

#[path = "tool_program_m012_broker_failures.inc"]
mod tool_program_m012_broker_failures;

#[path = "tool_program_m012_child_ownership.inc"]
mod tool_program_m012_child_ownership;

#[path = "tool_program_m012_hosted_status.inc"]
mod tool_program_m012_hosted_status;

#[path = "tool_program_m012_notifications.inc"]
mod tool_program_m012_notifications;

#[path = "tool_program_m012_process_recovery.inc"]
mod tool_program_m012_process_recovery;

#[path = "tool_program_m012_recovery.inc"]
mod tool_program_m012_recovery;

#[path = "tool_program_m013_authority.inc"]
mod tool_program_m013_authority;

#[path = "tool_program_m013_descendants.inc"]
mod tool_program_m013_descendants;

#[path = "tool_program_m013_ledger_integrity.inc"]
mod tool_program_m013_ledger_integrity;

#[path = "tool_program_m013_lineage.inc"]
mod tool_program_m013_lineage;

#[path = "tool_program_m013_notifications_sqlite.inc"]
mod tool_program_m013_notifications_sqlite;

#[path = "tool_program_m013_process_recovery.inc"]
mod tool_program_m013_process_recovery;

#[path = "tool_program_m013_replay.inc"]
mod tool_program_m013_replay;

#[path = "tool_program_m013_results.inc"]
mod tool_program_m013_results;

#[path = "tool_program_m014_artifacts.inc"]
mod tool_program_m014_artifacts;

#[path = "tool_program_m014_authority_pipeline.inc"]
mod tool_program_m014_authority_pipeline;

#[path = "tool_program_m014_checkpoint_recovery.inc"]
mod tool_program_m014_checkpoint_recovery;

#[path = "tool_program_m014_daemon_recovery.inc"]
mod tool_program_m014_daemon_recovery;

#[path = "tool_program_m014_lineage_migration.inc"]
mod tool_program_m014_lineage_migration;

#[path = "tool_program_m014_notification_delivery.inc"]
mod tool_program_m014_notification_delivery;

#[path = "tool_program_m014_recursive_descendants.inc"]
mod tool_program_m014_recursive_descendants;

#[path = "tool_program_m015_artifact_pipeline.inc"]
mod tool_program_m015_artifact_pipeline;

#[path = "tool_program_m015_authority_contract.inc"]
mod tool_program_m015_authority_contract;

#[path = "tool_program_m015_daemon_failpoints.inc"]
mod tool_program_m015_daemon_failpoints;

#[path = "tool_program_m015_descendant_convergence.inc"]
mod tool_program_m015_descendant_convergence;

#[path = "tool_program_m015_notification_recovery.inc"]
mod tool_program_m015_notification_recovery;

#[path = "tool_program_m015_recovery.inc"]
mod tool_program_m015_recovery;

#[path = "tool_program_m016_notification_replay.inc"]
mod tool_program_m016_notification_replay;

#[path = "tool_program_m017_notification_confirmation.inc"]
mod tool_program_m017_notification_confirmation;

#[path = "tool_program_model_behavior.inc"]
mod tool_program_model_behavior;

#[path = "tool_program_notifications.inc"]
mod tool_program_notifications;

#[path = "tool_program_projection.inc"]
mod tool_program_projection;

#[path = "tool_program_property.inc"]
mod tool_program_property;

#[path = "tool_program_read_palette.inc"]
mod tool_program_read_palette;

#[path = "tool_program_recovery.inc"]
mod tool_program_recovery;

#[path = "tool_program_resource_convergence.inc"]
mod tool_program_resource_convergence;

#[path = "tool_program_runtime.inc"]
mod tool_program_runtime;

#[path = "tool_program_scenarios.inc"]
mod tool_program_scenarios;

#[path = "tool_program_search_palette.inc"]
mod tool_program_search_palette;

#[path = "tool_program_static_bounds.inc"]
mod tool_program_static_bounds;

#[path = "tool_registry.inc"]
mod tool_registry;

#[path = "tool_structured_execution.inc"]
mod tool_structured_execution;
