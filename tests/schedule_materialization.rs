//! M004 — production schedule materialization and crash-safe occurrence
//! idempotency.
//!
//! The daemon used to persist schedules and never fire them: nothing called
//! [`ScheduleStore::claim_due`] outside a test, so a recurring schedule was
//! durable data with no trigger. These tests pin the production path that
//! closes that gap:
//!
//! * an occurrence materializes exactly one durable job, through the single
//!   production admission facade;
//! * the occurrence's job id is a pure function of the occurrence, so a crash
//!   between "the job row was written" and "the occurrence row was written"
//!   replays into the *same* job instead of a second one;
//! * an occurrence the materializer refuses is **not** recorded, so it stays
//!   retryable instead of being silently consumed;
//! * a recurring schedule keeps its own grid instead of drifting to the tick
//!   time;
//! * the host boundary is re-applied at materialization time, so an
//!   administrator who withdrew a node's approval stops the occurrence, and
//!   there is no local fallback.

mod common;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};

use codegg::core::remote_targets::RemoteTargetPolicy;
use codegg::scheduler::schedule_driver::ScheduleDriver;
use codegg::scheduler::schedule_materializer::ScheduleMaterializer;
use codegg::scheduler::submission::JobSubmissionService;
use codegg::scheduler::{JobScheduler, ResolvedSchedulerConfig};
use codegg_config::schema::{EggworkConfig, EggworkNodeProfile};
use codegg_core::jobs::{
    occurrence_job_id, DaemonGeneration, ExecutionTarget, IdempotencyClass, JobKind, JobPayload,
    JobPriority, JobStore, MaterializerError, MissedRunPolicy, OccurrenceMaterializer,
    OverlapPolicy, ResourceRequest, RetryPolicy, ScheduleKind, ScheduleRecord, ScheduleState,
    ScheduleStore, ScheduleTemplate, SqliteJobStore, SqliteScheduleStore,
};
use codegg_core::workspace::{InMemoryWorkspaceStore, WorkspaceId, WorkspaceRegistry};
use codegg_core::workspace_services::{
    ProductionWorkspaceServicesFactory, WorkspaceServicePolicy, WorkspaceServiceRegistry,
};

/// A local build template: the shape a recurring task actually uses.
fn local_template() -> codegg_core::jobs::JobTemplate {
    codegg_core::jobs::JobTemplate {
        kind: JobKind::Build,
        priority: JobPriority::Normal,
        payload: JobPayload::ManagedArgv {
            argv: vec!["cargo".to_owned(), "build".to_owned()],
            cwd: None,
            sandbox_profile: None,
            network_intent: codegg_core::approval::NetworkIntent::LegacyUnrestricted,
        },
        resource_request: ResourceRequest::default(),
        retry_policy: RetryPolicy::no_retry(),
        idempotency: IdempotencyClass::SafeRepeat,
        timeout: None,
        labels: HashMap::new(),
        target: ExecutionTarget::default(),
    }
}

fn template_with_kind(kind: ScheduleKind, target: ExecutionTarget) -> ScheduleTemplate {
    let mut template = local_template();
    template.target = target;
    ScheduleTemplate {
        workspace_id: WorkspaceId::new_unchecked("ws-materialization"),
        session_id: None,
        kind,
        job_template: template,
        overlap_policy: OverlapPolicy::Allow,
        missed_run_policy: MissedRunPolicy::RunOnceNow,
        next_run_at: None,
        labels: HashMap::new(),
    }
}

struct Harness {
    _root: tempfile::TempDir,
    pool: sqlx::SqlitePool,
    store: Arc<dyn JobStore>,
    schedules: Arc<dyn ScheduleStore>,
    submission: Arc<JobSubmissionService>,
    workspace_id: WorkspaceId,
}

impl Harness {
    async fn new() -> Self {
        let pool = common::pool::isolated_pool().await;
        let root = tempfile::tempdir().expect("temp workspace");
        let workspace_registry = WorkspaceRegistry::load(Arc::new(InMemoryWorkspaceStore::new()))
            .await
            .expect("workspace registry");
        let workspace = workspace_registry
            .get_or_register(root.path())
            .await
            .expect("register workspace");
        let services = WorkspaceServiceRegistry::new(
            workspace_registry,
            Arc::new(ProductionWorkspaceServicesFactory),
            WorkspaceServicePolicy::default(),
        );
        let store: Arc<dyn JobStore> = Arc::new(SqliteJobStore::new(pool.clone()));
        let scheduler = JobScheduler::new(
            store.clone(),
            services.clone(),
            ResolvedSchedulerConfig::default(),
            DaemonGeneration::new_unchecked("gen-m004"),
        );
        let submission = JobSubmissionService::new(
            store.clone(),
            scheduler,
            services,
            DaemonGeneration::new_unchecked("gen-m004"),
        );
        let schedules: Arc<dyn ScheduleStore> =
            Arc::new(SqliteScheduleStore::new(pool.clone(), store.clone()));
        Self {
            _root: root,
            pool,
            store,
            schedules,
            submission,
            workspace_id: workspace.id.clone(),
        }
    }

