//! M007: the daemon-owned scratch retention sweep.
//!
//! The daemon owns one disposable scratch tree per session. Scratch contents
//! are disposable by contract, but "disposable" is not "bounded": a session
//! that is never soft-deleted keeps its tree forever. This module bounds
//! that growth, and nothing else.
//!
//! ## The decision is durable, not filesystem-derived
//!
//! The sweep reads durable timestamps — the session's `time_updated` — and
//! never walks a tree to decide. There is no size traversal on the sweep
//! path, and the sweep never runs on the agent hot path.
//!
//! ## The lease
//!
//! A tree is reclaimable only when nothing is using it. The lease is derived
//! from durable state that already exists: an accepted turn admission
//! receipt, a non-terminal agent run, or a non-terminal job. There is no new
//! table, no lease column, and no migration.
//!
//! ## The shape
//!
//! Each candidate is decided inside one immediate transaction: take the
//! writer lock, re-read the session's activity and liveness inside that lock,
//! reclaim only if the recheck still says the tree is reclaimable, then
//! commit. This is the M004 pattern generalised from "deleted" to "idle by
//! age", and it uses the same shared `begin_immediate` helper: a deferred
//! transaction would have to upgrade a shared read lock at its first write,
//! which SQLite refuses without waiting.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use codegg_core::agent_run::{AgentRunStore, AgentRunStoreError};
use codegg_core::jobs::{JobState, JobStore, JobStoreError, JobStoreQuery};
use codegg_core::storage::immediate::begin_immediate;
use sqlx::SqlitePool;

/// What can go wrong while one sweep candidate is decided.
///
/// The reclaim validator's refusal is its **own** variant, because it is not
/// a failure: the tree is quarantined under a tombstone, the automatic path
/// will never accept it, and the operator disposition exists for it. Counting
/// a refusal as a failure would report a healthy sweep as broken.
#[derive(Debug)]
pub enum SweepError {
    /// The durable store could not be read or written.
    Store(sqlx::Error),
    /// The agent run store could not be read.
    Runs(AgentRunStoreError),
    /// The job store could not be read.
    Jobs(JobStoreError),
    /// The filesystem refused the reclaim.
    Scratch(crate::scratch::ScratchError),
}

impl From<sqlx::Error> for SweepError {
    fn from(error: sqlx::Error) -> Self {
        Self::Store(error)
    }
}

impl From<AgentRunStoreError> for SweepError {
    fn from(error: AgentRunStoreError) -> Self {
        Self::Runs(error)
    }
}

impl From<JobStoreError> for SweepError {
    fn from(error: JobStoreError) -> Self {
        Self::Jobs(error)
    }
}

impl From<crate::scratch::ScratchError> for SweepError {
    fn from(error: crate::scratch::ScratchError) -> Self {
        Self::Scratch(error)
    }
}

impl std::fmt::Display for SweepError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(error) => write!(formatter, "durable store: {error}"),
            Self::Runs(error) => write!(formatter, "agent run store: {error}"),
            Self::Jobs(error) => write!(formatter, "job store: {error}"),
            Self::Scratch(error) => write!(formatter, "scratch: {error}"),
        }
    }
}

impl SweepError {
    /// Whether this is the reclaim validator refusing the tree.
    ///
    /// A refusal is not a failure: the tree is quarantined and the operator
    /// disposition is how a human disposes of it.
    fn is_refusal(&self) -> bool {
        matches!(
            self,
            Self::Scratch(crate::scratch::ScratchError::Unreclaimable { .. })
        )
    }
}

/// How many trees one sweep pass will examine.
pub const SWEEP_MAX_TREES: usize = 512;

/// How many trees one sweep pass will reclaim.
///
/// A separate bound from the candidate count: a pass that finds a thousand
/// reclaimable trees must not spend the whole pass removing them.
pub const SWEEP_MAX_RECLAIMS: usize = 256;

/// The wall-clock budget for one sweep pass, in milliseconds.
///
/// The sweep holds SQLite's writer lock while it reclaims, so a pass that
/// ran unbounded would hold that lock against every other writer.
pub const SWEEP_WALL_CLOCK_MS: u64 = 30_000;

/// How long the sweep waits before retrying a pass that failed.
///
/// A pass that could not reach the durable store — a transient lock, a
/// database still migrating — is not a reason to wait the whole interval.
/// Without this, one unlucky pass would defer the next sweep by six hours.
pub const SWEEP_RETRY_SECONDS: u64 = 15;

/// How many times in a row the sweep retries a failed pass before it goes
/// back to waiting for the interval.
///
/// A store that is broken stays broken, and a broken store must not turn the
/// sweep into a busy loop.
pub const SWEEP_MAX_CONSECUTIVE_RETRIES: u32 = 3;

/// The bounds one sweep pass works within.
///
/// A pass that hits a bound stops and records that it stopped: a truncated
/// pass is never presented as a complete sweep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SweepBudget {
    /// The maximum number of candidate trees the pass examines.
    pub max_candidates: usize,
    /// The maximum number of trees the pass reclaims.
    pub max_reclaims: usize,
    /// The pass's wall-clock budget, in milliseconds.
    pub wall_clock_ms: u64,
}

impl Default for SweepBudget {
    fn default() -> Self {
        Self {
            max_candidates: SWEEP_MAX_TREES,
            max_reclaims: SWEEP_MAX_RECLAIMS,
            wall_clock_ms: SWEEP_WALL_CLOCK_MS,
        }
    }
}

/// Which bound stopped a sweep pass.
///
/// A truncated pass is never presented as a complete sweep: the bound that
/// stopped it is named, so the report says what it did *not* look at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SweepStop {
    /// The pass examined its maximum number of candidates.
    Candidates,
    /// The pass reclaimed its maximum number of trees.
    Reclaims,
    /// The pass exceeded its wall-clock budget.
    WallClock,
}

impl SweepStop {
    /// A stable, bounded label for the sweep's log line.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Candidates => "candidate_budget",
            Self::Reclaims => "reclaim_budget",
            Self::WallClock => "wall_clock_budget",
        }
    }
}

/// The daemon's scratch retention policy.
///
/// The defaults are **specified**, not measured: they are derived from the
/// disposal contract. M006's census ran as this milestone's WP0 and was
/// consulted — it did not contradict the default, so the default was not
/// adjusted — but the measured population was synthetic, so the number itself
/// is still contract-derived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScratchRetentionPolicy {
    /// Whether the sweep runs at all. Off means the sweep never fires; the
    /// existing event-driven reclaimers are unaffected either way.
    pub enabled: bool,
    /// Age, in days, past which an idle session's tree is reclaimable.
    pub retention_days: u32,
    /// How often the sweep runs, in minutes.
    pub sweep_interval_minutes: u32,
    /// How long a tree whose digest matches no durable session is left alone
    /// before it is reclaimed as an orphan, in hours.
    pub orphan_grace_hours: u32,
}

impl Default for ScratchRetentionPolicy {
    fn default() -> Self {
        Self {
            enabled: true,
            retention_days: codegg_config::SCRATCH_RETENTION_DAYS_DEFAULT,
            sweep_interval_minutes: codegg_config::SCRATCH_SWEEP_INTERVAL_MINUTES_DEFAULT,
            orphan_grace_hours: codegg_config::SCRATCH_ORPHAN_GRACE_HOURS_DEFAULT,
        }
    }
}

