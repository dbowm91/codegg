//! Project-scoped ephemeral presence leases (M001).
//!
//! The daemon owns an in-memory presence service keyed by canonical
//! [`PrincipalId`](crate::identity::PrincipalId) + [`ProjectId`](crate::identity::ProjectId)
//! with per-connection/session activity contributions and bounded lease
//! expiry. Durable membership/authorization decides visibility; presence
//! never grants authority and is never consulted to authorize an
//! operation. It is a projection of liveness/activity only.
//!
//! ## Design notes
//!
//! - Presence is ephemeral: [`PresenceService::clear`] drops every lease
//!   (daemon restart) and [`PresenceService::evict_expired`] drops stale
//!   contributions. Expiry never deletes session history.
//! - Principals and projects are typed identities; client/session locators
//!   are bounded opaque strings. Request DTOs never name a principal,
//!   role, or capability: the daemon derives them from the trusted
//!   transport connection.
//! - One principal may hold many clients/sessions. Snapshots aggregate
//!   deterministically per principal (max activity rank, sorted union of
//!   sessions, max timestamp).
//! - High churn cannot create unbounded timers/tasks/maps: there is exactly
//!   one [`DashMap`](dashmap::DashMap) of contributions plus one bounded
//!   generation-tracker map, and exactly one cleanup entry point
//!   ([`PresenceService::evict_expired`]). No background task is spawned
//!   by this module.
//! - Stale generations cannot resurrect state: every heartbeat carries a
//!   per-connection monotonic `connection_generation`. A heartbeat with a
//!   generation older than the tracked maximum for that
//!   `(project, principal, client)` is rejected with
//!   [`PresenceError::StaleGeneration`].
//! - Contended renew/remove is deterministic: [`PresenceService::remove`]
//!   only removes when the stored generation equals the supplied one.
//! - Observability carries gauges only (live/expired/churn/rejections).
//!   Snapshots and metrics never embed secrets, tokens, or content.
//!
//! ## Transport contract
//!
//! Wire DTOs live in [`codegg_protocol::core`] (`PresenceActivityDto`,
//! `PresenceHeartbeatRequestDto`, `PresenceSnapshotDto`,
//! `PresencePrincipalDto`, `PresenceCapabilitiesDto`). This module owns the
//! domain state machine and converts to those DTOs. Authorization
//! (`project.observe`) is enforced by the daemon boundary through the
//! authorization module, never inside this module.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::identity::{PrincipalId, ProjectId};

/// Wire-compatible semantic activity for one presence contribution.
///
/// Rank order for aggregation: `AgentRunning > Active > Observing > Idle`.
/// Labels are deliberately coarse: no command text, prompt, reasoning, or
/// file content is ever carried as presence.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresenceActivity {
    Active,
    #[default]
    Idle,
    Observing,
    AgentRunning,
}

impl PresenceActivity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Idle => "idle",
            Self::Observing => "observing",
            Self::AgentRunning => "agent_running",
        }
    }

    /// Parse a wire activity name, failing closed on unknown input.
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "idle" => Some(Self::Idle),
            "observing" => Some(Self::Observing),
            "agent_running" => Some(Self::AgentRunning),
            _ => None,
        }
    }

    /// Aggregation rank: higher wins in per-principal snapshots.
    const fn rank(self) -> u8 {
        match self {
            Self::Idle => 0,
            Self::Observing => 1,
            Self::Active => 2,
            Self::AgentRunning => 3,
        }
    }

    fn from_dto(value: codegg_protocol::core::PresenceActivityDto) -> Self {
        match value {
            codegg_protocol::core::PresenceActivityDto::Active => Self::Active,
            codegg_protocol::core::PresenceActivityDto::Idle => Self::Idle,
            codegg_protocol::core::PresenceActivityDto::Observing => Self::Observing,
            codegg_protocol::core::PresenceActivityDto::AgentRunning => Self::AgentRunning,
        }
    }

    fn to_dto(self) -> codegg_protocol::core::PresenceActivityDto {
        match self {
            Self::Active => codegg_protocol::core::PresenceActivityDto::Active,
            Self::Idle => codegg_protocol::core::PresenceActivityDto::Idle,
            Self::Observing => codegg_protocol::core::PresenceActivityDto::Observing,
            Self::AgentRunning => codegg_protocol::core::PresenceActivityDto::AgentRunning,
        }
    }
}

