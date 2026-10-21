//! M006: the daemon-owned scratch census.
//!
//! The daemon owns one disposable scratch tree per session. This module
//! measures that tree and answers the operator's questions about it. It is
//! deliberately **measurement only**: it creates nothing, renames nothing,
//! unlinks nothing, and never opens a file for reading.
//!
//! ## Why the report is daemon-held
//!
//! The census is a bounded filesystem walk. Running one per request would put
//! a size traversal in front of every operator question; running one per tool
//! call would put it on the agent hot path. Instead the daemon keeps the last
//! completed measurement and refreshes it on a timer, so a request either
//! answers from the snapshot or — when the caller asks for it — runs exactly
//! one bounded walk and replaces the snapshot.
//!
//! ## Disclosure bounds
//!
//! The report carries counts, sizes, ages, and the opaque one-way digest that
//! names a tree. It never carries an absolute path, a file name, file
//! content, or a session identity: the digest is one-way by design, so a
//! session id cannot be recovered from it.

use std::sync::Arc;
use std::time::Duration;

use codegg_protocol::scratch::{
    ScratchReclaimRefusalDto, ScratchTreeUsageDto, ScratchUsageReportDto,
};

use crate::core::daemon::CoreDaemon;
use crate::error::AppError;
use crate::protocol::core::{CoreRequest, CoreResponse};
use crate::scratch::{ReclaimRefusal, ScratchCensusBudget, ScratchTreeUsage, ScratchUsageReport};

/// How often the daemon refreshes its census snapshot.
const CENSUS_REFRESH_INTERVAL: Duration = Duration::from_secs(600);

/// The daemon-held census snapshot.
#[derive(Debug, Default)]
pub struct ScratchCensus {
    snapshot: parking_lot::RwLock<Option<CensusSnapshot>>,
    /// Serializes walks. Two callers cannot walk the tree concurrently: the
    /// second waits for the first to finish instead of doubling the
    /// filesystem work against a live tree.
    walk_lock: tokio::sync::Mutex<()>,
}

/// One completed measurement.
#[derive(Debug, Clone)]
pub struct CensusSnapshot {
    /// What the walk measured.
    pub report: ScratchUsageReport,
    /// When the walk completed, in milliseconds since the epoch.
    pub generated_at_ms: i64,
}

impl ScratchCensus {
    /// A census that has not completed a measurement yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// The last completed measurement, if any.
    pub fn last(&self) -> Option<CensusSnapshot> {
        self.snapshot.read().clone()
    }

    /// Run one bounded walk and replace the snapshot.
    ///
    /// A failed walk leaves the previous measurement in place, so a transient
    /// filesystem error never turns into "no scratch".
    pub async fn refresh(&self) -> Result<CensusSnapshot, AppError> {
        let _guard = self.walk_lock.lock().await;
        let report = run_census().await?;
        let snapshot = CensusSnapshot {
            report,
            generated_at_ms: chrono::Utc::now().timestamp_millis(),
        };
        *self.snapshot.write() = Some(snapshot.clone());
        Ok(snapshot)
    }
}

impl CoreDaemon {
    /// Dispatch one M006 scratch measurement request.
    pub(crate) async fn handle_scratch_request(
        &self,
        request: CoreRequest,
    ) -> Result<CoreResponse, AppError> {
        match request {
            CoreRequest::ScratchUsage { request } => {
                match self.scratch_usage_report(request.refresh).await {
                    Ok(report) => Ok(CoreResponse::ScratchUsage { report }),
                    Err(error) => {
                        // The failure detail stays in the daemon log: the
                        // client-visible message must not carry a path.
                        tracing::warn!(error = %error, "scratch census failed");
                        Ok(CoreResponse::Error {
                            code: "scratch_usage_unavailable".into(),
                            message: "the scratch usage report is unavailable".into(),
                        })
                    }
                }
            }
            other => Ok(CoreResponse::Error {
                code: "unsupported_scratch_request".into(),
                message: format!("{other:?} is not a scratch request"),
            }),
        }
    }