impl From<&codegg_config::ScratchConfig> for ScratchRetentionPolicy {
    fn from(config: &codegg_config::ScratchConfig) -> Self {
        let effective = config.effective();
        Self {
            enabled: effective.enabled,
            retention_days: effective.retention_days,
            sweep_interval_minutes: effective.sweep_interval_minutes,
            orphan_grace_hours: effective.orphan_grace_hours,
        }
    }
}

/// What one sweep pass did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScratchSweepReport {
    /// The candidates the sweep examined.
    pub candidates: usize,
    /// Trees reclaimed because their session had been idle past the window.
    pub reclaimed_idle: usize,
    /// Trees reclaimed because their session had been soft-deleted.
    ///
    /// The event-driven reclaimer owns that case at delete time. This is the
    /// sweep's retry for the tree it had to refuse because work was still
    /// running when the session was deleted.
    pub reclaimed_deleted: usize,
    /// Trees reclaimed because no durable session matched them.
    pub reclaimed_orphan: usize,
    /// Candidates refused because the lease was held.
    pub skipped_leased: usize,
    /// Candidates refused because the session was not idle yet.
    pub skipped_active: usize,
    /// Candidates whose tree was already gone when the sweep reached it.
    ///
    /// A concurrent reclaim removed it. That is not a failure, and it does
    /// not fail the pass.
    pub vanished: usize,
    /// Candidates the sweep could not reclaim.
    pub failed: usize,
    /// Candidates the reclaim validator refused — a tree holding a planted
    /// symlink, for instance. That is the residual the operator disposition
    /// exists for, not a sweep failure.
    pub refused: usize,
    /// Which bound stopped the pass, when one did.
    ///
    /// `None` means the pass examined everything it set out to examine.
    pub stopped: Option<SweepStop>,
}

impl ScratchSweepReport {
    /// The number of trees the pass removed.
    pub fn reclaimed_total(&self) -> usize {
        self.reclaimed_idle + self.reclaimed_deleted + self.reclaimed_orphan
    }
}

/// The durable facts one sweep pass reads.
#[derive(Debug, Clone)]
struct SessionRow {
    id: String,
    time_updated: i64,
    deleted: bool,
}

/// What the per-candidate transaction decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IdleOutcome {
    /// The tree was reclaimed.
    Reclaimed,
    /// The tree was reclaimed, and its session had been soft-deleted.
    ReclaimedDeleted,
    /// The session is gone or no longer live.
    Gone,
    /// The session was active again by the time the lock was held.
    NotIdle,
    /// Something is still using the session's tree.
    Leased,
    /// The tree was already gone when the sweep reached it.
    Vanished,
}

/// Run one sweep pass with the production bounds.
///
/// The pass is bounded and idempotent: a tree that was reclaimed is simply
/// absent the next time, and a tree that was refused is examined again.
pub async fn sweep_scratch_once(
    pool: &SqlitePool,
    runs: &dyn AgentRunStore,
    jobs: &dyn JobStore,
    policy: ScratchRetentionPolicy,
    now_ms: i64,
) -> ScratchSweepReport {
    sweep_scratch_once_with_budget(pool, runs, jobs, policy, now_ms, SweepBudget::default()).await
}

/// Run one sweep pass with an explicit bound.
///
/// The bound is a parameter so the bound itself is testable: a pass that hit
/// it must say so, and a test has to be able to reach it without planting
/// hundreds of trees.
pub async fn sweep_scratch_once_with_budget(
    pool: &SqlitePool,
    runs: &dyn AgentRunStore,
    jobs: &dyn JobStore,
    policy: ScratchRetentionPolicy,
    now_ms: i64,
    budget: SweepBudget,
) -> ScratchSweepReport {
    let mut report = ScratchSweepReport::default();
    if !policy.enabled {
        return report;
    }
    let retention_ms = i64::from(policy.retention_days) * 24 * 60 * 60 * 1000;
    let grace_ms = i64::from(policy.orphan_grace_hours) * 60 * 60 * 1000;
    let started = std::time::Instant::now();

    let sessions = match load_sessions(pool).await {
        Ok(sessions) => sessions,
        Err(error) => {
            tracing::warn!(%error, "scratch sweep could not read the session set");
            report.failed += 1;
            return report;
        }
    };

    // The trees the durable sessions own. The digest is one-way, so this map
    // is built forward — the durable session ids are hashed — rather than by
    // inverting a tree's name.
    let owned: HashSet<String> = sessions
        .iter()
        .map(|session| crate::scratch::session_scratch_digest_hex(&session.id))
        .collect();

    for session in sessions.iter() {
        // A soft-deleted session's tree is dead weight: the event-driven
        // reclaimer tries it at delete time, and the sweep is the retry for
        // the tree it had to refuse because work was still running. It does
        // not wait out the retention window — the session is gone.
        //
        // A live session's tree is protected by the window: it is idle, not
        // abandoned, and the operator may still return to it.
        if !session.deleted && now_ms.saturating_sub(session.time_updated) < retention_ms {
            report.skipped_active += 1;
            continue;
        }
        match check_sweep_bounds(&mut report, started, budget) {
            Ok(()) => {}
            Err(stop) => {
                report.stopped = Some(stop);
                break;
            }
        }
        report.candidates += 1;
        match reclaim_session_scratch_for_sweep(pool, runs, jobs, &session.id, retention_ms, now_ms)
            .await
        {
            Ok(IdleOutcome::Reclaimed) => report.reclaimed_idle += 1,
            Ok(IdleOutcome::ReclaimedDeleted) => report.reclaimed_deleted += 1,
            Ok(IdleOutcome::Leased) => report.skipped_leased += 1,
            Ok(IdleOutcome::NotIdle) => report.skipped_active += 1,
            Ok(IdleOutcome::Vanished) => report.vanished += 1,
            Ok(IdleOutcome::Gone) => {}
            Err(error) if error.is_refusal() => {
                // The automatic path refuses this tree. The operator
                // disposition is how a human disposes of it.
                report.refused += 1;
            }
            Err(error) => {
                report.failed += 1;
                tracing::warn!(
                    session_id = %session.id,
                    %error,
                    "scratch sweep could not reclaim a session's tree"
                );
            }
        }
    }

    // Orphans: a tree whose digest matches no durable session. The grace
    // period absorbs the window in which a tree exists before its durable
    // session row is visible to the sweeper.
    let trees = match crate::scratch::enumerate_session_scratch_trees(true, SWEEP_MAX_TREES) {
        Ok(trees) => trees,
        Err(error) => {
            tracing::warn!(%error, "scratch sweep could not enumerate the sessions root");
            report.failed += 1;
            return report;
        }
    };
    for tree in trees {
        if owned.contains(&tree.digest) {
            continue;
        }
        let Some(modified_ms) = tree.modified_ms else {
            // A tree whose mtime cannot be read has no creation time to
            // compare the grace period against, so it is left alone.
            continue;
        };
        if now_ms.saturating_sub(modified_ms) < grace_ms {
            continue;
        }
        match check_sweep_bounds(&mut report, started, budget) {
            Ok(()) => {}
            Err(stop) => {
                report.stopped = Some(stop);
                break;
            }
        }
        report.candidates += 1;
        match crate::scratch::reclaim_session_scratch_by_digest(&tree.digest) {
            Ok(true) => report.reclaimed_orphan += 1,
            Ok(false) => report.vanished += 1,
            Err(crate::scratch::ScratchError::Unreclaimable { .. }) => {
                // The automatic path refuses this tree — a planted symlink,
                // say. That is the residual the operator disposition exists
                // for, not a sweep failure.
                report.refused += 1;
            }
            Err(error) => {
                report.failed += 1;
                tracing::warn!(
                    digest = %tree.digest,
                    %error,
                    "scratch sweep could not reclaim an orphaned tree"
                );
            }
        }
    }

    report
}

