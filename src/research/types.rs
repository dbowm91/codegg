use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// -- IDs are plain String, generated with uuid::Uuid::new_v4().to_string() --

/// A research request capturing the full parameterization of a research run.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ResearchRequest {
    pub id: String,
    pub question: String,
    pub mode: ResearchMode,
    pub audience: ResearchAudience,
    pub depth: ResearchDepth,
    pub output_profiles: Vec<ResearchOutputProfile>,
    pub constraints: Vec<String>,
    pub sources: Vec<ResearchSourceSpec>,
    pub existing_context_refs: Vec<String>,
    pub budget: ResearchBudget,
    pub created_at: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ResearchMode {
    Landscape,
    ArchitectureDecision,
    LibraryEvaluation,
    ApiInvestigation,
    DebuggingInvestigation,
    SecurityReview,
    SpecDigest,
    NarrowAnswer,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ResearchAudience {
    Human,
    AgentPlanner,
    AgentCoder,
    AgentReviewer,
    AgentDebugger,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ResearchDepth {
    Low,
    Medium,
    High,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ResearchOutputProfile {
    HumanFullReport,
    HumanBrief,
    AgentAnswer,
    AgentHandoff,
    EvidenceBundle,
}

/// Resource budget governing a research run's resource consumption.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ResearchBudget {
    pub max_sources: usize,
    pub max_chunks_per_source: usize,
    pub max_evidence_spans: usize,
    pub max_model_calls: usize,
    pub max_output_tokens: Option<usize>,
    pub allow_network: bool,
}

/// A specification for a source to include or search within a research run.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ResearchSourceSpec {
    pub spec_type: SourceSpecType,
    pub value: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SourceSpecType {
    Local,
    File,
    Url,
    Text,
}

// -- Source records --

/// A record of a source that was collected during a research run.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SourceRecord {
    pub id: String,
    pub run_id: String,
    pub uri: String,
    pub title: Option<String>,
    pub source_type: SourceType,
    pub source_quality: SourceQuality,
    pub retrieved_at: DateTime<Utc>,
    pub published_at: Option<DateTime<Utc>>,
    pub content_hash: Option<String>,
    pub locator: SourceLocator,
    pub notes: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SourceType {
    LocalFile,
    LocalSearchResult,
    Url,
    HtmlPage,
    MarkdownPage,
    Pdf,
    GitHubFile,
    GitHubIssue,
    CratesIoMetadata,
    ManualText,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum SourceQuality {
    Primary,
    OfficialDocs,
    SourceCode,
    MaintainerComment,
    ReleaseNotes,
    StandardOrSpec,
    Academic,
    Secondary,
    Unknown,
    LowQuality,
}

/// A locator pointing to a specific region within a source.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum SourceLocator {
    #[serde(rename_all = "camelCase")]
    FileRange {
        path: PathBuf,
        start_line: usize,
        end_line: usize,
    },
    #[serde(rename_all = "camelCase")]
    Url {
        url: String,
        heading: Option<String>,
    },
    #[serde(rename_all = "camelCase")]
    TextSpan {
        label: String,
    },
}

// -- Evidence --

/// A specific span of text extracted from a source as evidence.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EvidenceSpan {
    pub id: String,
    pub run_id: String,
    pub source_id: String,
    pub locator: SourceLocator,
    pub text: String,
    pub summary: Option<String>,
    pub extracted_at: DateTime<Utc>,
}

// -- Claims --

/// A claim derived from evidence during a research run.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClaimRecord {
    pub id: String,
    pub run_id: String,
    pub text: String,
    pub claim_type: ClaimType,
    pub confidence: Confidence,
    pub evidence_ids: Vec<String>,
    pub caveats: Vec<String>,
    pub applies_to: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ClaimType {
    Fact,
    Comparison,
    Recommendation,
    Risk,
    Caveat,
    OpenQuestion,
    Inference,
}

impl ClaimType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ClaimType::Fact => "fact",
            ClaimType::Comparison => "comparison",
            ClaimType::Recommendation => "recommendation",
            ClaimType::Risk => "risk",
            ClaimType::Caveat => "caveat",
            ClaimType::OpenQuestion => "open_question",
            ClaimType::Inference => "inference",
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

// -- Contradictions --

/// A contradiction detected between two or more claims.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ContradictionRecord {
    pub id: String,
    pub run_id: String,
    pub description: String,
    pub claim_ids: Vec<String>,
    pub severity: ContradictionSeverity,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ContradictionSeverity {
    Low,
    Medium,
    High,
}

// -- Run status --

/// The current status of a research run, including timing and counts.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ResearchRunStatus {
    pub run_id: String,
    pub status: RunStatus,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub artifact_dir: PathBuf,
    pub error: Option<String>,
    pub counts: ResearchRunCounts,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Planning,
    Collecting,
    Extracting,
    Claiming,
    Contradicting,
    Synthesizing,
    Verifying,
    Completed,
    Failed,
}

