//! Project Work Orders M004: bounded global Workspace dashboard projection.
//!
//! The dashboard is a daemon-owned aggregate over canonical stores. The TUI
//! issues exactly one [`CoreRequest::WorkspaceDashboard`] request per
//! refresh; it MUST NOT fan out per-project `WorkOrderList`/`SessionList`
//! calls (N+1). Rows are coarse counts plus a closed-set status code: no
//! prompt, command argument, filesystem path, trigger secret, provider
//! secret, diff body, or hidden reasoning.
//!
//! Deliberate non-activation: this handler touches only
//!
//! - the probe-free `ProjectCatalog::list_projects` listing,
//! - cheap `COUNT(*)`/`MAX(updated_at)` aggregates over the durable
//!   `work_order` / `work_order_occurrence` tables,
//! - the in-memory [`SessionRuntimeRegistry`] (no session creation, no
//!   workspace resolution),
//! - the team membership store for the explicit per-row `session.read`
//!   counts decision.
//!
//! It never initializes LSP, Git, provider, build, or workspace services
//! for any project (active or inactive). `project.read` gates row
//! visibility (enumeration filtering, same as `ProjectList`);
//! `session.read` gates per-row counts. Callers with `project.read` but
//! without `session.read` see project presence (already visible via
//! `ProjectList`) with `counts_visible == false` and zeroed counts, so
//! row presence reveals nothing beyond `ProjectList`.
//!
//! Ordering is deterministic: attention (needs-attention/failed +
//! pending permission/question) descending, then running activity
//! (sessions + running occurrences) descending, then `last_activity_at`
//! descending, then `display_name` ascending, then `project_id`
//! ascending. Cursor pagination slices that order after the last
//! `project_id` of the previous page.

use std::collections::{HashMap, HashSet};

use codegg_core::team::{Capability, TeamStore};
use codegg_protocol::work_order::{
    ProjectActivitySummaryDto, DEFAULT_WORKSPACE_DASHBOARD_LIMIT, MAX_WORKSPACE_DASHBOARD_LIMIT,
};

use crate::error::AppError;
use crate::protocol::core::CoreResponse;

use super::daemon::CoreDaemon;

/// Per-project live-session aggregate scanned once from the in-memory
/// runtime registry (no activation, no workspace resolution).
#[derive(Debug, Default, Clone, Copy)]
struct SessionAgg {
    running: u64,
    pending_permission: u64,
    pending_question: u64,
}