/// Check the pass's bounds before it takes on another candidate.
///
/// `Err` carries the bound that stopped the pass, which the pass records:
/// a truncated pass is never presented as a complete sweep.
fn check_sweep_bounds(
    report: &mut ScratchSweepReport,
    started: std::time::Instant,
    budget: SweepBudget,
) -> Result<(), SweepStop> {
    if report.candidates >= budget.max_candidates {
        return Err(SweepStop::Candidates);
    }
    if report.reclaimed_total() >= budget.max_reclaims {
        return Err(SweepStop::Reclaims);
    }
    if started.elapsed() >= std::time::Duration::from_millis(budget.wall_clock_ms) {
        return Err(SweepStop::WallClock);
    }
    Ok(())
}

/// Read the durable session set.
///
/// Soft-deleted sessions are included: the event-driven reclaimer owns their
/// trees, so the sweep must not mistake them for orphans.
async fn load_sessions(pool: &SqlitePool) -> Result<Vec<SessionRow>, sqlx::Error> {
    let rows: Vec<(String, i64, Option<i64>)> =
        sqlx::query_as("SELECT id, time_updated, time_deleted FROM session")
            .fetch_all(pool)
            .await?;
    Ok(rows
        .into_iter()
        .map(|(id, time_updated, time_deleted)| SessionRow {
            id,
            time_updated,
            deleted: time_deleted.is_some(),
        })
        .collect())
}

/// Decide one candidate and reclaim it, inside one immediate transaction.
///
/// The writer lock is taken **first**, before any read. A deferred
/// transaction would take a shared read lock at its first `SELECT` and then
/// have to upgrade it at its first write, which SQLite refuses without
/// waiting — the C002 deadlock shape.
async fn reclaim_session_scratch_for_sweep(
    pool: &SqlitePool,
    runs: &dyn AgentRunStore,
    jobs: &dyn JobStore,
    session_id: &str,
    retention_ms: i64,
    now_ms: i64,
) -> Result<IdleOutcome, SweepError> {
    let mut tx = begin_immediate(pool).await?;

    // Claim the session under the writer lock. The write is a no-op — it
    // claims the row and confirms it still exists — so a concurrent delete or
    // restore either wins first (zero rows) or waits until this transaction
    // has committed.
    let claimed = sqlx::query("UPDATE session SET time_updated = time_updated WHERE id = ?")
        .bind(session_id)
        .execute(&mut **tx)
        .await?;
    if claimed.rows_affected() == 0 {
        drop(tx);
        return Ok(IdleOutcome::Gone);
    }

    // Re-read the session inside the lock: a turn that committed since the
    // sweep read the session set has made it active again, and a session
    // restored since then is live rather than deleted.
    let row: Option<(i64, Option<i64>)> =
        sqlx::query_as("SELECT time_updated, time_deleted FROM session WHERE id = ?")
            .bind(session_id)
            .fetch_optional(&mut **tx)
            .await?;
    let Some((time_updated, time_deleted)) = row else {
        drop(tx);
        return Ok(IdleOutcome::Gone);
    };
    let deleted = time_deleted.is_some();
    // A live session inside the retention window keeps its tree. A soft-
    // deleted session does not wait out the window: the session is gone.
    if !deleted && now_ms.saturating_sub(time_updated) < retention_ms {
        drop(tx);
        return Ok(IdleOutcome::NotIdle);
    }

    // The lease. An accepted turn receipt means a turn is admitted right now;
    // a non-terminal run or job means work is still executing against this
    // session's tree.
    if !session_tree_is_reclaimable(&mut tx, runs, jobs, session_id).await? {
        drop(tx);
        return Ok(IdleOutcome::Leased);
    }

    // The reclaim runs inside the transaction: the writer lock is what makes
    // the decision above safe to act on.
    let removed = crate::scratch::reclaim_session_scratch_dir(session_id)?;
    tx.commit_with(SweepError::Store).await?;
    Ok(if !removed {
        IdleOutcome::Vanished
    } else if deleted {
        IdleOutcome::ReclaimedDeleted
    } else {
        IdleOutcome::Reclaimed
    })
}

/// Whether the session's scratch tree is free of work.
///
/// The caller must already hold the writer lock, so a turn cannot be
/// admitted between this read and the reclaim that follows it: the
/// admission path needs the same lock.
async fn session_tree_is_reclaimable(
    conn: &mut sqlx::SqliteConnection,
    runs: &dyn AgentRunStore,
    jobs: &dyn JobStore,
    session_id: &str,
) -> Result<bool, SweepError> {
    if session_has_accepted_turn(conn, session_id).await? {
        return Ok(false);
    }
    let run_records = runs.list_by_session(session_id).await?;
    if run_records.iter().any(|run| !run.status.is_terminal()) {
        return Ok(false);
    }
    let job_records = jobs
        .list_job_records(JobStoreQuery {
            session_id: Some(session_id.to_string()),
            states: vec![
                JobState::Scheduled,
                JobState::Queued,
                JobState::Running,
                JobState::Interrupted,
                JobState::Blocked,
            ],
            limit: Some(1),
            ..Default::default()
        })
        .await?;
    Ok(job_records.is_empty())
}

/// Find the durable session that owns a tree's digest, if any.
///
/// The digest is one-way, so the mapping is computed forward: every durable
/// session id is hashed and the result compared. A tree that matches no
/// session is an orphan, and an orphan has no lease.
async fn session_owning_digest(
    pool: &SqlitePool,
    digest_hex: &str,
) -> Result<Option<String>, SweepError> {
    let ids: Vec<String> = sqlx::query_scalar("SELECT id FROM session")
        .fetch_all(pool)
        .await?;
    Ok(ids
        .into_iter()
        .find(|id| crate::scratch::session_scratch_digest_hex(id) == digest_hex))
}

/// Whether a turn is admitted for this session right now.
///
/// The receipt is the durable record of an accepted turn. It is read inside
/// the sweep's transaction, so a turn cannot be admitted between the read and
/// the reclaim: the admission path needs the same writer lock.
async fn session_has_accepted_turn(
    conn: &mut sqlx::SqliteConnection,
    session_id: &str,
) -> Result<bool, sqlx::Error> {
    let accepted: i64 = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM turn_admission_receipt \
         WHERE session_id = ? AND status = 'accepted')",
    )
    .bind(session_id)
    .fetch_one(conn)
    .await?;
    Ok(accepted == 1)
}