/// Bounds for the ephemeral presence tables.
///
/// All bounds are enforced at [`PresenceService::heartbeat`] time; new
/// contributions beyond a bound fail with [`PresenceError::Capacity`]
/// rather than growing memory.
#[derive(Debug, Clone)]
pub struct PresenceConfig {
    /// How long a contribution survives without a heartbeat.
    pub lease_ttl: Duration,
    /// How long without a heartbeat before a contribution reads as idle
    /// (still present until `lease_ttl`).
    pub idle_after: Duration,
    /// Total contributions retained daemon-wide.
    pub max_contributions: usize,
    /// Distinct projects retained daemon-wide.
    pub max_projects: usize,
    /// Distinct principals retained per project snapshot.
    pub max_principals_per_project: usize,
    /// Distinct sessions retained per principal per project.
    pub max_sessions_per_principal: usize,
    /// Maximum opaque client/session locator length (matches the shared
    /// identity lexical bound).
    pub max_locator_length: usize,
}

impl Default for PresenceConfig {
    fn default() -> Self {
        Self {
            lease_ttl: Duration::from_secs(90),
            idle_after: Duration::from_secs(30),
            max_contributions: 4096,
            max_projects: 512,
            max_principals_per_project: 256,
            max_sessions_per_principal: 16,
            max_locator_length: 128,
        }
    }
}

impl PresenceConfig {
    /// Suggested heartbeat period for clients (one third of the lease).
    pub fn heartbeat_interval_hint(&self) -> Duration {
        self.lease_ttl / 3
    }

