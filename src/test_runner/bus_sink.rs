use crate::bus::events::AppEvent;
use crate::bus::global::GlobalEventBus;
use crate::test_runner::types::{
    TestEventSink, TestRunCompletedSnapshot, TestRunProgressSnapshot, TestRunStartedSnapshot,
};

/// A [`TestEventSink`] that publishes lifecycle events to the
/// [`GlobalEventBus`]. This allows the core protocol bridge
/// (`map_app_event_to_core_event`) to convert them into wire-level
/// `CoreEvent::TestRun*` variants for remote frontends.
pub struct BusEventSink;

impl TestEventSink for BusEventSink {
    fn started(&self, snapshot: TestRunStartedSnapshot) {
        GlobalEventBus::publish(AppEvent::TestRunStarted {
            session_id: snapshot.session_id.clone(),
            job_id: snapshot.job_id,
            command: snapshot.command,
            cwd: snapshot.cwd,
        });
    }

    fn progress(&self, snapshot: TestRunProgressSnapshot) {
        GlobalEventBus::publish(AppEvent::TestRunProgress {
            session_id: snapshot.session_id.clone(),
            job_id: snapshot.job_id,
            message: snapshot.message,
        });
    }

    fn completed(&self, snapshot: TestRunCompletedSnapshot) {
        GlobalEventBus::publish(AppEvent::TestRunCompleted {
            session_id: snapshot.session_id.clone(),
            job_id: snapshot.job_id,
            status: snapshot.status,
            summary: snapshot.summary,
            log_dir: snapshot.log_dir,
            run_id: snapshot.run_id.clone(),
            parent_run_id: snapshot.parent_run_id.clone(),
        });
        if let (Some(parent_run_id), Some(child_run_id)) = (snapshot.parent_run_id, snapshot.run_id)
        {
            GlobalEventBus::publish(AppEvent::RunRerunLinked {
                session_id: snapshot.session_id,
                parent_run_id,
                child_run_id,
            });
        }
    }
}