    fn materializer(&self, policy: Option<Arc<RemoteTargetPolicy>>) -> Arc<ScheduleMaterializer> {
        ScheduleMaterializer::new(self.submission.clone(), policy)
    }

    /// Count the durable job rows. The occurrence-idempotency assertions are
    /// about the number of *jobs*, not the number of claims.
    async fn job_count(&self) -> i64 {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM job")
            .fetch_one(&self.pool)
            .await
            .expect("job count")
    }

    async fn occurrence_count(&self) -> i64 {
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM schedule_occurrence")
            .fetch_one(&self.pool)
            .await
            .expect("occurrence count")
    }

    async fn job_target(&self, job_id: &str) -> Option<(String, Option<String>)> {
        sqlx::query_as::<_, (String, Option<String>)>(
            "SELECT target_kind, target_node_id FROM job WHERE id = ?",
        )
        .bind(job_id)
        .fetch_optional(&self.pool)
        .await
        .expect("job target")
    }
}

/// A materializer that always refuses, standing in for a policy refusal or a
/// submission failure.
struct Refusing;

#[async_trait]
impl OccurrenceMaterializer for Refusing {
    async fn materialize(
        &self,
        _schedule: &ScheduleRecord,
        _scheduled_for: DateTime<Utc>,
    ) -> Result<codegg_core::jobs::JobId, MaterializerError> {
        Err(MaterializerError::Refused {
            code: "test_refused".to_owned(),
        })
    }
}

/// The administrator approves exactly one node.
fn approving_policy(node_id: &str) -> Arc<RemoteTargetPolicy> {
    let mut nodes = HashMap::new();
    nodes.insert(
        node_id.to_owned(),
        EggworkNodeProfile {
            node_id: Some(node_id.to_owned()),
            endpoint: Some("https://node.invalid".to_owned()),
            ..Default::default()
        },
    );
    let config = EggworkConfig {
        nodes: Some(nodes),
        allowed_target_nodes: Some(vec![node_id.to_owned()]),
        ..Default::default()
    };
    Arc::new(RemoteTargetPolicy::from_config(&config))
}

