use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::provider::Provider;
use crate::research::coordinator::{ClaimDiff, ResearchCoordinator};
use crate::research::error::{ResearchError, Result};
use crate::research::store::ResearchMetadata;
use crate::research::types::*;

/// Summary of a research run for listing/display purposes.
#[derive(Debug, Clone)]
pub struct ResearchRunSummary {
    pub run_id: String,
    pub status: RunStatus,
    pub question: String,
    pub mode: ResearchMode,
    pub depth: ResearchDepth,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    pub artifact_dir: PathBuf,
    pub counts: ResearchRunCounts,
}

/// Result of a rerun operation.
#[derive(Debug, Clone)]
pub struct RerunResult {
    pub new_run: ResearchRunResult,
    pub diff: ClaimDiff,
}

/// Agent-friendly wrapper around `ResearchCoordinator`.
///
/// Provides synchronous methods for common operations and an async
/// method for running research pipelines.
pub struct ResearchService {
    coordinator: ResearchCoordinator,
    artifact_root: PathBuf,
}

impl ResearchService {
    /// Create a new service from a project root path.
    ///
    /// Artifacts are stored under `<project_root>/.codegg/research/`.
    pub fn new(project_root: PathBuf) -> Self {
        let artifact_root = project_root.join(".codegg").join("research");
        let coordinator = ResearchCoordinator::new(project_root, artifact_root.clone());
        Self {
            coordinator,
            artifact_root,
        }
    }

    /// Create a new service with explicit artifact root.
    pub fn with_artifact_root(project_root: PathBuf, artifact_root: PathBuf) -> Self {
        let coordinator = ResearchCoordinator::new(project_root, artifact_root.clone());
        Self {
            coordinator,
            artifact_root,
        }
    }

    /// Create a service with an LLM provider for model-backed research phases.
    pub fn with_provider(
        project_root: PathBuf,
        provider: Arc<dyn Provider>,
        model: String,
    ) -> Self {
        let artifact_root = project_root.join(".codegg").join("research");
        let coordinator = ResearchCoordinator::new(project_root, artifact_root.clone())
            .with_provider(provider, model);
        Self {
            coordinator,
            artifact_root,
        }
    }

    /// Create a service with explicit artifact root and LLM provider.
    pub fn with_artifact_root_and_provider(
        project_root: PathBuf,
        artifact_root: PathBuf,
        provider: Arc<dyn Provider>,
        model: String,
    ) -> Self {
        let coordinator = ResearchCoordinator::new(project_root, artifact_root.clone())
            .with_provider(provider, model);
        Self {
            coordinator,
            artifact_root,
        }
    }

    /// Run a research pipeline and return the result.
    pub async fn run(&self, request: ResearchRequest) -> Result<ResearchRunResult> {
        self.coordinator.run(request).await
    }

    /// Run research and return a compact agent answer string.
    ///
    /// This is the primary method for agent integration: it runs the
    /// full pipeline and extracts the `AgentAnswer` output.
    pub async fn answer_for_agent(
        &self,
        question: &str,
        mode: ResearchMode,
        depth: ResearchDepth,
    ) -> Result<String> {
        let request = self.build_request(
            question,
            mode,
            depth,
            vec![ResearchOutputProfile::AgentAnswer],
        );
        let result = self.coordinator.run(request).await?;

        // Read the agent answer artifact
        let answer_path = result
            .outputs
            .iter()
            .find(|o| o.profile == ResearchOutputProfile::AgentAnswer)
            .map(|o| o.path.clone())
            .ok_or_else(|| {
                ResearchError::RunFailed("No agent answer output produced".to_string())
            })?;

        tokio::fs::read_to_string(&answer_path)
            .await
            .map_err(ResearchError::Io)
    }

