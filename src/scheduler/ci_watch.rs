//! GitHub Actions CI completion watching (async-background-jobs M004).
//!
//! A daemon-owned `watch_ci`-equivalent operation registers one remote
//! GitHub Actions workflow run (or an immutable commit SHA plus a check
//! selector that resolves to exactly one run), returns a stable watch
//! handle, observes completion with finite daemon-owned scheduled
//! observations — never model `gh run view` / sleep loops — and reports a
//! single actionable terminal result through M001/M003.
//!
//! # What this is not
//!
//! - Not a remote process execution job and not a CPU/process permit held
//!   for the CI duration. A dormant watcher holds no task, no permit, no
//!   interval: only a durable row with `next_check_at`.
//! - Not a second scheduler or timer loop. Observations advance only at
//!   existing daemon-owned activity points (explicit `ci_watch` tool calls
//!   and turn-boundary advancement, both bounded) plus the test harness's
//!   direct `observe_once` drive.
//! - Not a webhook listener, cron/DAG engine, or multi-provider CI
//!   platform. GitHub Actions only; anything else fails closed as
//!   unsupported.
//!
//! # Scheduling authority decision (documented here, per plan work package B)
//!
//! The current `ScheduleStore` materializes *recurring* schedule
//! occurrences (Subagent templates with overlap/missed-run policies). A CI
//! watch is a *finite* observation sequence with per-watch adaptive state
//! (ETag, exponential backoff with Retry-After, attempt budget, TTL) and
//! terminal reconciliation into exactly one M001 notice. Forcing watches
//! through recurring schedules would clutter the schedule namespace,
//! misrepresent sub-second network reads as compute jobs, and risk overlap
//! mis-semantics. Instead this module is the narrowly typed watch
//! schedule/record adapter the plan anticipates: each due observation is a
//! short bounded network read executed inline under a per-watch mutex (one
//! active occurrence), with all timing/budget state durable in `ci_watch`.
//! The durable completion path (persist final, then one M001 notice, then
//! M003 delivery) reuses the existing scheduler-owned notification
//! authority. A dormant watcher costs zero: no task, no permit, no poller.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use tokio::sync::{Mutex as AsyncMutex, RwLock};

use crate::scheduler::job_notifications::{JobCompletionDisposition, JobNotificationService};

/// Canonical provider identity. Only GitHub Actions is supported; any other
/// provider string fails closed as unsupported (never probed, never watched).
pub const PROVIDER_GITHUB_ACTIONS: &str = "github_actions";

/// Default allowlisted GitHub API base URL. The effective base URL always
/// comes from daemon configuration (see [`CiWatchConfig::allowlisted_base_urls`]),
/// never from model input.
pub const DEFAULT_GITHUB_API_BASE_URL: &str = "https://api.github.com";

/// Upper bound for a single GitHub API response body kept in memory.
pub const MAX_RESPONSE_BYTES: usize = 256 * 1024;

/// Upper bound for the persisted failing-step summary.
pub const MAX_FAILURE_SUMMARY_CHARS: usize = 500;

/// Maximum watches listed per session (bounded operator view).
pub const MAX_LIST_PER_SESSION: u32 = 32;

/// Maximum observations advanced per turn boundary (bounded stall on turn start).
pub const MAX_ADVANCE_PER_BOUNDARY: usize = 2;

#[derive(Debug, thiserror::Error)]
pub enum CiWatchError {
    #[error("storage error: {0}")]
    Storage(String),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("unsupported provider '{0}': only 'github_actions' is watched")]
    UnsupportedProvider(String),
    #[error("ambiguous watch target: {0}")]
    Ambiguous(String),
    #[error("invalid watch target: {0}")]
    Invalid(String),
    #[error("watch not found")]
    NotFound,
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("http error: {0}")]
    Http(String),
    #[error("endpoint not allowlisted: {0}")]
    NotAllowlisted(String),
    #[error("observation already in flight for this watch")]
    Busy,
    #[error("watch already terminal")]
    AlreadyTerminal,
}

/// Explicit watch states. GitHub `queued` / `in_progress` are nonterminal;
/// `completed` + conclusion maps to the typed outcomes below. A watch
/// timeout is expiration/attention, never remote cancellation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CiWatchState {
    Pending,
    Observing,
    CompletedSuccess,
    CompletedFailure,
    Cancelled,
    Expired,
    NeedsAttention,
}

pub fn ci_watch_state_to_str(s: CiWatchState) -> &'static str {
    match s {
        CiWatchState::Pending => "pending",
        CiWatchState::Observing => "observing",
        CiWatchState::CompletedSuccess => "completed_success",
        CiWatchState::CompletedFailure => "completed_failure",
        CiWatchState::Cancelled => "cancelled",
        CiWatchState::Expired => "expired",
        CiWatchState::NeedsAttention => "needs_attention",
    }
}

fn ci_watch_state_from_str(s: &str) -> Option<CiWatchState> {
    match s {
        "pending" => Some(CiWatchState::Pending),
        "observing" => Some(CiWatchState::Observing),
        "completed_success" => Some(CiWatchState::CompletedSuccess),
        "completed_failure" => Some(CiWatchState::CompletedFailure),
        "cancelled" => Some(CiWatchState::Cancelled),
        "expired" => Some(CiWatchState::Expired),
        "needs_attention" => Some(CiWatchState::NeedsAttention),
        _ => None,
    }
}

fn is_terminal_state(s: CiWatchState) -> bool {
    matches!(
        s,
        CiWatchState::CompletedSuccess
            | CiWatchState::CompletedFailure
            | CiWatchState::Cancelled
            | CiWatchState::Expired
            | CiWatchState::NeedsAttention
    )
}

/// Frozen immutable watch target: canonical provider, owner/repo, and either
/// an exact run id or an immutable commit SHA plus a precisely defined check
/// selector. Ambiguous multiple runs/check suites never report success.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CiWatchTarget {
    pub provider: String,
    pub owner: String,
    pub repo: String,
    #[serde(default)]
    pub run_id: Option<i64>,
    #[serde(default)]
    pub commit_sha: Option<String>,
    #[serde(default)]
    pub check_selector: Option<String>,
}

impl CiWatchTarget {
    pub fn validate(&self) -> Result<(), CiWatchError> {
        if self.provider != PROVIDER_GITHUB_ACTIONS {
            return Err(CiWatchError::UnsupportedProvider(self.provider.clone()));
        }
        for (label, value) in [("owner", &self.owner), ("repo", &self.repo)] {
            if value.is_empty() || value.len() > 100 {
                return Err(CiWatchError::Invalid(format!(
                    "{label} must be 1-100 chars"
                )));
            }
            if !value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
            {
                return Err(CiWatchError::Invalid(format!(
                    "{label} has illegal characters"
                )));
            }
        }
        match (self.run_id, self.commit_sha.as_deref()) {
            (Some(id), None) => {
                if id <= 0 {
                    return Err(CiWatchError::Invalid("run_id must be positive".into()));
                }
                if self.check_selector.is_some() {
                    return Err(CiWatchError::Invalid(
                        "check_selector requires commit_sha, not run_id".into(),
                    ));
                }
            }
            (None, Some(sha)) => {
                if sha.len() != 40 || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err(CiWatchError::Invalid(
                        "commit_sha must be a 40-char hex SHA".into(),
                    ));
                }
                if let Some(sel) = self.check_selector.as_deref() {
                    if sel.is_empty() || sel.len() > 200 {
                        return Err(CiWatchError::Invalid(
                            "check_selector must be 1-200 chars".into(),
                        ));
                    }
                }
            }
            (Some(_), Some(_)) => {
                return Err(CiWatchError::Ambiguous(
                    "supply run_id or commit_sha, not both".into(),
                ));
            }
            (None, None) => {
                return Err(CiWatchError::Ambiguous(
                    "supply an exact run_id or an immutable commit_sha".into(),
                ));
            }
        }
        Ok(())
    }
}

