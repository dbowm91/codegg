//! Daemon-owned runtime-asset refresh and publication.
//!
//! Refresh is deliberately separate from snapshot construction.  Builders
//! may read the workspace and resolve every asset outside the publication
//! lock; only a validated candidate is assigned a generation and swapped into
//! the scope's immutable publication state.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;

use super::asset_context::{AssetContext, ProjectIdSource};
use super::asset_snapshot::{ProjectAssetSnapshot, SnapshotBuildError, SnapshotBuilder};

const MAX_REPORT_ENTRIES: usize = 64;
const MAX_DIAGNOSTICS: usize = 32;
const MAX_REPORT_TEXT: usize = 160;

/// Stable daemon scope for one project/workspace asset publication stream.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct AssetScope {
    pub project_id: String,
    pub workspace_id: String,
}

impl AssetScope {
    pub fn new(project_id: impl Into<String>, workspace_id: impl Into<String>) -> Self {
        Self {
            project_id: project_id.into(),
            workspace_id: workspace_id.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshReason {
    Startup,
    ProjectActivation,
    SessionLifecycle,
    Manual,
    Reload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshOutcome {
    Published,
    Retained,
    Cancelled,
    Invalid,
    Coalesced,
}

/// Bounded operator-facing result. It contains names, digests, and
/// diagnostics only; snapshot bodies and absolute paths are never returned.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshReport {
    pub scope: AssetScope,
    pub reason: RefreshReason,
    pub outcome: RefreshOutcome,
    pub generation: Option<u64>,
    pub previous_generation: Option<u64>,
    pub fingerprint: Option<String>,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub changed: Vec<String>,
    pub shadowed: Vec<String>,
    pub invalid: Vec<String>,
    pub retained: Vec<String>,
    pub diagnostics: Vec<String>,
    pub coalesced: bool,
    pub completed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshStatus {
    pub scope: AssetScope,
    pub generation: Option<u64>,
    pub fingerprint: Option<String>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub in_flight: bool,
    pub last_outcome: Option<RefreshOutcome>,
    pub last_diagnostics: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PublishedAssetSnapshot {
    pub generation: u64,
    pub snapshot: Arc<ProjectAssetSnapshot>,
    pub published_at: DateTime<Utc>,
}

impl PublishedAssetSnapshot {
    /// Return the bounded identity captured by a turn or agent run.  The
    /// returned value contains no filesystem paths or asset bodies.
    pub fn runtime_asset_pin(&self) -> super::asset_snapshot::RuntimeAssetPin {
        self.snapshot.runtime_asset_pin(self.generation)
    }
}

struct ScopeState {
    publication: RwLock<Option<Arc<PublishedAssetSnapshot>>>,
    last_generation: AtomicU64,
    last_fingerprint: RwLock<Option<String>>,
    refresh_lock: Mutex<()>,
    last_report: RwLock<Option<RefreshReport>>,
}

impl ScopeState {
    fn new() -> Self {
        Self {
            publication: RwLock::new(None),
            last_generation: AtomicU64::new(0),
            last_fingerprint: RwLock::new(None),
            refresh_lock: Mutex::new(()),
            last_report: RwLock::new(None),
        }
    }
}

/// Per-daemon coordinator. The map isolates projects while each scope's
/// single-flight lock coalesces duplicate lifecycle/manual requests.
pub struct AssetRefreshCoordinator {
    builder: Arc<dyn SnapshotBuilder>,
    scopes: DashMap<AssetScope, Arc<ScopeState>>,
}

impl std::fmt::Debug for AssetRefreshCoordinator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AssetRefreshCoordinator")
            .field("scope_count", &self.scopes.len())
            .finish()
    }
}

impl AssetRefreshCoordinator {
    pub fn new(builder: Arc<dyn SnapshotBuilder>) -> Self {
        Self {
            builder,
            scopes: DashMap::new(),
        }
    }

    fn state(&self, scope: &AssetScope) -> Arc<ScopeState> {
        self.scopes
            .entry(scope.clone())
            .or_insert_with(|| Arc::new(ScopeState::new()))
            .clone()
    }

    /// Refresh without a caller-owned cancellation token.
    pub async fn refresh(
        &self,
        scope: AssetScope,
        context: AssetContext,
        reason: RefreshReason,
    ) -> RefreshReport {
        self.refresh_with_cancellation(scope, context, reason, CancellationToken::new())
            .await
    }

    /// Refresh a scope. A waiter that arrives while another request owns the
    /// lock returns the first request's result as `Coalesced`; it never builds
    /// a second candidate or publishes a duplicate generation.
    pub async fn refresh_with_cancellation(
        &self,
        scope: AssetScope,
        context: AssetContext,
        reason: RefreshReason,
        cancellation: CancellationToken,
    ) -> RefreshReport {
        let state = self.state(&scope);
        let guard = match state.refresh_lock.try_lock() {
            Ok(guard) => guard,
            Err(_) => {
                let guard = state.refresh_lock.lock().await;
                drop(guard);
                let mut report = state
                    .last_report
                    .read()
                    .await
                    .clone()
                    .unwrap_or_else(|| empty_report(scope.clone(), reason));
                report.outcome = RefreshOutcome::Coalesced;
                report.coalesced = true;
                report.reason = reason;
                return report;
            }
        };

        let previous = state.publication.read().await.clone();
        let previous_generation = match state.last_generation.load(Ordering::Acquire) {
            0 => previous.as_ref().map(|p| p.generation),
            generation => Some(generation),
        };

        let report = if cancellation.is_cancelled() {
            retained_report(
                &scope,
                reason,
                previous.as_ref(),
                RefreshOutcome::Cancelled,
                Some("refresh cancelled before build".to_string()),
            )
        } else if context.project_id_source() != ProjectIdSource::Authoritative {
            retained_report(
                &scope,
                reason,
                previous.as_ref(),
                RefreshOutcome::Invalid,
                Some("daemon refresh requires an authoritative project id".to_string()),
            )
        } else if !context.workspace_root().is_dir() {
            retained_report(
                &scope,
                reason,
                previous.as_ref(),
                RefreshOutcome::Invalid,
                Some("workspace root is not an accessible directory".to_string()),
            )
        } else {
            // The builder is intentionally outside the publication lock. It
            // may perform bounded filesystem discovery and can fail without
            // disturbing the previous immutable publication.
            match self.builder.build(&context) {
                Ok(candidate) if !cancellation.is_cancelled() => {
                    let candidate = Arc::new(candidate);
                    let generation = previous_generation.unwrap_or(0) + 1;
                    let published_at = Utc::now();
                    let diff = diff_snapshots(previous.as_ref(), &candidate);
                    let published = Arc::new(PublishedAssetSnapshot {
                        generation,
                        snapshot: candidate.clone(),
                        published_at,
                    });
                    *state.publication.write().await = Some(published);
                    state.last_generation.store(generation, Ordering::Release);
                    *state.last_fingerprint.write().await = Some(candidate.fingerprint.clone());
                    RefreshReport {
                        scope: scope.clone(),
                        reason,
                        outcome: RefreshOutcome::Published,
                        generation: Some(generation),
                        previous_generation,
                        fingerprint: Some(candidate.fingerprint.clone()),
                        added: diff.added,
                        removed: diff.removed,
                        changed: diff.changed,
                        shadowed: diff.shadowed,
                        invalid: diff.invalid,
                        retained: Vec::new(),
                        diagnostics: diff.diagnostics,
                        coalesced: false,
                        completed_at: published_at,
                    }
                }
                Ok(_) => retained_report(
                    &scope,
                    reason,
                    previous.as_ref(),
                    RefreshOutcome::Cancelled,
                    Some("refresh cancelled before publication".to_string()),
                ),
                Err(error) => retained_report(
                    &scope,
                    reason,
                    previous.as_ref(),
                    RefreshOutcome::Retained,
                    Some(error.to_string()),
                ),
            }
        };

        *state.last_report.write().await = Some(report.clone());
        drop(guard);
        report
    }

    pub async fn snapshot(&self, scope: &AssetScope) -> Option<Arc<PublishedAssetSnapshot>> {
        self.state(scope).publication.read().await.clone()
    }

    /// Restore bounded durable metadata after daemon restart. Snapshot bodies
    /// are still reconstructed from the explicit context; this only prevents
    /// generation reuse and lets operators compare the prior fingerprint.
    pub async fn restore_metadata(
        &self,
        scope: AssetScope,
        generation: u64,
        fingerprint: Option<String>,
    ) {
        let state = self.state(&scope);
        state
            .last_generation
            .fetch_max(generation, Ordering::AcqRel);
        if fingerprint.is_some() {
            *state.last_fingerprint.write().await = fingerprint;
        }
    }

    pub async fn status(&self, scope: &AssetScope) -> RefreshStatus {
        let state = self.state(scope);
        let publication = state.publication.read().await.clone();
        let last_report = state.last_report.read().await.clone();
        RefreshStatus {
            scope: scope.clone(),
            generation: publication.as_ref().map(|p| p.generation).or_else(|| {
                match state.last_generation.load(Ordering::Acquire) {
                    0 => None,
                    generation => Some(generation),
                }
            }),
            fingerprint: match publication.as_ref() {
                Some(published) => Some(published.snapshot.fingerprint.clone()),
                None => state.last_fingerprint.read().await.clone(),
            },
            last_success_at: publication.as_ref().map(|p| p.published_at),
            in_flight: {
                let result = state.refresh_lock.try_lock();
                result.is_err()
            },
            last_outcome: last_report.as_ref().map(|r| r.outcome),
            last_diagnostics: last_report
                .map(|r| bounded_diagnostics(r.diagnostics))
                .unwrap_or_default(),
        }
    }
}

#[derive(Default)]
struct SnapshotDiff {
    added: Vec<String>,
    removed: Vec<String>,
    changed: Vec<String>,
    shadowed: Vec<String>,
    invalid: Vec<String>,
    diagnostics: Vec<String>,
}

fn diff_snapshots(
    previous: Option<&Arc<PublishedAssetSnapshot>>,
    current: &ProjectAssetSnapshot,
) -> SnapshotDiff {
    let mut diff = SnapshotDiff::default();
    let Some(previous) = previous else {
        diff.added
            .extend(current.agents.keys().map(|n| format!("agent:{n}")));
        diff.added.extend(
            current
                .skills
                .effective
                .iter()
                .map(|s| format!("skill:{}", s.normalized_name)),
        );
        diff.added.extend(
            current
                .instructions
                .iter()
                .map(|i| format!("instruction:{}", i.content_digest)),
        );
        return finish_diagnostics(diff, current);
    };

    let old_agents: BTreeMap<_, _> = previous
        .snapshot
        .agents
        .iter()
        .map(|(name, agent)| (name.clone(), agent.content_digest()))
        .collect();
    let new_agents: BTreeMap<_, _> = current
        .agents
        .iter()
        .map(|(name, agent)| (name.clone(), agent.content_digest()))
        .collect();
    compare_maps(&old_agents, &new_agents, "agent", &mut diff);

    let old_skills: BTreeMap<_, _> = previous
        .snapshot
        .skills
        .effective
        .iter()
        .map(|skill| (skill.normalized_name.clone(), skill.content_digest.clone()))
        .collect();
    let new_skills: BTreeMap<_, _> = current
        .skills
        .effective
        .iter()
        .map(|skill| (skill.normalized_name.clone(), skill.content_digest.clone()))
        .collect();
    compare_maps(&old_skills, &new_skills, "skill", &mut diff);
    for skill in &current.skills.effective {
        if !skill.shadowed_alternatives.is_empty() {
            diff.shadowed
                .push(format!("skill:{}", skill.normalized_name));
        }
    }

    let old_instructions: BTreeSet<_> = previous
        .snapshot
        .instructions
        .iter()
        .map(|i| i.content_digest.clone())
        .collect();
    let new_instructions: BTreeSet<_> = current
        .instructions
        .iter()
        .map(|i| i.content_digest.clone())
        .collect();
    for digest in new_instructions.difference(&old_instructions) {
        diff.changed.push(format!("instruction:{digest}"));
    }
    for digest in old_instructions.difference(&new_instructions) {
        diff.removed.push(format!("instruction:{digest}"));
    }
    finish_diagnostics(diff, current)
}

fn compare_maps(
    old: &BTreeMap<String, String>,
    new: &BTreeMap<String, String>,
    prefix: &str,
    diff: &mut SnapshotDiff,
) {
    for (name, digest) in new {
        match old.get(name) {
            None => diff.added.push(format!("{prefix}:{name}")),
            Some(previous) if previous != digest => diff.changed.push(format!("{prefix}:{name}")),
            _ => {}
        }
    }
    for name in old.keys().filter(|name| !new.contains_key(*name)) {
        diff.removed.push(format!("{prefix}:{name}"));
    }
}

fn finish_diagnostics(mut diff: SnapshotDiff, snapshot: &ProjectAssetSnapshot) -> SnapshotDiff {
    for diagnostic in &snapshot.agent_diagnostics {
        let text = format!("agent:{}: {}", diagnostic.agent_name, diagnostic.message);
        if matches!(
            diagnostic.severity,
            super::registry::AgentDiagnosticSeverity::Error
        ) {
            diff.invalid.push(bounded_text(text.clone()));
        }
        diff.diagnostics.push(bounded_text(text));
    }
    for diagnostic in &snapshot.skills.diagnostics {
        let text = diagnostic.to_string();
        if diagnostic.severity == super::super::skills::Severity::Error {
            diff.invalid.push(bounded_text(text.clone()));
        }
        diff.diagnostics.push(bounded_text(text));
    }
    for diagnostic in &snapshot.instruction_diagnostics {
        diff.diagnostics
            .push(bounded_text(format!("instruction: {diagnostic:?}")));
    }
    diff.added = bounded_entries(diff.added);
    diff.removed = bounded_entries(diff.removed);
    diff.changed = bounded_entries(diff.changed);
    diff.shadowed = bounded_entries(diff.shadowed);
    diff.invalid = bounded_entries(diff.invalid);
    diff.diagnostics = bounded_diagnostics(diff.diagnostics);
    diff
}

fn empty_report(scope: AssetScope, reason: RefreshReason) -> RefreshReport {
    RefreshReport {
        scope,
        reason,
        outcome: RefreshOutcome::Retained,
        generation: None,
        previous_generation: None,
        fingerprint: None,
        added: Vec::new(),
        removed: Vec::new(),
        changed: Vec::new(),
        shadowed: Vec::new(),
        invalid: Vec::new(),
        retained: Vec::new(),
        diagnostics: vec!["refresh completed without a report".to_string()],
        coalesced: false,
        completed_at: Utc::now(),
    }
}

fn retained_report(
    scope: &AssetScope,
    reason: RefreshReason,
    previous: Option<&Arc<PublishedAssetSnapshot>>,
    outcome: RefreshOutcome,
    failure: Option<String>,
) -> RefreshReport {
    let generation = previous.map(|p| p.generation);
    let mut diagnostics = failure.into_iter().map(bounded_text).collect::<Vec<_>>();
    if let Some(generation) = generation {
        diagnostics.push(format!("retained generation {generation}"));
    }
    RefreshReport {
        scope: scope.clone(),
        reason,
        outcome,
        generation,
        previous_generation: generation,
        fingerprint: previous.map(|p| p.snapshot.fingerprint.clone()),
        added: Vec::new(),
        removed: Vec::new(),
        changed: Vec::new(),
        shadowed: Vec::new(),
        invalid: Vec::new(),
        retained: generation
            .map(|g| vec![format!("generation:{g}")])
            .unwrap_or_default(),
        diagnostics: bounded_diagnostics(diagnostics),
        coalesced: false,
        completed_at: Utc::now(),
    }
}

fn bounded_text(text: impl Into<String>) -> String {
    let text = text.into();
    text.chars().take(MAX_REPORT_TEXT).collect()
}

fn bounded_entries(mut values: Vec<String>) -> Vec<String> {
    values.truncate(MAX_REPORT_ENTRIES);
    values
}

fn bounded_diagnostics(mut values: Vec<String>) -> Vec<String> {
    values.truncate(MAX_DIAGNOSTICS);
    values
}

#[allow(dead_code)]
fn _snapshot_build_error_is_publicly_classified(error: &SnapshotBuildError) -> &'static str {
    match error {
        SnapshotBuildError::Context(_) => "invalid",
        SnapshotBuildError::Agent(_)
        | SnapshotBuildError::Skill(_)
        | SnapshotBuildError::Instruction(_)
        | SnapshotBuildError::MissingAgentDigest(_) => "retained",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::asset_context::{AssetContextBuilder, ProjectId};
    use crate::agent::asset_snapshot_builder::{
        ProjectAssetSnapshotBuilder, SnapshotBuilderConfig,
    };
    use crate::config::schema::Config;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::TempDir;
    use tokio::sync::Notify;

    fn context(root: &Path, project_id: &str) -> AssetContext {
        AssetContextBuilder::new()
            .with_project_id(ProjectId::parse(project_id).unwrap())
            .with_workspace_root(root)
            .build()
            .unwrap()
    }

    fn coordinator() -> AssetRefreshCoordinator {
        let builder = ProjectAssetSnapshotBuilder::new(
            SnapshotBuilderConfig::default(),
            Arc::new(Config::default()),
        );
        AssetRefreshCoordinator::new(Arc::new(builder))
    }

    struct BlockingBuilder {
        inner: ProjectAssetSnapshotBuilder,
        started: Arc<Notify>,
        thread: Arc<std::sync::Mutex<Option<std::thread::Thread>>>,
        calls: Arc<AtomicUsize>,
    }

    impl SnapshotBuilder for BlockingBuilder {
        fn build(
            &self,
            context: &AssetContext,
        ) -> Result<ProjectAssetSnapshot, SnapshotBuildError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            *self.thread.lock().unwrap() = Some(std::thread::current());
            self.started.notify_one();
            std::thread::park();
            self.inner.build(context)
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn publishes_generation_and_pins_previous_snapshot() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".codegg/skills/review")).unwrap();
        std::fs::write(
            tmp.path().join(".codegg/skills/review/SKILL.md"),
            "---\nname: review\ndescription: review files\n---\nReview safely.",
        )
        .unwrap();
        let ctx = context(tmp.path(), "project-1");
        let scope = AssetScope::new("project-1", "workspace-1");
        let service = coordinator();
        let first = service
            .refresh(scope.clone(), ctx.clone(), RefreshReason::Startup)
            .await;
        assert_eq!(first.outcome, RefreshOutcome::Published);
        assert_eq!(first.generation, Some(1));
        let pinned = service.snapshot(&scope).await.unwrap();
        let mut pin = pinned.runtime_asset_pin();
        assert_eq!(pin.generation, 1);
        assert_eq!(pin.fingerprint, pinned.snapshot.fingerprint);
        assert_eq!(pin.record_skill_activation("review").unwrap().len(), 64);
        assert!(pin.activated_skill_digest("review").is_some());
        std::fs::write(tmp.path().join("AGENTS.md"), "new instructions").unwrap();
        let second = service
            .refresh(scope.clone(), ctx, RefreshReason::Manual)
            .await;
        let second_fingerprint = second.fingerprint.clone().unwrap();
        assert_eq!(second.generation, Some(2));
        assert_eq!(pinned.generation, 1);
        assert_ne!(pinned.snapshot.fingerprint, second_fingerprint);
        assert_eq!(pin.generation, 1);
        assert_ne!(pin.fingerprint, second_fingerprint);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn invalid_context_retains_previous_generation() {
        let tmp = TempDir::new().unwrap();
        let service = coordinator();
        let scope = AssetScope::new("project-1", "workspace-1");
        let ctx = context(tmp.path(), "project-1");
        let first = service
            .refresh(scope.clone(), ctx.clone(), RefreshReason::Startup)
            .await;
        assert_eq!(first.generation, Some(1));
        let invalid = AssetContextBuilder::new()
            .with_synthetic_project_id(ProjectId::new())
            .with_workspace_root(tmp.path())
            .build()
            .unwrap();
        let retained = service.refresh(scope, invalid, RefreshReason::Manual).await;
        assert_eq!(retained.outcome, RefreshOutcome::Invalid);
        assert_eq!(retained.generation, Some(1));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn different_scopes_are_isolated() {
        let a = TempDir::new().unwrap();
        let b = TempDir::new().unwrap();
        std::fs::write(a.path().join("AGENTS.md"), "a").unwrap();
        std::fs::write(b.path().join("AGENTS.md"), "b").unwrap();
        let service = coordinator();
        let (left, right) = tokio::join!(
            service.refresh(
                AssetScope::new("p-a", "w-a"),
                context(a.path(), "p-a"),
                RefreshReason::ProjectActivation,
            ),
            service.refresh(
                AssetScope::new("p-b", "w-b"),
                context(b.path(), "p-b"),
                RefreshReason::ProjectActivation,
            )
        );
        assert_eq!(left.generation, Some(1));
        assert_eq!(right.generation, Some(1));
        assert_ne!(left.fingerprint, right.fingerprint);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cancelled_refresh_retains_the_last_valid_generation() {
        let tmp = TempDir::new().unwrap();
        let service = coordinator();
        let scope = AssetScope::new("project-1", "workspace-1");
        let ctx = context(tmp.path(), "project-1");
        let first = service
            .refresh(scope.clone(), ctx.clone(), RefreshReason::Startup)
            .await;
        assert_eq!(first.generation, Some(1));

        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let cancelled = service
            .refresh_with_cancellation(scope, ctx, RefreshReason::Manual, cancellation)
            .await;
        assert_eq!(cancelled.outcome, RefreshOutcome::Cancelled);
        assert_eq!(cancelled.generation, Some(1));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn restored_metadata_prevents_generation_reuse() {
        let tmp = TempDir::new().unwrap();
        let scope = AssetScope::new("project-1", "workspace-1");
        let ctx = context(tmp.path(), "project-1");
        let restored = coordinator();
        restored
            .restore_metadata(scope.clone(), 7, Some("prior-fingerprint".to_string()))
            .await;
        let status = restored.status(&scope).await;
        assert_eq!(status.generation, Some(7));
        assert_eq!(status.fingerprint.as_deref(), Some("prior-fingerprint"));

        let reference = coordinator()
            .refresh(scope.clone(), ctx.clone(), RefreshReason::Startup)
            .await;
        let report = restored.refresh(scope, ctx, RefreshReason::Startup).await;
        assert_eq!(report.generation, Some(8));
        assert_eq!(report.fingerprint, reference.fingerprint);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn same_scope_requests_coalesce_to_one_publication() {
        let tmp = TempDir::new().unwrap();
        let builder = BlockingBuilder {
            inner: ProjectAssetSnapshotBuilder::new(
                SnapshotBuilderConfig::default(),
                Arc::new(Config::default()),
            ),
            started: Arc::new(Notify::new()),
            thread: Arc::new(std::sync::Mutex::new(None)),
            calls: Arc::new(AtomicUsize::new(0)),
        };
        let started = builder.started.clone();
        let thread = builder.thread.clone();
        let calls = builder.calls.clone();
        let service = Arc::new(AssetRefreshCoordinator::new(Arc::new(builder)));
        let scope = AssetScope::new("project-1", "workspace-1");
        let ctx = context(tmp.path(), "project-1");

        let first_service = service.clone();
        let first_scope = scope.clone();
        let first_ctx = ctx.clone();
        let first = tokio::spawn(async move {
            first_service
                .refresh(first_scope, first_ctx, RefreshReason::Startup)
                .await
        });
        started.notified().await;

        let second_service = service.clone();
        let second_scope = scope.clone();
        let second_ctx = ctx.clone();
        let second = tokio::spawn(async move {
            second_service
                .refresh(second_scope, second_ctx, RefreshReason::Manual)
                .await
        });
        for _ in 0..20 {
            if service.status(&scope).await.in_flight {
                break;
            }
            tokio::task::yield_now().await;
        }
        thread.lock().unwrap().take().unwrap().unpark();

        let (first, second) = tokio::join!(first, second);
        let first = first.unwrap();
        let second = second.unwrap();
        assert_eq!(first.outcome, RefreshOutcome::Published);
        assert_eq!(second.outcome, RefreshOutcome::Coalesced);
        assert_eq!(first.generation, Some(1));
        assert_eq!(second.generation, Some(1));
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }
}