/// Aggregate counts for the artifacts produced by a research run.
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ResearchRunCounts {
    pub sources: usize,
    pub evidence_spans: usize,
    pub claims: usize,
    pub contradictions: usize,
}

// -- Research plan --

/// The research plan generated during the planning phase of a run.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ResearchPlan {
    pub scope: String,
    pub comparison_axes: Vec<String>,
    pub source_classes: Vec<String>,
    pub exclusion_criteria: Vec<String>,
    pub stopping_conditions: Vec<String>,
    pub expected_outputs: Vec<String>,
}

// -- Research bundle (loaded from store) --

/// A complete research bundle containing all artifacts from a run.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ResearchBundle {
    pub request: ResearchRequest,
    pub status: ResearchRunStatus,
    pub plan: Option<ResearchPlan>,
    pub sources: Vec<SourceRecord>,
    pub evidence: Vec<EvidenceSpan>,
    pub claims: Vec<ClaimRecord>,
    pub contradictions: Vec<ContradictionRecord>,
}

// -- Output helpers --

/// A reference to a research output artifact on disk.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ResearchArtifactRef {
    pub run_id: String,
    pub profile: ResearchOutputProfile,
    pub path: PathBuf,
}

/// The result of completing a research run.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ResearchRunResult {
    pub run_id: String,
    pub status: RunStatus,
    pub artifact_dir: PathBuf,
    pub outputs: Vec<ResearchArtifactRef>,
    pub counts: ResearchRunCounts,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn research_request_roundtrip() {
        let request = ResearchRequest {
            id: "test-id".to_string(),
            question: "What is the best web framework?".to_string(),
            mode: ResearchMode::ArchitectureDecision,
            audience: ResearchAudience::Human,
            depth: ResearchDepth::Medium,
            output_profiles: vec![
                ResearchOutputProfile::HumanFullReport,
                ResearchOutputProfile::AgentHandoff,
            ],
            constraints: vec!["Must be async".to_string()],
            sources: vec![ResearchSourceSpec {
                spec_type: SourceSpecType::File,
                value: "src/main.rs".to_string(),
            }],
            existing_context_refs: vec![],
            budget: ResearchBudget {
                max_sources: 30,
                max_chunks_per_source: 20,
                max_evidence_spans: 200,
                max_model_calls: 10,
                max_output_tokens: Some(4096),
                allow_network: false,
            },
            created_at: chrono::DateTime::parse_from_rfc3339("2026-01-15T10:30:00Z")
                .unwrap()
                .with_timezone(&Utc),
        };
        let json = serde_json::to_string(&request).unwrap();
        let decoded: ResearchRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.id, request.id);
        assert_eq!(decoded.question, request.question);
        assert_eq!(decoded.mode, ResearchMode::ArchitectureDecision);
        assert_eq!(decoded.depth, ResearchDepth::Medium);
        assert_eq!(decoded.output_profiles.len(), 2);
        assert_eq!(decoded.budget.max_sources, 30);
    }

    #[test]
    fn source_record_roundtrip() {
        let source = SourceRecord {
            id: "src-1".to_string(),
            run_id: "run-1".to_string(),
            uri: "https://docs.rs/axum".to_string(),
            title: Some("Axum docs".to_string()),
            source_type: SourceType::HtmlPage,
            source_quality: SourceQuality::OfficialDocs,
            retrieved_at: Utc::now(),
            published_at: None,
            content_hash: Some("abc123".to_string()),
            locator: SourceLocator::Url {
                url: "https://docs.rs/axum".to_string(),
                heading: Some("Getting Started".to_string()),
            },
            notes: vec!["fetched successfully".to_string()],
        };
        let json = serde_json::to_string(&source).unwrap();
        let decoded: SourceRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.id, "src-1");
        assert_eq!(decoded.source_type, SourceType::HtmlPage);
        match &decoded.locator {
            SourceLocator::Url { url, heading } => {
                assert_eq!(url, "https://docs.rs/axum");
                assert_eq!(heading.as_deref(), Some("Getting Started"));
            }
            _ => panic!("Expected Url locator"),
        }
    }

    #[test]
    fn claim_record_roundtrip() {
        let claim = ClaimRecord {
            id: "cl-1".to_string(),
            run_id: "run-1".to_string(),
            text: "Axum is well-maintained".to_string(),
            claim_type: ClaimType::Fact,
            confidence: Confidence::High,
            evidence_ids: vec!["ev-1".to_string(), "ev-2".to_string()],
            caveats: vec!["Based on limited data".to_string()],
            applies_to: vec!["axum".to_string()],
        };
        let json = serde_json::to_string(&claim).unwrap();
        let decoded: ClaimRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.claim_type, ClaimType::Fact);
        assert_eq!(decoded.confidence, Confidence::High);
        assert_eq!(decoded.evidence_ids.len(), 2);
    }

    #[test]
    fn source_locator_tagged_json() {
        let loc = SourceLocator::FileRange {
            path: PathBuf::from("src/main.rs"),
            start_line: 10,
            end_line: 20,
        };
        let json = serde_json::to_string(&loc).unwrap();
        assert!(json.contains("\"type\""));
        let decoded: SourceLocator = serde_json::from_str(&json).unwrap();
        match decoded {
            SourceLocator::FileRange {
                path,
                start_line,
                end_line,
            } => {
                assert_eq!(path, PathBuf::from("src/main.rs"));
                assert_eq!(start_line, 10);
                assert_eq!(end_line, 20);
            }
            _ => panic!("Expected FileRange"),
        }
    }

    #[test]
    fn claim_type_as_str() {
        assert_eq!(ClaimType::Fact.as_str(), "fact");
        assert_eq!(ClaimType::Comparison.as_str(), "comparison");
        assert_eq!(ClaimType::Recommendation.as_str(), "recommendation");
        assert_eq!(ClaimType::Risk.as_str(), "risk");
        assert_eq!(ClaimType::Caveat.as_str(), "caveat");
        assert_eq!(ClaimType::OpenQuestion.as_str(), "open_question");
        assert_eq!(ClaimType::Inference.as_str(), "inference");
    }

    #[test]
    fn research_run_counts_default() {
        let counts = ResearchRunCounts::default();
        assert_eq!(counts.sources, 0);
        assert_eq!(counts.evidence_spans, 0);
        assert_eq!(counts.claims, 0);
        assert_eq!(counts.contradictions, 0);
    }

    #[test]
    fn research_mode_serde_roundtrip() {
        let modes = [
            ResearchMode::Landscape,
            ResearchMode::ArchitectureDecision,
            ResearchMode::LibraryEvaluation,
            ResearchMode::ApiInvestigation,
            ResearchMode::DebuggingInvestigation,
            ResearchMode::SecurityReview,
            ResearchMode::SpecDigest,
            ResearchMode::NarrowAnswer,
        ];
        for mode in &modes {
            let json = serde_json::to_string(mode).unwrap();
            let decoded: ResearchMode = serde_json::from_str(&json).unwrap();
            assert_eq!(&decoded, mode);
        }
    }
}