impl CoreDaemon {
    /// Serve one bounded global-dashboard page.
    ///
    /// Enumeration authorization (`project.read`) is enforced by the
    /// dispatch preamble; this handler additionally privacy-filters the
    /// catalog listing to the caller's visible projects and zeroes
    /// per-row counts without `session.read`.
    pub(crate) async fn handle_workspace_dashboard_request(
        &self,
        cursor: Option<String>,
        limit: Option<u32>,
        include_archived: bool,
        trusted_client_id: &str,
    ) -> Result<CoreResponse, AppError> {
        let limit = limit
            .unwrap_or(DEFAULT_WORKSPACE_DASHBOARD_LIMIT)
            .clamp(1, MAX_WORKSPACE_DASHBOARD_LIMIT) as usize;
        let Some(pool) = self.pool.clone() else {
            return Ok(CoreResponse::Error {
                code: "workspace_dashboard_unavailable".into(),
                message: "global workspace dashboard requires a durable database pool".into(),
            });
        };
        let catalog = codegg_core::project_catalog::ProjectCatalog::new(pool.clone());
        let records = match catalog.list_projects(include_archived).await {
            Ok(records) => records,
            Err(error) => {
                return Ok(CoreResponse::Error {
                    code: "workspace_dashboard_list_failed".into(),
                    message: format!("workspace dashboard listing failed: {error}"),
                });
            }
        };
        // Enumeration privacy: only projects visible to the bound
        // principal (same filter as `ProjectList`).
        let records = self
            .filter_projects_for_principal(trusted_client_id, records)
            .await;

        let authority = self.request_authority_for_client(trusted_client_id);
        let principal = authority.principal().clone();
        let local_owner = codegg_core::authorization::is_local_owner_broad(authority.principal());
        let team = TeamStore::new(pool.clone());

        // One scan of the live-session registry for all visible
        // projects (no per-project fan-out, no activation).
        let visible_ids: HashSet<&str> = records
            .iter()
            .map(|record| record.project_id.as_str())
            .collect();
        let session_aggs = self.session_aggs_for_projects(&visible_ids).await;

        let mut rows = Vec::with_capacity(records.len());
        for record in &records {
            let project_str = record.project_id.as_str();
            let counts_visible = if local_owner {
                true
            } else {
                team.has_capability(
                    &record.project_id,
                    principal.principal_id(),
                    Capability::SessionRead,
                )
                .await
                .unwrap_or(false)
            };
            let lifecycle = match record.lifecycle {
                codegg_core::project_storage::ProjectLifecycle::Active => "active",
                codegg_core::project_storage::ProjectLifecycle::Archived => "archived",
            }
            .to_string();
            let display_name =
                codegg_core::protocol_conversions::project_catalog_record_to_dto(record)
                    .display_name;
            let catalog_activity = record.updated_at.timestamp_millis();
            if !counts_visible {
                let mut row = ProjectActivitySummaryDto {
                    project_id: project_str.to_owned(),
                    display_name,
                    lifecycle: lifecycle.clone(),
                    running_session_count: 0,
                    running_work_order_count: 0,
                    waiting_work_order_count: 0,
                    future_work_order_count: 0,
                    needs_attention_count: 0,
                    pending_permission_count: 0,
                    pending_question_count: 0,
                    last_activity_at: Some(catalog_activity),
                    coarse_status_code: if lifecycle == "archived" {
                        "archived".to_owned()
                    } else {
                        "idle".to_owned()
                    },
                    counts_visible: false,
                };
                // Presence-only rows must never leak hidden activity
                // through ordering inputs either; the zeroed shape is
                // canonical (also enforced by
                // `apply_counts_visibility`, unit-pinned below).
                row = apply_counts_visibility(row, false);
                rows.push(row);
                continue;
            }
            let counts = dashboard_counts(&pool, project_str).await;
            let agg = session_aggs.get(project_str).copied().unwrap_or_default();
            let last_activity_at = counts
                .max_updated_at
                .max(Some(catalog_activity))
                .or(Some(catalog_activity));
            let coarse_status_code = status_code(
                &lifecycle,
                &counts,
                agg.running,
                agg.pending_permission,
                agg.pending_question,
            );
            rows.push(ProjectActivitySummaryDto {
                project_id: project_str.to_owned(),
                display_name,
                lifecycle,
                running_session_count: agg.running,
                running_work_order_count: counts.running_occurrences,
                waiting_work_order_count: counts.waiting_occurrences,
                future_work_order_count: counts.future_templates,
                needs_attention_count: counts.needs_attention(),
                pending_permission_count: agg.pending_permission,
                pending_question_count: agg.pending_question,
                last_activity_at,
                coarse_status_code,
                counts_visible: true,
            });
        }

        // Deterministic dashboard order: attention, running activity,
        // recency, name, id.
        rows.sort_by(|a, b| {
            let attention_a =
                a.needs_attention_count + a.pending_permission_count + a.pending_question_count;
            let attention_b =
                b.needs_attention_count + b.pending_permission_count + b.pending_question_count;
            attention_b
                .cmp(&attention_a)
                .then(
                    (b.running_session_count + b.running_work_order_count)
                        .cmp(&(a.running_session_count + a.running_work_order_count)),
                )
                .then(b.last_activity_at.cmp(&a.last_activity_at))
                .then(a.display_name.cmp(&b.display_name))
                .then(a.project_id.cmp(&b.project_id))
        });

        let start = match cursor.as_deref() {
            None => 0,
            Some(id) => rows
                .iter()
                .position(|row| row.project_id == id)
                .map(|pos| pos + 1)
                .unwrap_or(0),
        };
        let end = (start + limit).min(rows.len());
        let page = rows[start..end].to_vec();
        let truncated = end < rows.len();
        let next_cursor = if truncated {
            page.last().map(|row| row.project_id.clone())
        } else {
            None
        };
        Ok(CoreResponse::WorkspaceDashboard {
            rows: page,
            next_cursor,
            truncated,
        })
    }

    /// Scan the in-memory session registry once, aggregating live counts
    /// for `visible` projects. Reads runtime snapshots only; creates no
    /// sessions, resolves no workspaces, probes no services.
    async fn session_aggs_for_projects(
        &self,
        visible: &HashSet<&str>,
    ) -> HashMap<String, SessionAgg> {
        let mut out: HashMap<String, SessionAgg> = HashMap::new();
        if visible.is_empty() {
            return out;
        }
        for session_id in self.sessions.list_sessions() {
            let Some(runtime) = self.sessions.get(&session_id) else {
                continue;
            };
            if !visible.contains(runtime.project_id.as_str()) {
                continue;
            }
            let snapshot = runtime.snapshot().await;
            let running = snapshot.has_active_turn
                || snapshot.status == crate::core::session_runtime::RuntimeSessionStatus::Running;
            let entry = out.entry(runtime.project_id.clone()).or_default();
            if running {
                entry.running = entry.running.saturating_add(1);
            }
            entry.pending_permission = entry
                .pending_permission
                .saturating_add(snapshot.pending_permissions.len() as u64);
            entry.pending_question = entry
                .pending_question
                .saturating_add(snapshot.pending_questions.len() as u64);
        }
        out
    }
}

