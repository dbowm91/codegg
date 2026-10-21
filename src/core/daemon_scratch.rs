//! M006: the daemon-held scratch usage census.
//!
//! The daemon measures the per-user scratch tree and keeps the last
//! completed report in memory. The report is refreshed on an internal
//! cadence, **off the hot path**: no tool call, permission check, turn
//! admission, or scheduler decision walks the tree.
//!
//! The census is read-only. It never creates, renames, unlinks, chmods,
//! truncates, or opens a file for writing, and it never creates a tombstone.
//! Nothing here reclaims anything — see
//! `plans/implementation/managed-agent-scratch/006-scratch-usage-census-and-operator-visibility.md`.
//!
//! Ownership:
//!
//! - `src/scratch.rs` owns the tree, its identity rules, the reclaim
//!   validator, and the census walker itself.
//! - This module owns the daemon-held state, the refresh cadence, and the
//!   request handler that serves the report.

use std::sync::Arc;

use crate::error::AppError;
use crate::protocol::core::{CoreRequest, CoreResponse};
use crate::protocol::dto::{
    ScratchCensusBudgetDto, ScratchReclaimRefusalDto, ScratchTreeUsageDto, ScratchUsageReportDto,
};
use crate::scratch::{
    self, ReclaimRefusal, ScratchCensusBudget, ScratchTreeUsage, ScratchUsageReport,
};

use super::daemon::CoreDaemon;

/// How often the daemon refreshes the scratch census.
///
/// The refresh is a metadata-only walk over a tree that a model may be
/// writing to, so it runs on its own cadence rather than on any request
/// path. The first tick fires immediately, so a fresh daemon has a report
/// within milliseconds of construction.
pub const SCRATCH_USAGE_REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

/// The daemon-held scratch census state.
///
/// The last successful report is kept when a later walk fails: the operator
/// sees the previous measurement *and* its age, rather than an empty report
/// that looks like "no scratch".
#[derive(Debug, Default)]
pub struct ScratchUsageState {
    /// The last completed report. `None` until the first walk succeeds.
    pub report: Option<ScratchUsageReport>,
    /// The last walk failure. Does not clear `report`.
    pub last_error: Option<String>,
    /// When the last walk attempt finished, in epoch milliseconds.
    pub last_attempt_ms: Option<i64>,
}

impl ScratchUsageState {
    /// The age of the held report, in milliseconds.
    pub fn report_age_ms(&self, now_ms: i64) -> Option<i64> {
        self.report
            .as_ref()
            .map(|report| now_ms.saturating_sub(report.generated_at_ms))
    }
}

/// The refresh cadence and the walk lock, shared by the periodic refresh and
/// an explicit refresh request.
#[derive(Debug)]
pub struct ScratchUsageRefresher {
    state: Arc<parking_lot::RwLock<ScratchUsageState>>,
    /// Serialises walks so two callers cannot walk the tree concurrently.
    walk_lock: Arc<tokio::sync::Mutex<()>>,
    budget: ScratchCensusBudget,
    /// The sessions root to measure.
    ///
    /// `None` means the daemon's own per-user scratch root. A root is a
    /// construction-time seam, never a client choice: the request carries
    /// no path.
    root: Option<std::path::PathBuf>,
}

impl ScratchUsageRefresher {
    /// Fresh refresher over the given state, measuring the daemon's own
    /// scratch root.
    pub fn new(state: Arc<parking_lot::RwLock<ScratchUsageState>>) -> Self {
        Self {
            state,
            walk_lock: Arc::new(tokio::sync::Mutex::new(())),
            budget: ScratchCensusBudget::default(),
            root: None,
        }
    }

    /// Fresh refresher that measures an explicit sessions root.
    pub fn with_root(
        state: Arc<parking_lot::RwLock<ScratchUsageState>>,
        root: std::path::PathBuf,
    ) -> Self {
        Self {
            state,
            walk_lock: Arc::new(tokio::sync::Mutex::new(())),
            budget: ScratchCensusBudget::default(),
            root: Some(root),
        }
    }