    /// Run research and return the path to the full human report.
    pub async fn create_report(
        &self,
        question: &str,
        mode: ResearchMode,
        depth: ResearchDepth,
    ) -> Result<PathBuf> {
        let request = self.build_request(
            question,
            mode,
            depth,
            vec![
                ResearchOutputProfile::HumanFullReport,
                ResearchOutputProfile::AgentAnswer,
            ],
        );
        let result = self.coordinator.run(request).await?;

        result
            .outputs
            .iter()
            .find(|o| o.profile == ResearchOutputProfile::HumanFullReport)
            .map(|o| o.path.clone())
            .ok_or_else(|| ResearchError::RunFailed("No report output produced".to_string()))
    }

    /// List recent research runs, most recent first.
    pub async fn list_runs(&self) -> Result<Vec<ResearchRunSummary>> {
        let store = self.coordinator.store();
        let statuses = store.list_runs().await?;

        let mut summaries = Vec::with_capacity(statuses.len());
        for status in statuses {
            // Load request to get question/mode/depth
            let bundle = store.load_run_bundle(&status.run_id).await?;
            summaries.push(ResearchRunSummary {
                run_id: status.run_id,
                status: status.status,
                question: bundle.request.question,
                mode: bundle.request.mode,
                depth: bundle.request.depth,
                started_at: status.started_at,
                finished_at: status.finished_at,
                artifact_dir: status.artifact_dir,
                counts: status.counts,
            });
        }

        Ok(summaries)
    }

    /// Load a run bundle by ID.
    pub async fn load_run(&self, run_id: &str) -> Result<ResearchBundle> {
        self.coordinator.store().load_run_bundle(run_id).await
    }

    /// Rerun a research run: re-collect sources, re-extract evidence, re-construct claims,
    /// and diff the new claims against the old ones.
    pub async fn rerun(&self, original_run_id: &str) -> Result<RerunResult> {
        let (new_run, diff) = self.coordinator.rerun(original_run_id).await?;
        Ok(RerunResult {
            new_run,
            diff,
        })
    }

    /// Re-synthesize output profiles from an existing run's evidence and claims.
    ///
    /// Does NOT re-collect sources or re-extract evidence. Only re-renders
    /// the requested output profiles from the existing claim graph.
    pub async fn resynthesize(
        &self,
        run_id: &str,
        profiles: Vec<ResearchOutputProfile>,
    ) -> Result<Vec<ResearchArtifactRef>> {
        self.coordinator.resynthesize(run_id, &profiles).await
    }

    /// List research run metadata from the SQLite index.
    pub async fn list_metadata(&self, project_root: Option<&str>) -> Result<Vec<ResearchMetadata>> {
        self.coordinator.store().list_metadata(project_root).await
    }

    /// Load research run metadata from the SQLite index.
    pub async fn load_metadata(&self, run_id: &str) -> Result<Option<ResearchMetadata>> {
        self.coordinator.store().load_metadata(run_id).await
    }

    /// Get the artifact root directory.
    pub fn artifact_root(&self) -> &Path {
        &self.artifact_root
    }

    fn build_request(
        &self,
        question: &str,
        mode: ResearchMode,
        depth: ResearchDepth,
        output_profiles: Vec<ResearchOutputProfile>,
    ) -> ResearchRequest {
        let max_sources = match depth {
            ResearchDepth::Low => 8,
            ResearchDepth::Medium => 30,
            ResearchDepth::High => 80,
        };

        ResearchRequest {
            id: uuid::Uuid::new_v4().to_string(),
            question: question.to_string(),
            mode,
            audience: ResearchAudience::Human,
            depth,
            output_profiles,
            constraints: vec![],
            sources: vec![ResearchSourceSpec {
                spec_type: SourceSpecType::Local,
                value: String::new(),
            }],
            existing_context_refs: vec![],
            budget: ResearchBudget {
                max_sources,
                max_chunks_per_source: 20,
                max_evidence_spans: 200,
                max_model_calls: 0,
                max_output_tokens: None,
                allow_network: false,
            },
            created_at: chrono::Utc::now(),
        }
    }
}