/// Cheap durable aggregates for one project (indexed `COUNT(*)` /
/// `MAX(updated_at)` only; no row bodies, no joins, no activation).
#[derive(Debug, Default)]
struct DashboardCounts {
    running_occurrences: u64,
    waiting_occurrences: u64,
    /// Occurrences in `needs_attention` (human-actionable attention).
    attention_occurrences: u64,
    /// Occurrences in `failed` (held sequence; also demands attention
    /// and is folded into the row's `needs_attention_count`).
    failed_occurrences: u64,
    future_templates: u64,
    max_updated_at: Option<i64>,
}

impl DashboardCounts {
    fn needs_attention(&self) -> u64 {
        self.attention_occurrences
            .saturating_add(self.failed_occurrences)
    }
}

async fn dashboard_counts(pool: &sqlx::SqlitePool, project_id: &str) -> DashboardCounts {
    let mut counts = DashboardCounts::default();
    let states: Vec<(String, i64)> = sqlx::query_as(
        "SELECT state, COUNT(*) FROM work_order_occurrence WHERE project_id = ? GROUP BY state",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    for (state, count) in states {
        let count = u64::try_from(count).unwrap_or(0);
        match state.as_str() {
            "claiming" | "running" => {
                counts.running_occurrences = counts.running_occurrences.saturating_add(count);
            }
            "waiting" | "ready" => {
                counts.waiting_occurrences = counts.waiting_occurrences.saturating_add(count);
            }
            "needs_attention" => {
                counts.attention_occurrences = counts.attention_occurrences.saturating_add(count);
            }
            "failed" => {
                counts.failed_occurrences = counts.failed_occurrences.saturating_add(count);
            }
            _ => {}
        }
    }
    let templates: Vec<(String, i64)> = sqlx::query_as(
        "SELECT state, COUNT(*) FROM work_order WHERE project_id = ? GROUP BY state",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await
    .unwrap_or_default();
    for (state, count) in templates {
        let count = u64::try_from(count).unwrap_or(0);
        if state == "active" || state == "paused" {
            counts.future_templates = counts.future_templates.saturating_add(count);
        }
    }
    let max_work_order: (Option<i64>,) =
        sqlx::query_as("SELECT MAX(updated_at) FROM work_order WHERE project_id = ?")
            .bind(project_id)
            .fetch_one(pool)
            .await
            .unwrap_or((None,));
    let max_occurrence: (Option<i64>,) =
        sqlx::query_as("SELECT MAX(updated_at) FROM work_order_occurrence WHERE project_id = ?")
            .bind(project_id)
            .fetch_one(pool)
            .await
            .unwrap_or((None,));
    counts.max_updated_at = [max_work_order.0, max_occurrence.0]
        .into_iter()
        .flatten()
        .max();
    counts
}

/// Enforce the explicit Viewer/presence privacy decision on one row:
/// callers with `project.read` but without `session.read` keep project
/// presence (already visible via `ProjectList`) with zeroed counts and
/// a leak-free status code. Pure so the redaction is unit-pinned.
fn apply_counts_visibility(
    mut row: ProjectActivitySummaryDto,
    counts_visible: bool,
) -> ProjectActivitySummaryDto {
    if counts_visible {
        row.counts_visible = true;
        return row;
    }
    row.running_session_count = 0;
    row.running_work_order_count = 0;
    row.waiting_work_order_count = 0;
    row.future_work_order_count = 0;
    row.needs_attention_count = 0;
    row.pending_permission_count = 0;
    row.pending_question_count = 0;
    row.coarse_status_code = if row.lifecycle == "archived" {
        "archived".to_owned()
    } else {
        "idle".to_owned()
    };
    row.counts_visible = false;
    row
}

/// Closed coarse status vocabulary for one row. `permission`/`question`/// reflect live pending requests (counts only); `attention` covers
/// `needs_attention` occurrences; `failed` covers held/failed sequences
/// with no live `needs_attention` occurrence (still folded into the
/// row's `needs_attention_count` badge).
fn status_code(
    lifecycle: &str,
    counts: &DashboardCounts,
    running_sessions: u64,
    pending_permission: u64,
    pending_question: u64,
) -> String {
    if lifecycle == "archived" {
        return "archived".to_owned();
    }
    if pending_permission > 0 {
        return "permission".to_owned();
    }
    if pending_question > 0 {
        return "question".to_owned();
    }
    if counts.attention_occurrences > 0 {
        return "attention".to_owned();
    }
    if counts.failed_occurrences > 0 {
        return "failed".to_owned();
    }
    if counts.running_occurrences > 0 || running_sessions > 0 {
        return "running".to_owned();
    }
    if counts.waiting_occurrences > 0 || counts.future_templates > 0 {
        return "waiting".to_owned();
    }
    "idle".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full_row() -> ProjectActivitySummaryDto {
        ProjectActivitySummaryDto {
            project_id: "project-1".to_string(),
            display_name: "Alpha".to_string(),
            lifecycle: "active".to_string(),
            running_session_count: 2,
            running_work_order_count: 1,
            waiting_work_order_count: 3,
            future_work_order_count: 4,
            needs_attention_count: 1,
            pending_permission_count: 1,
            pending_question_count: 1,
            last_activity_at: Some(123),
            coarse_status_code: "permission".to_string(),
            counts_visible: true,
        }
    }

    #[test]
    fn presence_only_row_zeroes_counts_and_status() {
        let row = apply_counts_visibility(full_row(), false);
        assert!(!row.counts_visible);
        assert_eq!(row.running_session_count, 0);
        assert_eq!(row.running_work_order_count, 0);
        assert_eq!(row.waiting_work_order_count, 0);
        assert_eq!(row.future_work_order_count, 0);
        assert_eq!(row.needs_attention_count, 0);
        assert_eq!(row.pending_permission_count, 0);
        assert_eq!(row.pending_question_count, 0);
        assert_eq!(row.coarse_status_code, "idle");
        // Presence survives: identity a `ProjectList` caller already
        // holds, plus catalog recency (no existence oracle beyond it).
        assert_eq!(row.project_id, "project-1");
        assert_eq!(row.display_name, "Alpha");
        assert!(row.last_activity_at.is_some());
        assert!(row.is_redacted());
    }

    #[test]
    fn presence_only_archived_row_reports_archived() {
        let mut row = full_row();
        row.lifecycle = "archived".to_string();
        let row = apply_counts_visibility(row, false);
        assert_eq!(row.coarse_status_code, "archived");
    }

    #[test]
    fn visible_row_passes_through() {
        let row = apply_counts_visibility(full_row(), true);
        assert!(row.counts_visible);
        assert_eq!(row.running_session_count, 2);
        assert_eq!(row.coarse_status_code, "permission");
    }

    #[test]
    fn status_code_precedence_is_closed_vocabulary() {
        use codegg_protocol::work_order::WORKSPACE_DASHBOARD_STATUS_CODES;
        let mut counts = DashboardCounts::default();
        // Idle baseline.
        assert_eq!(status_code("active", &counts, 0, 0, 0), "idle");
        counts.waiting_occurrences = 1;
        assert_eq!(status_code("active", &counts, 0, 0, 0), "waiting");
        counts.running_occurrences = 1;
        assert_eq!(status_code("active", &counts, 0, 0, 0), "running");
        // Live sessions alone (no occurrences yet) also report running.
        counts.running_occurrences = 0;
        assert_eq!(status_code("active", &counts, 1, 0, 0), "running");
        counts.failed_occurrences = 1;
        assert_eq!(status_code("active", &counts, 0, 0, 0), "failed");
        counts.attention_occurrences = 1;
        assert_eq!(status_code("active", &counts, 0, 0, 0), "attention");
        assert_eq!(status_code("active", &counts, 0, 0, 1), "question");
        assert_eq!(status_code("active", &counts, 0, 1, 1), "permission");
        assert_eq!(status_code("archived", &counts, 0, 1, 1), "archived");
        for code in [
            "idle",
            "waiting",
            "running",
            "failed",
            "attention",
            "question",
            "permission",
            "archived",
        ] {
            assert!(
                WORKSPACE_DASHBOARD_STATUS_CODES.contains(&code),
                "{code} must be closed vocabulary"
            );
        }
    }

    #[test]
    fn dashboard_projection_touches_no_heavy_activation() {
        // Static ownership evidence for the plan's "no eager
        // activation" requirement: the handler file must not name
        // project-activation or heavy service constructors. The
        // catalog listing stays probe-free and session data comes
        // from the in-memory registry only.
        let source = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/src/core/daemon_workspace_dashboard.rs"
        ));
        // Scan production code only: this test module necessarily names
        // the forbidden identifiers in its own assertion list.
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        for forbidden in [
            "LspService",
            "GitEnvPolicy",
            "WorkspaceServiceRegistry",
            "workspace_services",
            "project_activation",
            "ProjectActivationRegistry",
            "EggpoolProvisioner",
            "provider_connection",
            "build_service",
            "project_health(",
        ] {
            assert!(
                !production.contains(forbidden),
                "dashboard projection must not touch {forbidden}"
            );
        }
        for required in [
            "filter_projects_for_principal",
            "SessionRead",
            "MAX_WORKSPACE_DASHBOARD_LIMIT",
        ] {
            assert!(
                production.contains(required),
                "dashboard projection must keep {required}"
            );
        }
    }
}