    /// The shared state holder.
    pub fn state(&self) -> &Arc<parking_lot::RwLock<ScratchUsageState>> {
        &self.state
    }

    /// Walk the tree now and publish the result.
    ///
    /// A failed walk keeps the previous report and records the failure: a
    /// transient error must not erase the last good measurement.
    pub async fn refresh(&self) -> Result<ScratchUsageReport, String> {
        // The walk is serialised, so two callers cannot walk the tree at the
        // same time. The lock is held for the walk, not for the request.
        let _guard = self.walk_lock.lock().await;
        let budget = self.budget;
        let root = self.root.clone();
        // The walk is blocking filesystem I/O; it runs on the blocking pool
        // so it never occupies an async worker.
        let outcome = tokio::task::spawn_blocking(move || match root {
            Some(root) => scratch::census_scratch_usage_at(&root, budget),
            None => scratch::census_scratch_usage(budget),
        })
        .await
        .map_err(|error| format!("scratch census task failed: {error}"));
        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut state = self.state.write();
        state.last_attempt_ms = Some(now_ms);
        match outcome {
            Ok(report) => {
                state.last_error = None;
                let report = report.clone();
                state.report = Some(report.clone());
                Ok(report)
            }
            Err(error) => {
                state.last_error = Some(error.clone());
                Err(error)
            }
        }
    }

    /// The last completed report, if any.
    pub fn last_report(&self) -> Option<ScratchUsageReport> {
        self.state.read().report.clone()
    }
}

/// Spawn the periodic refresh task.
///
/// The task is a plain `tokio::spawn` loop: it holds the last report and
/// never blocks a request path. A cancelled walk (daemon shutdown) leaves
/// the previous report in place and is not an error.
pub fn spawn_scratch_usage_refresh(
    refresher: Arc<ScratchUsageRefresher>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(SCRATCH_USAGE_REFRESH_INTERVAL);
        loop {
            interval.tick().await;
            if let Err(error) = refresher.refresh().await {
                tracing::warn!(error = %error, "scratch usage census refresh failed");
            }
        }
    })
}

impl CoreDaemon {
    /// Serve the scratch usage census request.
    ///
    /// The request carries no path, no session identity, and no scope: the
    /// daemon always reports the single scratch root it owns.
    pub(crate) async fn handle_scratch_usage_request(
        &self,
        request: CoreRequest,
    ) -> Result<CoreResponse, AppError> {
        let CoreRequest::ScratchUsage { refresh } = request else {
            return Err(AppError::Other(anyhow::anyhow!(
                "scratch usage handler received a non-scratch request"
            )));
        };
        let refresher = self.scratch_usage_refresher.clone();
        let report = if refresh {
            match refresher.refresh().await {
                Ok(report) => report,
                Err(error) => {
                    // A failed walk keeps the previous report. Serve it with
                    // the failure attached rather than pretending the tree is
                    // empty.
                    let mut report = refresher.last_report().unwrap_or_default();
                    report.anomalies.push(error);
                    report
                }
            }
        } else {
            refresher.last_report().unwrap_or_default()
        };
        Ok(CoreResponse::ScratchUsage {
            report: scratch_usage_report_to_dto(&report),
        })
    }
}

/// Convert the domain census into its wire form.
pub fn scratch_usage_report_to_dto(report: &ScratchUsageReport) -> ScratchUsageReportDto {
    ScratchUsageReportDto {
        trees: report.trees.iter().map(scratch_tree_usage_to_dto).collect(),
        tree_count: report.tree_count,
        total_bytes: report.total_bytes,
        oldest_modified_ms: report.oldest_modified_ms,
        newest_modified_ms: report.newest_modified_ms,
        truncated: report.truncated,
        quarantined_tree_count: report.quarantined_tree_count,
        unrecognized_entry_count: report.unrecognized_entry_count,
        budget: ScratchCensusBudgetDto {
            max_entries: report.budget.max_entries,
            max_depth: report.budget.max_depth,
            max_paths_per_tree: report.budget.max_paths_per_tree,
            max_trees: report.budget.max_trees,
        },
        generated_at_ms: report.generated_at_ms,
        anomalies: report.anomalies.clone(),
    }
}