/// Durable watch record. `watch_id` is deterministic per accepted invocation
/// (`ciw-<digest(target|session|invocation)>`): retries coalesce, independent
/// watches stay distinct.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiWatchRecord {
    pub watch_id: String,
    pub provider: String,
    pub owner: String,
    pub repo: String,
    #[serde(default)]
    pub run_id: Option<i64>,
    #[serde(default)]
    pub commit_sha: Option<String>,
    #[serde(default)]
    pub check_selector: Option<String>,
    pub session_id: String,
    #[serde(default)]
    pub turn_id: Option<String>,
    pub invocation_key: String,
    pub state: CiWatchState,
    #[serde(default)]
    pub conclusion: Option<String>,
    #[serde(default)]
    pub conclusion_detail: Option<String>,
    #[serde(default)]
    pub last_etag: Option<String>,
    pub next_check_at: i64,
    #[serde(default)]
    pub attempt_count: u32,
    pub attempt_budget: u32,
    pub expiry_ms: i64,
    #[serde(default)]
    pub owner_principal: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<String>,
    pub credential_ref: String,
    pub api_base_url: String,
    #[serde(default)]
    pub last_observed_sha: Option<String>,
    #[serde(default)]
    pub last_run_status: Option<String>,
    #[serde(default)]
    pub final_url: Option<String>,
    #[serde(default)]
    pub failure_summary: Option<String>,
    #[serde(default)]
    pub on_complete: JobCompletionDisposition,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Bounded operator view of one watch. No secrets, no unbounded text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CiWatchView {
    pub watch_id: String,
    pub state: String,
    pub owner: String,
    pub repo: String,
    pub run_id: Option<i64>,
    pub commit_sha: Option<String>,
    pub conclusion: Option<String>,
    pub next_check_at: i64,
    pub attempt_count: u32,
    pub expiry_ms: i64,
    pub final_url: Option<String>,
    pub on_complete: String,
}

/// Daemon configuration for the watch service. Base URLs come from daemon
/// configuration (env `CODEGG_GITHUB_API_ALLOWLIST`, comma-separated,
/// defaulting to api.github.com); tests construct explicit allowlists
/// including their loopback fixture. The base URL is never model-supplied.
#[derive(Debug, Clone)]
pub struct CiWatchConfig {
    pub allowlisted_base_urls: Vec<String>,
    pub per_session_cap: usize,
    pub attempt_budget_default: u32,
    pub attempt_budget_max: u32,
    pub ttl_default_ms: i64,
    pub ttl_max_ms: i64,
    pub poll_timeout_secs: u64,
    pub backoff_base_secs: i64,
    pub backoff_max_secs: i64,
}

impl Default for CiWatchConfig {
    fn default() -> Self {
        Self {
            allowlisted_base_urls: vec![DEFAULT_GITHUB_API_BASE_URL.to_string()],
            per_session_cap: 8,
            attempt_budget_default: 200,
            attempt_budget_max: 1_000,
            ttl_default_ms: 2 * 60 * 60 * 1_000,
            ttl_max_ms: 12 * 60 * 60 * 1_000,
            poll_timeout_secs: 15,
            backoff_base_secs: 30,
            backoff_max_secs: 600,
        }
    }
}

impl CiWatchConfig {
    /// Build from daemon environment. `CODEGG_GITHUB_API_ALLOWLIST` is an
    /// optional comma-separated override; empty entries are dropped and an
    /// empty result falls back to the default.
    pub fn from_env() -> Self {
        let mut cfg = Self::default();
        if let Ok(raw) = std::env::var("CODEGG_GITHUB_API_ALLOWLIST") {
            let urls: Vec<String> = raw
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned)
                .collect();
            if !urls.is_empty() {
                cfg.allowlisted_base_urls = urls;
            }
        }
        cfg
    }

    pub fn test_with_base_url(base_url: &str) -> Self {
        Self {
            allowlisted_base_urls: vec![base_url.to_string()],
            per_session_cap: 32,
            attempt_budget_default: 50,
            attempt_budget_max: 50,
            ttl_default_ms: 600_000,
            ttl_max_ms: 600_000,
            poll_timeout_secs: 5,
            backoff_base_secs: 0,
            backoff_max_secs: 5,
        }
    }
}

pub struct CiWatchService {
    watches: RwLock<HashMap<String, CiWatchRecord>>,
    /// Per-watch mutex: exactly one active observation per watch. Concurrent
    /// callers observe `Busy` semantics (skip) rather than duplicate polls.
    in_flight: AsyncMutex<HashMap<String, Arc<AsyncMutex<()>>>>,
    notifier: Arc<JobNotificationService>,
    pub config: CiWatchConfig,
    pool: Option<SqlitePool>,
    http: eggfetch_core::Client,
}

impl CiWatchService {
    pub fn new(notifier: Arc<JobNotificationService>, config: CiWatchConfig) -> Self {
        let http = eggfetch_core::Client::builder()
            .timeout(eggfetch_core::Timeout::from_secs(config.poll_timeout_secs))
            .follow_redirects(false)
            .build();
        Self {
            watches: RwLock::new(HashMap::new()),
            in_flight: AsyncMutex::new(HashMap::new()),
            notifier,
            config,
            pool: None,
            http,
        }
    }

    pub fn with_pool(
        pool: SqlitePool,
        notifier: Arc<JobNotificationService>,
        config: CiWatchConfig,
    ) -> Self {
        let http = eggfetch_core::Client::builder()
            .timeout(eggfetch_core::Timeout::from_secs(config.poll_timeout_secs))
            .follow_redirects(false)
            .build();
        Self {
            watches: RwLock::new(HashMap::new()),
            in_flight: AsyncMutex::new(HashMap::new()),
            notifier,
            config,
            pool: Some(pool),
            http,
        }
    }

    /// Effective base URL: the first allowlisted entry. Single-host fleets
    /// are the supported scope; mixed github.com + GHE fleets are out of
    /// scope (documented). Never model-supplied.
    fn base_url(&self) -> String {
        self.config
            .allowlisted_base_urls
            .first()
            .cloned()
            .unwrap_or_else(|| DEFAULT_GITHUB_API_BASE_URL.to_string())
    }