/// What one operator reclaim attempt decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorReclaimOutcome {
    /// The opaque digest that names the tree.
    pub digest: String,
    /// The number of entries the tree held.
    pub entry_count: u64,
    /// The total size of the tree's regular files.
    pub byte_count: u64,
    /// Whether the tree exists at all.
    pub present: bool,
    /// Whether the tree is parked under a reclaim tombstone, which means the
    /// automatic path already refused it at least once.
    pub quarantined: bool,
    /// The automatic reclaim path's verdict for the tree's entries.
    pub reclaimable: bool,
    /// Which rule the automatic path refused on, when it did.
    pub refusal: Option<crate::scratch::ReclaimRefusal>,
    /// Whether the tree was removed.
    pub removed: bool,
    /// Whether the operator action refused because the tree is still in use.
    pub leased: bool,
}

/// Dispose of one scratch tree, addressed by its opaque digest, on the
/// operator's explicit instruction.
///
/// This is the disposition for a tree the automatic path can never accept.
/// The lease is honoured exactly as the sweep honours it — a tree whose
/// session still has an accepted turn, a non-terminal run, or a non-terminal
/// job is not removed — but the **entry-content** refusals are relaxed: a
/// planted symlink or a non-regular entry is removed by name. The ownership
/// check, the containment check, and the owner-record check are unchanged, so
/// this can only ever remove a tree the daemon itself created and still owns.
///
/// A dry run (`apply` false) measures and reports without removing anything.
pub async fn reclaim_session_scratch_for_operator(
    pool: &SqlitePool,
    runs: &dyn AgentRunStore,
    jobs: &dyn JobStore,
    digest_hex: &str,
    apply: bool,
) -> Result<OperatorReclaimOutcome, SweepError> {
    // The digest is a handle, not a path. Reject anything that is not one
    // before it reaches the filesystem.
    crate::scratch::decode_scratch_digest(digest_hex)?;

    let usage = crate::scratch::preview_session_scratch(digest_hex)?;
    let mut outcome = OperatorReclaimOutcome {
        digest: digest_hex.to_string(),
        entry_count: usage.file_count + usage.directory_count,
        byte_count: usage.total_bytes,
        present: !usage.vanished,
        quarantined: usage.quarantined,
        // Fail-closed: only an established "yes" is reclaimable. A tree the
        // preview could only partly read carries an unknown verdict, and an
        // unknown verdict is not a permission to reclaim.
        reclaimable: usage.reclaimable == Some(true),
        refusal: usage.refusal,
        removed: false,
        leased: false,
    };
    if usage.vanished {
        return Ok(outcome);
    }

    let owner = session_owning_digest(pool, digest_hex).await?;

    // The lease is decided under the writer lock, and the reclaim happens
    // inside it, so a turn cannot be admitted between the two.
    let mut tx = begin_immediate(pool).await?;
    if let Some(session_id) = owner.as_deref() {
        if !session_tree_is_reclaimable(&mut tx, runs, jobs, session_id).await? {
            drop(tx);
            outcome.leased = true;
            return Ok(outcome);
        }
    }
    if !apply {
        drop(tx);
        return Ok(outcome);
    }
    let removed = crate::scratch::reclaim_session_scratch_for_operator(digest_hex)?;
    tx.commit_with(SweepError::Store).await?;
    outcome.removed = removed;
    Ok(outcome)
}

