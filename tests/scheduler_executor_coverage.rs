//! Regression guard: every job kind a model-facing tool can submit must have a
//! registered executor in the production registration path.
//!
//! `python_script` shipped with `PythonJobExecutor` registered only in a dead
//! helper function, never in
//! `register_default_executors_sync_with_agent_runs_and_control_and_worktree`.
//! The scheduler therefore marked every Python job unschedulable ("no executor
//! registered for job kind"), and because that path neither published a
//! completion nor woke waiters, the tool blocked for its entire
//! `timeout + 30s` bound and then reported a timeout that said nothing about
//! the real cause.
//!
//! This asserts the coverage directly so the same gap cannot reappear silently
//! for any other kind.

use std::sync::Arc;

use codegg::scheduler::JobScheduler;
use codegg_core::jobs::{DaemonGeneration, InMemoryJobStore, JobStore};
use codegg_core::workspace::{InMemoryWorkspaceStore, WorkspaceRegistry};
use codegg_core::workspace_services::{
    ProductionWorkspaceServicesFactory, WorkspaceServicePolicy, WorkspaceServiceRegistry,
};

#[tokio::test]
async fn production_registration_covers_every_required_executor_kind() {
    let root = tempfile::tempdir().expect("temp workspace");
    let workspace_registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
        .await
        .expect("workspace registry");
    workspace_registry
        .get_or_register(root.path())
        .await
        .expect("register workspace");
    let services = WorkspaceServiceRegistry::new(
        workspace_registry,
        Arc::new(ProductionWorkspaceServicesFactory),
        WorkspaceServicePolicy::default(),
    );
    let store: Arc<dyn JobStore> = Arc::new(InMemoryJobStore::new());
    let scheduler = JobScheduler::new(
        store,
        services,
        codegg::scheduler::ResolvedSchedulerConfig::default(),
        DaemonGeneration::new_unchecked("gen-coverage"),
    );

    // The exact call the daemon makes. No subagent pool, matching a daemon that
    // has none — that is precisely when a conditional registration silently
    // drops out.
    scheduler
        .register_default_executors_sync_with_agent_runs_and_control_and_worktree(
            None, None, None, None, None,
        )
        .expect("register default executors");

    let registered: Vec<&'static str> = scheduler
        .executor_kinds_sync()
        .iter()
        .map(|kind| kind.as_str())
        .collect();

    let missing: Vec<&'static str> = codegg::scheduler::executor::REQUIRED_EXECUTOR_KINDS
        .iter()
        .map(|kind| kind.as_str())
        .filter(|name| !registered.contains(name))
        .collect();

    assert!(
        missing.is_empty(),
        "job kinds with no registered executor will stall their submitting tool \
         for the whole wait bound: {missing:?} (registered: {registered:?})"
    );
}