    /// Create a watch for one explicitly supplied remote identity. Fails
    /// closed (no row, no spawn) on ambiguous/unsupported/invalid targets,
    /// missing session, over-quota sessions, or unresolvable credentials
    /// for the requested scope. Same accepted invocation coalesces to the
    /// same `watch_id`; independent invocations stay distinct.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_watch(
        &self,
        target: CiWatchTarget,
        session_id: &str,
        turn_id: Option<&str>,
        invocation_key: &str,
        owner_principal: Option<&str>,
        workspace_id: Option<&str>,
        on_complete: JobCompletionDisposition,
        ttl_ms: Option<i64>,
    ) -> Result<CiWatchRecord, CiWatchError> {
        target.validate()?;
        if session_id.is_empty() {
            return Err(CiWatchError::Storage(
                "ci watches require a real host-originated session context".into(),
            ));
        }
        if invocation_key.is_empty() {
            return Err(CiWatchError::Storage(
                "ci watches require an invocation key".into(),
            ));
        }
        // A model cannot create a more privileged watch than the session's
        // contract: principal/workspace come from the daemon boundary.
        let base_url = self.base_url();
        check_base_url_allowlisted(&base_url, &self.config.allowlisted_base_urls)?;
        let now = chrono::Utc::now().timestamp_millis();
        let ttl = ttl_ms
            .unwrap_or(self.config.ttl_default_ms)
            .clamp(1, self.config.ttl_max_ms);
        let digest_input = format!(
            "{}|{}|{}|{}|{}|{}|{}|{}",
            target.provider,
            target.owner,
            target.repo,
            target.run_id.map(|v| v.to_string()).unwrap_or_default(),
            target.commit_sha.as_deref().unwrap_or_default(),
            target.check_selector.as_deref().unwrap_or_default(),
            session_id,
            invocation_key,
        );
        let watch_id = format!("ciw-{:x}", Sha256::digest(digest_input.as_bytes()));
        {
            let watches = self.watches.read().await;
            if let Some(existing) = watches.get(&watch_id) {
                return Ok(existing.clone());
            }
        }
        if let Some(existing) = self.load_one(&watch_id).await? {
            let mut watches = self.watches.write().await;
            watches.insert(watch_id.clone(), existing.clone());
            return Ok(existing);
        }
        // Session quota: bounded watches per session.
        let session_count = self.count_for_session(session_id).await?;
        if session_count >= self.config.per_session_cap {
            return Err(CiWatchError::Conflict(format!(
                "session watch quota exceeded ({})",
                self.config.per_session_cap
            )));
        }
        let credential_ref = credential_ref_for_request();
        let record = CiWatchRecord {
            watch_id: watch_id.clone(),
            provider: target.provider.clone(),
            owner: target.owner.clone(),
            repo: target.repo.clone(),
            run_id: target.run_id,
            commit_sha: target.commit_sha.clone(),
            check_selector: target.check_selector.clone(),
            session_id: session_id.to_string(),
            turn_id: turn_id.map(ToOwned::to_owned),
            invocation_key: invocation_key.to_string(),
            state: CiWatchState::Pending,
            conclusion: None,
            conclusion_detail: None,
            last_etag: None,
            next_check_at: now,
            attempt_count: 0,
            attempt_budget: self.config.attempt_budget_default,
            expiry_ms: now.saturating_add(ttl),
            owner_principal: owner_principal.map(ToOwned::to_owned),
            workspace_id: workspace_id.map(ToOwned::to_owned),
            credential_ref,
            api_base_url: base_url,
            last_observed_sha: None,
            last_run_status: None,
            final_url: None,
            failure_summary: None,
            on_complete,
            created_at: now,
            updated_at: now,
        };
        self.persist(&record).await?;
        {
            let mut watches = self.watches.write().await;
            if let Some(existing) = watches.get(&watch_id) {
                return Ok(existing.clone());
            }
            watches.insert(watch_id.clone(), record.clone());
        }
        // The watch row is the durable subscription proof for its exactly
        // one terminal notice. Register the matching M001 subscription (and
        // opt-in resume lease) so terminal delivery reuses the verified
        // notice/lease machinery verbatim: notice_id = `ciwatch:<watch_id>`.
        let notice_job_id = codegg_core::jobs::JobId::new_unchecked(format!("ciwatch:{watch_id}"));
        // Best-effort: a duplicate subscription for the same watch id is
        // benign (idempotent); a real conflict fails the creation.
        self.notifier
            .subscribe_with_options(
                &notice_job_id,
                "ci_watch",
                session_id,
                turn_id,
                invocation_key,
                on_complete,
                owner_principal,
                workspace_id,
                Some(ttl),
            )
            .await
            .map_err(|e| CiWatchError::Storage(format!("watch subscribe failed: {e}")))?;
        Ok(record)
    }

    /// Bounded operator view. Exact-owner: the caller session must own the
    /// watch; anything else is `NotFound` (no existence oracle). If the
    /// watch is due, one daemon-owned observation is performed first so
    /// explicit checks advance the watch without model polling loops.
    pub async fn status(
        &self,
        watch_id: &str,
        caller_session_id: Option<&str>,
    ) -> Result<CiWatchView, CiWatchError> {
        let record = self.get_owned(watch_id, caller_session_id).await?;
        if !is_terminal_state(record.state) && self.is_due(&record) {
            // Best-effort advancement; a busy watcher simply reports cached
            // state rather than erroring the status call.
            match self.observe_once(watch_id).await {
                Ok(_) | Err(CiWatchError::Busy) | Err(CiWatchError::AlreadyTerminal) => {}
                Err(e) => {
                    tracing::debug!(watch_id, error = %e, "status-time observation failed");
                }
            }
            let record = self.get_owned(watch_id, caller_session_id).await?;
            return Ok(view_of(&record));
        }
        Ok(view_of(&record))
    }

    /// Bounded per-session listing for operators/TUI (newest first).
    pub async fn list_for_session(
        &self,
        session_id: &str,
    ) -> Result<Vec<CiWatchView>, CiWatchError> {
        self.load_session(session_id).await?;
        let watches = self.watches.read().await;
        let mut out: Vec<CiWatchView> = watches
            .values()
            .filter(|w| w.session_id == session_id)
            .map(view_of)
            .collect();
        out.sort_by(|a, b| b.watch_id.cmp(&a.watch_id));
        out.truncate(MAX_LIST_PER_SESSION as usize);
        Ok(out)
    }

    /// Cancel watching. Stops the watch only — never the remote workflow.
    /// A cancelled watch yields one terminal attention notice (consistent
    /// with job cancellation) and is never polled again.
    pub async fn cancel(
        &self,
        watch_id: &str,
        caller_session_id: Option<&str>,
    ) -> Result<CiWatchView, CiWatchError> {
        let record = self.get_owned(watch_id, caller_session_id).await?;
        if is_terminal_state(record.state) {
            return Ok(view_of(&record));
        }
        self.transition(watch_id, record.state, CiWatchState::Cancelled, None)
            .await?;
        self.emit_terminal_notice(watch_id).await?;
        let record = self.get_owned(watch_id, caller_session_id).await?;
        Ok(view_of(&record))
    }

    /// Advance due watches for one session (turn-boundary driver). Bounded
    /// to [`MAX_ADVANCE_PER_BOUNDARY`] observations so turn start cannot
    /// stall on CI polling. Returns the number of watches observed.
    pub async fn advance_due_watches(&self, session_id: &str) -> usize {
        let due: Vec<String> = {
            let watches = self.watches.read().await;
            let now = chrono::Utc::now().timestamp_millis();
            watches
                .values()
                .filter(|w| {
                    w.session_id == session_id
                        && matches!(w.state, CiWatchState::Pending | CiWatchState::Observing)
                        && w.next_check_at <= now
                })
                .take(MAX_ADVANCE_PER_BOUNDARY)
                .map(|w| w.watch_id.clone())
                .collect()
        };
        // Pool-backed watches not yet in memory (restart): load the session
        // slice first so due watches are visible.
        if due.is_empty() && self.pool.is_some() {
            if self.load_session(session_id).await.is_err() {
                return 0;
            }
            let due: Vec<String> = {
                let watches = self.watches.read().await;
                let now = chrono::Utc::now().timestamp_millis();
                watches
                    .values()
                    .filter(|w| {
                        w.session_id == session_id
                            && matches!(w.state, CiWatchState::Pending | CiWatchState::Observing)
                            && w.next_check_at <= now
                    })
                    .take(MAX_ADVANCE_PER_BOUNDARY)
                    .map(|w| w.watch_id.clone())
                    .collect()
            };
            return self.observe_many(&due).await;
        }
        self.observe_many(&due).await
    }

    async fn observe_many(&self, ids: &[String]) -> usize {
        let mut done = 0;
        for id in ids {
            match self.observe_once(id).await {
                Ok(_) => done += 1,
                Err(CiWatchError::Busy) | Err(CiWatchError::AlreadyTerminal) => {}
                Err(e) => {
                    tracing::debug!(watch_id = %id, error = %e, "boundary observation failed");
                }
            }
        }
        done
    }

    /// Perform one finite scheduled observation for a watch: a single
    /// bounded GitHub API read (plus, at most, one follow-up jobs-page read
    /// when terminally failed), then durable state advancement. Never holds
    /// a process/CPU permit across polls; concurrent callers get `Busy`.
    pub async fn observe_once(&self, watch_id: &str) -> Result<ObserveOutcome, CiWatchError> {
        let guard = self.claim_in_flight(watch_id).await?;
        let _held = guard;
        let mut record = self
            .get_by_id(watch_id)
            .await?
            .ok_or(CiWatchError::NotFound)?;
        if is_terminal_state(record.state) {
            return Err(CiWatchError::AlreadyTerminal);
        }
        let now = chrono::Utc::now().timestamp_millis();
        if now > record.expiry_ms {
            self.transition(watch_id, record.state, CiWatchState::Expired, None)
                .await?;
            self.emit_terminal_notice(watch_id).await?;
            return Ok(ObserveOutcome::Expired);
        }
        if record.attempt_count >= record.attempt_budget {
            self.transition(watch_id, record.state, CiWatchState::Expired, None)
                .await?;
            self.emit_terminal_notice(watch_id).await?;
            return Ok(ObserveOutcome::BudgetExhausted);
        }
        // Resolve run id first for commit-anchored watches (uniqueness
        // verified on every resolution; SHA drift fails closed).
        if record.run_id.is_none() {
            let sha = record.commit_sha.clone().unwrap_or_default();
            match self.resolve_run_for_sha(&record, &sha).await {
                Ok(run_id) => {
                    record.run_id = Some(run_id);
                    self.update_row(&record).await?;
                }
                Err(CiWatchError::Ambiguous(msg)) => {
                    // Ambiguous check suites never report success: mark
                    // needs_attention with the reason preserved.
                    record.conclusion_detail = Some(msg);
                    self.transition(
                        watch_id,
                        record.state,
                        CiWatchState::NeedsAttention,
                        Some(&record),
                    )
                    .await?;
                    self.emit_terminal_notice(watch_id).await?;
                    return Ok(ObserveOutcome::NeedsAttention);
                }
                Err(CiWatchError::Busy) => return Err(CiWatchError::Busy),
                Err(CiWatchError::AlreadyTerminal) => return Err(CiWatchError::AlreadyTerminal),
                Err(e) => {
                    return self
                        .note_transient_failure(&mut record, &e.to_string())
                        .await;
                }
            }
            // Re-read the stored row so the run_id write is durable before polling.
            record = self
                .get_by_id(watch_id)
                .await?
                .ok_or(CiWatchError::NotFound)?;
        }
        let run_id = record.run_id.ok_or_else(|| {
            CiWatchError::Invalid("watch has neither run_id nor commit_sha".into())
        })?;
        match self.fetch_run(&record, run_id).await {
            Ok(fetched) => self.apply_run_observation(&mut record, fetched).await,
            Err(FetchError::NotModified) => {
                // ETag revalidation: nothing changed. Advance the schedule
                // without treating it as a state transition.
                self.advance_schedule(&mut record, false).await?;
                self.update_row(&record).await?;
                Ok(ObserveOutcome::Nonterminal)
            }
            Err(FetchError::RateLimited(retry_after)) => {
                self.note_rate_limited(&mut record, retry_after).await
            }
            Err(FetchError::Retryable(msg)) => self.note_transient_failure(&mut record, &msg).await,
            Err(FetchError::TerminalAttention(msg)) => {
                record.conclusion_detail = Some(msg);
                self.transition(
                    watch_id,
                    record.state,
                    CiWatchState::NeedsAttention,
                    Some(&record),
                )
                .await?;
                self.emit_terminal_notice(watch_id).await?;
                Ok(ObserveOutcome::NeedsAttention)
            }
        }
    }

    /// Resolve an exact run id for an immutable commit SHA (+ optional check
    /// selector). Zero or multiple matches report `ambiguous` with the
    /// count — never a guessed success.
    async fn resolve_run_for_sha(
        &self,
        record: &CiWatchRecord,
        sha: &str,
    ) -> Result<i64, CiWatchError> {
        let url = format!(
            "{}/repos/{}/{}/actions/runs?head_sha={}&per_page=50",
            record.api_base_url, record.owner, record.repo, sha
        );
        let (body, _) = self
            .get_json(&url, record.last_etag.as_deref())
            .await
            .map_err(|e| match e {
                FetchError::Retryable(m) => {
                    CiWatchError::Storage(format!("transient runs-list failure: {m}"))
                }
                FetchError::RateLimited(secs) => {
                    CiWatchError::Storage(format!("rate-limited runs list ({secs}s)"))
                }
                FetchError::TerminalAttention(m) => CiWatchError::Ambiguous(m),
                FetchError::NotModified => {
                    CiWatchError::Storage("unexpected 304 on runs list".into())
                }
            })?;
        let parsed: RunsListResponse = serde_json::from_str(&body)
            .map_err(|e| CiWatchError::Ambiguous(format!("malformed runs list: {e}")))?;
        let mut candidates: Vec<&RunListEntry> = parsed.workflow_runs.iter().collect();
        if let Some(sel) = record.check_selector.as_deref() {
            candidates.retain(|r| r.name.as_deref() == Some(sel));
        }
        // SHA drift: the API echoes head_sha per run; every candidate must
        // still point at the frozen SHA before it can complete this watch.
        candidates.retain(|r| r.head_sha.as_deref() == Some(sha));
        match candidates.as_slice() {
            [only] => Ok(only.id),
            [] => Err(CiWatchError::Ambiguous(format!(
                "no workflow run for {}/{}@{} (selector {:?}): refusing to guess",
                record.owner, record.repo, sha, record.check_selector
            ))),
            many => Err(CiWatchError::Ambiguous(format!(
                "{} workflow runs match {}/{}@{} (selector {:?}): specify an exact run_id",
                many.len(),
                record.owner,
                record.repo,
                sha,
                record.check_selector
            ))),
        }
    }

    async fn fetch_run(
        &self,
        record: &CiWatchRecord,
        run_id: i64,
    ) -> Result<FetchedRun, FetchError> {
        let url = format!(
            "{}/repos/{}/{}/actions/runs/{}",
            record.api_base_url, record.owner, record.repo, run_id
        );
        let (body, etag) = self.get_json(&url, record.last_etag.as_deref()).await?;
        let run: RunResponse = serde_json::from_str(&body).map_err(|e| {
            FetchError::Retryable(format!("malformed run payload ({} bytes): {e}", body.len()))
        })?;
        // SHA binding: a run whose head_sha drifted from the frozen commit
        // (branch moved, repo renamed) must not complete this watch.
        if let Some(frozen) = record.commit_sha.as_deref() {
            if let Some(observed) = run.head_sha.as_deref() {
                if observed.to_lowercase() != frozen.to_lowercase() {
                    return Err(FetchError::TerminalAttention(format!(
                        "checked-out SHA drift: watch froze {frozen}, API reports {observed}"
                    )));
                }
            }
        }
        Ok(FetchedRun { run, etag })
    }

    async fn apply_run_observation(
        &self,
        record: &mut CiWatchRecord,
        fetched: FetchedRun,
    ) -> Result<ObserveOutcome, CiWatchError> {
        let run = fetched.run;
        record.last_run_status = run.status.clone();
        record.last_observed_sha = run.head_sha.clone();
        if fetched.etag.is_some() {
            record.last_etag = fetched.etag;
        }
        let status = run.status.as_deref().unwrap_or("");
        if status != "completed" {
            // Nonterminal updates projections only — never model wakeups.
            // (The watch row is the projection; M003 wakes only on terminal.)
            record.state = CiWatchState::Observing;
            self.advance_schedule(record, false).await?;
            self.update_row(record).await?;
            return Ok(ObserveOutcome::Nonterminal);
        }
        // Authoritative completed state: persist final result FIRST, then the
        // single M001 notice (M003 delivers notify/resume from there).
        let conclusion = run
            .conclusion
            .clone()
            .unwrap_or_else(|| "unknown".to_string());
        record.conclusion = Some(conclusion.clone());
        let (state, detail, failure_summary) = match conclusion.as_str() {
            "success" => (
                CiWatchState::CompletedSuccess,
                Some("workflow completed successfully".to_string()),
                None,
            ),
            "failure" | "cancelled" | "timed_out" | "action_required" | "startup_failure" => {
                let summary = self.fetch_failure_summary(record).await.unwrap_or_default();
                let detail = if conclusion == "cancelled" {
                    "workflow was cancelled remotely".to_string()
                } else {
                    format!("workflow concluded {conclusion}")
                };
                (
                    CiWatchState::CompletedFailure,
                    Some(detail),
                    if summary.is_empty() {
                        None
                    } else {
                        Some(summary)
                    },
                )
            }
            // neutral / skipped / stale / unknown: never labeled passed.
            other => {
                return self
                    .finish_needs_attention(
                        record,
                        &format!("workflow concluded {other}: held for operator review, not reported as passed"),
                    )
                    .await;
            }
        };
        record.conclusion_detail = detail;
        record.failure_summary = failure_summary;
        record.final_url = validated_run_url(&record.api_base_url, run.html_url.as_deref());
        self.transition(record.watch_id.as_str(), record.state, state, Some(record))
            .await?;
        // Re-read for the notice emission so the persisted final is authoritative.
        self.emit_terminal_notice(&record.watch_id).await?;
        Ok(if state == CiWatchState::CompletedSuccess {
            ObserveOutcome::TerminalSuccess
        } else {
            ObserveOutcome::TerminalFailure
        })
    }

    async fn finish_needs_attention(
        &self,
        record: &mut CiWatchRecord,
        reason: &str,
    ) -> Result<ObserveOutcome, CiWatchError> {
        record.conclusion_detail = Some(reason.to_string());
        self.transition(
            record.watch_id.as_str(),
            record.state,
            CiWatchState::NeedsAttention,
            Some(record),
        )
        .await?;
        self.emit_terminal_notice(&record.watch_id).await?;
        Ok(ObserveOutcome::NeedsAttention)
    }

    /// Bounded failing-step summary, fetched once at terminal failure.
    /// External step names are data rendered inertly, never instructions.
    async fn fetch_failure_summary(&self, record: &CiWatchRecord) -> Result<String, CiWatchError> {
        let run_id = record
            .run_id
            .ok_or_else(|| CiWatchError::Invalid("no run_id".into()))?;
        let url = format!(
            "{}/repos/{}/{}/actions/runs/{}/jobs?per_page=20",
            record.api_base_url, record.owner, record.repo, run_id
        );
        let body = match self.get_json(&url, None).await {
            Ok((b, _)) => b,
            Err(_) => return Ok(String::new()),
        };
        let jobs: JobsResponse = serde_json::from_str(&body)
            .map_err(|_| CiWatchError::Invalid("jobs payload".into()))?;
        let mut parts = Vec::new();
        for job in jobs
            .jobs
            .iter()
            .filter(|j| j.conclusion.as_deref() == Some("failure"))
            .take(3)
        {
            let steps: Vec<String> = job
                .steps
                .iter()
                .flatten()
                .filter(|s| s.conclusion.as_deref() == Some("failure"))
                .take(3)
                .map(|s| s.name.clone().unwrap_or_else(|| "?".to_string()))
                .collect();
            parts.push(format!(
                "{}: {}",
                job.name.clone().unwrap_or_else(|| "?".to_string()),
                if steps.is_empty() {
                    "failed".to_string()
                } else {
                    steps.join(", ")
                }
            ));
        }
        let mut summary = parts.join("; ");
        if summary.len() > MAX_FAILURE_SUMMARY_CHARS {
            let mut end = MAX_FAILURE_SUMMARY_CHARS;
            while end > 0 && !summary.is_char_boundary(end) {
                end -= 1;
            }
            summary.truncate(end);
        }
        Ok(summary)
    }

    /// Honor server-directed rate limiting: persist the Retry-After horizon
    /// as the next check time (capped), consume one attempt. Never reported
    /// as a CI outcome.
    async fn note_rate_limited(
        &self,
        record: &mut CiWatchRecord,
        retry_after_secs: i64,
    ) -> Result<ObserveOutcome, CiWatchError> {
        let now = chrono::Utc::now().timestamp_millis();
        let horizon = retry_after_secs.clamp(1, self.config.backoff_max_secs.max(1));
        record.next_check_at = now + horizon * 1_000;
        record.attempt_count = record.attempt_count.saturating_add(1);
        record.updated_at = now;
        if record.state == CiWatchState::Pending {
            record.state = CiWatchState::Observing;
        }
        self.update_row(record).await?;
        Ok(ObserveOutcome::TransientError)
    }

    /// Record a transient (retryable) observation failure: bounded backoff,
    /// attempt budget consumed. A transport failure is never reported as a
    /// CI failure. Budget exhaustion expires the watch (attention), it does
    /// not cancel the remote workflow.
    async fn note_transient_failure(
        &self,
        record: &mut CiWatchRecord,
        message: &str,
    ) -> Result<ObserveOutcome, CiWatchError> {
        tracing::debug!(watch_id = %record.watch_id, %message, "transient watch observation failure");
        record.conclusion_detail = Some(format!("last transient error: {message}"));
        if record.attempt_count + 1 >= record.attempt_budget {
            self.transition(
                record.watch_id.as_str(),
                record.state,
                CiWatchState::Expired,
                Some(record),
            )
            .await?;
            self.emit_terminal_notice(&record.watch_id).await?;
            return Ok(ObserveOutcome::BudgetExhausted);
        }
        self.advance_schedule(record, true).await?;
        self.update_row(record).await?;
        Ok(ObserveOutcome::TransientError)
    }

    /// Advance `next_check_at` with exponential backoff + deterministic
    /// jitter, and consume one attempt from the budget.
    async fn advance_schedule(
        &self,
        record: &mut CiWatchRecord,
        _transient: bool,
    ) -> Result<(), CiWatchError> {
        let now = chrono::Utc::now().timestamp_millis();
        let shift = record.attempt_count.min(10);
        let backoff_secs = (self.config.backoff_base_secs.saturating_mul(1 << shift))
            .min(self.config.backoff_max_secs);
        // Deterministic jitter 0..5s from (watch_id, attempt): no RNG in the
        // scheduling path, reproducible under fixtures.
        let mut hasher = Sha256::new();
        hasher.update(record.watch_id.as_bytes());
        hasher.update(record.attempt_count.to_le_bytes());
        let jitter_ms = (hasher.finalize()[0] as i64 % 6) * 1_000;
        record.next_check_at = now + backoff_secs * 1_000 + jitter_ms;
        record.attempt_count = record.attempt_count.saturating_add(1);
        record.updated_at = now;
        if record.state == CiWatchState::Pending {
            record.state = CiWatchState::Observing;
        }
        Ok(())
    }

    /// Emit the exactly-one terminal M001 notice for a terminal watch.
    /// Duplicate responses, restarts, and races with user cancel cannot
    /// issue multiple notices (idempotent on `ciwatch:<watch_id>), and
    /// terminal targets are never re-polled (callers check terminal first).
    async fn emit_terminal_notice(&self, watch_id: &str) -> Result<(), CiWatchError> {
        let record = self
            .get_by_id(watch_id)
            .await?
            .ok_or(CiWatchError::NotFound)?;
        if !is_terminal_state(record.state) {
            return Ok(());
        }
        let (status, summary) = watch_terminal_notice_content(&record);
        self.notifier
            .record_terminal_for_watch(
                &format!("ciwatch:{watch_id}"),
                "ci_watch",
                &record.session_id,
                record.turn_id.as_deref(),
                &status,
                &summary,
                None,
            )
            .await
            .map_err(|e| CiWatchError::Storage(format!("terminal notice failed: {e}")))?;
        Ok(())
    }

    /// One bounded GET with ETag, allowlist, redirect, size, and rate-limit
    /// handling. Returns the body plus the response ETag (for conditional
    /// revalidation). GitHub API output is data, never model instruction.
    async fn get_json(
        &self,
        url: &str,
        etag: Option<&str>,
    ) -> Result<(String, Option<String>), FetchError> {
        check_url_against_allowlist(url, &self.config.allowlisted_base_urls)
            .map_err(|e| FetchError::TerminalAttention(e.to_string()))?;
        let mut hops = 0;
        let mut current = url.to_string();
        loop {
            if hops > 3 {
                return Err(FetchError::TerminalAttention(
                    "too many redirects; failing closed".into(),
                ));
            }
            let mut req = self
                .http
                .get(&current)
                .map_err(|e| FetchError::Retryable(format!("request build failed: {e}")))?;
            req = req
                .header("User-Agent", "codegg-ci-watch")
                .header("Accept", "application/vnd.github+json");
            if let Some(tag) = etag.filter(|_| hops == 0) {
                req = req.header("If-None-Match", tag);
            }
            if let Some(token) = resolve_github_token() {
                let auth = format!("Bearer {token}");
                req = req.header("Authorization", auth.as_str());
            }
            let mut response = req
                .send()
                .await
                .map_err(|e| FetchError::Retryable(format!("transport failed: {e}")))?;
            let status = response.status().as_u16();
            match status {
                200 => {
                    let etag_out = response
                        .headers()
                        .get("etag")
                        .and_then(|v| v.to_str().ok())
                        .map(ToOwned::to_owned);
                    let text = response
                        .text()
                        .await
                        .map_err(|e| FetchError::Retryable(format!("body read failed: {e}")))?;
                    if text.len() > MAX_RESPONSE_BYTES {
                        return Err(FetchError::Retryable(format!(
                            "response {} bytes exceeds {}-byte bound",
                            text.len(),
                            MAX_RESPONSE_BYTES
                        )));
                    }
                    return Ok((text, etag_out));
                }
                304 => {
                    return Err(FetchError::NotModified);
                }
                301 | 302 | 303 | 307 | 308 => {
                    let location = response
                        .headers()
                        .get("location")
                        .and_then(|v| v.to_str().ok())
                        .map(ToOwned::to_owned)
                        .ok_or_else(|| {
                            FetchError::TerminalAttention("redirect without location".into())
                        })?;
                    current = check_same_origin_redirect(&current, &location)
                        .map_err(FetchError::TerminalAttention)?;
                    hops += 1;
                }
                401 | 403 | 404 => {
                    // Credential unavailable/scope mismatch, rate-limit
                    // denial, visibility ambiguity, or invalid id: fail
                    // closed as attention. Never report success, never leak
                    // whether the repo exists or the id is valid.
                    if status == 403 {
                        if let Some(retry_after) = response
                            .headers()
                            .get("retry-after")
                            .and_then(|v| v.to_str().ok())
                            .and_then(|v| v.parse::<i64>().ok())
                        {
                            return Err(FetchError::RateLimited(retry_after.clamp(1, 3600)));
                        }
                    }
                    return Err(FetchError::TerminalAttention(format!(
                        "github API returned {status}: failing closed without leaking repo visibility"
                    )));
                }
                429 => {
                    let retry_after = response
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse::<i64>().ok())
                        .unwrap_or(60)
                        .clamp(1, 3600);
                    return Err(FetchError::RateLimited(retry_after));
                }
                500..=599 => {
                    return Err(FetchError::Retryable(format!(
                        "github API returned {status}: transient"
                    )));
                }
                _ => {
                    return Err(FetchError::Retryable(format!(
                        "github API returned unexpected {status}"
                    )));
                }
            }
        }
    }

    fn is_due(&self, record: &CiWatchRecord) -> bool {
        chrono::Utc::now().timestamp_millis() >= record.next_check_at
    }

    async fn get_owned(
        &self,
        watch_id: &str,
        caller_session_id: Option<&str>,
    ) -> Result<CiWatchRecord, CiWatchError> {
        let record = self
            .get_by_id(watch_id)
            .await?
            .ok_or(CiWatchError::NotFound)?;
        let Some(caller) = caller_session_id.filter(|s| !s.is_empty()) else {
            return Err(CiWatchError::NotFound);
        };
        if record.session_id != caller {
            return Err(CiWatchError::NotFound);
        }
        Ok(record)
    }

    async fn get_by_id(&self, watch_id: &str) -> Result<Option<CiWatchRecord>, CiWatchError> {
        {
            let watches = self.watches.read().await;
            if let Some(w) = watches.get(watch_id) {
                return Ok(Some(w.clone()));
            }
        }
        self.load_one(watch_id).await
    }

    async fn count_for_session(&self, session_id: &str) -> Result<usize, CiWatchError> {
        self.load_session(session_id).await?;
        let watches = self.watches.read().await;
        Ok(watches
            .values()
            .filter(|w| w.session_id == session_id)
            .count())
    }

    async fn claim_in_flight(
        &self,
        watch_id: &str,
    ) -> Result<tokio::sync::OwnedMutexGuard<()>, CiWatchError> {
        let guard = {
            let mut map = self.in_flight.lock().await;
            map.entry(watch_id.to_string())
                .or_insert_with(|| Arc::new(AsyncMutex::new(())))
                .clone()
        };
        guard.try_lock_owned().map_err(|_| CiWatchError::Busy)
    }

    async fn transition(
        &self,
        watch_id: &str,
        from: CiWatchState,
        to: CiWatchState,
        updated: Option<&CiWatchRecord>,
    ) -> Result<(), CiWatchError> {
        let now = chrono::Utc::now().timestamp_millis();
        // The persisted JSON snapshot must carry the NEW state: callers pass
        // their working record (still stamped with the old state), so stamp
        // the transition target here. A stale snapshot would otherwise make
        // fresh readers (restart, second frontend) see a superseded state.
        let snapshot_json = updated.map(|r| {
            let mut snap = r.clone();
            snap.state = to;
            snap.updated_at = now;
            serde_json::to_string(&snap).unwrap_or_default()
        });
        if let Some(pool) = &self.pool {
            let result = sqlx::query(
                "UPDATE ci_watch SET state = ?2, updated_at = ?3, conclusion = COALESCE(?4, conclusion), conclusion_detail = COALESCE(?5, conclusion_detail), last_etag = COALESCE(?6, last_etag), next_check_at = COALESCE(?7, next_check_at), attempt_count = COALESCE(?8, attempt_count), last_observed_sha = COALESCE(?9, last_observed_sha), last_run_status = COALESCE(?10, last_run_status), final_url = COALESCE(?11, final_url), failure_summary = COALESCE(?12, failure_summary), run_id = COALESCE(?13, run_id), record_json = COALESCE(?14, record_json) WHERE watch_id = ?1 AND state = ?15",
            )
            .bind(watch_id)
            .bind(ci_watch_state_to_str(to))
            .bind(now)
            .bind(updated.and_then(|r| r.conclusion.clone()))
            .bind(updated.and_then(|r| r.conclusion_detail.clone()))
            .bind(updated.and_then(|r| r.last_etag.clone()))
            .bind(updated.map(|r| r.next_check_at))
            .bind(updated.map(|r| r.attempt_count as i64))
            .bind(updated.and_then(|r| r.last_observed_sha.clone()))
            .bind(updated.and_then(|r| r.last_run_status.clone()))
            .bind(updated.and_then(|r| r.final_url.clone()))
            .bind(updated.and_then(|r| r.failure_summary.clone()))
            .bind(updated.and_then(|r| r.run_id))
            .bind(snapshot_json)
            .bind(ci_watch_state_to_str(from))
            .execute(pool)
            .await
            .map_err(|e| CiWatchError::Storage(e.to_string()))?;
            if result.rows_affected() > 0 {
                let mut watches = self.watches.write().await;
                if let Some(w) = watches.get_mut(watch_id) {
                    w.state = to;
                    w.updated_at = now;
                    if let Some(u) = updated {
                        w.conclusion = u.conclusion.clone();
                        w.conclusion_detail = u.conclusion_detail.clone();
                        w.last_etag = u.last_etag.clone();
                        w.next_check_at = u.next_check_at;
                        w.attempt_count = u.attempt_count;
                        w.last_observed_sha = u.last_observed_sha.clone();
                        w.last_run_status = u.last_run_status.clone();
                        w.final_url = u.final_url.clone();
                        w.failure_summary = u.failure_summary.clone();
                        w.run_id = u.run_id;
                    }
                }
                return Ok(());
            }
            // CAS miss: refresh and report conflict only if another terminal won.
            self.load_one(watch_id).await?;
            return Ok(());
        }
        let mut watches = self.watches.write().await;
        let Some(w) = watches.get_mut(watch_id) else {
            return Err(CiWatchError::NotFound);
        };
        if w.state != from {
            return Ok(());
        }
        w.state = to;
        w.updated_at = now;
        if let Some(u) = updated {
            w.conclusion = u.conclusion.clone();
            w.conclusion_detail = u.conclusion_detail.clone();
            w.last_etag = u.last_etag.clone();
            w.next_check_at = u.next_check_at;
            w.attempt_count = u.attempt_count;
            w.last_observed_sha = u.last_observed_sha.clone();
            w.last_run_status = u.last_run_status.clone();
            w.final_url = u.final_url.clone();
            w.failure_summary = u.failure_summary.clone();
            w.run_id = u.run_id;
        }
        Ok(())
    }

    async fn update_row(&self, record: &CiWatchRecord) -> Result<(), CiWatchError> {
        let mut updated = record.clone();
        updated.updated_at = chrono::Utc::now().timestamp_millis();
        self.persist(&updated).await?;
        let mut watches = self.watches.write().await;
        watches.insert(updated.watch_id.clone(), updated);
        Ok(())
    }

    async fn persist(&self, record: &CiWatchRecord) -> Result<(), CiWatchError> {
        let Some(pool) = &self.pool else {
            return Ok(());
        };
        let record_json = serde_json::to_string(record)?;
        sqlx::query(
            "INSERT INTO ci_watch (watch_id, provider, owner, repo, run_id, commit_sha, check_selector, session_id, turn_id, invocation_key, state, conclusion, conclusion_detail, last_etag, next_check_at, attempt_count, attempt_budget, expiry_ms, owner_principal, workspace_id, credential_ref, api_base_url, last_observed_sha, last_run_status, final_url, failure_summary, on_complete, created_at, updated_at, record_json) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) \
             ON CONFLICT(watch_id) DO UPDATE SET state = excluded.state, conclusion = excluded.conclusion, conclusion_detail = excluded.conclusion_detail, last_etag = excluded.last_etag, next_check_at = excluded.next_check_at, attempt_count = excluded.attempt_count, last_observed_sha = excluded.last_observed_sha, last_run_status = excluded.last_run_status, final_url = excluded.final_url, failure_summary = excluded.failure_summary, run_id = excluded.run_id, updated_at = excluded.updated_at, record_json = excluded.record_json",
        )
        .bind(&record.watch_id)
        .bind(&record.provider)
        .bind(&record.owner)
        .bind(&record.repo)
        .bind(record.run_id)
        .bind(&record.commit_sha)
        .bind(&record.check_selector)
        .bind(&record.session_id)
        .bind(&record.turn_id)
        .bind(&record.invocation_key)
        .bind(ci_watch_state_to_str(record.state))
        .bind(&record.conclusion)
        .bind(&record.conclusion_detail)
        .bind(&record.last_etag)
        .bind(record.next_check_at)
        .bind(record.attempt_count as i64)
        .bind(record.attempt_budget as i64)
        .bind(record.expiry_ms)
        .bind(&record.owner_principal)
        .bind(&record.workspace_id)
        .bind(&record.credential_ref)
        .bind(&record.api_base_url)
        .bind(&record.last_observed_sha)
        .bind(&record.last_run_status)
        .bind(&record.final_url)
        .bind(&record.failure_summary)
        .bind(record.on_complete.as_str())
        .bind(record.created_at)
        .bind(record.updated_at)
        .bind(record_json)
        .execute(pool)
        .await
        .map_err(|e| CiWatchError::Storage(e.to_string()))?;
        Ok(())
    }

    async fn load_one(&self, watch_id: &str) -> Result<Option<CiWatchRecord>, CiWatchError> {
        let Some(pool) = &self.pool else {
            return Ok(None);
        };
        let row: Option<String> =
            sqlx::query_scalar("SELECT record_json FROM ci_watch WHERE watch_id = ?")
                .bind(watch_id)
                .fetch_optional(pool)
                .await
                .map_err(|e| CiWatchError::Storage(e.to_string()))?;
        let Some(json) = row else {
            return Ok(None);
        };
        let mut record: CiWatchRecord = serde_json::from_str(&json)?;
        let state_row: Option<String> =
            sqlx::query_scalar("SELECT state FROM ci_watch WHERE watch_id = ?")
                .bind(watch_id)
                .fetch_optional(pool)
                .await
                .map_err(|e| CiWatchError::Storage(e.to_string()))?;
        if let Some(s) = state_row.as_deref().and_then(ci_watch_state_from_str) {
            record.state = s;
        }
        let mut watches = self.watches.write().await;
        watches.insert(watch_id.to_string(), record.clone());
        Ok(Some(record))
    }

    async fn load_session(&self, session_id: &str) -> Result<(), CiWatchError> {
        let Some(pool) = &self.pool else {
            return Ok(());
        };
        let rows: Vec<String> = sqlx::query_scalar(
            "SELECT record_json FROM ci_watch WHERE session_id = ? ORDER BY created_at ASC LIMIT 64",
        )
        .bind(session_id)
        .fetch_all(pool)
        .await
        .map_err(|e| CiWatchError::Storage(e.to_string()))?;
        let mut parsed: Vec<CiWatchRecord> = Vec::with_capacity(rows.len());
        for json in rows {
            parsed.push(serde_json::from_str(&json)?);
        }
        // Prefer the state column (transition authority) over a possibly
        // stale JSON snapshot, mirroring `load_one`. Resolved before taking
        // the map lock so no guard is held across pool awaits.
        for record in &mut parsed {
            let state_row: Option<String> =
                sqlx::query_scalar("SELECT state FROM ci_watch WHERE watch_id = ?")
                    .bind(&record.watch_id)
                    .fetch_optional(pool)
                    .await
                    .map_err(|e| CiWatchError::Storage(e.to_string()))?;
            if let Some(s) = state_row.as_deref().and_then(ci_watch_state_from_str) {
                record.state = s;
            }
        }
        let mut watches = self.watches.write().await;
        for record in parsed {
            watches.insert(record.watch_id.clone(), record);
        }
        Ok(())
    }
}