/// Spawn the retention sweep.
///
/// The sweep is a plain background task on its own interval. It owns no
/// authority beyond the daemon's own scratch tree, and a daemon shutdown
/// simply ends it.
pub fn spawn_scratch_sweep(
    pool: SqlitePool,
    runs: Arc<dyn AgentRunStore>,
    jobs: Arc<dyn JobStore>,
    policy: ScratchRetentionPolicy,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        if !policy.enabled {
            return;
        }
        let interval = Duration::from_secs(u64::from(policy.sweep_interval_minutes) * 60);
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut consecutive_failures: u32 = 0;
        loop {
            ticker.tick().await;
            let report = sweep_scratch_once(
                &pool,
                runs.as_ref(),
                jobs.as_ref(),
                policy,
                chrono::Utc::now().timestamp_millis(),
            )
            .await;
            if report.reclaimed_total() > 0
                || report.refused > 0
                || report.failed > 0
                || report.stopped.is_some()
            {
                tracing::info!(
                    candidates = report.candidates,
                    reclaimed_idle = report.reclaimed_idle,
                    reclaimed_deleted = report.reclaimed_deleted,
                    reclaimed_orphan = report.reclaimed_orphan,
                    skipped_leased = report.skipped_leased,
                    skipped_active = report.skipped_active,
                    vanished = report.vanished,
                    refused = report.refused,
                    failed = report.failed,
                    stopped = report.stopped.map(SweepStop::as_str),
                    "scratch retention sweep completed"
                );
            }
            // A pass that could not reach the durable store is retried
            // shortly, not at the next interval. The store's own busy timeout
            // is 30 seconds, so a pass that lost a race with another writer
            // would otherwise wait out the whole interval to try again. The
            // retry is bounded: a broken store must not become a busy loop.
            if report.failed > 0 {
                consecutive_failures = consecutive_failures.saturating_add(1);
                if consecutive_failures <= SWEEP_MAX_CONSECUTIVE_RETRIES {
                    tokio::time::sleep(Duration::from_secs(SWEEP_RETRY_SECONDS)).await;
                    continue;
                }
            } else {
                consecutive_failures = 0;
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use codegg_core::agent_run::{
        AgentRunBudget, AgentRunStore, InMemoryAgentRunStore, NewAgentRun, NewAgentTask,
    };
    use codegg_core::identity::{AgentRunId, AgentTaskId, ProjectId};
    use codegg_core::jobs::{
        IdempotencyClass, InMemoryJobStore, JobKind, JobPriority, JobSource, NewJob,
        ResourceRequest, RetryPolicy,
    };
    use codegg_core::session::{CreateSession, SessionStore};
    use codegg_core::workspace::WorkspaceId;
    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
    use std::str::FromStr;

    const DAY_MS: i64 = 24 * 60 * 60 * 1000;

    /// A temporary scratch root for one test.
    ///
    /// The sweep **reclaims** trees, so a test that ran against the
    /// developer's real scratch root would delete their own scratch. The
    /// redirect is thread-local, so concurrent tests never see each other's
    /// tree.
    fn scratch_root(label: &str) -> crate::scratch::TestScratchRoot {
        let base = std::fs::canonicalize(std::env::temp_dir())
            .unwrap_or_else(|_| std::env::temp_dir().to_path_buf());
        crate::scratch::TestScratchRoot::install(base.join(format!(
            "cg-sweep-{label}-{}",
            &uuid::Uuid::new_v4().simple().to_string()[..6]
        )))
    }

    async fn sweep_pool() -> SqlitePool {
        let url = format!(
            "file:scratch-sweep-{}?mode=memory&cache=shared",
            uuid::Uuid::new_v4().simple()
        );
        let options = SqliteConnectOptions::from_str(&url)
            .expect("valid sqlite url")
            .create_if_missing(true)
            .foreign_keys(true)
            .busy_timeout(std::time::Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await
            .expect("sqlite pool");
        codegg_core::session::schema::migrate(&pool)
            .await
            .expect("session schema");
        pool
    }

    async fn create_session(pool: &SqlitePool) -> String {
        let store = SessionStore::new(pool.clone());
        let session = store
            .create(CreateSession {
                project_id: format!("scratch-sweep-project-{}", uuid::Uuid::new_v4()),
                directory: std::env::temp_dir().to_string_lossy().into_owned(),
                title: Some("scratch sweep test".into()),
                ..Default::default()
            })
            .await
            .expect("create session");
        session.id
    }

    /// Backdate a session's last activity by `age_ms`.
    async fn set_activity(pool: &SqlitePool, session_id: &str, age_ms: i64) {
        let now = chrono::Utc::now().timestamp_millis();
        sqlx::query("UPDATE session SET time_updated = ? WHERE id = ?")
            .bind(now - age_ms)
            .bind(session_id)
            .execute(pool)
            .await
            .expect("backdate the session");
    }

    /// Mark a session as soft-deleted, the way the M004 event-driven
    /// reclaimer sees it.
    async fn soft_delete(pool: &SqlitePool, session_id: &str) {
        let now = chrono::Utc::now().timestamp_millis();
        sqlx::query("UPDATE session SET time_deleted = ? WHERE id = ?")
            .bind(now)
            .bind(session_id)
            .execute(pool)
            .await
            .expect("soft-delete the session");
    }

    /// Restore a soft-deleted session, the way the restore path does.
    async fn restore(pool: &SqlitePool, session_id: &str) {
        sqlx::query("UPDATE session SET time_deleted = NULL WHERE id = ?")
            .bind(session_id)
            .execute(pool)
            .await
            .expect("restore the session");
    }

    fn policy() -> ScratchRetentionPolicy {
        ScratchRetentionPolicy {
            enabled: true,
            retention_days: 14,
            sweep_interval_minutes: 360,
            orphan_grace_hours: 24,
        }
    }

    fn active_test_job(session_id: &str) -> NewJob {
        NewJob {
            workspace_id: WorkspaceId::new_unchecked("scratch-sweep-workspace"),
            session_id: Some(session_id.to_string()),
            turn_id: None,
            kind: JobKind::Test,
            source: JobSource::Interactive,
            priority: JobPriority::Interactive,
            payload: codegg_core::jobs::JobPayload::Test {
                command: "cargo test".into(),
                argv: vec!["cargo".into(), "test".into()],
                cwd: None,
                scope: None,
                parent_run_id: None,
                sandbox_profile: None,
                network_intent: codegg_core::approval::NetworkIntent::LegacyUnrestricted,
            },
            resource_request: ResourceRequest::for_kind(JobKind::Test),
            timeout: None,
            retry_policy: RetryPolicy::no_retry(),
            idempotency: IdempotencyClass::SafeRepeat,
            not_before: None,
            deadline: None,
            schedule_id: None,
            depends_on: Vec::new(),
            parent_job_id: None,
            parent_attempt_id: None,
            parent_call_id: None,
            parent_program_id: None,
            parent_instruction_sequence: None,
            relation_kind: None,
            target: Default::default(),
        }
    }

    /// Accept a turn for a session, which leases its scratch tree.
    async fn accept_turn(pool: &SqlitePool, session_id: &str) {
        let now = chrono::Utc::now().timestamp_millis();
        sqlx::query(
            "INSERT INTO turn_admission_receipt (submission_id, session_id, principal_id, \
             turn_id, request_sha256, status, accepted_at_ms, updated_at_ms) \
             VALUES (?, ?, ?, ?, ?, 'accepted', ?, ?)",
        )
        .bind(format!("submission-{session_id}"))
        .bind(session_id)
        .bind("local-owner")
        .bind(format!("turn-{session_id}"))
        .bind("0".repeat(64))
        .bind(now)
        .bind(now)
        .execute(pool)
        .await
        .expect("accept a turn");
    }

    /// Start a delegated run for a session, which leases its scratch tree.
    async fn start_run(runs: &InMemoryAgentRunStore, session_id: &str) {
        let task = NewAgentTask {
            task_id: AgentTaskId::new(),
            parent_task_id: None,
            originating_session_id: session_id.to_string(),
            originating_turn_id: Some(format!("turn-{session_id}")),
            project_id: ProjectId::new(),
            repository_id: None,
            workspace_id: WorkspaceId::new_unchecked("scratch-sweep-workspace"),
            requested_agent: "general".into(),
            delegation_key: format!("scratch-sweep-{session_id}"),
            request_fingerprint: format!("scratch-sweep-{session_id}"),
            description: "a leased delegated run".into(),
        };
        let run = NewAgentRun {
            run_id: AgentRunId::new(),
            parent_run_id: None,
            depth: 1,
            workspace_id: WorkspaceId::new_unchecked("scratch-sweep-workspace"),
            agent_name: "general".into(),
            agent_digest: Some("digest".into()),
            provider: "openai".into(),
            model: "model".into(),
            authority_digest: "authority".into(),
            budget: AgentRunBudget {
                max_tool_calls: Some(10),
                ..Default::default()
            },
            shared_scratch: Vec::new(),
        };
        runs.create_or_get(task, run)
            .await
            .expect("start a delegated run");
    }

    /// The sweep reclaims a tree whose session has been idle past the window.
    #[tokio::test]
    async fn the_sweep_reclaims_an_idle_session_tree() {
        let _root = scratch_root("the-sweep-reclaims-an-id");
        let pool = sweep_pool().await;
        let runs = InMemoryAgentRunStore::new();
        let jobs = InMemoryJobStore::new();
        let session_id = create_session(&pool).await;
        let root = crate::scratch::session_scratch_dir(&session_id).expect("create the tree");
        std::fs::write(root.join("artifact.bin"), b"hello").expect("write an artifact");
        set_activity(&pool, &session_id, 30 * DAY_MS).await;

        let report = sweep_scratch_once(
            &pool,
            &runs,
            &jobs,
            policy(),
            chrono::Utc::now().timestamp_millis(),
        )
        .await;

        assert_eq!(report.reclaimed_idle, 1, "report: {report:?}");
        assert!(!root.exists(), "the idle tree is reclaimed");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A session that was active recently is left alone, whatever the tree
    /// holds.
    #[tokio::test]
    async fn the_sweep_leaves_an_active_session_tree() {
        let _root = scratch_root("the-sweep-leaves-an-acti");
        let pool = sweep_pool().await;
        let runs = InMemoryAgentRunStore::new();
        let jobs = InMemoryJobStore::new();
        let session_id = create_session(&pool).await;
        let root = crate::scratch::session_scratch_dir(&session_id).expect("create the tree");
        set_activity(&pool, &session_id, DAY_MS).await;

        let report = sweep_scratch_once(
            &pool,
            &runs,
            &jobs,
            policy(),
            chrono::Utc::now().timestamp_millis(),
        )
        .await;

        assert_eq!(report.reclaimed_idle, 0, "report: {report:?}");
        assert_eq!(report.skipped_active, 1, "report: {report:?}");
        assert!(root.exists(), "an active session keeps its tree");
        let _ = crate::scratch::reclaim_session_scratch_dir(&session_id);
    }

    /// An accepted turn leases the session's tree, even when the session has
    /// been idle long enough.
    #[tokio::test]
    async fn the_sweep_refuses_a_leased_session_tree() {
        let _root = scratch_root("the-sweep-refuses-a-leas");
        let pool = sweep_pool().await;
        let runs = InMemoryAgentRunStore::new();
        let jobs = InMemoryJobStore::new();
        let session_id = create_session(&pool).await;
        let root = crate::scratch::session_scratch_dir(&session_id).expect("create the tree");
        set_activity(&pool, &session_id, 30 * DAY_MS).await;
        accept_turn(&pool, &session_id).await;

        let report = sweep_scratch_once(
            &pool,
            &runs,
            &jobs,
            policy(),
            chrono::Utc::now().timestamp_millis(),
        )
        .await;

        assert_eq!(report.reclaimed_idle, 0, "report: {report:?}");
        assert_eq!(report.skipped_leased, 1, "report: {report:?}");
        assert!(root.exists(), "a leased tree is not reclaimed");
        let _ = crate::scratch::reclaim_session_scratch_dir(&session_id);
    }

    /// A non-terminal delegated run leases the session's tree too.
    #[tokio::test]
    async fn the_sweep_refuses_a_session_with_a_non_terminal_run() {
        let _root = scratch_root("the-sweep-refuses-a-sess");
        let pool = sweep_pool().await;
        let runs = InMemoryAgentRunStore::new();
        let jobs = InMemoryJobStore::new();
        let session_id = create_session(&pool).await;
        let root = crate::scratch::session_scratch_dir(&session_id).expect("create the tree");
        set_activity(&pool, &session_id, 30 * DAY_MS).await;
        start_run(&runs, &session_id).await;

        let report = sweep_scratch_once(
            &pool,
            &runs,
            &jobs,
            policy(),
            chrono::Utc::now().timestamp_millis(),
        )
        .await;

        assert_eq!(report.reclaimed_idle, 0, "report: {report:?}");
        assert_eq!(report.skipped_leased, 1, "report: {report:?}");
        assert!(root.exists(), "a tree with a live run is not reclaimed");
        let _ = crate::scratch::reclaim_session_scratch_dir(&session_id);
    }

    /// A non-terminal job leases the session's tree too.
    #[tokio::test]
    async fn the_sweep_refuses_a_session_with_a_non_terminal_job() {
        let _root = scratch_root("the-sweep-refuses-a-sess");
        let pool = sweep_pool().await;
        let runs = InMemoryAgentRunStore::new();
        let jobs = InMemoryJobStore::new();
        let session_id = create_session(&pool).await;
        let root = crate::scratch::session_scratch_dir(&session_id).expect("create the tree");
        set_activity(&pool, &session_id, 30 * DAY_MS).await;
        jobs.create_job(active_test_job(&session_id))
            .await
            .expect("create a job");

        let report = sweep_scratch_once(
            &pool,
            &runs,
            &jobs,
            policy(),
            chrono::Utc::now().timestamp_millis(),
        )
        .await;

        assert_eq!(report.reclaimed_idle, 0, "report: {report:?}");
        assert_eq!(report.skipped_leased, 1, "report: {report:?}");
        assert!(root.exists(), "a tree with live work is not reclaimed");
        let _ = crate::scratch::reclaim_session_scratch_dir(&session_id);
    }

    /// A soft-deleted session's tree is reclaimed by the sweep.
    ///
    /// The event-driven reclaimer owns that case at delete time. The sweep is
    /// the retry for the tree it had to refuse because work was still running
    /// when the session was deleted, and it does not wait out the retention
    /// window: the session is gone.
    #[tokio::test]
    async fn the_sweep_reclaims_a_soft_deleted_session_tree() {
        let _root = scratch_root("the-sweep-reclaims-a-sof");
        let pool = sweep_pool().await;
        let runs = InMemoryAgentRunStore::new();
        let jobs = InMemoryJobStore::new();
        let session_id = create_session(&pool).await;
        let root = crate::scratch::session_scratch_dir(&session_id).expect("create the tree");
        std::fs::write(root.join("artifact.bin"), b"hello").expect("write an artifact");
        soft_delete(&pool, &session_id).await;

        let report = sweep_scratch_once(
            &pool,
            &runs,
            &jobs,
            policy(),
            chrono::Utc::now().timestamp_millis(),
        )
        .await;

        assert_eq!(report.reclaimed_deleted, 1, "report: {report:?}");
        assert_eq!(report.reclaimed_orphan, 0, "report: {report:?}");
        assert!(!root.exists(), "the deleted session's tree is reclaimed");
    }

    /// A soft-deleted session whose work is still running keeps its tree.
    #[tokio::test]
    async fn the_sweep_refuses_a_deleted_session_with_live_work() {
        let _root = scratch_root("the-sweep-refuses-a-dele");
        let pool = sweep_pool().await;
        let runs = InMemoryAgentRunStore::new();
        let jobs = InMemoryJobStore::new();
        let session_id = create_session(&pool).await;
        let root = crate::scratch::session_scratch_dir(&session_id).expect("create the tree");
        soft_delete(&pool, &session_id).await;
        jobs.create_job(active_test_job(&session_id))
            .await
            .expect("create a job");

        let report = sweep_scratch_once(
            &pool,
            &runs,
            &jobs,
            policy(),
            chrono::Utc::now().timestamp_millis(),
        )
        .await;

        assert_eq!(report.reclaimed_deleted, 0, "report: {report:?}");
        assert_eq!(report.skipped_leased, 1, "report: {report:?}");
        assert!(root.exists(), "a tree with live work is not reclaimed");
        let _ = crate::scratch::reclaim_session_scratch_dir(&session_id);
    }

    /// A session restored before the sweep is live again, so the retention
    /// window protects it.
    ///
    /// This is the writer-lock recheck: the sweep read the session set before
    /// the restore, and the re-read inside the lock is what sees it.
    #[tokio::test]
    async fn the_sweep_leaves_a_restored_session_alone() {
        let _root = scratch_root("the-sweep-leaves-a-resto");
        let pool = sweep_pool().await;
        let runs = InMemoryAgentRunStore::new();
        let jobs = InMemoryJobStore::new();
        let session_id = create_session(&pool).await;
        let root = crate::scratch::session_scratch_dir(&session_id).expect("create the tree");
        soft_delete(&pool, &session_id).await;
        restore(&pool, &session_id).await;
        set_activity(&pool, &session_id, 0).await;

        let report = sweep_scratch_once(
            &pool,
            &runs,
            &jobs,
            policy(),
            chrono::Utc::now().timestamp_millis(),
        )
        .await;

        assert_eq!(report.reclaimed_idle, 0, "report: {report:?}");
        assert_eq!(report.reclaimed_deleted, 0, "report: {report:?}");
        assert_eq!(report.skipped_active, 1, "report: {report:?}");
        assert!(root.exists(), "a restored session keeps its tree");
        let _ = crate::scratch::reclaim_session_scratch_dir(&session_id);
    }

    /// A tree that is already gone is recorded as vanished, and it does not
    /// fail the pass.
    #[tokio::test]
    async fn the_sweep_records_a_tree_that_vanished() {
        let _root = scratch_root("the-sweep-records-a-tree");
        let pool = sweep_pool().await;
        let runs = InMemoryAgentRunStore::new();
        let jobs = InMemoryJobStore::new();
        let session_id = create_session(&pool).await;
        // No tree at all: the session exists, its scratch does not.
        set_activity(&pool, &session_id, 30 * DAY_MS).await;

        let report = sweep_scratch_once(
            &pool,
            &runs,
            &jobs,
            policy(),
            chrono::Utc::now().timestamp_millis(),
        )
        .await;

        assert_eq!(report.vanished, 1, "report: {report:?}");
        assert_eq!(report.failed, 0, "report: {report:?}");
        assert_eq!(report.reclaimed_idle, 0, "report: {report:?}");
    }

    /// Backdate a tree's own modification time, so the orphan grace period
    /// can be exercised without waiting a day.
    #[cfg(unix)]
    fn backdate_tree(path: &std::path::Path, age_ms: i64) {
        let modified = std::time::SystemTime::UNIX_EPOCH
            + std::time::Duration::from_millis(
                (chrono::Utc::now().timestamp_millis() - age_ms).max(0) as u64,
            );
        let times = std::fs::FileTimes::new().set_modified(modified);
        std::fs::File::open(path)
            .expect("open the tree root")
            .set_times(times)
            .expect("backdate the tree");
    }

    /// A tree whose digest matches no durable session is an orphan, and the
    /// sweep reclaims it once the grace period has passed.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_sweep_reclaims_an_orphan_tree_after_the_grace_period() {
        let _root = scratch_root("the-sweep-reclaims-an-or");
        let pool = sweep_pool().await;
        let runs = InMemoryAgentRunStore::new();
        let jobs = InMemoryJobStore::new();
        // A tree with no session row at all.
        let orphan_id = format!("orphan-{}", uuid::Uuid::new_v4());
        let root = crate::scratch::session_scratch_dir(&orphan_id).expect("create the tree");
        backdate_tree(&root, 48 * 60 * 60 * 1000);

        let report = sweep_scratch_once(
            &pool,
            &runs,
            &jobs,
            policy(),
            chrono::Utc::now().timestamp_millis(),
        )
        .await;

        assert_eq!(report.reclaimed_orphan, 1, "report: {report:?}");
        assert!(!root.exists(), "the orphan tree is reclaimed");
    }

    /// An orphan inside the grace period is left alone: the grace absorbs the
    /// window in which a tree exists before its durable session row is
    /// visible to the sweeper.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_sweep_leaves_an_orphan_inside_the_grace_period() {
        let _root = scratch_root("the-sweep-leaves-an-orph");
        let pool = sweep_pool().await;
        let runs = InMemoryAgentRunStore::new();
        let jobs = InMemoryJobStore::new();
        let orphan_id = format!("orphan-{}", uuid::Uuid::new_v4());
        let root = crate::scratch::session_scratch_dir(&orphan_id).expect("create the tree");
        backdate_tree(&root, 60 * 1000);

        let report = sweep_scratch_once(
            &pool,
            &runs,
            &jobs,
            policy(),
            chrono::Utc::now().timestamp_millis(),
        )
        .await;

        assert_eq!(report.reclaimed_orphan, 0, "report: {report:?}");
        assert!(
            root.exists(),
            "an orphan inside the grace period is left alone"
        );
        let _ = crate::scratch::reclaim_session_scratch_dir(&orphan_id);
    }

    /// A pass that hits its bound stops and records that it stopped.
    ///
    /// A truncated pass is never presented as a complete sweep.
    #[tokio::test]
    async fn the_sweep_records_the_bound_that_stopped_it() {
        let _root = scratch_root("the-sweep-records-the-bo");
        let pool = sweep_pool().await;
        let runs = InMemoryAgentRunStore::new();
        let jobs = InMemoryJobStore::new();
        for _ in 0..3 {
            let session_id = create_session(&pool).await;
            let root = crate::scratch::session_scratch_dir(&session_id).expect("create the tree");
            std::fs::write(root.join("artifact.bin"), b"hello").expect("write an artifact");
            set_activity(&pool, &session_id, 30 * DAY_MS).await;
        }

        let report = sweep_scratch_once_with_budget(
            &pool,
            &runs,
            &jobs,
            policy(),
            chrono::Utc::now().timestamp_millis(),
            SweepBudget {
                max_candidates: 2,
                max_reclaims: 16,
                wall_clock_ms: 30_000,
            },
        )
        .await;

        assert_eq!(report.candidates, 2, "report: {report:?}");
        assert_eq!(report.reclaimed_idle, 2, "report: {report:?}");
        assert_eq!(
            report.stopped,
            Some(SweepStop::Candidates),
            "report: {report:?}"
        );
    }

    /// A pass that hits its reclaim bound stops and records that it stopped.
    #[tokio::test]
    async fn the_sweep_records_the_reclaim_bound_that_stopped_it() {
        let _root = scratch_root("the-sweep-records-the-re");
        let pool = sweep_pool().await;
        let runs = InMemoryAgentRunStore::new();
        let jobs = InMemoryJobStore::new();
        for _ in 0..3 {
            let session_id = create_session(&pool).await;
            let root = crate::scratch::session_scratch_dir(&session_id).expect("create the tree");
            std::fs::write(root.join("artifact.bin"), b"hello").expect("write an artifact");
            set_activity(&pool, &session_id, 30 * DAY_MS).await;
        }

        let report = sweep_scratch_once_with_budget(
            &pool,
            &runs,
            &jobs,
            policy(),
            chrono::Utc::now().timestamp_millis(),
            SweepBudget {
                max_candidates: 16,
                max_reclaims: 1,
                wall_clock_ms: 30_000,
            },
        )
        .await;

        assert_eq!(report.reclaimed_idle, 1, "report: {report:?}");
        assert_eq!(
            report.stopped,
            Some(SweepStop::Reclaims),
            "report: {report:?}"
        );
    }

    /// A disabled policy never fires.
    #[tokio::test]
    async fn a_disabled_policy_never_sweeps() {
        let _root = scratch_root("a-disabled-policy-never-");
        let pool = sweep_pool().await;
        let runs = InMemoryAgentRunStore::new();
        let jobs = InMemoryJobStore::new();
        let session_id = create_session(&pool).await;
        let root = crate::scratch::session_scratch_dir(&session_id).expect("create the tree");
        set_activity(&pool, &session_id, 30 * DAY_MS).await;

        let report = sweep_scratch_once(
            &pool,
            &runs,
            &jobs,
            ScratchRetentionPolicy {
                enabled: false,
                ..policy()
            },
            chrono::Utc::now().timestamp_millis(),
        )
        .await;

        assert_eq!(report.candidates, 0, "report: {report:?}");
        assert!(root.exists(), "a disabled policy reclaims nothing");
        let _ = crate::scratch::reclaim_session_scratch_dir(&session_id);
    }

    /// The operator disposition is a dry run by default: it reports and
    /// removes nothing.
    #[tokio::test]
    async fn the_operator_reclaim_is_a_dry_run_by_default() {
        let _root = scratch_root("the-operator-reclaim-is-");
        let pool = sweep_pool().await;
        let runs = InMemoryAgentRunStore::new();
        let jobs = InMemoryJobStore::new();
        let session_id = create_session(&pool).await;
        let root = crate::scratch::session_scratch_dir(&session_id).expect("create the tree");
        std::fs::write(root.join("artifact.bin"), b"hello").expect("write an artifact");
        let digest = crate::scratch::session_scratch_digest_hex(&session_id);

        let outcome = reclaim_session_scratch_for_operator(&pool, &runs, &jobs, &digest, false)
            .await
            .expect("the dry run succeeds");

        assert!(outcome.present, "outcome: {outcome:?}");
        assert!(!outcome.removed, "outcome: {outcome:?}");
        assert!(outcome.reclaimable, "outcome: {outcome:?}");
        // The tree holds the daemon's owner record and the artifact.
        assert_eq!(outcome.entry_count, 2, "outcome: {outcome:?}");
        assert_eq!(outcome.byte_count, 96, "outcome: {outcome:?}");
        assert!(root.exists(), "a dry run removes nothing");
        let _ = crate::scratch::reclaim_session_scratch_dir(&session_id);
    }

    /// The operator disposition removes the tree when it is applied.
    #[tokio::test]
    async fn the_operator_reclaim_removes_the_tree_when_applied() {
        let _root = scratch_root("the-operator-reclaim-rem");
        let pool = sweep_pool().await;
        let runs = InMemoryAgentRunStore::new();
        let jobs = InMemoryJobStore::new();
        let session_id = create_session(&pool).await;
        let root = crate::scratch::session_scratch_dir(&session_id).expect("create the tree");
        std::fs::write(root.join("artifact.bin"), b"hello").expect("write an artifact");
        let digest = crate::scratch::session_scratch_digest_hex(&session_id);

        let outcome = reclaim_session_scratch_for_operator(&pool, &runs, &jobs, &digest, true)
            .await
            .expect("the reclaim succeeds");

        assert!(outcome.removed, "outcome: {outcome:?}");
        assert!(!outcome.leased, "outcome: {outcome:?}");
        assert!(!root.exists(), "the tree is removed");
    }

    /// The operator disposition refuses a tree that is still in use.
    #[tokio::test]
    async fn the_operator_reclaim_refuses_a_leased_tree() {
        let _root = scratch_root("the-operator-reclaim-ref");
        let pool = sweep_pool().await;
        let runs = InMemoryAgentRunStore::new();
        let jobs = InMemoryJobStore::new();
        let session_id = create_session(&pool).await;
        let root = crate::scratch::session_scratch_dir(&session_id).expect("create the tree");
        let digest = crate::scratch::session_scratch_digest_hex(&session_id);
        accept_turn(&pool, &session_id).await;

        let outcome = reclaim_session_scratch_for_operator(&pool, &runs, &jobs, &digest, true)
            .await
            .expect("the reclaim is decided");

        assert!(outcome.leased, "outcome: {outcome:?}");
        assert!(!outcome.removed, "outcome: {outcome:?}");
        assert!(root.exists(), "a leased tree is not removed");
        let _ = crate::scratch::reclaim_session_scratch_dir(&session_id);
    }

    /// A digest that names no tree is a clean no-op.
    #[tokio::test]
    async fn the_operator_reclaim_of_an_unknown_digest_is_a_clean_no_op() {
        let _root = scratch_root("the-operator-reclaim-of-");
        let pool = sweep_pool().await;
        let runs = InMemoryAgentRunStore::new();
        let jobs = InMemoryJobStore::new();
        let digest = crate::scratch::session_scratch_digest_hex("no-such-session");

        let outcome = reclaim_session_scratch_for_operator(&pool, &runs, &jobs, &digest, true)
            .await
            .expect("the reclaim is decided");

        assert!(!outcome.present, "outcome: {outcome:?}");
        assert!(!outcome.removed, "outcome: {outcome:?}");
        assert!(!outcome.leased, "outcome: {outcome:?}");
        assert_eq!(outcome.byte_count, 0, "outcome: {outcome:?}");
    }

    /// A digest is a handle, not a path. A path never reaches the filesystem.
    #[tokio::test]
    async fn the_operator_reclaim_refuses_anything_that_is_not_a_digest() {
        let _root = scratch_root("the-operator-reclaim-ref");
        let pool = sweep_pool().await;
        let runs = InMemoryAgentRunStore::new();
        let jobs = InMemoryJobStore::new();

        for candidate in [
            "/etc",
            "../../etc",
            "not-a-digest",
            &"a".repeat(63),
            &"z".repeat(64),
            // A session id is not a digest either: the mapping runs forward,
            // so the request can never name a session.
            "scratch-operator-plain-session",
        ] {
            let outcome =
                reclaim_session_scratch_for_operator(&pool, &runs, &jobs, candidate, true).await;
            assert!(
                outcome.is_err(),
                "{candidate} was accepted as a digest: {outcome:?}"
            );
        }
    }

    /// The operator path removes a tree holding a symlink to an outside
    /// directory, and leaves the outside directory's contents intact.
    ///
    /// This is the link-safety proof the operator disposition's safety
    /// argument rests on: `remove_dir_all` does not follow a symbolic link.
    #[cfg(unix)]
    #[tokio::test]
    async fn the_operator_reclaim_removes_a_linked_tree_without_touching_its_target() {
        let _root = scratch_root("the-operator-reclaim-rem");
        let pool = sweep_pool().await;
        let runs = InMemoryAgentRunStore::new();
        let jobs = InMemoryJobStore::new();
        let session_id = create_session(&pool).await;
        let root = crate::scratch::session_scratch_dir(&session_id).expect("create the tree");

        // An outside directory the tree must not be able to reach into.
        let outside = std::env::temp_dir().join(format!(
            "scratch-operator-outside-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&outside).expect("create the outside directory");
        std::fs::write(outside.join("precious.txt"), b"outside content").expect("write outside");
        std::os::unix::fs::symlink(&outside, root.join("link")).expect("plant a symlink");

        let digest = crate::scratch::session_scratch_digest_hex(&session_id);

        // The automatic path refuses the tree.
        let automatic = crate::scratch::reclaim_session_scratch_dir(&session_id);
        assert!(
            automatic.is_err(),
            "the automatic path must refuse a tree holding a symlink"
        );

        // The operator path removes it.
        let outcome = reclaim_session_scratch_for_operator(&pool, &runs, &jobs, &digest, true)
            .await
            .expect("the operator reclaim succeeds");

        assert!(outcome.removed, "outcome: {outcome:?}");
        assert!(!root.exists(), "the tree is removed");
        assert!(
            outside.join("precious.txt").exists(),
            "the operator path must not remove the symlink's target"
        );
        assert_eq!(
            std::fs::read(outside.join("precious.txt")).expect("read outside"),
            b"outside content",
            "the operator path must not modify the symlink's target"
        );
        let _ = std::fs::remove_dir_all(&outside);
    }
}
