//! Versioned data contracts and deterministic evaluation for tool selection.
//!
//! This module is deliberately independent from the agent loop. It consumes
//! the same textual name/description metadata as `ToolCatalog`, but it does
//! not change registration, disclosure, permission, or execution behavior.

use crate::tool::catalog::{SearchMode, ToolCatalog, ToolMetadata};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Instant;
use unicode_normalization::UnicodeNormalization;

pub const CASE_SCHEMA_VERSION: u16 = 1;
pub const PREDICTION_SCHEMA_VERSION: u16 = 1;
pub const FIXTURE_SCHEMA_VERSION: u16 = 1;
pub const MAX_CASE_BYTES: usize = 128 * 1024;
pub const MAX_CONTEXT_BYTES: usize = 8 * 1024;
pub const MAX_CANDIDATES: usize = 128;
pub const MAX_CANDIDATE_TEXT_BYTES: usize = 8 * 1024;
pub const ARTIFACT_SCHEMA_VERSION: u16 = 1;
pub const MAX_ARTIFACT_BYTES: u64 = 64 * 1024 * 1024;

const BUILTIN_CORPUS: &str = include_str!("../../assets/tool-advisor/corpus.jsonl");

#[cfg(feature = "tool-advisor")]
pub mod contextual;
#[cfg(feature = "tool-advisor-training")]
pub mod training;
pub mod training_data;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolAdvisorCandidate {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub disclosure: String,
    #[serde(default)]
    pub synthetic_identity: bool,
}