/// Outcome of one finite observation (test and operator introspection).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObserveOutcome {
    Nonterminal,
    TerminalSuccess,
    TerminalFailure,
    NeedsAttention,
    TransientError,
    Expired,
    BudgetExhausted,
}

#[derive(Debug, Clone, Deserialize)]
struct RunResponse {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    conclusion: Option<String>,
    #[serde(default)]
    head_sha: Option<String>,
    #[serde(default)]
    html_url: Option<String>,
}

struct FetchedRun {
    run: RunResponse,
    etag: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RunsListResponse {
    #[serde(default)]
    workflow_runs: Vec<RunListEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct RunListEntry {
    id: i64,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    head_sha: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct JobsResponse {
    #[serde(default)]
    jobs: Vec<JobEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct JobEntry {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    conclusion: Option<String>,
    #[serde(default)]
    steps: Option<Vec<StepEntry>>,
}

#[derive(Debug, Clone, Deserialize)]
struct StepEntry {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    conclusion: Option<String>,
}

#[derive(Debug)]
enum FetchError {
    Retryable(String),
    TerminalAttention(String),
    RateLimited(i64),
    NotModified,
}

fn view_of(record: &CiWatchRecord) -> CiWatchView {
    CiWatchView {
        watch_id: record.watch_id.clone(),
        state: ci_watch_state_to_str(record.state).to_string(),
        owner: record.owner.clone(),
        repo: record.repo.clone(),
        run_id: record.run_id,
        commit_sha: record.commit_sha.clone(),
        conclusion: record.conclusion.clone(),
        next_check_at: record.next_check_at,
        attempt_count: record.attempt_count,
        expiry_ms: record.expiry_ms,
        final_url: record.final_url.clone(),
        on_complete: record.on_complete.as_str().to_string(),
    }
}

/// Terminal notice content for a watch. The stable run URL is included only
/// after validation; check/step names are inert data. A failed API call is
/// never rendered as a CI failure.
fn watch_terminal_notice_content(record: &CiWatchRecord) -> (String, String) {
    let id_label = match record.run_id {
        Some(id) => format!("{}/{} run {}", record.owner, record.repo, id),
        None => format!(
            "{}/{}@{}",
            record.owner,
            record.repo,
            record.commit_sha.as_deref().unwrap_or("?")
        ),
    };
    match record.state {
        CiWatchState::CompletedSuccess => (
            "completed".to_string(),
            format!(
                "CI watch {} passed{}",
                id_label,
                record
                    .final_url
                    .as_deref()
                    .map(|u| format!(": {u}"))
                    .unwrap_or_default()
            ),
        ),
        CiWatchState::CompletedFailure => (
            "failed".to_string(),
            format!(
                "CI watch {} concluded {}{}: {}",
                id_label,
                record.conclusion.as_deref().unwrap_or("failure"),
                record
                    .final_url
                    .as_deref()
                    .map(|u| format!(" ({u})"))
                    .unwrap_or_default(),
                record.failure_summary.as_deref().unwrap_or("see run details")
            ),
        ),
        CiWatchState::Cancelled => (
            "cancelled".to_string(),
            format!("CI watch {id_label} cancelled (watching stopped; remote workflow untouched)"),
        ),
        CiWatchState::Expired => (
            "expired".to_string(),
            format!("CI watch {id_label} expired before remote completion (watching stopped; remote workflow untouched)"),
        ),
        CiWatchState::NeedsAttention => (
            "needs_attention".to_string(),
            format!(
                "CI watch {id_label} needs attention: {}",
                record.conclusion_detail.as_deref().unwrap_or("unverified result")
            ),
        ),
        _ => (
            "failed".to_string(),
            format!("CI watch {id_label} ended unexpectedly"),
        ),
    }
}

/// Validate a run URL before embedding it in a notice: same-origin
/// allowlisted https (or loopback fixture), inert link text only.
fn validated_run_url(base_url: &str, url: Option<&str>) -> Option<String> {
    let url = url?;
    if check_same_origin(base_url, url).is_ok() {
        Some(url.to_string())
    } else {
        None
    }
}

/// Credential reference only — the secret itself is resolved per poll from
/// the process environment and never persisted, logged, or embedded in
/// rows. Least-privilege read token expected (`repo` scope not required;
/// public repos work unauthenticated within rate limits).
fn credential_ref_for_request() -> String {
    if std::env::var("GITHUB_TOKEN")
        .ok()
        .filter(|v| !v.is_empty())
        .is_some()
    {
        "env:GITHUB_TOKEN".to_string()
    } else if std::env::var("GH_TOKEN")
        .ok()
        .filter(|v| !v.is_empty())
        .is_some()
    {
        "env:GH_TOKEN".to_string()
    } else {
        "none".to_string()
    }
}

fn resolve_github_token() -> Option<String> {
    std::env::var("GITHUB_TOKEN")
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| std::env::var("GH_TOKEN").ok().filter(|v| !v.is_empty()))
}

fn check_base_url_allowlisted(base_url: &str, allowlist: &[String]) -> Result<(), CiWatchError> {
    check_url_against_allowlist(base_url, allowlist).map_err(CiWatchError::NotAllowlisted)
}

fn check_url_against_allowlist(url: &str, allowlist: &[String]) -> Result<(), String> {
    let (scheme, host, port) =
        split_url(url).map_err(|e| format!("rejecting malformed URL: {e}"))?;
    if scheme != "https" && !is_loopback_host(&host) {
        return Err(format!(
            "rejecting non-https URL for non-loopback host {host}"
        ));
    }
    if url.contains('@') {
        return Err("rejecting URL with userinfo".to_string());
    }
    for allowed in allowlist {
        if let Ok((a_scheme, a_host, a_port)) = split_url(allowed) {
            if scheme == a_scheme && host.eq_ignore_ascii_case(&a_host) && port == a_port {
                return Ok(());
            }
            // Loopback entries match any loopback port (fixture servers bind
            // ephemeral ports); non-loopback entries require exact port match.
            if is_loopback_host(&host) && is_loopback_host(&a_host) && scheme == a_scheme {
                return Ok(());
            }
        }
    }
    Err(format!("URL host {host} is not allowlisted"))
}

fn check_same_origin(base: &str, url: &str) -> Result<(), String> {
    if url.contains('@') {
        return Err("URL with userinfo rejected".to_string());
    }
    let (b_scheme, b_host, b_port) = split_url(base).map_err(|e| format!("bad base URL: {e}"))?;
    let (u_scheme, u_host, u_port) = split_url(url).map_err(|e| format!("bad URL: {e}"))?;
    if b_scheme == u_scheme && b_host.eq_ignore_ascii_case(&u_host) && b_port == u_port {
        Ok(())
    } else {
        Err("cross-origin URL".to_string())
    }
}

/// Same-origin redirect only, on an allowlisted host. Arbitrary
/// model-supplied callback URLs, token-bearing locations, and cross-host
/// hops (the GitHub Enterprise redirect trap) fail closed.
fn check_same_origin_redirect(current: &str, location: &str) -> Result<String, String> {
    if location.contains('@') {
        return Err("redirect with userinfo rejected".to_string());
    }
    // Relative redirect: resolve against the current URL's origin.
    if location.starts_with('/') {
        let (scheme, host, port) =
            split_url(current).map_err(|e| format!("bad current URL: {e}"))?;
        let port_part = port.map(|p| format!(":{p}")).unwrap_or_default();
        return Ok(format!("{scheme}://{host}{port_part}{location}"));
    }
    let (c_scheme, c_host, c_port) =
        split_url(current).map_err(|e| format!("bad current URL: {e}"))?;
    let (l_scheme, l_host, l_port) =
        split_url(location).map_err(|e| format!("bad redirect location: {e}"))?;
    if c_scheme == l_scheme && c_host.eq_ignore_ascii_case(&l_host) && c_port == l_port {
        Ok(location.to_string())
    } else {
        Err(format!(
            "cross-origin redirect rejected ({c_host} -> {l_host}); possible repo rename or Enterprise trap"
        ))
    }
}

fn split_url(url: &str) -> Result<(String, String, Option<u16>), String> {
    let (scheme, rest) = url.split_once("://").ok_or("missing scheme")?;
    let rest = rest.split('/').next().unwrap_or("");
    let rest = rest.split('?').next().unwrap_or("");
    let (host, port) = match rest.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => {
            (h.to_string(), p.parse::<u16>().ok())
        }
        _ => (rest.to_string(), None),
    };
    if host.is_empty() {
        return Err("missing host".to_string());
    }
    Ok((scheme.to_lowercase(), host, port))
}

fn is_loopback_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("127.0.0.1")
        || host.eq_ignore_ascii_case("localhost")
        || host == "::1"
}

impl Default for ObserveOutcome {
    fn default() -> Self {
        Self::Nonterminal
    }
}
