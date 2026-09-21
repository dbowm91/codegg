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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    if !matches!(config.mode.as_deref().unwrap_or("off"), "observe") {
        return (
            Box::new(NoopAdvisor),
            AdvisorRuntimeStatus {
                state: AdvisorRuntimeState::Incompatible,
                detail: "M002 supports only off and observe".into(),
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
    match load_artifact(path).and_then(LinearAdvisor::new) {
        Ok(advisor) => {
            let version = advisor.artifact().manifest.model_version.clone();
            (
                Box::new(advisor),
                AdvisorRuntimeStatus {
                    state: AdvisorRuntimeState::Ready,
                    detail: "observe-only advisor loaded".into(),
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
pub fn split_for(group_id: &str) -> &'static str {
    let digest = Sha256::digest(group_id.as_bytes());
    match digest[0] % 10 {
        0..=1 => "dev",
        2..=3 => "test",
        _ => "train",
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

pub fn unknown_tool_holdout(case: &ToolAdvisorCase) -> Result<ToolAdvisorCase> {
    let mut transformed = case.clone();
    transformed.case_id = format!("{}::unknown", case.case_id);
    transformed.group_id = format!("{}::unknown", case.group_id);
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
        assert!(cases.len() >= 12);
        assert!(cases.iter().any(|case| case.none));
        assert!(cases
            .iter()
            .any(|case| case.tags.iter().any(|tag| tag == "hard-negative")));
        assert!(cases.iter().any(|case| case
            .candidates
            .iter()
            .any(|candidate| candidate.synthetic_identity)));
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
    fn config_without_model_degrades_to_noop_and_active_modes_are_rejected() {
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
        assert_eq!(status.state, AdvisorRuntimeState::Incompatible);
    }
}