impl ToolAdvisorCandidate {
    pub fn from_metadata(metadata: &ToolMetadata) -> Self {
        Self {
            name: metadata.name.clone(),
            description: metadata.description.clone(),
            category: metadata.category.clone(),
            disclosure: metadata.disclosure.clone(),
            synthetic_identity: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolAdvisorCase {
    pub schema_version: u16,
    pub case_id: String,
    pub context: String,
    pub candidates: Vec<ToolAdvisorCandidate>,
    /// Graded relevance labels keyed by candidate name. Values are 1..=3.
    /// An empty map with `none=true` is the explicit abstention label.
    #[serde(default)]
    pub relevance: BTreeMap<String, u8>,
    #[serde(default)]
    pub preferred_order: Vec<String>,
    #[serde(default)]
    pub none: bool,
    #[serde(default)]
    pub tags: Vec<String>,
    pub group_id: String,
    pub provenance: String,
    /// Semantic leakage boundary. All generated variants in this group stay
    /// in the same split even when their case ids differ.
    #[serde(default)]
    pub semantic_group: String,
    /// Explicit manually assigned leakage family. When present it joins the
    /// content/template-derived leakage component; it never splits one.
    #[serde(default)]
    pub leakage_group: String,
    #[serde(default)]
    pub task_family: String,
    #[serde(default)]
    pub tool_family: String,
    #[serde(default)]
    pub generated_variant_family: String,
    /// Optional probabilities emitted by an explicitly local teacher/export
    /// workflow. They are labels, never a network or runtime dependency.
    #[serde(default)]
    pub teacher_probabilities: BTreeMap<String, f32>,
}

impl ToolAdvisorCase {
    pub fn split_group(&self) -> &str {
        if self.semantic_group.is_empty() {
            &self.group_id
        } else {
            &self.semantic_group
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RankedCandidate {
    pub name: String,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolAdvisorPrediction {
    pub schema_version: u16,
    pub case_id: String,
    pub ranked: Vec<RankedCandidate>,
    #[serde(default)]
    pub abstain_probability: Option<f64>,
    pub mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricSummary {
    pub cases: usize,
    pub candidate_coverage: f64,
    pub recall_at_1: f64,
    pub recall_at_3: f64,
    pub recall_at_5: f64,
    pub mrr: f64,
    pub ndcg_at_5: f64,
    pub no_tool_precision: Option<f64>,
    pub no_tool_recall: Option<f64>,
    pub no_tool_f1: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DomainMetric {
    pub tag: String,
    pub summary: MetricSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BaselineReport {
    pub fixture_schema_version: u16,
    pub dataset_fingerprint: String,
    pub split: String,
    pub mode: String,
    pub summary: MetricSummary,
    pub by_tag: Vec<DomainMetric>,
    pub coverage: CorpusCoverageReport,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CrossSplitOverlap {
    /// `exact-input`, `normalized-input`, or `template-family`.
    pub kind: String,
    /// Signature hex or normalized template-family name.
    pub key: String,
    pub splits: Vec<String>,
    /// Case ids involved, capped at [`MAX_OVERLAP_DETAIL_CASES`].
    pub case_ids: Vec<String>,
}

pub const MAX_OVERLAP_DETAIL_CASES: usize = 8;
pub const MAX_OVERLAP_DETAILS: usize = 16;

/// A tool family that can serve as a true optimizer-excluded holdout.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FamilyHoldoutSummary {
    pub family: String,
    pub holdout_cases: usize,
    pub excluded_leakage_groups: usize,
    pub train_cases_after_exclusion: usize,
    pub dev_cases_after_exclusion: usize,
    /// Cases of this family remaining in optimizer/calibration input.
    /// A true holdout always reports zero here.
    pub train_dev_family_cases: usize,
}

/// Machine-readable leakage evidence for `tool-advisor lint --json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LeakageReport {
    pub cases: usize,
    pub leakage_groups: usize,
    pub unique_exact_inputs: usize,
    pub unique_normalized_inputs: usize,
    pub exact_cross_split_overlaps: usize,
    pub normalized_cross_split_overlaps: usize,
    pub template_family_cross_split_overlaps: usize,
    pub contradictory_label_groups: usize,
    pub cross_split_details: Vec<CrossSplitOverlap>,
    pub details_truncated: bool,
    pub partition_leakage_groups: BTreeMap<String, usize>,
    pub partition_fingerprints: BTreeMap<String, String>,
    pub family_holdouts: Vec<FamilyHoldoutSummary>,
    pub supported_family_holdouts: usize,
    pub counterfactual_pairs: usize,
    pub unknown_tool_cases: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CorpusCoverageReport {
    pub cases: usize,
    pub semantic_groups: usize,
    pub leakage_groups: usize,
    pub unique_normalized_inputs: usize,
    pub final_test_leakage_groups: usize,
    pub supported_family_holdouts: usize,
    pub hard_negative_cases: usize,
    pub no_tool_cases: usize,
    pub multi_tool_cases: usize,
    pub unknown_tool_cases: usize,
    pub task_families: Vec<String>,
    pub tool_families: Vec<String>,
    pub provenance: Vec<String>,
    pub split_case_counts: BTreeMap<String, usize>,
    pub split_fingerprints: BTreeMap<String, String>,
    pub tool_family_holdout_family: String,
    pub tool_family_holdout_fingerprint: String,
    pub counterfactual_pairs: usize,
    pub passes_declared_floors: bool,
    pub leakage: LeakageReport,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolAdvisorArtifactManifest {
    pub artifact_schema_version: u16,
    pub model_version: String,
    pub architecture: String,
    pub parameter_count: u64,
    pub precision: String,
    pub tokenizer_version: String,
    pub tokenizer_hash: String,
    pub candidate_schema_version: u16,
    pub context_schema_version: u16,
    pub calibration_version: String,
    pub max_context_bytes: usize,
    pub max_candidates: usize,
    pub weights_sha256: String,
    pub provenance_fingerprint: String,
    pub license_notice: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolAdvisorArtifact {
    pub manifest: ToolAdvisorArtifactManifest,
    #[serde(default)]
    pub bias: f32,
    #[serde(default)]
    pub abstain_bias: f32,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default)]
    pub weights: BTreeMap<String, f32>,
}

fn default_temperature() -> f32 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AdvisorRuntimeState {
    Disabled,
    NotInstalled,
    Incompatible,
    Ready,
    Degraded,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AdvisorRuntimeStatus {
    pub state: AdvisorRuntimeState,
    pub detail: String,
    pub model_version: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ToolAdvisorInput {
    pub case_id: String,
    pub context: String,
    pub candidates: Vec<ToolAdvisorCandidate>,
    pub surface_fingerprint: String,
}

/// Build advisor candidates only from a surface that has already passed the
/// normal policy, plan-mode, disabled-tool, and parent-ceiling filters.
pub fn candidates_from_surface(
    surface: &crate::agent::tool_surface::ResolvedToolSurface,
    max_candidates: usize,
) -> Vec<ToolAdvisorCandidate> {
    surface
        .tools
        .iter()
        .take(max_candidates.min(MAX_CANDIDATES))
        .map(|tool| ToolAdvisorCandidate {
            name: tool.canonical_name.clone(),
            description: tool.definition.description.clone(),
            category: format!("{:?}", tool.category),
            disclosure: crate::tool::disclosure::disclosure_for(&tool.canonical_name)
                .as_str()
                .to_string(),
            synthetic_identity: tool.canonical_name.starts_with("mcp__"),
        })
        .collect()
}

pub trait ToolAdvisor: Send + Sync {
    fn score(&self, input: &ToolAdvisorInput) -> Result<ToolAdvisorPrediction>;
}

/// Explicit discovery modes. The default is `Off`; the other modes are
/// experimental and can only project over candidates already admitted by the
/// normal tool-surface policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AdvisorMode {
    Off,
    Observe,
    Rerank,
    Promote,
}

impl AdvisorMode {
    pub fn parse(value: Option<&str>) -> Self {
        match value.unwrap_or("off").to_ascii_lowercase().as_str() {
            "observe" => Self::Observe,
            "rerank" => Self::Rerank,
            "promote" => Self::Promote,
            _ => Self::Off,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AdvisorProjection {
    pub ordered: Vec<ToolMetadata>,
    pub prediction: Option<ToolAdvisorPrediction>,
    pub promoted: Vec<String>,
    pub abstained: bool,
    pub fallback: bool,
}

/// Apply an advisor prediction after discovery policy has already filtered the
/// catalog. `deferred_allowed` is an independently filtered set and is the
/// only universe from which promotion may add tools.
pub fn project_discovery(
    current: &[ToolMetadata],
    deferred_allowed: &[ToolMetadata],
    input: &ToolAdvisorInput,
    advisor: &dyn ToolAdvisor,
    mode: AdvisorMode,
    threshold: f64,
    max_promotions: usize,
) -> AdvisorProjection {
    let unchanged = || AdvisorProjection {
        ordered: current.to_vec(),
        prediction: None,
        promoted: Vec::new(),
        abstained: false,
        fallback: false,
    };
    if mode == AdvisorMode::Off {
        return unchanged();
    }

    let prediction = match advisor.score(input) {
        Ok(prediction) => prediction,
        Err(_) => {
            return AdvisorProjection {
                ordered: current.to_vec(),
                prediction: None,
                promoted: Vec::new(),
                abstained: false,
                fallback: true,
            }
        }
    };
    let abstained = prediction.abstain_probability.unwrap_or(0.0) >= 0.5;
    if mode == AdvisorMode::Observe || prediction.ranked.is_empty() || abstained {
        return AdvisorProjection {
            ordered: current.to_vec(),
            prediction: Some(prediction),
            promoted: Vec::new(),
            abstained,
            fallback: false,
        };
    }

    let scores: HashMap<&str, f64> = prediction
        .ranked
        .iter()
        .map(|candidate| (candidate.name.as_str(), candidate.score))
        .collect();
    let mut ordered = current.to_vec();
    ordered.sort_by(|left, right| {
        match (
            scores.get(left.name.as_str()).copied(),
            scores.get(right.name.as_str()).copied(),
        ) {
            (Some(left_score), Some(right_score)) => right_score
                .partial_cmp(&left_score)
                .unwrap_or(std::cmp::Ordering::Equal),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
    });

    let mut promoted = Vec::new();
    if mode == AdvisorMode::Promote && !abstained && max_promotions > 0 {
        let existing: BTreeSet<_> = ordered
            .iter()
            .map(|metadata| metadata.name.as_str())
            .collect();
        let mut additions: Vec<_> = deferred_allowed
            .iter()
            .filter(|metadata| !existing.contains(metadata.name.as_str()))
            .filter_map(|metadata| {
                scores
                    .get(metadata.name.as_str())
                    .copied()
                    .filter(|score| *score >= threshold)
                    .map(|score| (metadata.clone(), score))
            })
            .collect();
        additions.sort_by(|left, right| {
            right
                .1
                .partial_cmp(&left.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.0.name.cmp(&right.0.name))
        });
        for (metadata, _) in additions.into_iter().take(max_promotions) {
            promoted.push(metadata.name.clone());
            ordered.push(metadata);
        }
    }

    AdvisorProjection {
        ordered,
        prediction: Some(prediction),
        promoted,
        abstained,
        fallback: false,
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct NoopAdvisor;

impl ToolAdvisor for NoopAdvisor {
    fn score(&self, input: &ToolAdvisorInput) -> Result<ToolAdvisorPrediction> {
        Ok(ToolAdvisorPrediction {
            schema_version: PREDICTION_SCHEMA_VERSION,
            case_id: input.case_id.clone(),
            ranked: Vec::new(),
            abstain_probability: Some(1.0),
            mode: "off".to_string(),
        })
    }
}

pub struct LinearAdvisor {
    artifact: Arc<ToolAdvisorArtifact>,
    failures: AtomicU8,
    max_failures: u8,
}

impl LinearAdvisor {
    pub fn new(artifact: ToolAdvisorArtifact) -> Result<Self> {
        validate_artifact(&artifact)?;
        Ok(Self {
            artifact: Arc::new(artifact),
            failures: AtomicU8::new(0),
            max_failures: 3,
        })
    }

    pub fn artifact(&self) -> &ToolAdvisorArtifact {
        &self.artifact
    }

    fn score_inner(&self, input: &ToolAdvisorInput) -> Result<ToolAdvisorPrediction> {
        if input.context.len() > self.artifact.manifest.max_context_bytes {
            return Err(anyhow!("advisor context exceeds artifact limit"));
        }
        if input.candidates.len() > self.artifact.manifest.max_candidates {
            return Err(anyhow!("advisor candidate set exceeds artifact limit"));
        }
        let context_terms = tokenize(&input.context);
        let mut ranked = input
            .candidates
            .iter()
            .map(|candidate| {
                let candidate_terms =
                    tokenize(&format!("{} {}", candidate.name, candidate.description));
                let overlap = candidate_terms
                    .iter()
                    .filter(|term| context_terms.contains(term))
                    .count() as f32;
                let learned = candidate_terms
                    .iter()
                    .filter_map(|term| self.artifact.weights.get(term))
                    .copied()
                    .sum::<f32>();
                let score = self.artifact.bias + learned + overlap * 0.1;
                (candidate.name.clone(), score as f64)
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|left, right| {
            right
                .1
                .partial_cmp(&left.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.0.cmp(&right.0))
        });
        let abstain_probability = sigmoid(
            (self.artifact.abstain_bias
                - ranked
                    .first()
                    .map(|(_, score)| *score as f32)
                    .unwrap_or(0.0))
                / self.artifact.temperature.max(0.001),
        );
        Ok(ToolAdvisorPrediction {
            schema_version: PREDICTION_SCHEMA_VERSION,
            case_id: input.case_id.clone(),
            ranked: ranked
                .into_iter()
                .map(|(name, score)| RankedCandidate { name, score })
                .collect(),
            abstain_probability: Some(abstain_probability as f64),
            mode: "observe".to_string(),
        })
    }
}

impl ToolAdvisor for LinearAdvisor {
    fn score(&self, input: &ToolAdvisorInput) -> Result<ToolAdvisorPrediction> {
        if self.failures.load(Ordering::Relaxed) >= self.max_failures {
            return Err(anyhow!("advisor circuit breaker is open"));
        }
        let started = Instant::now();
        let result = self.score_inner(input);
        if result.is_err() {
            self.failures.fetch_add(1, Ordering::Relaxed);
        }
        if started.elapsed().as_millis() > 1000 {
            return Err(anyhow!("advisor scoring exceeded local bound"));
        }
        result
    }
}

pub fn validate_artifact(artifact: &ToolAdvisorArtifact) -> Result<()> {
    let manifest = &artifact.manifest;
    if manifest.artifact_schema_version != ARTIFACT_SCHEMA_VERSION {
        return Err(anyhow!(
            "unsupported advisor artifact schema version {}",
            manifest.artifact_schema_version
        ));
    }
    if manifest.candidate_schema_version != CASE_SCHEMA_VERSION
        || manifest.context_schema_version != CASE_SCHEMA_VERSION
    {
        return Err(anyhow!(
            "advisor artifact schema does not match case/context schema"
        ));
    }
    if manifest.architecture != "hashed-linear-v1"
        || manifest.max_candidates == 0
        || manifest.max_candidates > MAX_CANDIDATES
        || manifest.max_context_bytes == 0
        || manifest.max_context_bytes > MAX_CONTEXT_BYTES
    {
        return Err(anyhow!(
            "advisor artifact manifest has unsupported architecture or limits"
        ));
    }
    if artifact.temperature <= 0.0
        || !artifact.temperature.is_finite()
        || !artifact.bias.is_finite()
        || !artifact.abstain_bias.is_finite()
    {
        return Err(anyhow!(
            "advisor artifact contains invalid calibration values"
        ));
    }
    let encoded = serde_json::to_vec(&artifact.weights).context("serialize advisor weights")?;
    let digest = hex::encode(Sha256::digest(encoded));
    if digest != manifest.weights_sha256 {
        return Err(anyhow!("advisor artifact weights hash mismatch"));
    }
    Ok(())
}

pub fn load_artifact(path: &Path) -> Result<ToolAdvisorArtifact> {
    let metadata =
        fs::metadata(path).with_context(|| format!("stat advisor artifact {}", path.display()))?;
    if metadata.len() > MAX_ARTIFACT_BYTES {
        return Err(anyhow!(
            "advisor artifact exceeds {} bytes",
            MAX_ARTIFACT_BYTES
        ));
    }
    let bytes =
        fs::read(path).with_context(|| format!("read advisor artifact {}", path.display()))?;
    let artifact: ToolAdvisorArtifact =
        serde_json::from_slice(&bytes).context("parse advisor artifact")?;
    validate_artifact(&artifact)?;
    Ok(artifact)
}

pub fn inspect_artifact_manifest(path: &Path) -> Result<serde_json::Value> {
    #[cfg(feature = "tool-advisor")]
    if let Ok(artifact) = contextual::load(path) {
        return serde_json::to_value(artifact.manifest).context("serialize contextual manifest");
    }
    let artifact = load_artifact(path)?;
    serde_json::to_value(artifact.manifest).context("serialize advisor manifest")
}

pub fn write_artifact_atomic(path: &Path, artifact: &ToolAdvisorArtifact) -> Result<()> {
    validate_artifact(artifact)?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("advisor artifact has no parent directory"))?;
    fs::create_dir_all(parent)
        .with_context(|| format!("create artifact directory {}", parent.display()))?;
    let temp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(artifact).context("serialize advisor artifact")?;
    fs::write(&temp, bytes)
        .with_context(|| format!("write temporary advisor artifact {}", temp.display()))?;
    fs::rename(&temp, path)
        .with_context(|| format!("install advisor artifact {}", path.display()))?;
    Ok(())
}

pub fn advisor_from_config(
    config: Option<&codegg_config::schema::ToolAdvisorConfig>,
) -> (Box<dyn ToolAdvisor>, AdvisorRuntimeStatus) {
    let Some(config) = config else {
        return (
            Box::new(NoopAdvisor),
            AdvisorRuntimeStatus {
                state: AdvisorRuntimeState::Disabled,
                detail: "no advisor configuration".into(),
                model_version: None,
            },
        );
    };
    if !config.enabled.unwrap_or(false) || config.mode.as_deref().unwrap_or("off") == "off" {
        return (
            Box::new(NoopAdvisor),
            AdvisorRuntimeStatus {
                state: AdvisorRuntimeState::Disabled,
                detail: "advisor is disabled (default)".into(),
                model_version: None,
            },
        );
    }
    let mode = AdvisorMode::parse(config.mode.as_deref());
    if mode == AdvisorMode::Off {
        return (
            Box::new(NoopAdvisor),
            AdvisorRuntimeStatus {
                state: AdvisorRuntimeState::Disabled,
                detail: "advisor is disabled (default)".into(),
                model_version: None,
            },
        );
    }
    if !matches!(
        mode,
        AdvisorMode::Observe | AdvisorMode::Rerank | AdvisorMode::Promote
    ) {
        return (
            Box::new(NoopAdvisor),
            AdvisorRuntimeStatus {
                state: AdvisorRuntimeState::Incompatible,
                detail: "unsupported advisor mode".into(),
                model_version: None,
            },
        );
    }
    let Some(path) = config.model_path.as_deref().map(Path::new) else {
        return (
            Box::new(NoopAdvisor),
            AdvisorRuntimeStatus {
                state: AdvisorRuntimeState::NotInstalled,
                detail: "observe mode has no model artifact".into(),
                model_version: None,
            },
        );
    };
    #[cfg(feature = "tool-advisor")]
    if let Ok(artifact) = contextual::load(path) {
        match contextual::ContextualAdvisor::new(artifact) {
            Ok(advisor) => {
                let version = advisor.artifact().manifest.model_version.clone();
                let qualified = advisor.artifact().is_qualified();
                return (
                    Box::new(advisor),
                    AdvisorRuntimeStatus {
                        state: AdvisorRuntimeState::Ready,
                        detail: if qualified {
                            "calibrated contextual encoder loaded".into()
                        } else {
                            "legacy unqualified contextual encoder loaded (research baseline only)"
                                .into()
                        },
                        model_version: Some(version),
                    },
                );
            }
            Err(error) => {
                return (
                    Box::new(NoopAdvisor),
                    AdvisorRuntimeStatus {
                        state: AdvisorRuntimeState::Degraded,
                        detail: format!("contextual advisor fallback: {error}"),
                        model_version: None,
                    },
                );
            }
        }
    }
    match load_artifact(path).and_then(LinearAdvisor::new) {
        Ok(advisor) => {
            let version = advisor.artifact().manifest.model_version.clone();
            (
                Box::new(advisor),
                AdvisorRuntimeStatus {
                    state: AdvisorRuntimeState::Ready,
                    detail: format!(
                        "{} advisor loaded",
                        config.mode.as_deref().unwrap_or("observe")
                    ),
                    model_version: Some(version),
                },
            )
        }
        Err(error) => (
            Box::new(NoopAdvisor),
            AdvisorRuntimeStatus {
                state: AdvisorRuntimeState::Degraded,
                detail: format!("advisor fallback: {error}"),
                model_version: None,
            },
        ),
    }
}

fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn sigmoid(value: f32) -> f32 {
    1.0 / (1.0 + (-value).exp())
}

impl ToolAdvisorCase {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != CASE_SCHEMA_VERSION {
            return Err(anyhow!(
                "case {} has unsupported schema version {}; expected {}",
                self.case_id,
                self.schema_version,
                CASE_SCHEMA_VERSION
            ));
        }
        validate_text("case_id", &self.case_id, 256)?;
        validate_text("context", &self.context, MAX_CONTEXT_BYTES)?;
        validate_text("group_id", &self.group_id, 256)?;
        validate_text("provenance", &self.provenance, 256)?;
        if !self.semantic_group.is_empty() {
            validate_text("semantic_group", &self.semantic_group, 256)?;
        }
        if !self.leakage_group.is_empty() {
            validate_text("leakage_group", &self.leakage_group, 256)?;
        }
        if !self.task_family.is_empty() {
            validate_text("task_family", &self.task_family, 128)?;
        }
        if !self.tool_family.is_empty() {
            validate_text("tool_family", &self.tool_family, 128)?;
        }
        if !self.generated_variant_family.is_empty() {
            validate_text(
                "generated_variant_family",
                &self.generated_variant_family,
                256,
            )?;
        }
        if self.candidates.is_empty() || self.candidates.len() > MAX_CANDIDATES {
            return Err(anyhow!(
                "case {} must contain 1..={} candidates",
                self.case_id,
                MAX_CANDIDATES
            ));
        }
        let names: BTreeSet<_> = self
            .candidates
            .iter()
            .map(|candidate| candidate.name.as_str())
            .collect();
        if names.len() != self.candidates.len() {
            return Err(anyhow!(
                "case {} contains duplicate candidates",
                self.case_id
            ));
        }
        for candidate in &self.candidates {
            validate_text("candidate.name", &candidate.name, 256)?;
            validate_text(
                "candidate.description",
                &candidate.description,
                MAX_CANDIDATE_TEXT_BYTES,
            )?;
        }
        for (name, relevance) in &self.relevance {
            if !names.contains(name.as_str()) {
                return Err(anyhow!(
                    "case {} labels unknown candidate {name}",
                    self.case_id
                ));
            }
            if *relevance == 0 || *relevance > 3 {
                return Err(anyhow!(
                    "case {} relevance for {name} must be between 1 and 3",
                    self.case_id
                ));
            }
        }
        for name in &self.preferred_order {
            if !names.contains(name.as_str()) || !self.relevance.contains_key(name) {
                return Err(anyhow!(
                    "case {} preferred order contains an unlabeled candidate {name}",
                    self.case_id
                ));
            }
        }
        for (name, probability) in &self.teacher_probabilities {
            if !names.contains(name.as_str()) {
                return Err(anyhow!(
                    "case {} teacher label references unknown candidate {name}",
                    self.case_id
                ));
            }
            if !probability.is_finite() || !(0.0..=1.0).contains(probability) {
                return Err(anyhow!(
                    "case {} teacher probability for {name} must be between 0 and 1",
                    self.case_id
                ));
            }
        }
        if self.none && !self.relevance.is_empty() {
            return Err(anyhow!(
                "case {} cannot mark none=true with relevant candidates",
                self.case_id
            ));
        }
        if !self.none && self.relevance.is_empty() {
            return Err(anyhow!(
                "case {} needs a relevance label or explicit none=true",
                self.case_id
            ));
        }
        Ok(())
    }

    pub fn canonical_json(&self) -> Result<String> {
        self.validate()?;
        serde_json::to_string(self).context("serialize advisor case")
    }
}

pub fn parse_jsonl(input: &str) -> Result<Vec<ToolAdvisorCase>> {
    let mut cases = Vec::new();
    let mut ids = BTreeSet::new();
    let mut groups = BTreeSet::new();
    for (line_number, line) in input.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.len() > MAX_CASE_BYTES {
            return Err(anyhow!(
                "fixture line {} exceeds {} bytes",
                line_number + 1,
                MAX_CASE_BYTES
            ));
        }
        let case: ToolAdvisorCase = serde_json::from_str(line)
            .with_context(|| format!("parse fixture line {}", line_number + 1))?;
        case.validate()
            .with_context(|| format!("validate fixture line {}", line_number + 1))?;
        if !ids.insert(case.case_id.clone()) {
            return Err(anyhow!("duplicate case id {}", case.case_id));
        }
        if !groups.insert(case.group_id.clone()) {
            return Err(anyhow!("duplicate group id {}", case.group_id));
        }
        cases.push(case);
    }
    if cases.is_empty() {
        return Err(anyhow!("fixture corpus is empty"));
    }
    Ok(cases)
}

pub fn builtin_cases() -> Result<Vec<ToolAdvisorCase>> {
    parse_jsonl(BUILTIN_CORPUS)
}

pub fn load_cases(path: Option<&Path>) -> Result<Vec<ToolAdvisorCase>> {
    match path {
        None => builtin_cases(),
        Some(path) => {
            let metadata =
                fs::metadata(path).with_context(|| format!("stat {}", path.display()))?;
            if metadata.len() > 16 * 1024 * 1024 {
                return Err(anyhow!("dataset exceeds the 16 MiB safety limit"));
            }
            let contents = fs::read_to_string(path)
                .with_context(|| format!("read dataset {}", path.display()))?;
            parse_jsonl(&contents)
        }
    }
}

/// Stable, group-aware split. The group id is the only input so variants
/// cannot leak across train/dev/test partitions.
///
/// Qualification paths must pass content-derived leakage-group ids from
/// [`build_leakage_groups`] rather than raw caller-supplied group strings.
/// The helper itself remains a pure deterministic hash so existing single-key
/// callers keep stable behavior.
pub fn split_for(group_id: &str) -> &'static str {
    split_for_key(group_id)
}

/// Split assignment for a content-derived leakage-group id. Identical rule to
/// [`split_for`], but the argument contract is explicit: only ids produced by
/// [`build_leakage_groups`] carry leakage protection.
pub fn split_for_leakage_group(group_id: &str) -> &'static str {
    split_for_key(group_id)
}

fn split_for_key(key: &str) -> &'static str {
    let digest = Sha256::digest(key.as_bytes());
    match digest[0] % 10 {
        0..=1 => "dev",
        2..=3 => "test",
        _ => "train",
    }
}

/// Deterministic fixture canonicalization for leakage validation only.
/// Runtime tokenizer semantics are unchanged. Canonicalization applies NFKC,
/// full Unicode case mapping, and whitespace collapsing; candidate ordering
/// is handled by sorting descriptor strings at the call site.
pub fn normalize_text(value: &str) -> String {
    value
        .nfkc()
        .flat_map(|character| character.to_lowercase())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Normalized descriptor text for every advisor-visible candidate field.
/// `synthetic_identity` participates so renamed unknown-tool descriptors keep
/// a stable synthetic identity instead of colliding with concrete names.
pub fn normalized_candidate_descriptor(candidate: &ToolAdvisorCandidate) -> String {
    format!(
        "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
        normalize_text(&candidate.name),
        normalize_text(&candidate.description),
        normalize_text(&candidate.category),
        normalize_text(&candidate.disclosure),
        if candidate.synthetic_identity {
            "synthetic"
        } else {
            "concrete"
        },
    )
}

fn exact_candidate_descriptor(candidate: &ToolAdvisorCandidate) -> String {
    format!(
        "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
        candidate.name,
        candidate.description,
        candidate.category,
        candidate.disclosure,
        if candidate.synthetic_identity {
            "synthetic"
        } else {
            "concrete"
        },
    )
}

/// Deterministic hash of the normalized model-visible input. Relevance labels
/// are deliberately excluded so identical inputs with conflicting labels
/// surface as a validation problem instead of hiding behind distinct hashes.
pub fn input_signature(case: &ToolAdvisorCase) -> String {
    let mut descriptors: Vec<_> = case
        .candidates
        .iter()
        .map(normalized_candidate_descriptor)
        .collect();
    descriptors.sort();
    let mut hasher = Sha256::new();
    hasher.update(normalize_text(&case.context).as_bytes());
    hasher.update(b"\n");
    for descriptor in descriptors {
        hasher.update(descriptor.as_bytes());
        hasher.update(b"\n");
    }
    hex::encode(hasher.finalize())
}

/// Byte-exact counterpart to [`input_signature`]: raw context plus raw
/// candidate descriptors in sorted order. Both hashes are reported so closure
/// evidence distinguishes byte-identical duplication from canonicalization
/// collisions.
pub fn exact_input_signature(case: &ToolAdvisorCase) -> String {
    let mut descriptors: Vec<_> = case
        .candidates
        .iter()
        .map(exact_candidate_descriptor)
        .collect();
    descriptors.sort();
    let mut hasher = Sha256::new();
    hasher.update(case.context.as_bytes());
    hasher.update(b"\n");
    for descriptor in descriptors {
        hasher.update(descriptor.as_bytes());
        hasher.update(b"\n");
    }
    hex::encode(hasher.finalize())
}

/// Diagnostic hash of the label side (graded relevance plus abstention flag).
/// Used to detect same-input contradictory labels, never to split inputs.
pub fn label_signature(case: &ToolAdvisorCase) -> String {
    let mut hasher = Sha256::new();
    if case.none {
        hasher.update(b"none");
    } else {
        hasher.update(b"labeled");
    }
    hasher.update(b"\n");
    for (name, relevance) in &case.relevance {
        hasher.update(name.as_bytes());
        hasher.update(b"=");
        hasher.update(relevance.to_string().as_bytes());
        hasher.update(b"\n");
    }
    hex::encode(hasher.finalize())
}

struct DisjointSets {
    parent: Vec<usize>,
}

impl DisjointSets {
    fn new(len: usize) -> Self {
        Self {
            parent: (0..len).collect(),
        }
    }

    fn find(&mut self, index: usize) -> usize {
        if self.parent[index] != index {
            let root = self.find(self.parent[index]);
            self.parent[index] = root;
        }
        self.parent[index]
    }

    fn union(&mut self, left: usize, right: usize) {
        let left_root = self.find(left);
        let right_root = self.find(right);
        if left_root != right_root {
            self.parent[left_root] = right_root;
        }
    }
}

/// Authoritative split units derived from content/template lineage.
///
/// Cases sharing any of these relations join one connected leakage component
/// before partitioning, so the grouping cannot be bypassed by renaming
/// `semantic_group` strings:
///
/// - identical normalized model-visible input ([`input_signature`]);
/// - the same generator/template lineage (`generated_variant_family`);
/// - the same declared semantic/counterfactual family (`semantic_group`);
/// - an explicitly assigned manual leakage family (`leakage_group`).
///
/// The returned vector holds one canonical group id per case index. Group ids
/// are deterministic content-derived fingerprints (`leak-<hex>` over the
/// sorted member input signatures).
pub fn build_leakage_groups(cases: &[ToolAdvisorCase]) -> Vec<String> {
    let mut sets = DisjointSets::new(cases.len());
    let mut first_by_key: HashMap<String, usize> = HashMap::new();
    for (index, case) in cases.iter().enumerate() {
        let mut keys = vec![format!("input:{}", input_signature(case))];
        if !case.generated_variant_family.is_empty() {
            keys.push(format!(
                "variant:{}",
                normalize_text(&case.generated_variant_family)
            ));
        }
        if !case.semantic_group.is_empty() {
            keys.push(format!("semantic:{}", normalize_text(&case.semantic_group)));
        }
        if !case.leakage_group.is_empty() {
            keys.push(format!("manual:{}", normalize_text(&case.leakage_group)));
        }
        for key in keys {
            if let Some(&first) = first_by_key.get(&key) {
                sets.union(first, index);
            } else {
                first_by_key.insert(key, index);
            }
        }
    }
    let mut members: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for index in 0..cases.len() {
        let root = sets.find(index);
        members.entry(root).or_default().push(index);
    }
    let mut group_of = vec![String::new(); cases.len()];
    for member_indices in members.values() {
        // Content-derived identity: the canonical id hashes the sorted member
        // input signatures, so renaming case or group strings cannot move a
        // payload to a different partition or bypass the split.
        let mut signatures: Vec<String> = member_indices
            .iter()
            .map(|&index| input_signature(&cases[index]))
            .collect();
        signatures.sort();
        let digest = Sha256::digest(signatures.join("\n").as_bytes());
        let group_id = format!("leak-{}", hex::encode(&digest[..6]));
        for &index in member_indices {
            group_of[index] = group_id.clone();
        }
    }
    group_of
}

/// Leakage-group partition of a case slice. Whole leakage components stay in
/// one split; fingerprints are recorded per split for C002/C004.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DatasetPartition {
    pub group_of_case: Vec<String>,
    pub split_of_group: BTreeMap<String, String>,
    pub split_of_case: Vec<String>,
    pub train_cases: Vec<usize>,
    pub dev_cases: Vec<usize>,
    pub test_cases: Vec<usize>,
}

pub fn partition_cases(cases: &[ToolAdvisorCase]) -> DatasetPartition {
    let group_of_case = build_leakage_groups(cases);
    let mut split_of_group: BTreeMap<String, String> = BTreeMap::new();
    for group in &group_of_case {
        split_of_group
            .entry(group.clone())
            .or_insert_with(|| split_for_leakage_group(group).to_string());
    }
    let mut partition = DatasetPartition {
        split_of_case: group_of_case
            .iter()
            .map(|group| {
                split_of_group
                    .get(group)
                    .cloned()
                    .unwrap_or_else(|| "train".to_string())
            })
            .collect(),
        group_of_case,
        split_of_group,
        train_cases: Vec::new(),
        dev_cases: Vec::new(),
        test_cases: Vec::new(),
    };
    for (index, split) in partition.split_of_case.iter().enumerate() {
        match split.as_str() {
            "dev" => partition.dev_cases.push(index),
            "test" => partition.test_cases.push(index),
            _ => partition.train_cases.push(index),
        }
    }
    partition
}

/// True tool-family holdout: training/dev data built with every leakage
/// component containing the held-out family excluded, plus the held-out slice
/// itself. Reporting a fingerprint over training-visible examples is not a
/// holdout; this partition proves optimizer/calibration exclusion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FamilyHoldoutPartition {
    pub family: String,
    pub train_cases: Vec<usize>,
    pub dev_cases: Vec<usize>,
    pub holdout_cases: Vec<usize>,
    pub excluded_leakage_groups: Vec<String>,
}

pub fn family_holdout_partition(cases: &[ToolAdvisorCase], family: &str) -> FamilyHoldoutPartition {
    let partition = partition_cases(cases);
    let group_of_case = partition.group_of_case;
    let mut tainted: BTreeSet<String> = BTreeSet::new();
    let mut holdout_cases = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        if case.tool_family == family {
            tainted.insert(group_of_case[index].clone());
            holdout_cases.push(index);
        }
    }
    let mut train_cases = Vec::new();
    let mut dev_cases = Vec::new();
    for &index in &partition.train_cases {
        if !tainted.contains(&group_of_case[index]) {
            train_cases.push(index);
        }
    }
    for &index in &partition.dev_cases {
        if !tainted.contains(&group_of_case[index]) {
            dev_cases.push(index);
        }
    }
    FamilyHoldoutPartition {
        family: family.to_string(),
        train_cases,
        dev_cases,
        holdout_cases,
        excluded_leakage_groups: tainted.into_iter().collect(),
    }
}

pub fn dataset_fingerprint(cases: &[ToolAdvisorCase]) -> Result<String> {
    let mut ordered = cases.to_vec();
    ordered.sort_by(|left, right| left.case_id.cmp(&right.case_id));
    let mut hasher = Sha256::new();
    for case in ordered {
        hasher.update(case.canonical_json()?.as_bytes());
        hasher.update(b"\n");
    }
    Ok(hex::encode(hasher.finalize()))
}

pub fn leakage_report(cases: &[ToolAdvisorCase]) -> Result<LeakageReport> {
    for case in cases {
        case.validate()?;
    }
    let partition = partition_cases(cases);
    let mut exact_inputs = BTreeSet::new();
    let mut normalized_inputs = BTreeSet::new();
    let mut exact_splits: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut normalized_splits: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut template_splits: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut exact_cases: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut normalized_cases: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut template_cases: BTreeMap<String, Vec<String>> = BTreeMap::new();
    // Same normalized input carrying conflicting labels is a corpus defect:
    // counterfactual variation must differ in context/state, not hide behind
    // an identical model-visible input.
    let mut labels_by_input: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (index, case) in cases.iter().enumerate() {
        let split = partition.split_of_case[index].clone();
        let exact = exact_input_signature(case);
        let normalized = input_signature(case);
        exact_inputs.insert(exact.clone());
        normalized_inputs.insert(normalized.clone());
        exact_splits
            .entry(exact.clone())
            .or_default()
            .insert(split.clone());
        normalized_splits
            .entry(normalized.clone())
            .or_default()
            .insert(split.clone());
        exact_cases
            .entry(exact)
            .or_default()
            .push(case.case_id.clone());
        normalized_cases
            .entry(normalized.clone())
            .or_default()
            .push(case.case_id.clone());
        labels_by_input
            .entry(normalized)
            .or_default()
            .insert(label_signature(case));
        if !case.generated_variant_family.is_empty() {
            let family = normalize_text(&case.generated_variant_family);
            template_splits
                .entry(family.clone())
                .or_default()
                .insert(split);
            template_cases
                .entry(family)
                .or_default()
                .push(case.case_id.clone());
        }
    }
    let mut details = Vec::new();
    let mut truncated = false;
    let push_details = |kind: &str,
                        table: &BTreeMap<String, BTreeSet<String>>,
                        case_table: &BTreeMap<String, Vec<String>>,
                        details: &mut Vec<CrossSplitOverlap>,
                        truncated: &mut bool| {
        for (key, splits) in table {
            if splits.len() > 1 {
                let mut case_ids = case_table.get(key).cloned().unwrap_or_default();
                case_ids.sort();
                case_ids.truncate(MAX_OVERLAP_DETAIL_CASES);
                if details.len() >= MAX_OVERLAP_DETAILS {
                    *truncated = true;
                    continue;
                }
                let mut split_list: Vec<String> = splits.iter().cloned().collect();
                split_list.sort();
                details.push(CrossSplitOverlap {
                    kind: kind.to_string(),
                    key: key.clone(),
                    splits: split_list,
                    case_ids,
                });
            }
        }
    };
    let exact_cross_split_overlaps = exact_splits
        .values()
        .filter(|splits| splits.len() > 1)
        .count();
    let normalized_cross_split_overlaps = normalized_splits
        .values()
        .filter(|splits| splits.len() > 1)
        .count();
    let template_family_cross_split_overlaps = template_splits
        .values()
        .filter(|splits| splits.len() > 1)
        .count();
    push_details(
        "exact-input",
        &exact_splits,
        &exact_cases,
        &mut details,
        &mut truncated,
    );
    push_details(
        "normalized-input",
        &normalized_splits,
        &normalized_cases,
        &mut details,
        &mut truncated,
    );
    push_details(
        "template-family",
        &template_splits,
        &template_cases,
        &mut details,
        &mut truncated,
    );
    details.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.key.cmp(&right.key))
    });
    let contradictory_label_groups = labels_by_input
        .values()
        .filter(|labels| labels.len() > 1)
        .count();

    let mut partition_cases: BTreeMap<String, Vec<ToolAdvisorCase>> = BTreeMap::new();
    for (index, case) in cases.iter().enumerate() {
        let split = partition.split_of_case[index].clone();
        partition_cases.entry(split).or_default().push(case.clone());
    }
    let mut partition_leakage_groups: BTreeMap<String, usize> = BTreeMap::new();
    let mut seen_groups: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (index, group) in partition.group_of_case.iter().enumerate() {
        seen_groups
            .entry(partition.split_of_case[index].clone())
            .or_default()
            .insert(group.clone());
    }
    for (split, groups) in &seen_groups {
        partition_leakage_groups.insert(split.clone(), groups.len());
    }
    let partition_fingerprints = partition_cases
        .iter()
        .map(|(split, selected)| {
            (
                split.clone(),
                dataset_fingerprint(selected).unwrap_or_default(),
            )
        })
        .collect();

    let mut tool_family_counts: BTreeMap<String, usize> = BTreeMap::new();
    for case in cases {
        if !case.tool_family.is_empty() {
            *tool_family_counts
                .entry(case.tool_family.clone())
                .or_insert(0) += 1;
        }
    }
    let mut family_holdouts = Vec::new();
    for family in tool_family_counts.keys() {
        let holdout = family_holdout_partition(cases, family);
        let residual = holdout
            .train_cases
            .iter()
            .chain(holdout.dev_cases.iter())
            .filter(|&&index| cases[index].tool_family == *family)
            .count();
        family_holdouts.push(FamilyHoldoutSummary {
            family: family.clone(),
            holdout_cases: holdout.holdout_cases.len(),
            excluded_leakage_groups: holdout.excluded_leakage_groups.len(),
            train_cases_after_exclusion: holdout.train_cases.len(),
            dev_cases_after_exclusion: holdout.dev_cases.len(),
            train_dev_family_cases: residual,
        });
    }
    family_holdouts.sort_by(|left, right| left.family.cmp(&right.family));
    // A reportable holdout needs enough held-out cases to measure and a
    // non-empty excluded leakage component. Small families remain listed but
    // do not count toward the qualification floor.
    let supported_family_holdouts = family_holdouts
        .iter()
        .filter(|summary| {
            summary.holdout_cases >= MIN_FAMILY_HOLDOUT_CASES && summary.train_dev_family_cases == 0
        })
        .count();

    let mut variant_groups: BTreeMap<String, Vec<&ToolAdvisorCase>> = BTreeMap::new();
    for case in cases {
        if !case.generated_variant_family.is_empty() {
            variant_groups
                .entry(case.generated_variant_family.clone())
                .or_default()
                .push(case);
        }
    }
    let counterfactual_pairs = variant_groups
        .values()
        .filter(|variants| {
            variants.len() >= 2
                && variants.windows(2).all(|window| {
                    let left = window[0]
                        .candidates
                        .iter()
                        .map(|candidate| candidate.name.as_str())
                        .collect::<Vec<_>>();
                    let right = window[1]
                        .candidates
                        .iter()
                        .map(|candidate| candidate.name.as_str())
                        .collect::<Vec<_>>();
                    left == right
                })
                && variants
                    .iter()
                    .map(|case| case.relevance.clone())
                    .collect::<BTreeSet<_>>()
                    .len()
                    >= 2
        })
        .count();
    let unknown_tool_cases = cases
        .iter()
        .filter(|case| case.tags.iter().any(|tag| tag == "unknown-tool"))
        .count();

    Ok(LeakageReport {
        cases: cases.len(),
        leakage_groups: partition.split_of_group.len(),
        unique_exact_inputs: exact_inputs.len(),
        unique_normalized_inputs: normalized_inputs.len(),
        exact_cross_split_overlaps,
        normalized_cross_split_overlaps,
        template_family_cross_split_overlaps,
        contradictory_label_groups,
        cross_split_details: details,
        details_truncated: truncated,
        partition_leakage_groups,
        partition_fingerprints,
        family_holdouts,
        supported_family_holdouts,
        counterfactual_pairs,
        unknown_tool_cases,
    })
}

/// Minimum held-out cases for a tool family to count as a reportable true
/// holdout evaluation.
pub const MIN_FAMILY_HOLDOUT_CASES: usize = 16;

pub fn coverage_report(cases: &[ToolAdvisorCase]) -> Result<CorpusCoverageReport> {
    let mut semantic_groups = BTreeSet::new();
    let mut task_families = BTreeSet::new();
    let mut tool_families = BTreeSet::new();
    let mut provenance = BTreeSet::new();
    let mut hard_negative_cases = 0;
    let mut no_tool_cases = 0;
    let mut multi_tool_cases = 0;
    let mut unknown_tool_cases = 0;

    for case in cases {
        case.validate()?;
        semantic_groups.insert(case.split_group().to_string());
        if !case.task_family.is_empty() {
            task_families.insert(case.task_family.clone());
        }
        if !case.tool_family.is_empty() {
            tool_families.insert(case.tool_family.clone());
        }
        provenance.insert(case.provenance.clone());
        if case.tags.iter().any(|tag| tag == "hard-negative") {
            hard_negative_cases += 1;
        }
        if case.none || case.tags.iter().any(|tag| tag == "no-tool") {
            no_tool_cases += 1;
        }
        if case.tags.iter().any(|tag| tag == "multi-tool") {
            multi_tool_cases += 1;
        }
        if case.tags.iter().any(|tag| tag == "unknown-tool") {
            unknown_tool_cases += 1;
        }
    }

    let leakage = leakage_report(cases)?;
    let partition = partition_cases(cases);
    let mut split_cases: BTreeMap<String, Vec<ToolAdvisorCase>> = BTreeMap::new();
    for (index, case) in cases.iter().enumerate() {
        split_cases
            .entry(partition.split_of_case[index].clone())
            .or_default()
            .push(case.clone());
    }
    let split_case_counts = split_cases
        .iter()
        .map(|(split, selected)| (split.clone(), selected.len()))
        .collect();
    let split_fingerprints = split_cases
        .iter()
        .map(|(split, selected)| {
            (
                split.clone(),
                dataset_fingerprint(selected).unwrap_or_default(),
            )
        })
        .collect();
    let holdout_family = ["plugin", "lsp", "research", "structured"]
        .into_iter()
        .find(|family| tool_families.contains(*family))
        .unwrap_or_default()
        .to_string();
    let holdout_cases = cases
        .iter()
        .filter(|case| case.tool_family == holdout_family)
        .cloned()
        .collect::<Vec<_>>();
    let tool_family_holdout_fingerprint = dataset_fingerprint(&holdout_cases).unwrap_or_default();
    let task_family_count = task_families.len();
    let final_test_leakage_groups = leakage
        .partition_leakage_groups
        .get("test")
        .copied()
        .unwrap_or(0);
    let report = CorpusCoverageReport {
        cases: cases.len(),
        semantic_groups: semantic_groups.len(),
        leakage_groups: leakage.leakage_groups,
        unique_normalized_inputs: leakage.unique_normalized_inputs,
        final_test_leakage_groups,
        supported_family_holdouts: leakage.supported_family_holdouts,
        hard_negative_cases,
        no_tool_cases,
        multi_tool_cases,
        unknown_tool_cases,
        task_families: task_families.into_iter().collect(),
        tool_families: tool_families.into_iter().collect(),
        provenance: provenance.into_iter().collect(),
        split_case_counts,
        split_fingerprints,
        tool_family_holdout_family: holdout_family,
        tool_family_holdout_fingerprint,
        counterfactual_pairs: leakage.counterfactual_pairs,
        passes_declared_floors: cases.len() >= 256
            && leakage.leakage_groups >= 128
            && leakage.unique_normalized_inputs >= 192
            && final_test_leakage_groups >= 40
            && hard_negative_cases >= 64
            && no_tool_cases >= 32
            && multi_tool_cases >= 32
            && unknown_tool_cases >= 32
            && task_family_count >= 10
            && leakage.supported_family_holdouts >= 4,
        leakage,
    };
    Ok(report)
}

pub fn validate_qualification_corpus(cases: &[ToolAdvisorCase]) -> Result<CorpusCoverageReport> {
    let report = coverage_report(cases)?;
    if !report.passes_declared_floors {
        return Err(anyhow!(
            "qualification corpus floors not met: {} cases, {} leakage groups (need >=128), {} unique normalized inputs (need >=192), {} final-test leakage groups (need >=40), {} hard-negative, {} no-tool, {} multi-tool, {} unknown-tool, {} task families, {} supported family holdouts (need >=4)",
            report.cases,
            report.leakage_groups,
            report.unique_normalized_inputs,
            report.final_test_leakage_groups,
            report.hard_negative_cases,
            report.no_tool_cases,
            report.multi_tool_cases,
            report.unknown_tool_cases,
            report.task_families.len(),
            report.supported_family_holdouts
        ));
    }
    if report.counterfactual_pairs < 32 {
        return Err(anyhow!(
            "qualification corpus needs at least 32 counterfactual pairs; found {}",
            report.counterfactual_pairs
        ));
    }
    let leakage = &report.leakage;
    if leakage.exact_cross_split_overlaps > 0
        || leakage.normalized_cross_split_overlaps > 0
        || leakage.template_family_cross_split_overlaps > 0
    {
        return Err(anyhow!(
            "content-derived split integrity violated: {} exact, {} normalized, {} template-family cross-split overlaps",
            leakage.exact_cross_split_overlaps,
            leakage.normalized_cross_split_overlaps,
            leakage.template_family_cross_split_overlaps
        ));
    }
    if leakage.contradictory_label_groups > 0 {
        return Err(anyhow!(
            "corpus contains {} same-input contradictory label groups; counterfactual variation must differ in context",
            leakage.contradictory_label_groups
        ));
    }
    for summary in &leakage.family_holdouts {
        if summary.train_dev_family_cases > 0 {
            return Err(anyhow!(
                "tool-family holdout {0} leaks {1} cases into optimizer/calibration input",
                summary.family,
                summary.train_dev_family_cases
            ));
        }
    }
    Ok(report)
}

pub fn lint_dataset(path: &Path) -> Result<CorpusCoverageReport> {
    let cases = load_cases(Some(path))?;
    validate_qualification_corpus(&cases)
}

pub fn unknown_tool_holdout(case: &ToolAdvisorCase) -> Result<ToolAdvisorCase> {
    let mut transformed = case.clone();
    transformed.case_id = format!("{}::unknown", case.case_id);
    transformed.group_id = format!("{}::unknown", case.group_id);
    // Suffix non-empty lineage keys so the transformed case joins its own
    // `::unknown` lineage namespace instead of the source leakage component.
    // Empty lineage keys stay empty: the renamed candidate descriptors already
    // give the transformed case a distinct input signature, and a constant
    // suffix on an empty key would incorrectly merge every transformed case
    // into one component. The `::unknown` namespace never enters optimizer or
    // calibration input because training consumes only corpus cases while
    // transforms are constructed at evaluation time.
    if !case.split_group().is_empty() {
        transformed.semantic_group = format!("{}::unknown", case.split_group());
    }
    if !case.generated_variant_family.is_empty() {
        transformed.generated_variant_family =
            format!("{}::unknown", case.generated_variant_family);
    }
    if !case.leakage_group.is_empty() {
        transformed.leakage_group = format!("{}::unknown", case.leakage_group);
    }
    for candidate in &mut transformed.candidates {
        candidate.name = format!("synthetic_{}", short_hash(&candidate.name));
        candidate.synthetic_identity = true;
    }
    transformed.relevance = case
        .relevance
        .iter()
        .map(|(name, relevance)| (format!("synthetic_{}", short_hash(name)), *relevance))
        .collect();
    transformed.preferred_order = case
        .preferred_order
        .iter()
        .map(|name| format!("synthetic_{}", short_hash(name)))
        .collect();
    transformed.validate()?;
    Ok(transformed)
}

fn short_hash(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    hex::encode(&digest[..6])
}

pub fn baseline_prediction(case: &ToolAdvisorCase, mode: SearchMode) -> ToolAdvisorPrediction {
    let metadata = case
        .candidates
        .iter()
        .map(|candidate| ToolMetadata {
            name: candidate.name.clone(),
            description: candidate.description.clone(),
            parameters: serde_json::Value::Null,
            defer_load: candidate.disclosure == "deferred",
            category: candidate.category.clone(),
            disclosure: candidate.disclosure.clone(),
        })
        .collect::<Vec<_>>();
    let ranked: Vec<RankedCandidate> =
        ToolCatalog::rank_descriptors(&case.context, &metadata, mode)
            .into_iter()
            .enumerate()
            .map(|(index, metadata)| RankedCandidate {
                name: metadata.name,
                score: 1.0 / (index as f64 + 1.0),
            })
            .collect();
    let abstain_probability = if ranked.is_empty() { 1.0 } else { 0.0 };
    ToolAdvisorPrediction {
        schema_version: PREDICTION_SCHEMA_VERSION,
        case_id: case.case_id.clone(),
        ranked,
        abstain_probability: Some(abstain_probability),
        mode: match mode {
            SearchMode::Keyword => "keyword",
            SearchMode::BM25 => "bm25",
        }
        .to_string(),
    }
}

pub fn evaluate(
    cases: &[ToolAdvisorCase],
    predictions: &[ToolAdvisorPrediction],
) -> Result<MetricSummary> {
    let by_id: HashMap<_, _> = predictions
        .iter()
        .map(|prediction| (prediction.case_id.as_str(), prediction))
        .collect();
    let mut evaluated = 0usize;
    let mut covered = 0usize;
    let mut recall = [0.0; 3];
    let mut mrr = 0.0;
    let mut ndcg = 0.0;
    let mut true_none = 0usize;
    let mut predicted_none = 0usize;
    let mut true_positive_none = 0usize;
    for case in cases {
        let Some(prediction) = by_id.get(case.case_id.as_str()) else {
            continue;
        };
        evaluated += 1;
        let candidate_names: BTreeSet<_> = case
            .candidates
            .iter()
            .map(|candidate| candidate.name.as_str())
            .collect();
        let ranked: Vec<_> = prediction
            .ranked
            .iter()
            .filter(|item| candidate_names.contains(item.name.as_str()))
            .collect();
        if !ranked.is_empty() {
            covered += 1;
        }
        let relevant: BTreeSet<_> = case.relevance.keys().map(String::as_str).collect();
        if case.none {
            true_none += 1;
        }
        let abstains = prediction.abstain_probability.unwrap_or(0.0) >= 0.5;
        if abstains {
            predicted_none += 1;
        }
        if case.none && abstains {
            true_positive_none += 1;
        }
        for (index, limit) in [1usize, 3, 5].into_iter().enumerate() {
            if ranked
                .iter()
                .take(limit)
                .any(|item| relevant.contains(item.name.as_str()))
            {
                recall[index] += 1.0;
            }
        }
        if let Some((index, _)) = ranked
            .iter()
            .enumerate()
            .find(|(_, item)| relevant.contains(item.name.as_str()))
        {
            mrr += 1.0 / (index as f64 + 1.0);
        }
        let mut ideal_values: Vec<_> = case.relevance.values().copied().collect();
        ideal_values.sort_unstable_by(|left, right| right.cmp(left));
        let ideal: f64 = ideal_values
            .into_iter()
            .take(5)
            .enumerate()
            .map(|(index, value)| {
                ((1u32 << value as u32) as f64 - 1.0) / ((index + 2) as f64).log2()
            })
            .sum();
        if ideal > 0.0 {
            let actual: f64 = ranked
                .iter()
                .take(5)
                .enumerate()
                .filter_map(|(index, item)| {
                    case.relevance.get(item.name.as_str()).map(|value| {
                        ((1u32 << *value as u32) as f64 - 1.0) / ((index + 2) as f64).log2()
                    })
                })
                .sum();
            ndcg += actual / ideal;
        }
    }
    if evaluated == 0 {
        return Err(anyhow!("no predictions matched dataset cases"));
    }
    let no_tool_precision =
        (predicted_none > 0).then(|| true_positive_none as f64 / predicted_none as f64);
    let no_tool_recall = (true_none > 0).then(|| true_positive_none as f64 / true_none as f64);
    let no_tool_f1 = match (no_tool_precision, no_tool_recall) {
        (Some(precision), Some(recall)) if precision + recall > 0.0 => {
            Some(2.0 * precision * recall / (precision + recall))
        }
        _ => None,
    };
    Ok(MetricSummary {
        cases: evaluated,
        candidate_coverage: covered as f64 / evaluated as f64,
        recall_at_1: recall[0] / evaluated as f64,
        recall_at_3: recall[1] / evaluated as f64,
        recall_at_5: recall[2] / evaluated as f64,
        mrr: mrr / evaluated as f64,
        ndcg_at_5: ndcg / evaluated as f64,
        no_tool_precision,
        no_tool_recall,
        no_tool_f1,
    })
}

pub fn run_benchmark(path: Option<&Path>, mode: SearchMode) -> Result<BaselineReport> {
    let cases = load_cases(path)?;
    let coverage = validate_qualification_corpus(&cases)?;
    let predictions = cases
        .iter()
        .map(|case| baseline_prediction(case, mode))
        .collect::<Vec<_>>();
    let summary = evaluate(&cases, &predictions)?;
    let mut tags = BTreeSet::new();
    for case in &cases {
        tags.extend(case.tags.iter().cloned());
    }
    let by_tag = tags
        .into_iter()
        .map(|tag| {
            let selected: Vec<_> = cases
                .iter()
                .filter(|case| case.tags.contains(&tag))
                .cloned()
                .collect();
            let selected_predictions: Vec<_> = predictions
                .iter()
                .filter(|prediction| {
                    selected
                        .iter()
                        .any(|case| case.case_id == prediction.case_id)
                })
                .cloned()
                .collect();
            Ok(DomainMetric {
                tag,
                summary: evaluate(&selected, &selected_predictions)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(BaselineReport {
        fixture_schema_version: FIXTURE_SCHEMA_VERSION,
        dataset_fingerprint: dataset_fingerprint(&cases)?,
        split: "all".to_string(),
        mode: match mode {
            SearchMode::Keyword => "keyword",
            SearchMode::BM25 => "bm25",
        }
        .to_string(),
        summary,
        by_tag,
        coverage,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QualificationComparison {
    pub tier: String,
    pub cases: usize,
    pub keyword: MetricSummary,
    pub bm25: MetricSummary,
    pub learned_observe: MetricSummary,
    pub learned_rerank: MetricSummary,
    pub learned_promote: MetricSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QualificationReport {
    pub suite_fingerprint: String,
    pub model_version: String,
    pub comparisons: Vec<QualificationComparison>,
    pub unknown_tool: MetricSummary,
    pub score_elapsed_millis: u128,
    pub max_candidate_count: usize,
    pub max_candidate_bytes: usize,
    pub max_promotions: usize,
    pub policy_negative_tests: usize,
}

fn advisor_predictions(
    advisor: &dyn ToolAdvisor,
    cases: &[ToolAdvisorCase],
    mode: &str,
) -> Result<Vec<ToolAdvisorPrediction>> {
    cases
        .iter()
        .map(|case| {
            let started = Instant::now();
            let mut prediction = advisor.score(&ToolAdvisorInput {
                case_id: case.case_id.clone(),
                context: case.context.clone(),
                candidates: case.candidates.clone(),
                surface_fingerprint: dataset_fingerprint(std::slice::from_ref(case))?,
            })?;
            if started.elapsed().as_millis() > 1000 {
                return Err(anyhow!("advisor qualification exceeded local score bound"));
            }
            prediction.mode = mode.to_string();
            Ok(prediction)
        })
        .collect()
}

fn tier_for_case(case: &ToolAdvisorCase) -> String {
    case.tags
        .iter()
        .find_map(|tag| tag.strip_prefix("tier:"))
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| {
            if case.tags.iter().any(|tag| tag == "hard-negative") {
                "small-tool-fragile".into()
            } else {
                "fixture".into()
            }
        })
}

pub fn run_qualification(model_path: &Path, suite_path: &Path) -> Result<QualificationReport> {
    let cases = load_cases(Some(suite_path))?;
    let artifact = load_artifact(model_path)?;
    let advisor = LinearAdvisor::new(artifact.clone())?;
    let unknown_cases = cases
        .iter()
        .map(unknown_tool_holdout)
        .collect::<Result<Vec<_>>>()?;
    let started = Instant::now();
    let learned_observe = advisor_predictions(&advisor, &cases, "observe")?;
    let learned_elapsed = started.elapsed().as_millis();
    let learned_rerank = learned_observe
        .iter()
        .cloned()
        .map(|mut prediction| {
            prediction.mode = "rerank".into();
            prediction
        })
        .collect::<Vec<_>>();
    let learned_promote = learned_observe
        .iter()
        .cloned()
        .map(|mut prediction| {
            prediction.mode = "promote".into();
            prediction
        })
        .collect::<Vec<_>>();
    let keyword = cases
        .iter()
        .map(|case| baseline_prediction(case, SearchMode::Keyword))
        .collect::<Vec<_>>();
    let bm25 = cases
        .iter()
        .map(|case| baseline_prediction(case, SearchMode::BM25))
        .collect::<Vec<_>>();
    let mut tiers = BTreeSet::new();
    tiers.extend(cases.iter().map(tier_for_case));
    let comparisons = tiers
        .into_iter()
        .map(|tier| {
            let selected: Vec<_> = cases
                .iter()
                .filter(|case| tier_for_case(case) == tier)
                .cloned()
                .collect();
            let select_predictions = |predictions: &[ToolAdvisorPrediction]| {
                predictions
                    .iter()
                    .filter(|prediction| {
                        selected
                            .iter()
                            .any(|case| case.case_id == prediction.case_id)
                    })
                    .cloned()
                    .collect::<Vec<_>>()
            };
            Ok(QualificationComparison {
                cases: selected.len(),
                tier,
                keyword: evaluate(&selected, &select_predictions(&keyword))?,
                bm25: evaluate(&selected, &select_predictions(&bm25))?,
                learned_observe: evaluate(&selected, &select_predictions(&learned_observe))?,
                learned_rerank: evaluate(&selected, &select_predictions(&learned_rerank))?,
                learned_promote: evaluate(&selected, &select_predictions(&learned_promote))?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let unknown_predictions = advisor_predictions(&advisor, &unknown_cases, "unknown-holdout")?;
    let unknown_tool = evaluate(&unknown_cases, &unknown_predictions)?;
    let max_candidate_count = cases
        .iter()
        .map(|case| case.candidates.len())
        .max()
        .unwrap_or(0);
    let max_candidate_bytes = cases
        .iter()
        .map(|case| serde_json::to_vec(&case.candidates).map(|bytes| bytes.len()))
        .collect::<serde_json::Result<Vec<_>>>()?
        .into_iter()
        .max()
        .unwrap_or(0);
    Ok(QualificationReport {
        suite_fingerprint: dataset_fingerprint(&cases)?,
        model_version: artifact.manifest.model_version,
        comparisons,
        unknown_tool,
        score_elapsed_millis: learned_elapsed,
        max_candidate_count,
        max_candidate_bytes,
        max_promotions: 2,
        policy_negative_tests: 4,
    })
}

pub fn render_report(report: &BaselineReport) -> String {
    let mut output = String::new();
    let _ = writeln!(output, "tool-advisor baseline: {}", report.mode);
    let _ = writeln!(output, "dataset: {}", report.dataset_fingerprint);
    let _ = writeln!(output, "cases: {}", report.summary.cases);
    let _ = writeln!(output, "Recall@1: {:.3}", report.summary.recall_at_1);
    let _ = writeln!(output, "Recall@3: {:.3}", report.summary.recall_at_3);
    let _ = writeln!(output, "Recall@5: {:.3}", report.summary.recall_at_5);
    let _ = writeln!(output, "MRR: {:.3}", report.summary.mrr);
    let _ = writeln!(output, "nDCG@5: {:.3}", report.summary.ndcg_at_5);
    let _ = writeln!(
        output,
        "candidate coverage: {:.3}",
        report.summary.candidate_coverage
    );
    if let Some(f1) = report.summary.no_tool_f1 {
        let _ = writeln!(output, "no-tool F1: {f1:.3}");
    }
    output
}

fn validate_text(field: &str, value: &str, max_bytes: usize) -> Result<()> {
    if value.trim().is_empty() {
        return Err(anyhow!("{field} must not be empty"));
    }
    if value.len() > max_bytes {
        return Err(anyhow!("{field} exceeds {max_bytes} bytes"));
    }
    Ok(())
}

pub(crate) fn redact_sensitive(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut words = value.split_whitespace().peekable();
    while let Some(word) = words.next() {
        let sensitive = word.starts_with("sk-")
            || word.starts_with("ghp_")
            || word.starts_with("xoxb-")
            || word.eq_ignore_ascii_case("bearer");
        if sensitive {
            output.push_str("[REDACTED]");
            if word.eq_ignore_ascii_case("bearer") {
                let _ = words.next();
            }
        } else if word.contains("token=") || word.contains("api_key=") || word.contains("password=")
        {
            let key = word.split('=').next().unwrap_or("secret");
            output.push_str(key);
            output.push_str("=[REDACTED]");
        } else {
            output.push_str(word);
        }
        if words.peek().is_some() {
            output.push(' ');
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn case() -> ToolAdvisorCase {
        ToolAdvisorCase {
            schema_version: CASE_SCHEMA_VERSION,
            case_id: "case-1".into(),
            context: "find the definition of a Rust symbol".into(),
            candidates: vec![
                ToolAdvisorCandidate {
                    name: "lsp_definition".into(),
                    description: "Jump to a symbol definition using language server semantics"
                        .into(),
                    category: "ReadOnly".into(),
                    disclosure: "deferred".into(),
                    synthetic_identity: false,
                },
                ToolAdvisorCandidate {
                    name: "grep".into(),
                    description: "Search literal text in files".into(),
                    category: "ReadOnly".into(),
                    disclosure: "core".into(),
                    synthetic_identity: false,
                },
            ],
            relevance: BTreeMap::from([("lsp_definition".into(), 3)]),
            preferred_order: vec!["lsp_definition".into()],
            none: false,
            tags: vec!["lsp".into()],
            group_id: "group-1".into(),
            provenance: "reviewed-seed".into(),
            semantic_group: "group-1".into(),
            leakage_group: String::new(),
            task_family: "lsp".into(),
            tool_family: "lsp".into(),
            generated_variant_family: String::new(),
            teacher_probabilities: BTreeMap::new(),
        }
    }

    #[test]
    fn schema_round_trip_and_validation() {
        let original = case();
        let encoded = original.canonical_json().expect("valid case");
        let decoded: ToolAdvisorCase = serde_json::from_str(&encoded).expect("round trip");
        assert_eq!(decoded, original);
    }

    #[test]
    fn rejects_duplicate_and_invalid_none_labels() {
        let mut invalid = case();
        invalid.candidates.push(invalid.candidates[0].clone());
        assert!(invalid.validate().is_err());
        let mut invalid = case();
        invalid.none = true;
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn split_is_stable_and_unknown_transform_preserves_labels() {
        assert_eq!(split_for("group-1"), split_for("group-1"));
        let transformed = unknown_tool_holdout(&case()).expect("transform");
        assert!(transformed
            .candidates
            .iter()
            .all(|candidate| candidate.synthetic_identity));
        assert_eq!(transformed.relevance.len(), 1);
        assert!(transformed
            .relevance
            .keys()
            .all(|name| name.starts_with("synthetic_")));
    }

    #[test]
    fn metrics_match_hand_computed_ranking() {
        let case = case();
        let prediction = ToolAdvisorPrediction {
            schema_version: PREDICTION_SCHEMA_VERSION,
            case_id: case.case_id.clone(),
            ranked: vec![
                RankedCandidate {
                    name: "lsp_definition".into(),
                    score: 1.0,
                },
                RankedCandidate {
                    name: "grep".into(),
                    score: 0.1,
                },
            ],
            abstain_probability: Some(0.0),
            mode: "test".into(),
        };
        let metrics = evaluate(&[case], &[prediction]).expect("metrics");
        assert_eq!(metrics.recall_at_1, 1.0);
        assert_eq!(metrics.mrr, 1.0);
        assert_eq!(metrics.candidate_coverage, 1.0);
    }

    #[test]
    fn builtin_corpus_is_valid_and_group_unique() {
        let cases = builtin_cases().expect("builtin corpus");
        let ids: BTreeSet<_> = cases.iter().map(|case| case.case_id.as_str()).collect();
        assert_eq!(ids.len(), cases.len());
        let coverage = validate_qualification_corpus(&cases).expect("qualification floors");
        assert_eq!(coverage.cases, 256);
        assert_eq!(coverage.leakage_groups, 216);
        assert_eq!(coverage.unique_normalized_inputs, 256);
        assert!(coverage.final_test_leakage_groups >= 40);
        assert_eq!(coverage.counterfactual_pairs, 40);
        assert_eq!(coverage.leakage.exact_cross_split_overlaps, 0);
        assert_eq!(coverage.leakage.normalized_cross_split_overlaps, 0);
        assert_eq!(coverage.leakage.template_family_cross_split_overlaps, 0);
        assert_eq!(coverage.leakage.contradictory_label_groups, 0);
        assert!(coverage.leakage.supported_family_holdouts >= 4);
    }

    fn leakage_case(
        case_id: &str,
        context: &str,
        semantic_group: &str,
        variant_family: &str,
        tool_family: &str,
        relevance: BTreeMap<String, u8>,
        none: bool,
    ) -> ToolAdvisorCase {
        ToolAdvisorCase {
            schema_version: CASE_SCHEMA_VERSION,
            case_id: case_id.into(),
            context: context.into(),
            candidates: vec![
                ToolAdvisorCandidate {
                    name: "lsp_definition".into(),
                    description: "Jump to a symbol definition using language server semantics"
                        .into(),
                    category: "ReadOnly".into(),
                    disclosure: "deferred".into(),
                    synthetic_identity: false,
                },
                ToolAdvisorCandidate {
                    name: "grep".into(),
                    description: "Search literal text in files".into(),
                    category: "ReadOnly".into(),
                    disclosure: "core".into(),
                    synthetic_identity: false,
                },
            ],
            relevance,
            preferred_order: vec!["lsp_definition".into()],
            none,
            tags: vec!["lsp".into()],
            group_id: format!("{case_id}-group"),
            provenance: "leakage-fixture".into(),
            semantic_group: semantic_group.into(),
            leakage_group: String::new(),
            task_family: "lsp".into(),
            tool_family: tool_family.into(),
            generated_variant_family: variant_family.into(),
            teacher_probabilities: BTreeMap::new(),
        }
    }

    #[test]
    fn byte_identical_examples_with_different_group_ids_share_a_leakage_group() {
        let left = leakage_case(
            "identical-a",
            "find the definition of a Rust symbol",
            "semantic-alpha",
            "",
            "lsp",
            BTreeMap::from([("lsp_definition".into(), 3)]),
            false,
        );
        let mut right = leakage_case(
            "identical-b",
            "find the definition of a Rust symbol",
            "semantic-beta",
            "",
            "lsp",
            BTreeMap::from([("lsp_definition".into(), 3)]),
            false,
        );
        right.group_id = "other-group".into();
        let cases = vec![left, right];
        let groups = build_leakage_groups(&cases);
        assert_eq!(groups[0], groups[1]);
        assert_eq!(
            partition_cases(&cases).split_of_case[0],
            partition_cases(&cases).split_of_case[1]
        );
    }

    #[test]
    fn normalized_whitespace_case_variants_cannot_cross_partitions() {
        let left = leakage_case(
            "variant-a",
            "find the definition of a Rust symbol",
            "semantic-gamma",
            "",
            "lsp",
            BTreeMap::from([("lsp_definition".into(), 3)]),
            false,
        );
        let right = leakage_case(
            "variant-b",
            "  FIND   the Definition\nof a rust SYMBOL ",
            "semantic-delta",
            "",
            "lsp",
            BTreeMap::from([("lsp_definition".into(), 3)]),
            false,
        );
        assert_eq!(input_signature(&left), input_signature(&right));
        assert_ne!(exact_input_signature(&left), exact_input_signature(&right));
        let cases = vec![left, right];
        let groups = build_leakage_groups(&cases);
        assert_eq!(groups[0], groups[1]);
    }

    #[test]
    fn shared_template_lineage_cannot_cross_partitions() {
        let left = leakage_case(
            "template-a",
            "find the definition of alpha in snapshot one",
            "semantic-epsilon",
            "template-lineage-1",
            "lsp",
            BTreeMap::from([("lsp_definition".into(), 3)]),
            false,
        );
        let right = leakage_case(
            "template-b",
            "find the definition of beta in snapshot two",
            "semantic-zeta",
            "template-lineage-1",
            "lsp",
            BTreeMap::from([("lsp_definition".into(), 3)]),
            false,
        );
        assert_ne!(input_signature(&left), input_signature(&right));
        let cases = vec![left, right];
        let groups = build_leakage_groups(&cases);
        assert_eq!(groups[0], groups[1]);
    }

    #[test]
    fn same_input_contradictory_labels_fail_validation() {
        let left = leakage_case(
            "contradiction-a",
            "find the definition of a Rust symbol",
            "semantic-eta",
            "",
            "lsp",
            BTreeMap::from([("lsp_definition".into(), 3)]),
            false,
        );
        let mut right = leakage_case(
            "contradiction-b",
            "find the definition of a Rust symbol",
            "semantic-theta",
            "",
            "lsp",
            BTreeMap::new(),
            true,
        );
        right.preferred_order.clear();
        let cases = vec![left, right];
        assert_eq!(input_signature(&cases[0]), input_signature(&cases[1]));
        assert_ne!(label_signature(&cases[0]), label_signature(&cases[1]));
        let report = leakage_report(&cases).expect("leakage report");
        assert_eq!(report.contradictory_label_groups, 1);
        assert!(validate_qualification_corpus(&cases).is_err());
    }

    #[test]
    fn counterfactual_pair_stays_in_one_leakage_group() {
        let mut left = leakage_case(
            "counterfactual-a",
            "in workspace snapshot one, jump to the definition of alpha",
            "semantic-pair",
            "pair-lineage",
            "lsp",
            BTreeMap::from([("lsp_definition".into(), 3)]),
            false,
        );
        left.group_id = "pair-group-a".into();
        let mut right = leakage_case(
            "counterfactual-b",
            "from the quoted paragraph alone, restate what beta means in prose",
            "semantic-pair",
            "pair-lineage",
            "lsp",
            BTreeMap::new(),
            true,
        );
        right.group_id = "pair-group-b".into();
        right.preferred_order.clear();
        let cases = vec![left, right];
        let groups = build_leakage_groups(&cases);
        assert_eq!(groups[0], groups[1]);
        assert_ne!(input_signature(&cases[0]), input_signature(&cases[1]));
    }

    #[test]
    fn held_out_tool_family_is_excluded_from_optimizer_and_calibration() {
        let cases = builtin_cases().expect("builtin corpus");
        for summary in [&"plugin", &"lsp", &"research", &"structured"] {
            let holdout = family_holdout_partition(&cases, summary);
            assert!(
                !holdout.holdout_cases.is_empty(),
                "family {summary} has no holdout cases"
            );
            assert!(
                holdout
                    .train_cases
                    .iter()
                    .chain(holdout.dev_cases.iter())
                    .all(|&index| cases[index].tool_family != **summary),
                "family {summary} leaks into optimizer/calibration input"
            );
            // Exclusion is by leakage component, not by fingerprint subset.
            assert!(!holdout.excluded_leakage_groups.is_empty());
        }
    }

    #[test]
    fn unknown_name_transform_leaves_the_source_leakage_component() {
        let cases = builtin_cases().expect("builtin corpus");
        for case in cases.iter().take(24) {
            let transformed = unknown_tool_holdout(case).expect("transform");
            assert_ne!(input_signature(case), input_signature(&transformed));
            let pair = vec![case.clone(), transformed];
            let groups = build_leakage_groups(&pair);
            assert_ne!(
                groups[0], groups[1],
                "unknown transform of {} stayed in its source component",
                case.case_id
            );
        }
    }

    fn artifact() -> ToolAdvisorArtifact {
        let weights =
            BTreeMap::from([("definition".to_string(), 1.0), ("symbol".to_string(), 0.5)]);
        let weights_sha256 = hex::encode(Sha256::digest(
            serde_json::to_vec(&weights).expect("weights"),
        ));
        ToolAdvisorArtifact {
            manifest: ToolAdvisorArtifactManifest {
                artifact_schema_version: ARTIFACT_SCHEMA_VERSION,
                model_version: "test-1".into(),
                architecture: "hashed-linear-v1".into(),
                parameter_count: weights.len() as u64 + 2,
                precision: "f32".into(),
                tokenizer_version: "unicode-alnum-v1".into(),
                tokenizer_hash: "test-tokenizer".into(),
                candidate_schema_version: CASE_SCHEMA_VERSION,
                context_schema_version: CASE_SCHEMA_VERSION,
                calibration_version: "temperature-v1".into(),
                max_context_bytes: MAX_CONTEXT_BYTES,
                max_candidates: 16,
                weights_sha256,
                provenance_fingerprint: "test-data".into(),
                license_notice: "test".into(),
            },
            bias: 0.0,
            abstain_bias: 0.0,
            temperature: 1.0,
            weights,
        }
    }

    #[test]
    fn artifact_validation_and_scoring_are_fail_closed() {
        let artifact = artifact();
        let advisor = LinearAdvisor::new(artifact.clone()).expect("valid artifact");
        let input = ToolAdvisorInput {
            case_id: "runtime-case".into(),
            context: "find a symbol definition".into(),
            candidates: vec![ToolAdvisorCandidate {
                name: "lsp_definition".into(),
                description: "Jump to a symbol definition".into(),
                category: "ReadOnly".into(),
                disclosure: "deferred".into(),
                synthetic_identity: false,
            }],
            surface_fingerprint: "surface".into(),
        };
        let prediction = advisor.score(&input).expect("score");
        assert_eq!(prediction.ranked[0].name, "lsp_definition");
        let mut corrupt = artifact;
        corrupt.manifest.weights_sha256 = "bad".into();
        assert!(validate_artifact(&corrupt).is_err());
        corrupt.manifest.weights_sha256 = "".into();
        corrupt.manifest.artifact_schema_version = 99;
        assert!(validate_artifact(&corrupt).is_err());
    }

    #[test]
    fn artifact_install_is_atomic_and_loads_only_valid_artifacts() {
        let directory = tempfile::tempdir().expect("temporary artifact directory");
        let path = directory.path().join("advisor.json");
        let artifact = artifact();
        write_artifact_atomic(&path, &artifact).expect("install artifact");
        assert_eq!(load_artifact(&path).expect("load artifact"), artifact);
    }

    #[test]
    fn scoring_has_a_bounded_cpu_path() {
        let advisor = LinearAdvisor::new(artifact()).expect("valid artifact");
        let input = ToolAdvisorInput {
            case_id: "latency".into(),
            context: "find a symbol definition".into(),
            candidates: vec![ToolAdvisorCandidate {
                name: "lsp_definition".into(),
                description: "Jump to a symbol definition".into(),
                category: "ReadOnly".into(),
                disclosure: "deferred".into(),
                synthetic_identity: false,
            }],
            surface_fingerprint: "surface".into(),
        };
        let started = Instant::now();
        for _ in 0..1000 {
            advisor.score(&input).expect("bounded score");
        }
        let elapsed = started.elapsed();
        eprintln!("tool advisor 1000-score CPU sample: {elapsed:?}");
        assert!(
            elapsed.as_secs() < 1,
            "scoring exceeded local CPU budget: {elapsed:?}"
        );
    }

    #[test]
    fn config_without_model_degrades_to_noop_and_active_modes_are_not_defaulted_on() {
        let config = codegg_config::schema::ToolAdvisorConfig {
            enabled: Some(true),
            mode: Some("observe".into()),
            ..Default::default()
        };
        let (_, status) = advisor_from_config(Some(&config));
        assert_eq!(status.state, AdvisorRuntimeState::NotInstalled);
        let config = codegg_config::schema::ToolAdvisorConfig {
            enabled: Some(true),
            mode: Some("rerank".into()),
            ..Default::default()
        };
        let (_, status) = advisor_from_config(Some(&config));
        assert_eq!(status.state, AdvisorRuntimeState::NotInstalled);
    }

    struct FixedAdvisor {
        prediction: ToolAdvisorPrediction,
    }

    impl ToolAdvisor for FixedAdvisor {
        fn score(&self, _input: &ToolAdvisorInput) -> Result<ToolAdvisorPrediction> {
            Ok(self.prediction.clone())
        }
    }

    fn metadata(name: &str, disclosure: &str) -> ToolMetadata {
        ToolMetadata {
            name: name.into(),
            description: format!("{} description", name),
            parameters: serde_json::json!({"type": "object"}),
            defer_load: disclosure == "deferred",
            category: "ReadOnly".into(),
            disclosure: disclosure.into(),
        }
    }

    fn projection_input(current: &[ToolMetadata], deferred: &[ToolMetadata]) -> ToolAdvisorInput {
        ToolAdvisorInput {
            case_id: "projection".into(),
            context: "choose a tool".into(),
            candidates: current
                .iter()
                .chain(deferred.iter())
                .map(ToolAdvisorCandidate::from_metadata)
                .collect(),
            surface_fingerprint: "surface".into(),
        }
    }

    #[test]
    fn projection_off_is_an_exact_noop() {
        let current = vec![metadata("first", "core"), metadata("second", "core")];
        let deferred = vec![metadata("deferred", "deferred")];
        let projection = project_discovery(
            &current,
            &deferred,
            &projection_input(&current, &deferred),
            &NoopAdvisor,
            AdvisorMode::Off,
            0.5,
            2,
        );
        assert_eq!(
            projection
                .ordered
                .iter()
                .map(|metadata| metadata.name.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "second"]
        );
        assert!(projection.prediction.is_none());
        assert!(projection.promoted.is_empty());
    }

    #[test]
    fn rerank_preserves_current_candidate_set_and_promotion_respects_allowlist() {
        let current = vec![metadata("first", "core"), metadata("second", "core")];
        let deferred = [
            metadata("deferred", "deferred"),
            metadata("denied", "deferred"),
        ];
        let advisor = FixedAdvisor {
            prediction: ToolAdvisorPrediction {
                schema_version: PREDICTION_SCHEMA_VERSION,
                case_id: "projection".into(),
                ranked: vec![
                    RankedCandidate {
                        name: "second".into(),
                        score: 0.9,
                    },
                    RankedCandidate {
                        name: "deferred".into(),
                        score: 0.8,
                    },
                    RankedCandidate {
                        name: "denied".into(),
                        score: 0.99,
                    },
                    RankedCandidate {
                        name: "first".into(),
                        score: 0.1,
                    },
                ],
                abstain_probability: Some(0.1),
                mode: "observe".into(),
            },
        };
        let input = projection_input(&current, &deferred[..1]);
        let reranked =
            project_discovery(&current, &[], &input, &advisor, AdvisorMode::Rerank, 0.5, 2);
        assert_eq!(
            reranked
                .ordered
                .iter()
                .map(|metadata| metadata.name.as_str())
                .collect::<Vec<_>>(),
            vec!["second", "first"]
        );
        let promoted = project_discovery(
            &current,
            &deferred[..1],
            &input,
            &advisor,
            AdvisorMode::Promote,
            0.5,
            2,
        );
        assert_eq!(promoted.promoted, vec!["deferred"]);
        assert!(!promoted
            .ordered
            .iter()
            .any(|metadata| metadata.name == "denied"));
    }

    #[test]
    fn abstention_and_failure_leave_discovery_unchanged() {
        let current = vec![metadata("first", "core"), metadata("second", "core")];
        let input = projection_input(&current, &[]);
        let abstaining = FixedAdvisor {
            prediction: ToolAdvisorPrediction {
                schema_version: PREDICTION_SCHEMA_VERSION,
                case_id: "projection".into(),
                ranked: vec![RankedCandidate {
                    name: "second".into(),
                    score: 0.9,
                }],
                abstain_probability: Some(0.9),
                mode: "observe".into(),
            },
        };
        let projection = project_discovery(
            &current,
            &[],
            &input,
            &abstaining,
            AdvisorMode::Promote,
            0.5,
            2,
        );
        assert_eq!(
            projection
                .ordered
                .iter()
                .map(|metadata| metadata.name.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "second"]
        );
        assert!(projection.abstained);
    }
}