/// Convert one tree's measured usage into its wire form.
pub fn scratch_tree_usage_to_dto(usage: &ScratchTreeUsage) -> ScratchTreeUsageDto {
    ScratchTreeUsageDto {
        digest: usage.digest.clone(),
        total_bytes: usage.total_bytes,
        file_count: usage.file_count,
        dir_count: usage.dir_count,
        link_count: usage.link_count,
        multiply_linked_count: usage.multiply_linked_count,
        non_regular_count: usage.non_regular_count,
        unreadable_count: usage.unreadable_count,
        vanished_entries: usage.vanished_entries,
        oldest_modified_ms: usage.oldest_modified_ms,
        newest_modified_ms: usage.newest_modified_ms,
        reclaimable: usage.reclaimable,
        refusal_rules: usage
            .refusal_rules
            .iter()
            .copied()
            .map(scratch_refusal_to_dto)
            .collect(),
        refusal_paths: usage.refusal_paths.clone(),
        vanished: usage.vanished,
        truncated: usage.truncated,
    }
}

/// Convert one reclaim refusal into its wire form.
pub fn scratch_refusal_to_dto(refusal: ReclaimRefusal) -> ScratchReclaimRefusalDto {
    match refusal {
        ReclaimRefusal::Link => ScratchReclaimRefusalDto::Link,
        ReclaimRefusal::NotRegular => ScratchReclaimRefusalDto::NotRegular,
        ReclaimRefusal::MultiplyLinked => ScratchReclaimRefusalDto::MultiplyLinked,
        ReclaimRefusal::Unreadable => ScratchReclaimRefusalDto::Unreadable,
    }
}

/// How many digest characters the human table shows.
///
/// The digest is opaque; a prefix is enough to tell two trees apart in a
/// listing, and `--json` carries the full value for anything that needs to
/// name a tree exactly.
const DIGEST_DISPLAY_CHARS: usize = 16;

/// Render the census as the operator-facing table.
///
/// The report is a measurement of the local user's own scratch tree. It
/// shows sizes, counts, ages, and the reclaim verdict; it never shows file
/// content, an absolute host path, or a session identity.
pub fn format_scratch_usage_report(report: &ScratchUsageReportDto, now_ms: i64) -> String {
    let mut out = String::new();
    let age = now_ms.saturating_sub(report.generated_at_ms);
    out.push_str(&format!(
        "Scratch usage — {} tree(s), {}, measured {}\n",
        report.tree_count,
        crate::shell::projector::format_bytes(report.total_bytes),
        format_age(age),
    ));
    if report.trees.is_empty() {
        out.push_str("\n  No scratch trees are present.\n");
    } else {
        out.push('\n');
        out.push_str("  DIGEST                SIZE        FILES  DIRS  LINKS  MLINKS  RECLAIM\n");
        for tree in &report.trees {
            out.push_str(&format!(
                "  {:<19} {:>9}  {:>5}  {:>4}  {:>5}  {:>6}  {}\n",
                format_digest(&tree.digest),
                crate::shell::projector::format_bytes(tree.total_bytes),
                tree.file_count,
                tree.dir_count,
                tree.link_count,
                tree.multiply_linked_count,
                format_reclaim_verdict(tree),
            ));
        }
    }

    let oldest = report
        .oldest_modified_ms
        .map(|ms| format_age(now_ms.saturating_sub(ms)));
    let newest = report
        .newest_modified_ms
        .map(|ms| format_age(now_ms.saturating_sub(ms)));
    let mut footer = format!(
        "\n  Total {} across {} tree(s)",
        crate::shell::projector::format_bytes(report.total_bytes),
        report.tree_count
    );
    if let (Some(oldest), Some(newest)) = (oldest, newest) {
        footer.push_str(&format!("; oldest {oldest}, newest {newest}"));
    }
    out.push_str(&footer);
    out.push('\n');

    if report.quarantined_tree_count > 0 {
        out.push_str(&format!(
            "  {} reclaim tombstone(s) are quarantined under the scratch root.\n",
            report.quarantined_tree_count
        ));
    }
    if report.unrecognized_entry_count > 0 {
        out.push_str(&format!(
            "  {} entr(y/ies) under the scratch root are not managed trees.\n",
            report.unrecognized_entry_count
        ));
    }
    if report.truncated {
        out.push_str(
            "  The walk hit a budget and is truncated; the totals above are a partial measurement.\n",
        );
    }
    for anomaly in &report.anomalies {
        out.push_str(&format!("  Note: {anomaly}\n"));
    }
    out
}