#[tokio::test(flavor = "current_thread")]
async fn occurrence_materializes_one_durable_job_and_is_recorded() {
    let harness = Harness::new().await;
    let now = Utc::now();
    let mut template = template_with_kind(
        ScheduleKind::OneShot {
            run_at: now - chrono::Duration::seconds(1),
        },
        ExecutionTarget::default(),
    );
    template.workspace_id = harness.workspace_id.clone();
    let record = harness
        .schedules
        .create(template)
        .await
        .expect("schedule created");

    let materializer = harness.materializer(None);
    let claimed = harness
        .schedules
        .claim_due(now, materializer.as_ref())
        .await
        .expect("claim");
    assert_eq!(claimed.len(), 1, "one occurrence is claimed");
    let job_id = claimed[0]
        .job_id
        .clone()
        .expect("a materialized occurrence owns a job");

    assert_eq!(harness.job_count().await, 1, "exactly one durable job");
    assert_eq!(harness.occurrence_count().await, 1, "one occurrence row");

    let job = harness
        .store
        .get_job(&job_id)
        .await
        .expect("job read")
        .expect("job exists");
    assert_eq!(
        job.schedule_id.as_ref(),
        Some(&record.schedule_id),
        "the job records the schedule it came from"
    );
    assert_eq!(job.kind, JobKind::Build);

    // The occurrence's job id is a pure function of the occurrence, so it is
    // reproducible from the durable identity alone.
    assert_eq!(
        job_id,
        occurrence_job_id(&record.schedule_id, claimed[0].scheduled_for)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_crash_after_the_job_row_leaves_exactly_one_job() {
    let harness = Harness::new().await;
    let now = Utc::now();
    let mut template = template_with_kind(
        ScheduleKind::OneShot {
            run_at: now - chrono::Duration::seconds(1),
        },
        ExecutionTarget::default(),
    );
    template.workspace_id = harness.workspace_id.clone();
    let record = harness
        .schedules
        .create(template)
        .await
        .expect("schedule created");

    let materializer = harness.materializer(None);

    // Simulate the crash: the job row is written, then the daemon stops
    // before the occurrence row is written. The schedule is still due,
    // because its cursor never advanced.
    let occurrence = now - chrono::Duration::seconds(1);
    let job_id = materializer
        .materialize(&record, occurrence)
        .await
        .expect("materialize");
    assert_eq!(harness.job_count().await, 1);
    assert_eq!(
        harness.occurrence_count().await,
        0,
        "the occurrence was never recorded — this is the crash window"
    );

    // The next daemon replays the same occurrence.
    let claimed = harness
        .schedules
        .claim_due(now, materializer.as_ref())
        .await
        .expect("claim");
    assert_eq!(claimed.len(), 1);
    assert_eq!(
        claimed[0].job_id.as_ref(),
        Some(&job_id),
        "the replay converges on the job the crashed attempt already wrote"
    );
    assert_eq!(
        harness.job_count().await,
        1,
        "one occurrence never owns two jobs"
    );
    assert_eq!(harness.occurrence_count().await, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn a_refused_occurrence_is_not_recorded_and_stays_due() {
    let harness = Harness::new().await;
    let now = Utc::now();
    let mut template = template_with_kind(
        ScheduleKind::OneShot {
            run_at: now - chrono::Duration::seconds(1),
        },
        ExecutionTarget::default(),
    );
    template.workspace_id = harness.workspace_id.clone();
    let record = harness
        .schedules
        .create(template)
        .await
        .expect("schedule created");

    // The materializer refuses.
    let claimed = harness
        .schedules
        .claim_due(now, &Refusing)
        .await
        .expect("claim");
    assert_eq!(claimed.len(), 1);
    assert!(
        claimed[0].job_id.is_none(),
        "a refused occurrence never claims a job"
    );
    assert_eq!(
        claimed[0].failure_code.as_deref(),
        Some("test_refused"),
        "the refusal carries a stable, secret-free reason code"
    );
    assert_eq!(
        harness.occurrence_count().await,
        0,
        "a refused occurrence is not recorded, so the retry is not a no-op"
    );
    assert_eq!(harness.job_count().await, 0, "no job was created");

    // The schedule stayed due: its cursor did not advance past the
    // refused occurrence, and a one-shot is not completed.
    let after = harness
        .schedules
        .get(&record.schedule_id)
        .await
        .expect("schedule read")
        .expect("schedule exists");
    assert_eq!(
        after.last_occurrence_at, None,
        "the cursor did not advance past the refused occurrence"
    );
    assert_eq!(
        after.state,
        ScheduleState::Active,
        "a refused one-shot is not completed"
    );

    // The retry succeeds.
    let materializer = harness.materializer(None);
    let retried = harness
        .schedules
        .claim_due(now, materializer.as_ref())
        .await
        .expect("claim");
    assert_eq!(retried.len(), 1);
    assert!(
        retried[0].job_id.is_some(),
        "the retried occurrence materializes"
    );
    assert_eq!(harness.job_count().await, 1);
    assert_eq!(harness.occurrence_count().await, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn a_recurring_schedule_keeps_its_own_grid() {
    let harness = Harness::new().await;
    let anchor = Utc::now() - chrono::Duration::seconds(125);
    let mut template = template_with_kind(
        ScheduleKind::Interval {
            every: Duration::from_secs(60),
            anchor,
        },
        ExecutionTarget::default(),
    );
    template.workspace_id = harness.workspace_id.clone();
    // Make the schedule due now: `create` otherwise schedules the next
    // grid point after the current instant, which is in the future.
    template.next_run_at = Some(Utc::now() - chrono::Duration::seconds(1));
    let record = harness
        .schedules
        .create(template)
        .await
        .expect("schedule created");

    let materializer = harness.materializer(None);
    let claimed = harness
        .schedules
        .claim_due(Utc::now(), materializer.as_ref())
        .await
        .expect("claim");
    assert_eq!(claimed.len(), 1);

    // The occurrence is on the anchor's grid, not on the tick's clock.
    let occurrence = claimed[0].scheduled_for;
    assert_eq!(
        occurrence.timestamp() % 60,
        anchor.timestamp() % 60,
        "the occurrence lands on the schedule's own grid"
    );
    assert!(
        occurrence <= Utc::now(),
        "the occurrence is not in the future"
    );
    assert!(
        (Utc::now() - occurrence).num_seconds() < 60,
        "the occurrence is the last grid point at or before the tick"
    );

    let after = harness
        .schedules
        .get(&record.schedule_id)
        .await
        .expect("schedule read")
        .expect("schedule exists");
    assert_eq!(
        after.last_occurrence_at,
        Some(occurrence),
        "the cursor advances to the occurrence, not to the tick time"
    );
    assert!(
        after.next_run_at.is_some(),
        "the recurring schedule stays scheduled"
    );
    assert!(
        after.next_run_at.expect("next run") > Utc::now(),
        "the next horizon is in the future"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn an_unapproved_node_refuses_the_occurrence_and_creates_no_local_job() {
    let harness = Harness::new().await;
    let now = Utc::now();
    let mut template = template_with_kind(
        ScheduleKind::OneShot {
            run_at: now - chrono::Duration::seconds(1),
        },
        ExecutionTarget::EggworkNode {
            node_id: "builder".to_owned(),
        },
    );
    template.workspace_id = harness.workspace_id.clone();
    let _record = harness
        .schedules
        .create(template)
        .await
        .expect("schedule created");

    // The administrator approves no node at all.
    let materializer = harness.materializer(Some(Arc::new(RemoteTargetPolicy::from_config(
        &EggworkConfig::default(),
    ))));
    let claimed = harness
        .schedules
        .claim_due(now, materializer.as_ref())
        .await
        .expect("claim");
    assert_eq!(claimed.len(), 1);
    assert!(
        claimed[0].job_id.is_none(),
        "an unapproved node refuses the occurrence"
    );
    assert_eq!(
        harness.job_count().await,
        0,
        "a refused remote occurrence has no local fallback"
    );
    assert_eq!(harness.occurrence_count().await, 0);
}

#[tokio::test(flavor = "current_thread")]
async fn an_approved_node_materializes_the_remote_job() {
    let harness = Harness::new().await;
    let now = Utc::now();
    let mut template = template_with_kind(
        ScheduleKind::OneShot {
            run_at: now - chrono::Duration::seconds(1),
        },
        ExecutionTarget::EggworkNode {
            node_id: "builder".to_owned(),
        },
    );
    template.workspace_id = harness.workspace_id.clone();
    let _record = harness
        .schedules
        .create(template)
        .await
        .expect("schedule created");

    let materializer = harness.materializer(Some(approving_policy("builder")));
    let claimed = harness
        .schedules
        .claim_due(now, materializer.as_ref())
        .await
        .expect("claim");
    assert_eq!(claimed.len(), 1);
    let job_id = claimed[0]
        .job_id
        .clone()
        .expect("the occurrence materialized");

    assert_eq!(harness.job_count().await, 1);
    let (kind, node) = harness.job_target(job_id.as_str()).await.expect("job row");
    assert_eq!(kind, "eggwork_node");
    assert_eq!(node.as_deref(), Some("builder"));
}

#[tokio::test(flavor = "current_thread")]
async fn the_driver_claims_due_occurrences_and_stops_on_shutdown() {
    let harness = Harness::new().await;
    let now = Utc::now();
    let mut template = template_with_kind(
        ScheduleKind::OneShot {
            run_at: now - chrono::Duration::seconds(1),
        },
        ExecutionTarget::default(),
    );
    template.workspace_id = harness.workspace_id.clone();
    harness
        .schedules
        .create(template)
        .await
        .expect("schedule created");

    let materializer = harness.materializer(None);
    let driver = ScheduleDriver::new(harness.schedules.clone(), materializer);
    let claimed = driver.run_once().await;
    assert_eq!(claimed.len(), 1, "the driver claims the due occurrence");
    assert_eq!(harness.job_count().await, 1);

    // A second tick with nothing due claims nothing.
    assert!(driver.run_once().await.is_empty());

    driver.request_shutdown();
    assert!(driver.is_shutdown());
}

#[tokio::test(flavor = "current_thread")]
async fn a_paused_schedule_is_not_materialized() {
    let harness = Harness::new().await;
    let now = Utc::now();
    let mut template = template_with_kind(
        ScheduleKind::OneShot {
            run_at: now - chrono::Duration::seconds(1),
        },
        ExecutionTarget::default(),
    );
    template.workspace_id = harness.workspace_id.clone();
    let record = harness
        .schedules
        .create(template)
        .await
        .expect("schedule created");
    harness
        .schedules
        .set_state(&record.schedule_id, ScheduleState::Paused)
        .await
        .expect("pause");

    let materializer = harness.materializer(None);
    let claimed = harness
        .schedules
        .claim_due(now, materializer.as_ref())
        .await
        .expect("claim");
    assert!(claimed.is_empty(), "a paused schedule fires nothing");
    assert_eq!(harness.job_count().await, 0);
}

#[tokio::test(flavor = "current_thread")]
async fn the_job_store_treats_a_replayed_occurrence_as_the_same_job() {
    // The durable uniqueness constraint is the `job.id` primary key, so the
    // store itself refuses to create a second job for one occurrence.
    let harness = Harness::new().await;
    let now = Utc::now();
    let mut template = template_with_kind(
        ScheduleKind::OneShot {
            run_at: now - chrono::Duration::seconds(1),
        },
        ExecutionTarget::default(),
    );
    template.workspace_id = harness.workspace_id.clone();
    let record = harness
        .schedules
        .create(template)
        .await
        .expect("schedule created");

    let occurrence = now - chrono::Duration::seconds(1);
    let spec = record.job_template.new_job_for_occurrence(
        harness.workspace_id.clone(),
        record.schedule_id.clone(),
        record.session_id.clone(),
        occurrence,
    );
    let expected_id = occurrence_job_id(&record.schedule_id, occurrence);

    let first = harness
        .store
        .create_job(spec.clone())
        .await
        .expect("first create");
    assert_eq!(first.job_id, expected_id);

    let second = harness
        .store
        .create_job(spec)
        .await
        .expect("replayed create returns the same job");
    assert_eq!(second.job_id, expected_id);
    assert_eq!(harness.job_count().await, 1);
}

/// The daemon wiring, end to end.
///
/// The other tests exercise the store, the materializer and the driver
/// directly. This one proves the part only a daemon can prove: that
/// `CoreDaemon::with_deps` installs the materialization driver, that a
/// schedule created through the daemon's own request surface is claimed by
/// that driver, and that the job it produces is a real durable job carrying
/// the schedule it came from.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_daemon_fires_a_schedule_it_created() {
    use codegg::core::daemon::CoreDaemon;
    use codegg::core::new_request;
    use codegg::core::runtime_deps::CoreRuntimeDeps;
    use codegg::protocol::core::{CoreRequest, CoreResponse};
    use codegg_protocol::dto::ScheduleCreateDto;

    let pool = common::pool::isolated_pool().await;
    let daemon = CoreDaemon::with_deps(CoreRuntimeDeps::with_jobs(pool.clone(), None, None));

    let dir = tempfile::tempdir().expect("temp workspace");
    let workspace = daemon
        .workspaces
        .get_or_register(dir.path())
        .await
        .expect("register workspace");

    let spec = ScheduleCreateDto {
        workspace_id: workspace.id.as_str().to_owned(),
        session_id: None,
        kind: serde_json::to_value(ScheduleKind::OneShot {
            run_at: Utc::now() - chrono::Duration::seconds(1),
        })
        .expect("schedule kind"),
        job_template: serde_json::to_value(local_template()).expect("job template"),
        overlap_policy: "allow".to_owned(),
        missed_run_policy: serde_json::to_value(MissedRunPolicy::RunOnceNow)
            .expect("missed run policy"),
        labels: HashMap::new(),
    };

    let response = daemon
        .handle_request(new_request(
            "req-schedule-materialization".to_owned(),
            CoreRequest::ScheduleCreate { spec },
        ))
        .await
        .expect("schedule create request");
    let CoreResponse::ScheduleCreated { schedule_id } = response else {
        panic!("expected ScheduleCreated, got {response:?}");
    };

    // The daemon-owned driver claims the due occurrence on its own.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    let rows: Vec<(String, Option<String>)> = loop {
        let rows = sqlx::query_as::<_, (String, Option<String>)>(
            "SELECT id, schedule_id FROM job ORDER BY id",
        )
        .fetch_all(&pool)
        .await
        .expect("job rows");
        if !rows.is_empty() {
            break rows;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the daemon never fired the schedule it created"
        );
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    };

    assert_eq!(rows.len(), 1, "one occurrence owns one job");
    assert_eq!(
        rows[0].1.as_deref(),
        Some(schedule_id.as_str()),
        "the job records the schedule it came from"
    );
    assert!(
        rows[0].0.starts_with("occ-"),
        "the job id is the occurrence's derived identity, got {}",
        rows[0].0
    );
}