    /// Answer a scratch usage request.
    ///
    /// Without `refresh` this answers from the daemon's snapshot. With
    /// `refresh` — or when no measurement has completed yet — it runs exactly
    /// one bounded walk and replaces the snapshot.
    async fn scratch_usage_report(&self, refresh: bool) -> Result<ScratchUsageReportDto, AppError> {
        if !refresh {
            if let Some(snapshot) = self.scratch_census.last() {
                return Ok(report_to_dto(snapshot));
            }
        }
        let snapshot = self.scratch_census.refresh().await?;
        Ok(report_to_dto(snapshot))
    }
}

/// Spawn the census refresh task.
///
/// The refresh is a plain background task that holds the last measurement; it
/// never blocks a request path and it is never on an agent hot path. A
/// failed walk leaves the previous measurement in place rather than clearing
/// it, so a transient filesystem error never turns into "no scratch".
pub fn spawn_census_refresh(census: Arc<ScratchCensus>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(CENSUS_REFRESH_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            if let Err(error) = census.refresh().await {
                tracing::warn!(error = %error, "scratch census refresh failed");
            }
        }
    })
}

/// Run one bounded census walk off the async runtime's worker threads.
async fn run_census() -> Result<ScratchUsageReport, AppError> {
    let budget = ScratchCensusBudget::default();
    tokio::task::spawn_blocking(move || crate::scratch::scratch_usage_census(&budget))
        .await
        .map_err(|error| AppError::Other(anyhow::anyhow!("scratch census task failed: {error}")))?
        .map_err(|error| AppError::Other(anyhow::anyhow!("scratch census failed: {error}")))
}

/// Map the domain report onto the wire DTO.
fn report_to_dto(snapshot: CensusSnapshot) -> ScratchUsageReportDto {
    ScratchUsageReportDto {
        tree_count: snapshot.report.tree_count,
        unreported_trees: snapshot.report.unreported_trees,
        total_bytes: snapshot.report.total_bytes,
        truncated: snapshot.report.truncated,
        generated_at_ms: snapshot.generated_at_ms,
        trees: snapshot.report.trees.iter().map(tree_to_dto).collect(),
    }
}

/// Map one tree's measurement onto the wire DTO.
fn tree_to_dto(tree: &ScratchTreeUsage) -> ScratchTreeUsageDto {
    ScratchTreeUsageDto {
        digest: tree.digest.clone(),
        quarantined: tree.quarantined,
        total_bytes: tree.total_bytes,
        file_count: tree.file_count,
        directory_count: tree.directory_count,
        symlink_count: tree.symlink_count,
        non_regular_count: tree.non_regular_count,
        multiply_linked_count: tree.multiply_linked_count,
        oldest_modified_ms: tree.oldest_modified_ms,
        newest_modified_ms: tree.newest_modified_ms,
        reclaimable: tree.reclaimable,
        refusal: tree.refusal.map(refusal_to_dto),
        owner_record_present: tree.owner_record_present,
        truncated: tree.truncated,
        vanished: tree.vanished,
    }
}

/// Map a refusal reason onto the wire DTO.
fn refusal_to_dto(refusal: ReclaimRefusal) -> ScratchReclaimRefusalDto {
    match refusal {
        ReclaimRefusal::Symlink => ScratchReclaimRefusalDto::SymbolicLink,
        ReclaimRefusal::NonRegular => ScratchReclaimRefusalDto::NonRegularEntry,
        ReclaimRefusal::MultiplyLinked => ScratchReclaimRefusalDto::MultiplyLinkedFile,
    }
}

#[cfg(test)]
mod tests {
    /// The census must never run on the agent hot path.
    ///
    /// A size traversal in front of a tool call would put a filesystem walk
    /// in the latency of every turn. The census is reachable only from the
    /// `Scratch` request family and the refresh task; this test pins that by
    /// asserting the tool, permission, and turn-admission modules never name
    /// it.
    #[test]
    fn no_agent_hot_path_module_reaches_the_census() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        for relative in [
            "src/tool/mod.rs",
            "src/tool/broker.rs",
            "src/agent/tool_batch.rs",
            "src/agent/loop.rs",
            "src/permission/mod.rs",
        ] {
            let path = root.join(relative);
            let Ok(source) = std::fs::read_to_string(&path) else {
                continue;
            };
            assert!(
                !source.contains("scratch_census") && !source.contains("scratch_usage_census"),
                "{relative} reaches the scratch census; the walk must stay off the agent hot path"
            );
        }
    }
}