/// Parse a mode string into `ResearchMode`.
pub fn parse_mode(s: &str) -> Result<ResearchMode> {
    match s {
        "landscape" => Ok(ResearchMode::Landscape),
        "architecture-decision" => Ok(ResearchMode::ArchitectureDecision),
        "library-evaluation" => Ok(ResearchMode::LibraryEvaluation),
        "api-investigation" => Ok(ResearchMode::ApiInvestigation),
        "debugging-investigation" => Ok(ResearchMode::DebuggingInvestigation),
        "security-review" => Ok(ResearchMode::SecurityReview),
        "spec-digest" => Ok(ResearchMode::SpecDigest),
        "narrow-answer" => Ok(ResearchMode::NarrowAnswer),
        _ => Err(ResearchError::Config(format!(
            "Unknown mode: {s}. Use: landscape, architecture-decision, library-evaluation, api-investigation, debugging-investigation, security-review, spec-digest, narrow-answer"
        ))),
    }
}

/// Parse a depth string into `ResearchDepth`.
pub fn parse_depth(s: &str) -> Result<ResearchDepth> {
    match s {
        "low" => Ok(ResearchDepth::Low),
        "medium" => Ok(ResearchDepth::Medium),
        "high" => Ok(ResearchDepth::High),
        _ => Err(ResearchError::Config(format!(
            "Unknown depth: {s}. Use: low, medium, high"
        ))),
    }
}

/// Parse an audience string into `ResearchAudience`.
pub fn parse_audience(s: &str) -> Result<ResearchAudience> {
    match s {
        "human" => Ok(ResearchAudience::Human),
        "agent-planner" => Ok(ResearchAudience::AgentPlanner),
        "agent-coder" => Ok(ResearchAudience::AgentCoder),
        "agent-reviewer" => Ok(ResearchAudience::AgentReviewer),
        "agent-debugger" => Ok(ResearchAudience::AgentDebugger),
        _ => Err(ResearchError::Config(format!(
            "Unknown audience: {s}. Use: human, agent-planner, agent-coder, agent-reviewer, agent-debugger"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_mode_valid() {
        assert!(matches!(
            parse_mode("landscape"),
            Ok(ResearchMode::Landscape)
        ));
        assert!(matches!(
            parse_mode("architecture-decision"),
            Ok(ResearchMode::ArchitectureDecision)
        ));
        assert!(matches!(
            parse_mode("narrow-answer"),
            Ok(ResearchMode::NarrowAnswer)
        ));
    }

    #[test]
    fn parse_mode_invalid() {
        assert!(parse_mode("invalid").is_err());
    }

    #[test]
    fn parse_depth_valid() {
        assert!(matches!(parse_depth("low"), Ok(ResearchDepth::Low)));
        assert!(matches!(parse_depth("medium"), Ok(ResearchDepth::Medium)));
        assert!(matches!(parse_depth("high"), Ok(ResearchDepth::High)));
    }

    #[test]
    fn parse_depth_invalid() {
        assert!(parse_depth("extreme").is_err());
    }

    #[test]
    fn parse_audience_valid() {
        assert!(matches!(
            parse_audience("human"),
            Ok(ResearchAudience::Human)
        ));
        assert!(matches!(
            parse_audience("agent-coder"),
            Ok(ResearchAudience::AgentCoder)
        ));
    }

    #[test]
    fn parse_audience_invalid() {
        assert!(parse_audience("robot").is_err());
    }

    #[test]
    fn service_builds_request_with_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let service = ResearchService::new(tmp.path().to_path_buf());
        let request = service.build_request(
            "test question",
            ResearchMode::Landscape,
            ResearchDepth::Medium,
            vec![ResearchOutputProfile::AgentAnswer],
        );
        assert_eq!(request.question, "test question");
        assert_eq!(request.mode, ResearchMode::Landscape);
        assert_eq!(request.depth, ResearchDepth::Medium);
        assert_eq!(request.budget.max_sources, 30);
        assert!(request
            .sources
            .iter()
            .any(|s| s.spec_type == SourceSpecType::Local));
    }

    #[test]
    fn service_artifact_root() {
        let tmp = tempfile::tempdir().unwrap();
        let service = ResearchService::new(tmp.path().to_path_buf());
        assert_eq!(
            service.artifact_root(),
            tmp.path().join(".codegg").join("research")
        );
    }
}