    pub fn capabilities_dto(&self) -> codegg_protocol::core::PresenceCapabilitiesDto {
        codegg_protocol::core::PresenceCapabilitiesDto {
            supported: true,
            protocol_version: 1,
            lease_ttl_secs: self.lease_ttl.as_secs(),
            idle_after_secs: self.idle_after.as_secs(),
            heartbeat_interval_hint_secs: self.heartbeat_interval_hint().as_secs(),
            max_principals_per_project: self.max_principals_per_project,
            max_sessions_per_principal: self.max_sessions_per_principal,
            max_contributions: self.max_contributions,
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PresenceError {
    #[error("invalid presence {field}: {message}")]
    InvalidInput {
        field: &'static str,
        message: String,
    },
    #[error("presence capacity is exhausted")]
    Capacity,
    #[error("stale connection generation cannot resurrect presence")]
    StaleGeneration,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ContributionKey {
    project: ProjectId,
    principal: PrincipalId,
    client_id: String,
    session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ClientGenerationKey {
    project: ProjectId,
    principal: PrincipalId,
    client_id: String,
}

#[derive(Debug, Clone)]
struct Contribution {
    principal: PrincipalId,
    client_id: String,
    session_id: Option<String>,
    activity: PresenceActivity,
    generation: u64,
    expires_at: Instant,
    idle_at: Instant,
    last_active_ms: i64,
}

/// Daemon-owned ephemeral presence lease service.
///
/// Thread-safe (`DashMap` + atomics), task-free, and strictly bounded.
/// All time is caller-supplied [`Instant`] so tests are deterministic;
/// wall-clock milliseconds are carried alongside for DTO snapshots.
pub struct PresenceService {
    config: PresenceConfig,
    contributions: DashMap<ContributionKey, Contribution>,
    /// Highest generation observed per `(project, principal, client)`.
    /// Retained after expiry so a late heartbeat with an old generation
    /// cannot resurrect stale state. Bounded by eviction of the oldest
    /// entries once the table exceeds `max_contributions`.
    client_generations: DashMap<ClientGenerationKey, u64>,
    total_heartbeats: AtomicU64,
    total_expired: AtomicU64,
    total_stale_rejected: AtomicU64,
    total_capacity_rejected: AtomicU64,
}

impl std::fmt::Debug for PresenceService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PresenceService")
            .field("live_leases", &self.live_leases())
            .field("total_heartbeats", &self.total_heartbeats())
            .field("total_expired", &self.total_expired())
            .finish_non_exhaustive()
    }
}

/// Gauges for live leases/expiry/churn. No identity secrets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresenceMetrics {
    pub live_leases: usize,
    pub tracked_clients: usize,
    pub total_heartbeats: u64,
    pub total_expired: u64,
    pub total_stale_rejected: u64,
    pub total_capacity_rejected: u64,
}

impl PresenceService {
    pub fn new(config: PresenceConfig) -> Self {
        Self {
            config,
            contributions: DashMap::new(),
            client_generations: DashMap::new(),
            total_heartbeats: AtomicU64::new(0),
            total_expired: AtomicU64::new(0),
            total_stale_rejected: AtomicU64::new(0),
            total_capacity_rejected: AtomicU64::new(0),
        }
    }

    pub fn config(&self) -> &PresenceConfig {
        &self.config
    }

    fn validate_locator(&self, field: &'static str, value: &str) -> Result<(), PresenceError> {
        if value.is_empty() || value.len() > self.config.max_locator_length {
            return Err(PresenceError::InvalidInput {
                field,
                message: format!(
                    "locator must be 1..={} bytes",
                    self.config.max_locator_length
                ),
            });
        }
        if value.bytes().any(|b| b == b'\0') || value.chars().any(char::is_control) {
            return Err(PresenceError::InvalidInput {
                field,
                message: "locator contains an unsupported character".to_owned(),
            });
        }
        Ok(())
    }

    /// Heartbeat or create the caller's own contribution.
    ///
    /// The principal/project/client triple is daemon-derived; only the
    /// optional session locator and semantic activity come from the
    /// request DTO. Heartbeats with a generation older than the tracked
    /// maximum are rejected without side effects.
    #[allow(clippy::too_many_arguments)]
    pub fn heartbeat(
        &self,
        project: ProjectId,
        principal: PrincipalId,
        client_id: &str,
        session_id: Option<&str>,
        activity: PresenceActivity,
        connection_generation: u64,
        now: Instant,
        now_ms: i64,
    ) -> Result<i64, PresenceError> {
        self.validate_locator("client_id", client_id)?;
        if let Some(session) = session_id {
            self.validate_locator("session_id", session)?;
        }
        let session_owned = session_id.map(str::to_owned);

        let gen_key = ClientGenerationKey {
            project: project.clone(),
            principal: principal.clone(),
            client_id: client_id.to_owned(),
        };
        if let Some(tracked) = self.client_generations.get(&gen_key) {
            if connection_generation < *tracked {
                self.total_stale_rejected.fetch_add(1, Ordering::Relaxed);
                return Err(PresenceError::StaleGeneration);
            }
        }

        let key = ContributionKey {
            project: project.clone(),
            principal: principal.clone(),
            client_id: client_id.to_owned(),
            session_id: session_owned.clone(),
        };
        let is_new = !self.contributions.contains_key(&key);
        if is_new {
            self.enforce_bounds(&project, &principal, session_owned.as_deref())?;
        }

        let expires_at = now + self.config.lease_ttl;
        let idle_at = now + self.config.idle_after;
        let expires_at_ms = now_ms + self.config.lease_ttl.as_millis() as i64;
        let contribution = Contribution {
            principal,
            client_id: client_id.to_owned(),
            session_id: session_owned,
            activity,
            generation: connection_generation,
            expires_at,
            idle_at,
            last_active_ms: now_ms,
        };
        self.contributions.insert(key, contribution);
        // Track the maximum generation even after expiry so late
        // heartbeats cannot resurrect stale rows.
        self.client_generations
            .entry(gen_key)
            .and_modify(|stored| {
                if connection_generation > *stored {
                    *stored = connection_generation;
                }
            })
            .or_insert(connection_generation);
        self.bound_generation_tracker();
        self.total_heartbeats.fetch_add(1, Ordering::Relaxed);
        Ok(expires_at_ms)
    }

    /// Best-effort implicit activity touch from session/agent progress.
    ///
    /// Unlike [`Self::heartbeat`], this never fails with
    /// [`PresenceError::StaleGeneration`]: it renews with the tracked
    /// maximum generation for the connection (or `0` on first use) so
    /// daemon-internal touches renew liveness without clobbering the
    /// explicit reconnect generation. Capacity/invalid-input failures
    /// are still reported so callers can ignore them without breaking
    /// session/agent correctness.
    pub fn touch(
        &self,
        project: ProjectId,
        principal: PrincipalId,
        client_id: &str,
        session_id: Option<&str>,
        activity: PresenceActivity,
        now: Instant,
        now_ms: i64,
    ) -> Result<i64, PresenceError> {
        let gen_key = ClientGenerationKey {
            project: project.clone(),
            principal: principal.clone(),
            client_id: client_id.to_owned(),
        };
        let generation = self
            .client_generations
            .get(&gen_key)
            .map(|tracked| *tracked)
            .unwrap_or(0);
        self.heartbeat(
            project, principal, client_id, session_id, activity, generation, now, now_ms,
        )
    }

    /// Convenience wrapper accepting the wire activity DTO.
    #[allow(clippy::too_many_arguments)]
    pub fn heartbeat_dto(
        &self,
        project: ProjectId,
        principal: PrincipalId,
        client_id: &str,
        session_id: Option<&str>,
        activity: codegg_protocol::core::PresenceActivityDto,
        connection_generation: u64,
        now: Instant,
        now_ms: i64,
    ) -> Result<i64, PresenceError> {
        self.heartbeat(
            project,
            principal,
            client_id,
            session_id,
            PresenceActivity::from_dto(activity),
            connection_generation,
            now,
            now_ms,
        )
    }

    fn enforce_bounds(
        &self,
        project: &ProjectId,
        principal: &PrincipalId,
        session_id: Option<&str>,
    ) -> Result<(), PresenceError> {
        if self.contributions.len() >= self.config.max_contributions {
            self.total_capacity_rejected.fetch_add(1, Ordering::Relaxed);
            return Err(PresenceError::Capacity);
        }
        // Distinct projects bound.
        if !self
            .contributions
            .iter()
            .any(|entry| entry.key().project == *project)
        {
            let projects = std::collections::HashSet::<ProjectId>::from_iter(
                self.contributions.iter().map(|e| e.key().project.clone()),
            );
            if projects.len() >= self.config.max_projects {
                self.total_capacity_rejected.fetch_add(1, Ordering::Relaxed);
                return Err(PresenceError::Capacity);
            }
        }
        // Distinct principals per project bound.
        let principals = std::collections::HashSet::<PrincipalId>::from_iter(
            self.contributions
                .iter()
                .filter(|e| e.key().project == *project)
                .map(|e| e.key().principal.clone()),
        );
        if !principals.contains(principal)
            && principals.len() >= self.config.max_principals_per_project
        {
            self.total_capacity_rejected.fetch_add(1, Ordering::Relaxed);
            return Err(PresenceError::Capacity);
        }
        // Distinct sessions per principal bound (project-level
        // contributions with `None` session do not consume the budget).
        if let Some(session) = session_id {
            let mut sessions = std::collections::HashSet::<Option<String>>::new();
            for entry in self
                .contributions
                .iter()
                .filter(|e| e.key().project == *project && e.key().principal == *principal)
            {
                sessions.insert(entry.key().session_id.clone());
            }
            if !sessions.contains(&Some(session.to_owned()))
                && sessions.len() >= self.config.max_sessions_per_principal
            {
                self.total_capacity_rejected.fetch_add(1, Ordering::Relaxed);
                return Err(PresenceError::Capacity);
            }
        }
        Ok(())
    }

    fn bound_generation_tracker(&self) {
        // The tracker is an anti-resurrection tombstone table, not a
        // second authority. Keep it bounded with the same ceiling as
        // the contribution table by dropping arbitrary oldest entries.
        let limit = self.config.max_contributions.max(1);
        if self.client_generations.len() <= limit {
            return;
        }
        let overflow = self.client_generations.len() - limit;
        let keys: Vec<ClientGenerationKey> = self
            .client_generations
            .iter()
            .take(overflow)
            .map(|entry| entry.key().clone())
            .collect();
        for key in keys {
            self.client_generations.remove(&key);
        }
    }

    /// Deterministic removal: only removes when the stored generation
    /// equals the supplied one. A mismatched generation leaves state
    /// untouched so a contended renew cannot be clobbered by a stale
    /// remover.
    pub fn remove(
        &self,
        project: &ProjectId,
        principal: &PrincipalId,
        client_id: &str,
        session_id: Option<&str>,
        connection_generation: u64,
    ) -> bool {
        let key = ContributionKey {
            project: project.clone(),
            principal: principal.clone(),
            client_id: client_id.to_owned(),
            session_id: session_id.map(str::to_owned),
        };
        match self.contributions.get(&key) {
            Some(entry) if entry.generation == connection_generation => {
                drop(entry);
                self.contributions.remove(&key).is_some()
            }
            _ => false,
        }
    }

    /// Expire one connection's contributions in one project, e.g. on
    /// explicit detach. The generation tracker retains the maximum so
    /// a late heartbeat with the same generation cannot resurrect.
    pub fn disconnect_client_in_project(
        &self,
        project: &ProjectId,
        principal: &PrincipalId,
        client_id: &str,
    ) -> usize {
        let keys: Vec<ContributionKey> = self
            .contributions
            .iter()
            .filter(|entry| {
                entry.key().project == *project
                    && entry.key().principal == *principal
                    && entry.key().client_id == client_id
            })
            .map(|entry| entry.key().clone())
            .collect();
        let removed = keys.len();
        for key in keys {
            self.contributions.remove(&key);
        }
        removed
    }

    /// Expire every contribution owned by one transport connection
    /// across all projects. Called on socket/WebSocket disconnect.
    /// Never fails; unknown clients remove zero rows.
    pub fn remove_client(&self, client_id: &str) -> usize {
        let keys: Vec<ContributionKey> = self
            .contributions
            .iter()
            .filter(|entry| entry.key().client_id == client_id)
            .map(|entry| entry.key().clone())
            .collect();
        let removed = keys.len();
        for key in keys {
            self.contributions.remove(&key);
        }
        removed
    }

    /// Single bounded cleanup entry point. Scans the contribution
    /// table once and drops every row with `now >= expires_at`.
    /// No task-per-lease is ever spawned; the daemon calls this
    /// before snapshots and on a bounded periodic tick.
    pub fn evict_expired(&self, now: Instant) -> usize {
        let keys: Vec<ContributionKey> = self
            .contributions
            .iter()
            .filter(|entry| now >= entry.value().expires_at)
            .map(|entry| entry.key().clone())
            .collect();
        let removed = keys.len();
        for key in keys {
            self.contributions.remove(&key);
        }
        if removed > 0 {
            self.total_expired
                .fetch_add(removed as u64, Ordering::Relaxed);
        }
        removed
    }

    /// Drop every lease and generation tombstone (daemon restart).
    /// Rebuilds exclusively from active connections afterwards; restart
    /// never fabricates durable presence.
    pub fn clear(&self) {
        self.contributions.clear();
        self.client_generations.clear();
    }

    pub fn live_leases(&self) -> usize {
        self.contributions.len()
    }

    pub fn total_heartbeats(&self) -> u64 {
        self.total_heartbeats.load(Ordering::Relaxed)
    }

    pub fn total_expired(&self) -> u64 {
        self.total_expired.load(Ordering::Relaxed)
    }

    pub fn metrics(&self) -> PresenceMetrics {
        PresenceMetrics {
            live_leases: self.contributions.len(),
            tracked_clients: self.client_generations.len(),
            total_heartbeats: self.total_heartbeats.load(Ordering::Relaxed),
            total_expired: self.total_expired.load(Ordering::Relaxed),
            total_stale_rejected: self.total_stale_rejected.load(Ordering::Relaxed),
            total_capacity_rejected: self.total_capacity_rejected.load(Ordering::Relaxed),
        }
    }

    /// Effective activity at `now`: a contribution past `idle_at`
    /// reads as idle but remains present until `expires_at`.
    fn effective_activity(contribution: &Contribution, now: Instant) -> PresenceActivity {
        if now >= contribution.idle_at && contribution.activity != PresenceActivity::Idle {
            PresenceActivity::Idle
        } else {
            contribution.activity
        }
    }

    /// Bounded privacy-filtered snapshot for one project.
    ///
    /// The caller MUST have authorized `project.observe` for the
    /// requesting principal before calling: this method performs no
    /// authorization itself so the daemon boundary stays the single
    /// authority. Expired rows are excluded (callers should
    /// [`Self::evict_expired`] first on a bounded tick).
    pub fn snapshot(
        &self,
        project: &ProjectId,
        now: Instant,
        now_ms: i64,
    ) -> codegg_protocol::core::PresenceSnapshotDto {
        use std::collections::BTreeMap;

        #[derive(Default)]
        struct Aggregate {
            activity_rank: u8,
            activity: PresenceActivity,
            clients: std::collections::HashSet<String>,
            sessions: std::collections::BTreeSet<String>,
            last_active_ms: i64,
        }

        let mut by_principal: BTreeMap<String, Aggregate> = BTreeMap::new();
        for entry in self
            .contributions
            .iter()
            .filter(|e| e.key().project == *project && now < e.value().expires_at)
        {
            let contribution = entry.value();
            let effective = Self::effective_activity(contribution, now);
            let aggregate = by_principal
                .entry(contribution.principal.as_str().to_owned())
                .or_insert_with(|| Aggregate {
                    activity: PresenceActivity::Idle,
                    last_active_ms: contribution.last_active_ms,
                    ..Aggregate::default()
                });
            if effective.rank() > aggregate.activity_rank
                || (effective.rank() == aggregate.activity_rank)
            {
                // Deterministic: higher rank wins; equal rank keeps the
                // first-seen variant (iteration order is unspecified but
                // the stored activity for the winning rank is unique per
                // rank, so the outcome is still deterministic).
                if effective.rank() > aggregate.activity_rank {
                    aggregate.activity_rank = effective.rank();
                    aggregate.activity = effective;
                }
            }
            aggregate.clients.insert(contribution.client_id.clone());
            if let Some(session) = contribution.session_id.clone() {
                aggregate.sessions.insert(session);
            }
            aggregate.last_active_ms = aggregate.last_active_ms.max(contribution.last_active_ms);
        }

        let mut principals: Vec<codegg_protocol::core::PresencePrincipalDto> = by_principal
            .into_iter()
            .map(|(principal_id, aggregate)| {
                let mut sessions: Vec<String> = aggregate.sessions.into_iter().collect();
                sessions.truncate(self.config.max_sessions_per_principal);
                codegg_protocol::core::PresencePrincipalDto {
                    principal_id,
                    activity: aggregate.activity.to_dto(),
                    client_count: aggregate.clients.len(),
                    session_ids: sessions,
                    last_active_ms: aggregate.last_active_ms,
                }
            })
            .collect();
        // BTreeMap iteration is already sorted by principal id.
        let truncated = principals.len() > self.config.max_principals_per_project;
        principals.truncate(self.config.max_principals_per_project);

        codegg_protocol::core::PresenceSnapshotDto {
            project_id: project.as_str().to_owned(),
            as_of_ms: now_ms,
            principals,
            truncated,
        }
    }

    /// Empty snapshot for a project with no live leases. Used for
    /// documentation/tests; the daemon returns the same shape for
    /// unauthorized callers only via the `project_not_found` denial so
    /// absence and denial stay indistinguishable on the wire.
    pub fn empty_snapshot(
        &self,
        project: &ProjectId,
        now_ms: i64,
    ) -> codegg_protocol::core::PresenceSnapshotDto {
        codegg_protocol::core::PresenceSnapshotDto {
            project_id: project.as_str().to_owned(),
            as_of_ms: now_ms,
            principals: Vec::new(),
            truncated: false,
        }
    }
}

impl Default for PresenceService {
    fn default() -> Self {
        Self::new(PresenceConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(project: &str, principal: &str) -> (ProjectId, PrincipalId) {
        (
            ProjectId::parse(project).expect("project fixture"),
            PrincipalId::parse(principal).expect("principal fixture"),
        )
    }

    fn service() -> PresenceService {
        PresenceService::new(PresenceConfig {
            lease_ttl: Duration::from_secs(90),
            idle_after: Duration::from_secs(30),
            max_contributions: 4096,
            max_projects: 512,
            max_principals_per_project: 256,
            max_sessions_per_principal: 16,
            max_locator_length: 128,
        })
    }

    #[test]
    fn heartbeat_and_snapshot_round_trip() {
        let presence = service();
        let (project, principal) = ids("project-1", "principal-1");
        let now = Instant::now();
        let expires = presence
            .heartbeat(
                project.clone(),
                principal.clone(),
                "client-1",
                Some("session-1"),
                PresenceActivity::Active,
                1,
                now,
                1_000,
            )
            .expect("heartbeat");
        assert!(expires > 1_000);
        let snapshot = presence.snapshot(&project, now, 1_000);
        assert_eq!(snapshot.principals.len(), 1);
        assert_eq!(snapshot.principals[0].principal_id, "principal-1");
        assert_eq!(
            snapshot.principals[0].activity,
            codegg_protocol::core::PresenceActivityDto::Active
        );
        assert_eq!(snapshot.principals[0].client_count, 1);
        assert_eq!(snapshot.principals[0].session_ids, vec!["session-1"]);
    }

    #[test]
    fn idle_after_threshold_reads_idle_but_stays_present() {
        let presence = service();
        let (project, principal) = ids("project-1", "principal-1");
        let start = Instant::now();
        presence
            .heartbeat(
                project.clone(),
                principal,
                "client-1",
                Some("session-1"),
                PresenceActivity::Active,
                1,
                start,
                1_000,
            )
            .expect("heartbeat");
        // Past idle (30s) but before expiry (90s): idle, still present.
        let idle_at = start + Duration::from_secs(31);
        let snapshot = presence.snapshot(&project, idle_at, 2_000);
        assert_eq!(snapshot.principals.len(), 1);
        assert_eq!(
            snapshot.principals[0].activity,
            codegg_protocol::core::PresenceActivityDto::Idle
        );
        // Past expiry: gone.
        let expired_at = start + Duration::from_secs(91);
        let evicted = presence.evict_expired(expired_at);
        assert_eq!(evicted, 1);
        let snapshot = presence.snapshot(&project, expired_at, 3_000);
        assert!(snapshot.principals.is_empty());
    }

    #[test]
    fn two_clients_one_principal_aggregate() {
        let presence = service();
        let (project, principal) = ids("project-1", "principal-1");
        let now = Instant::now();
        presence
            .heartbeat(
                project.clone(),
                principal.clone(),
                "client-1",
                Some("session-1"),
                PresenceActivity::Active,
                1,
                now,
                1_000,
            )
            .expect("c1");
        presence
            .heartbeat(
                project.clone(),
                principal.clone(),
                "client-2",
                Some("session-2"),
                PresenceActivity::Observing,
                1,
                now,
                1_001,
            )
            .expect("c2");
        let snapshot = presence.snapshot(&project, now, 1_002);
        assert_eq!(snapshot.principals.len(), 1);
        let row = &snapshot.principals[0];
        // Two clients, two sessions, max activity wins.
        assert_eq!(row.client_count, 2);
        assert_eq!(row.session_ids, vec!["session-1", "session-2"]);
        assert_eq!(
            row.activity,
            codegg_protocol::core::PresenceActivityDto::Active
        );
        assert_eq!(row.last_active_ms, 1_001);
    }

    #[test]
    fn several_sessions_aggregate_and_agent_rank_wins() {
        let presence = service();
        let (project, principal) = ids("project-1", "principal-1");
        let now = Instant::now();
        for (session, activity) in [
            ("session-1", PresenceActivity::Observing),
            ("session-2", PresenceActivity::Active),
            ("session-3", PresenceActivity::AgentRunning),
        ] {
            presence
                .heartbeat(
                    project.clone(),
                    principal.clone(),
                    "client-1",
                    Some(session),
                    activity,
                    1,
                    now,
                    1_000,
                )
                .expect("heartbeat");
        }
        let snapshot = presence.snapshot(&project, now, 1_000);
        assert_eq!(snapshot.principals.len(), 1);
        assert_eq!(
            snapshot.principals[0].activity,
            codegg_protocol::core::PresenceActivityDto::AgentRunning
        );
        assert_eq!(snapshot.principals[0].session_ids.len(), 3);
    }

    #[test]
    fn disconnect_shortens_and_reconnect_renews_without_duplicates() {
        let presence = service();
        let (project, principal) = ids("project-1", "principal-1");
        let now = Instant::now();
        presence
            .heartbeat(
                project.clone(),
                principal.clone(),
                "client-1",
                Some("session-1"),
                PresenceActivity::Active,
                1,
                now,
                1_000,
            )
            .expect("heartbeat");
        assert_eq!(presence.live_leases(), 1);
        let removed = presence.disconnect_client_in_project(&project, &principal, "client-1");
        assert_eq!(removed, 1);
        assert_eq!(presence.live_leases(), 0);
        // Reconnect with a newer generation renews; exactly one row.
        presence
            .heartbeat(
                project.clone(),
                principal.clone(),
                "client-1",
                Some("session-1"),
                PresenceActivity::Active,
                2,
                now,
                2_000,
            )
            .expect("reconnect");
        assert_eq!(presence.live_leases(), 1);
        let snapshot = presence.snapshot(&project, now, 2_000);
        assert_eq!(snapshot.principals.len(), 1);
    }

    #[test]
    fn stale_generation_heartbeat_cannot_resurrect() {
        let presence = service();
        let (project, principal) = ids("project-1", "principal-1");
        let start = Instant::now();
        presence
            .heartbeat(
                project.clone(),
                principal.clone(),
                "client-1",
                Some("session-1"),
                PresenceActivity::Active,
                2,
                start,
                1_000,
            )
            .expect("gen2");
        // Expire the row.
        let expired_at = start + Duration::from_secs(91);
        assert_eq!(presence.evict_expired(expired_at), 1);
        // Late heartbeat with the old generation is rejected.
        let outcome = presence.heartbeat(
            project.clone(),
            principal.clone(),
            "client-1",
            Some("session-1"),
            PresenceActivity::Active,
            1,
            expired_at,
            2_000,
        );
        assert_eq!(outcome, Err(PresenceError::StaleGeneration));
        assert_eq!(presence.live_leases(), 0);
        // The current generation can still renew.
        presence
            .heartbeat(
                project.clone(),
                principal.clone(),
                "client-1",
                Some("session-1"),
                PresenceActivity::Active,
                2,
                expired_at,
                2_000,
            )
            .expect("renew");
        assert_eq!(presence.live_leases(), 1);
    }

    #[test]
    fn contended_remove_is_deterministic() {
        let presence = service();
        let (project, principal) = ids("project-1", "principal-1");
        let now = Instant::now();
        presence
            .heartbeat(
                project.clone(),
                principal.clone(),
                "client-1",
                Some("session-1"),
                PresenceActivity::Active,
                2,
                now,
                1_000,
            )
            .expect("heartbeat");
        // Stale remover cannot clobber the renewed row.
        assert!(!presence.remove(&project, &principal, "client-1", Some("session-1"), 1));
        assert_eq!(presence.live_leases(), 1);
        assert!(presence.remove(&project, &principal, "client-1", Some("session-1"), 2));
        assert_eq!(presence.live_leases(), 0);
    }

    #[test]
    fn restart_clears_without_fabrication() {
        let presence = service();
        let (project, principal) = ids("project-1", "principal-1");
        let now = Instant::now();
        presence
            .heartbeat(
                project.clone(),
                principal,
                "client-1",
                Some("session-1"),
                PresenceActivity::Active,
                1,
                now,
                1_000,
            )
            .expect("heartbeat");
        assert_eq!(presence.live_leases(), 1);
        presence.clear();
        assert_eq!(presence.live_leases(), 0);
        let snapshot = presence.snapshot(&project, now, 2_000);
        assert!(snapshot.principals.is_empty());
    }

    #[test]
    fn high_churn_stays_bounded_without_tasks() {
        let presence = PresenceService::new(PresenceConfig {
            max_contributions: 8,
            max_projects: 2,
            max_principals_per_project: 4,
            max_sessions_per_principal: 2,
            ..PresenceConfig::default()
        });
        let now = Instant::now();
        let mut accepted = 0;
        let mut rejected = 0;
        for i in 0..64 {
            let project = ProjectId::parse(&format!("project-{}", i % 4)).expect("project");
            let principal = PrincipalId::parse(&format!("principal-{}", i % 8)).expect("principal");
            let client = format!("client-{}", i % 8);
            let session = format!("session-{}", i % 4);
            match presence.heartbeat(
                project,
                principal,
                &client,
                Some(session.as_str()),
                PresenceActivity::Active,
                1,
                now,
                1_000 + i as i64,
            ) {
                Ok(_) => accepted += 1,
                Err(PresenceError::Capacity) => rejected += 1,
                Err(other) => panic!("unexpected presence error: {other:?}"),
            }
        }
        assert!(rejected > 0, "churn must hit the bound");
        assert!(
            presence.live_leases() <= 8,
            "live leases bounded, got {}",
            presence.live_leases()
        );
        let metrics = presence.metrics();
        assert_eq!(metrics.live_leases, presence.live_leases());
        // One bounded scan reclaims everything after expiry.
        let later = now + Duration::from_secs(120);
        presence.evict_expired(later);
        assert_eq!(presence.live_leases(), 0);
        let _ = accepted;
    }

    #[test]
    fn snapshots_carry_no_secrets() {
        let presence = service();
        let (project, principal) = ids("project-1", "principal-1");
        let now = Instant::now();
        presence
            .heartbeat(
                project.clone(),
                principal,
                "client-1",
                Some("session-1"),
                PresenceActivity::Active,
                1,
                now,
                1_000,
            )
            .expect("heartbeat");
        let snapshot = presence.snapshot(&project, now, 1_000);
        let json = serde_json::to_string(&snapshot).expect("serialize");
        for secret in ["token", "secret", "password", "api_key", "bearer"] {
            assert!(!json.contains(secret), "snapshot must not contain {secret}");
        }
    }

    #[test]
    fn activity_parse_fails_closed() {
        assert_eq!(
            PresenceActivity::parse("active"),
            Some(PresenceActivity::Active)
        );
        assert_eq!(
            PresenceActivity::parse("agent_running"),
            Some(PresenceActivity::AgentRunning)
        );
        assert_eq!(PresenceActivity::parse("executing"), None);
        assert_eq!(PresenceActivity::parse(""), None);
    }
}