/// Render the one-line summary used by `codegg daemon status`.
pub fn format_scratch_usage_summary(report: &ScratchUsageReportDto, now_ms: i64) -> String {
    let age = now_ms.saturating_sub(report.generated_at_ms);
    let blocked = report
        .trees
        .iter()
        .filter(|tree| tree.reclaimable == Some(false))
        .count();
    let mut summary = format!(
        "Scratch: {} tree(s), {}, measured {}",
        report.tree_count,
        crate::shell::projector::format_bytes(report.total_bytes),
        format_age(age)
    );
    if blocked > 0 {
        summary.push_str(&format!(", {blocked} blocked by the reclaim validator"));
    }
    if report.truncated {
        summary.push_str(", truncated");
    }
    summary
}

/// Render one tree's reclaim verdict for the table.
fn format_reclaim_verdict(tree: &ScratchTreeUsageDto) -> String {
    if tree.vanished {
        return "vanished".to_string();
    }
    match tree.reclaimable {
        Some(true) => "yes".to_string(),
        Some(false) => {
            let reasons: Vec<&str> = tree
                .refusal_rules
                .iter()
                .map(|rule| match rule {
                    ScratchReclaimRefusalDto::Link => "link",
                    ScratchReclaimRefusalDto::NotRegular => "not_regular",
                    ScratchReclaimRefusalDto::MultiplyLinked => "multiply_linked",
                    ScratchReclaimRefusalDto::Unreadable => "unreadable",
                })
                .collect();
            format!("no ({})", reasons.join(", "))
        }
        None => "unknown".to_string(),
    }
}

/// Shorten a digest for the human table.
fn format_digest(digest: &str) -> String {
    if digest.chars().count() <= DIGEST_DISPLAY_CHARS {
        return digest.to_string();
    }
    let prefix: String = digest.chars().take(DIGEST_DISPLAY_CHARS).collect();
    format!("{prefix}…")
}

/// Render a duration as a short age.
pub fn format_age(ms: i64) -> String {
    let seconds = ms / 1000;
    if seconds < 60 {
        format!("{seconds}s ago")
    } else if seconds < 3_600 {
        format!("{}m ago", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h ago", seconds / 3_600)
    } else {
        format!("{}d ago", seconds / 86_400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::dto::ScratchReclaimRefusalDto;

    fn report_with_trees() -> ScratchUsageReportDto {
        ScratchUsageReportDto {
            trees: vec![
                ScratchTreeUsageDto {
                    digest: "a".repeat(64),
                    total_bytes: 2048,
                    file_count: 4,
                    dir_count: 2,
                    reclaimable: Some(true),
                    ..ScratchTreeUsageDto::default()
                },
                ScratchTreeUsageDto {
                    digest: "b".repeat(64),
                    total_bytes: 512,
                    file_count: 1,
                    dir_count: 1,
                    reclaimable: Some(false),
                    refusal_rules: vec![ScratchReclaimRefusalDto::MultiplyLinked],
                    refusal_paths: vec!["linked.txt".to_string()],
                    ..ScratchTreeUsageDto::default()
                },
            ],
            tree_count: 2,
            total_bytes: 2560,
            generated_at_ms: 1_000_000,
            budget: ScratchCensusBudgetDto {
                max_entries: 20_000,
                max_depth: 16,
                max_paths_per_tree: 32,
                max_trees: 2_000,
            },
            ..ScratchUsageReportDto::default()
        }
    }

    #[test]
    fn the_human_table_shows_the_totals_the_verdict_and_the_rule_that_refused_it() {
        let rendered = format_scratch_usage_report(&report_with_trees(), 1_012_000);

        assert!(rendered.contains("2 tree(s)"), "{rendered}");
        assert!(rendered.contains("2.5 KiB"), "{rendered}");
        assert!(rendered.contains("yes"), "{rendered}");
        assert!(
            rendered.contains("no (multiply_linked)"),
            "the rule that refused the tree is named: {rendered}"
        );
        assert!(
            rendered.contains("12s ago"),
            "the report carries its age: {rendered}"
        );
    }

    #[test]
    fn the_summary_line_is_one_line_and_counts_the_blocked_trees() {
        let summary = format_scratch_usage_summary(&report_with_trees(), 1_012_000);

        assert!(
            !summary.contains('\n'),
            "the summary is one line: {summary}"
        );
        assert!(summary.contains("Scratch:"), "{summary}");
        assert!(summary.contains("2 tree(s)"), "{summary}");
        assert!(summary.contains("2.5 KiB"), "{summary}");
        assert!(
            summary.contains("1 blocked by the reclaim validator"),
            "a refused tree is counted: {summary}"
        );
    }

    #[test]
    fn a_truncated_report_says_so_in_the_summary() {
        let mut report = report_with_trees();
        report.truncated = true;
        let summary = format_scratch_usage_summary(&report, 1_012_000);
        assert!(summary.contains("truncated"), "{summary}");
    }

    #[test]
    fn the_held_report_carries_its_age() {
        let state = ScratchUsageState {
            report: Some(ScratchUsageReport {
                generated_at_ms: 1_000_000,
                ..ScratchUsageReport::default()
            }),
            last_error: Some("a walk failed".to_string()),
            last_attempt_ms: Some(1_030_000),
        };
        assert_eq!(state.report_age_ms(1_012_000), Some(12_000));
        assert_eq!(
            ScratchUsageState::default().report_age_ms(1_012_000),
            None,
            "an empty state has no report age"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_refresh_publishes_the_report_and_keeps_it_on_the_next_read() {
        let root = tempfile::tempdir().expect("temp scratch root");
        let tree = root.path().join("a".repeat(64));
        std::fs::create_dir_all(&tree).expect("fixture tree");
        std::fs::write(tree.join("a.txt"), b"hello").expect("fixture file");

        let refresher = ScratchUsageRefresher::with_root(
            Arc::new(parking_lot::RwLock::new(ScratchUsageState::default())),
            root.path().to_path_buf(),
        );

        let report = refresher.refresh().await.expect("the walk succeeds");
        assert_eq!(report.tree_count, 1);

        let held = refresher.last_report().expect("the report is held");
        assert_eq!(held.tree_count, 1);
        assert_eq!(held.total_bytes, 5);
        assert!(refresher.state().read().last_error.is_none());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn two_concurrent_refreshes_do_not_walk_the_tree_at_the_same_time() {
        let root = tempfile::tempdir().expect("temp scratch root");
        let tree = root.path().join("a".repeat(64));
        std::fs::create_dir_all(&tree).expect("fixture tree");

        let refresher = Arc::new(ScratchUsageRefresher::with_root(
            Arc::new(parking_lot::RwLock::new(ScratchUsageState::default())),
            root.path().to_path_buf(),
        ));

        let first = {
            let refresher = Arc::clone(&refresher);
            tokio::spawn(async move { refresher.refresh().await })
        };
        let second = {
            let refresher = Arc::clone(&refresher);
            tokio::spawn(async move { refresher.refresh().await })
        };
        let first = first
            .await
            .expect("first walk")
            .expect("first walk succeeds");
        let second = second
            .await
            .expect("second walk")
            .expect("second walk succeeds");

        assert_eq!(first.tree_count, second.tree_count);
        assert_eq!(first.total_bytes, second.total_bytes);
    }
}
