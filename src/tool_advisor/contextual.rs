//! Optional pure-Rust contextual advisor runtime and trainer.
//!
//! This is deliberately self-contained instead of depending on a native ML
//! runtime. It uses deterministic hashed token embeddings plus a learned
//! context/candidate interaction score. The fixed-size embedding table gives
//! auditable small/medium capacity points while unseen tool names continue to
//! work through their textual descriptors.

use super::{
    sigmoid, tokenize, MetricSummary, RankedCandidate, ToolAdvisor, ToolAdvisorCase,
    ToolAdvisorInput, ToolAdvisorPrediction, MAX_CONTEXT_BYTES, PREDICTION_SCHEMA_VERSION,
};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

pub const ARTIFACT_MAGIC: &[u8] = b"CODEGG-TAE1";
/// Writer schema version. Version 1 artifacts remain loadable for
/// compatibility but are always reported as legacy/unqualified: they predate
/// corrected gradients, partition discipline, and serialized calibration.
pub const CONTEXTUAL_SCHEMA_VERSION: u16 = 2;
pub const CONTEXTUAL_SCHEMA_VERSION_V1: u16 = 1;
pub const CONTEXTUAL_ARCHITECTURE_V1: &str = "contextual-embedding-v1";
pub const CONTEXTUAL_ARCHITECTURE_V2: &str = "contextual-embedding-v2";
pub const VOCAB_BUCKETS: usize = 65_536;
pub const SMALL_EMBEDDING_DIM: usize = 80;
pub const MEDIUM_EMBEDDING_DIM: usize = 240;
pub const MAX_CONTEXT_TOKENS: usize = 96;
pub const MAX_CANDIDATE_TOKENS: usize = 128;
/// Discipline marker for artifacts trained on explicit C001 partitions.
pub const DISCIPLINE_PARTITIONED: &str = "partitioned-qualification";
/// Discipline marker for test-only/smoke artifacts built without partitions.
pub const DISCIPLINE_FALLBACK: &str = "unpartitioned-fallback";
/// Discipline marker for pre-C002 artifacts that predate the flag.
pub const DISCIPLINE_LEGACY: &str = "legacy-pre-partition";
/// Calibration method for artifacts whose abstention head was fit on dev.
pub const CALIBRATION_DEV_GRID: &str = "dev-grid-search-v1";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Capacity {
    Small,
    Medium,
}

impl Capacity {
    pub fn parse(value: Option<&str>) -> Result<Self> {
        match value.unwrap_or("small").to_ascii_lowercase().as_str() {
            "small" => Ok(Self::Small),
            "medium" => Ok(Self::Medium),
            other => Err(anyhow!("unsupported contextual capacity {other}")),
        }
    }

    pub fn embedding_dim(self) -> usize {
        match self {
            Self::Small => SMALL_EMBEDDING_DIM,
            Self::Medium => MEDIUM_EMBEDDING_DIM,
        }
    }

    pub fn parameter_count(self) -> u64 {
        (VOCAB_BUCKETS * self.embedding_dim() + 1) as u64
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContextualTrainingConfig {
    pub capacity: Capacity,
    #[serde(default = "default_epochs")]
    pub epochs: u32,
    #[serde(default = "default_learning_rate")]
    pub learning_rate: f32,
    #[serde(default)]
    pub seed: u64,
    #[serde(default = "default_max_candidates")]
    pub max_candidates: usize,
    #[serde(default)]
    pub model_version: Option<String>,
    /// Variable embedding-table size for `contextual-embedding-v2`. `None`
    /// preserves the historical 65,536 buckets. Small tables are the honest
    /// capacity comparison: the corpus touches only a few hundred buckets.
    #[serde(default)]
    pub vocab_buckets: Option<usize>,
    /// Test-only escape hatch for fitting without C001 partitions (tiny
    /// fixtures, smoke wiring). Output is permanently marked
    /// `unpartitioned-fallback` and can never qualify.
    #[serde(default)]
    pub allow_unpartitioned_fallback: bool,
}

fn default_epochs() -> u32 {
    3
}

fn default_learning_rate() -> f32 {
    0.03
}

fn default_max_candidates() -> usize {
    32
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContextualArtifactManifest {
    pub artifact_schema_version: u16,
    pub model_version: String,
    pub architecture: String,
    pub capacity: String,
    pub parameter_count: u64,
    pub embedding_dim: usize,
    pub vocab_buckets: usize,
    pub tokenizer_version: String,
    pub context_schema_version: u16,
    pub candidate_schema_version: u16,
    pub max_context_tokens: usize,
    pub max_candidate_tokens: usize,
    pub dataset_fingerprint: String,
    pub weights_sha256: String,
    pub provenance: String,
    pub license_notice: String,
    /// How this artifact was trained. Missing (pre-C002 files) deserializes
    /// to the legacy marker and is never treated as qualified.
    #[serde(default = "default_discipline")]
    pub training_discipline: String,
    /// Dev-selected abstention calibration. `None` means uncalibrated legacy
    /// behavior (`sigmoid(-top_score)`); the runtime reports that honestly.
    #[serde(default)]
    pub calibration: Option<ContextualCalibration>,
}

fn default_discipline() -> String {
    DISCIPLINE_LEGACY.into()
}

/// Abstention calibration selected on dev data only. The runtime abstention
/// probability is `sigmoid((abstain_bias - max_score) / temperature)`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContextualCalibration {
    pub method: String,
    pub abstain_bias: f32,
    pub temperature: f32,
    pub dev_cases: usize,
    pub dev_nll: f64,
    pub dev_brier: f64,
    pub dev_ece: f64,
    pub dev_no_tool_precision: Option<f64>,
    pub dev_no_tool_recall: Option<f64>,
    pub dev_no_tool_f1: Option<f64>,
    /// Uncalibrated (`bias = 0`, `temperature = 1`) reference on the same dev
    /// data, so closure can check calibrated non-inferiority directly.
    pub uncalibrated_dev_nll: f64,
    pub uncalibrated_dev_brier: f64,
}

/// Whether the loaded artifact may back qualification evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CalibrationStatus {
    /// Schema v2 with dev-selected calibration and partitioned discipline.
    Calibrated,
    /// Anything else: v1 files, fallback builds, or missing calibration.
    /// Loadable and inspectable, but never qualification evidence.
    LegacyUnqualified,
}

#[derive(Debug, Clone)]
pub struct ContextualArtifact {
    pub manifest: ContextualArtifactManifest,
    pub bias: f32,
    weights: Vec<f32>,
}

impl ContextualArtifact {
    fn validate(&self) -> Result<()> {
        let manifest = &self.manifest;
        let is_v1 = manifest.artifact_schema_version == CONTEXTUAL_SCHEMA_VERSION_V1
            && manifest.architecture == CONTEXTUAL_ARCHITECTURE_V1;
        let is_v2 = manifest.artifact_schema_version == CONTEXTUAL_SCHEMA_VERSION
            && manifest.architecture == CONTEXTUAL_ARCHITECTURE_V2;
        if !is_v1 && !is_v2 {
            return Err(anyhow!(
                "contextual artifact manifest is incompatible (schema/architecture)"
            ));
        }
        if is_v1 {
            if manifest.vocab_buckets != VOCAB_BUCKETS || manifest.calibration.is_some() {
                return Err(anyhow!(
                    "contextual v1 artifact has unexpected buckets or calibration"
                ));
            }
        } else if let Some(calibration) = &manifest.calibration {
            if !calibration.temperature.is_finite()
                || calibration.temperature <= 0.0
                || !calibration.abstain_bias.is_finite()
                || calibration.method.trim().is_empty()
            {
                return Err(anyhow!(
                    "contextual artifact carries invalid calibration values"
                ));
            }
        } else {
            return Err(anyhow!(
                "contextual v2 artifact has no serialized calibration"
            ));
        }
        if manifest.vocab_buckets == 0
            || manifest.embedding_dim == 0
            || manifest.embedding_dim * manifest.vocab_buckets + 1
                != manifest.parameter_count as usize
            || self.weights.len() + 1 != manifest.parameter_count as usize
            || manifest.context_schema_version != super::CASE_SCHEMA_VERSION
            || manifest.candidate_schema_version != super::CASE_SCHEMA_VERSION
            || manifest.max_context_tokens == 0
            || manifest.max_context_tokens > MAX_CONTEXT_TOKENS
            || manifest.max_candidate_tokens == 0
            || manifest.max_candidate_tokens > MAX_CANDIDATE_TOKENS
        {
            return Err(anyhow!("contextual artifact manifest is incompatible"));
        }
        if !self.bias.is_finite() || self.weights.iter().any(|weight| !weight.is_finite()) {
            return Err(anyhow!("contextual artifact contains non-finite weights"));
        }
        let bytes = weights_bytes(&self.weights);
        let digest = hex::encode(Sha256::digest(bytes));
        if digest != manifest.weights_sha256 {
            return Err(anyhow!("contextual artifact weights hash mismatch"));
        }
        Ok(())
    }

    /// Authoritative qualification gate for this artifact. Only schema-v2
    /// artifacts with partitioned discipline and dev-selected calibration
    /// may back C004 evidence; everything older or fallback-built stays a
    /// research baseline.
    pub fn calibration_status(&self) -> CalibrationStatus {
        let calibrated = self.manifest.artifact_schema_version == CONTEXTUAL_SCHEMA_VERSION
            && self.manifest.training_discipline == DISCIPLINE_PARTITIONED
            && matches!(
                self.manifest
                    .calibration
                    .as_ref()
                    .map(|calibration| calibration.method.as_str()),
                Some(CALIBRATION_DEV_GRID)
            );
        if calibrated {
            CalibrationStatus::Calibrated
        } else {
            CalibrationStatus::LegacyUnqualified
        }
    }

    pub fn is_qualified(&self) -> bool {
        self.calibration_status() == CalibrationStatus::Calibrated
    }

    pub fn geometry(&self) -> ModelGeometry {
        ModelGeometry {
            vocab_buckets: self.manifest.vocab_buckets,
            embedding_dim: self.manifest.embedding_dim,
            max_context_tokens: self.manifest.max_context_tokens,
            max_candidate_tokens: self.manifest.max_candidate_tokens,
        }
    }
}

pub struct ContextualAdvisor {
    artifact: Arc<ContextualArtifact>,
}

impl ContextualAdvisor {
    pub fn new(artifact: ContextualArtifact) -> Result<Self> {
        artifact.validate()?;
        Ok(Self {
            artifact: Arc::new(artifact),
        })
    }

    pub fn artifact(&self) -> &ContextualArtifact {
        &self.artifact
    }

    pub fn calibration_status(&self) -> CalibrationStatus {
        self.artifact.calibration_status()
    }

    fn score_pair(&self, context: &str, candidate: &str) -> f32 {
        let geometry = self.artifact.geometry();
        score_pair_with(&self.artifact.weights, geometry, context, candidate) + self.artifact.bias
    }

    fn abstain_probability(&self, top_score: Option<f32>) -> f32 {
        let top = top_score.unwrap_or(0.0);
        match &self.artifact.manifest.calibration {
            Some(calibration) => super::sigmoid(
                (calibration.abstain_bias - top) / calibration.temperature.max(0.001),
            ),
            // Legacy uncalibrated behavior, preserved bit-identically so old
            // artifacts score exactly as before while reporting unqualified.
            None => super::sigmoid(-top),
        }
    }
}

impl ToolAdvisor for ContextualAdvisor {
    fn score(&self, input: &ToolAdvisorInput) -> Result<ToolAdvisorPrediction> {
        if input.context.len() > MAX_CONTEXT_BYTES {
            return Err(anyhow!("contextual advisor context exceeds limit"));
        }
        if input.candidates.len() > super::MAX_CANDIDATES {
            return Err(anyhow!("contextual advisor candidate set exceeds limit"));
        }
        let mut ranked = input
            .candidates
            .iter()
            .map(|candidate| {
                let descriptor = format!(
                    "{} {} {} {}",
                    candidate.name, candidate.description, candidate.category, candidate.disclosure
                );
                (
                    candidate.name.clone(),
                    self.score_pair(&input.context, &descriptor) as f64,
                )
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|left, right| {
            right
                .1
                .partial_cmp(&left.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| left.0.cmp(&right.0))
        });
        let abstain_probability =
            self.abstain_probability(ranked.first().map(|(_, score)| *score as f32));
        Ok(ToolAdvisorPrediction {
            schema_version: PREDICTION_SCHEMA_VERSION,
            case_id: input.case_id.clone(),
            ranked: ranked
                .into_iter()
                .map(|(name, score)| RankedCandidate { name, score })
                .collect(),
            abstain_probability: Some(abstain_probability as f64),
            mode: "contextual".into(),
        })
    }
}

pub fn load(path: &Path) -> Result<ContextualArtifact> {
    let bytes =
        fs::read(path).with_context(|| format!("read contextual artifact {}", path.display()))?;
    if bytes.len() < ARTIFACT_MAGIC.len() + 4 || !bytes.starts_with(ARTIFACT_MAGIC) {
        return Err(anyhow!("not a contextual advisor artifact"));
    }
    let manifest_len_offset = ARTIFACT_MAGIC.len();
    let manifest_len = u32::from_le_bytes(
        bytes[manifest_len_offset..manifest_len_offset + 4]
            .try_into()
            .expect("fixed manifest length"),
    ) as usize;
    let manifest_start = manifest_len_offset + 4;
    let manifest_end = manifest_start
        .checked_add(manifest_len)
        .ok_or_else(|| anyhow!("contextual manifest length overflow"))?;
    if manifest_end + 4 > bytes.len() {
        return Err(anyhow!("truncated contextual artifact manifest"));
    }
    let manifest: ContextualArtifactManifest =
        serde_json::from_slice(&bytes[manifest_start..manifest_end])
            .context("parse contextual artifact manifest")?;
    let weight_bytes = &bytes[manifest_end + 4..];
    let expected = manifest
        .parameter_count
        .checked_sub(1)
        .ok_or_else(|| anyhow!("contextual artifact has no weights"))? as usize
        * 4;
    if weight_bytes.len() != expected {
        return Err(anyhow!("contextual artifact weight length mismatch"));
    }
    let bias = f32::from_le_bytes(
        bytes[manifest_end..manifest_end + 4]
            .try_into()
            .expect("fixed bias"),
    );
    let (weight_chunks, remainder) = weight_bytes.as_chunks::<4>();
    if !remainder.is_empty() {
        return Err(anyhow!("contextual artifact has partial weight"));
    }
    let weights = weight_chunks
        .iter()
        .map(|chunk| f32::from_le_bytes(*chunk))
        .collect();
    let artifact = ContextualArtifact {
        manifest,
        bias,
        weights,
    };
    artifact.validate()?;
    Ok(artifact)
}

pub fn write_atomic(path: &Path, artifact: &ContextualArtifact) -> Result<()> {
    artifact.validate()?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("contextual artifact has no parent"))?;
    fs::create_dir_all(parent)?;
    let manifest = serde_json::to_vec(&artifact.manifest)?;
    let mut bytes = Vec::with_capacity(
        ARTIFACT_MAGIC.len() + 4 + manifest.len() + 4 + artifact.weights.len() * 4,
    );
    bytes.extend_from_slice(ARTIFACT_MAGIC);
    bytes.extend_from_slice(&(manifest.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&manifest);
    bytes.extend_from_slice(&artifact.bias.to_le_bytes());
    bytes.extend_from_slice(&weights_bytes(&artifact.weights));
    let temp = path.with_extension("bin.tmp");
    fs::write(&temp, bytes)?;
    fs::rename(temp, path)?;
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContextualCapacityReport {
    pub allocated_parameter_count: u64,
    pub artifact_bytes: u64,
    pub vocab_buckets: usize,
    pub embedding_dim: usize,
    pub train_unique_normalized_tokens: usize,
    pub train_distinct_buckets_touched: usize,
    pub train_collided_tokens: usize,
    pub train_collision_rate: f64,
    /// Embedding rows whose values differ from initialization after training:
    /// the exact set that received data-derived gradient.
    pub rows_changed_by_training: usize,
    pub trained_parameter_estimate: u64,
    pub trained_parameter_fraction: f64,
    pub cold_load_millis: u128,
    pub peak_rss_bytes: Option<u64>,
    pub score_latency_p50_micros: u128,
    pub score_latency_p95_micros: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContextualTrainingReport {
    pub artifact_path: String,
    pub capacity: String,
    pub parameter_count: u64,
    pub vocab_buckets: usize,
    pub embedding_dim: usize,
    pub train_cases: usize,
    pub dev_cases: usize,
    pub test_cases: usize,
    pub dataset_fingerprint: String,
    pub train_partition_fingerprint: String,
    pub dev_partition_fingerprint: String,
    pub test_partition_fingerprint: String,
    /// True only for the explicit test-only fallback path. Such artifacts
    /// are permanently marked `unpartitioned-fallback` and never qualify.
    pub unqualified_fallback: bool,
    pub training_discipline: String,
    pub train_metrics: MetricSummary,
    pub dev_metrics: MetricSummary,
    pub calibration: ContextualCalibration,
    pub capacity_report: ContextualCapacityReport,
    pub artifact_bytes: u64,
}

pub fn train(
    cases: &[ToolAdvisorCase],
    dataset_fingerprint: &str,
    config: &ContextualTrainingConfig,
    output: &Path,
) -> Result<ContextualTrainingReport> {
    if config.epochs == 0
        || config.epochs > 100
        || !config.learning_rate.is_finite()
        || config.learning_rate <= 0.0
        || config.max_candidates == 0
        || config.max_candidates > super::MAX_CANDIDATES
    {
        return Err(anyhow!("invalid contextual training configuration"));
    }
    let buckets = config.vocab_buckets.unwrap_or(VOCAB_BUCKETS);
    let dim = config.capacity.embedding_dim();
    if buckets == 0 || buckets > 1_048_576 {
        return Err(anyhow!("contextual vocab buckets out of range"));
    }
    if (buckets as u64) * (dim as u64) + 1 > 64 * 1024 * 1024 {
        return Err(anyhow!(
            "contextual embedding table exceeds the 256 MiB operator bound"
        ));
    }
    let geometry = ModelGeometry {
        vocab_buckets: buckets,
        embedding_dim: dim,
        max_context_tokens: MAX_CONTEXT_TOKENS,
        max_candidate_tokens: MAX_CANDIDATE_TOKENS,
    };
    let partition = super::partition_cases(cases);
    let mut train_indices = partition.train_cases.clone();
    let mut dev_indices = partition.dev_cases.clone();
    let test_indices = partition.test_cases.clone();
    let unqualified_fallback = train_indices.is_empty() || dev_indices.is_empty();
    if unqualified_fallback && !config.allow_unpartitioned_fallback {
        return Err(anyhow!(
            "contextual training requires non-empty C001 train and dev partitions; \
             refusing to fit on unpartitioned data in qualification mode"
        ));
    }
    if unqualified_fallback {
        train_indices = (0..cases.len()).collect();
        dev_indices = (0..cases.len()).collect();
    }
    let train_cases: Vec<&ToolAdvisorCase> =
        train_indices.iter().map(|&index| &cases[index]).collect();
    let dev_cases: Vec<&ToolAdvisorCase> = dev_indices.iter().map(|&index| &cases[index]).collect();

    let mut weights = vec![0.0_f32; buckets * dim];
    initialize(&mut weights, config.seed);
    let initial_weights = weights.clone();
    let mut bias = 0.0_f32;
    for _ in 0..config.epochs {
        for case in &train_cases {
            for candidate in case.candidates.iter().take(config.max_candidates) {
                let descriptor = format!(
                    "{} {} {} {}",
                    candidate.name, candidate.description, candidate.category, candidate.disclosure
                );
                let score = score_pair_with(&weights, geometry, &case.context, &descriptor) + bias;
                let probability = sigmoid(score);
                let target = case.relevance.get(&candidate.name).copied().unwrap_or(0) as f32 / 3.0;
                let error = probability - target;
                apply_pair_gradient(
                    &mut weights,
                    geometry,
                    &case.context,
                    &descriptor,
                    config.learning_rate,
                    error,
                );
                bias -= config.learning_rate * error;
            }
        }
    }

    // Dev-only calibration. The fallback path never sees a real dev split,
    // so it records an explicit uncalibrated marker instead of tuning.
    let calibration = if unqualified_fallback {
        ContextualCalibration {
            method: DISCIPLINE_FALLBACK.into(),
            abstain_bias: 0.0,
            temperature: 1.0,
            dev_cases: 0,
            dev_nll: 0.0,
            dev_brier: 0.0,
            dev_ece: 0.0,
            dev_no_tool_precision: None,
            dev_no_tool_recall: None,
            dev_no_tool_f1: None,
            uncalibrated_dev_nll: 0.0,
            uncalibrated_dev_brier: 0.0,
        }
    } else {
        let dev_scores: Vec<(f32, bool)> = dev_cases
            .iter()
            .map(|case| {
                let top = case
                    .candidates
                    .iter()
                    .take(config.max_candidates)
                    .map(|candidate| {
                        let descriptor = format!(
                            "{} {} {} {}",
                            candidate.name,
                            candidate.description,
                            candidate.category,
                            candidate.disclosure
                        );
                        score_pair_with(&weights, geometry, &case.context, &descriptor) + bias
                    })
                    .fold(f32::MIN, f32::max);
                (top, case.none)
            })
            .collect();
        calibrate_abstention(&dev_scores)
    };

    let discipline = if unqualified_fallback {
        DISCIPLINE_FALLBACK
    } else {
        DISCIPLINE_PARTITIONED
    };
    let parameter_count = buckets as u64 * dim as u64 + 1;
    let weights_sha256 = hex::encode(Sha256::digest(weights_bytes(&weights)));
    let manifest = ContextualArtifactManifest {
        artifact_schema_version: CONTEXTUAL_SCHEMA_VERSION,
        model_version: config.model_version.clone().unwrap_or_else(|| {
            format!(
                "contextual-encoder-v2-{}-{}buckets",
                config.capacity.as_str(),
                buckets
            )
        }),
        architecture: CONTEXTUAL_ARCHITECTURE_V2.into(),
        capacity: config.capacity.as_str().into(),
        parameter_count,
        embedding_dim: dim,
        vocab_buckets: buckets,
        tokenizer_version: "unicode-alnum-hash-v1".into(),
        context_schema_version: super::CASE_SCHEMA_VERSION,
        candidate_schema_version: super::CASE_SCHEMA_VERSION,
        max_context_tokens: MAX_CONTEXT_TOKENS,
        max_candidate_tokens: MAX_CANDIDATE_TOKENS,
        dataset_fingerprint: dataset_fingerprint.into(),
        weights_sha256,
        provenance: "local-rust-contextual-training".into(),
        license_notice: "MIT; trained from repository-local advisor fixtures".into(),
        training_discipline: discipline.into(),
        calibration: Some(calibration.clone()),
    };
    let artifact = ContextualArtifact {
        manifest,
        bias,
        weights,
    };
    write_atomic(output, &artifact)?;

    // Partition-aware metrics: train/dev only. Final-test metrics are never
    // computed during tuning; C004 evaluates them through the explicit
    // qualification path.
    let advisor = ContextualAdvisor::new(artifact)?;
    let predict = |indices: &[usize], mode: &str| {
        indices
            .iter()
            .map(|&index| {
                let case = &cases[index];
                let mut prediction = advisor.score(&ToolAdvisorInput {
                    case_id: case.case_id.clone(),
                    context: case.context.clone(),
                    candidates: case.candidates.clone(),
                    surface_fingerprint: dataset_fingerprint.to_string(),
                })?;
                prediction.mode = mode.to_string();
                Ok(prediction)
            })
            .collect::<Result<Vec<_>>>()
    };
    let train_predictions = predict(&train_indices, "contextual-train")?;
    let dev_predictions = predict(&dev_indices, "contextual-dev")?;
    let train_owned: Vec<ToolAdvisorCase> = train_indices
        .iter()
        .map(|&index| cases[index].clone())
        .collect();
    let dev_owned: Vec<ToolAdvisorCase> = dev_indices
        .iter()
        .map(|&index| cases[index].clone())
        .collect();
    let train_metrics = super::evaluate(&train_owned, &train_predictions)?;
    let dev_metrics = super::evaluate(&dev_owned, &dev_predictions)?;

    let metadata = fs::metadata(output)?;
    let capacity_report = capacity_report_for(
        &train_cases,
        geometry,
        parameter_count,
        metadata.len(),
        &initial_weights,
        advisor.artifact(),
        &dev_owned,
        output,
    )?;

    let fingerprint_of = |indices: &[usize]| {
        super::dataset_fingerprint(
            &indices
                .iter()
                .map(|&index| cases[index].clone())
                .collect::<Vec<_>>(),
        )
        .unwrap_or_default()
    };
    let training_report = ContextualTrainingReport {
        artifact_path: output.display().to_string(),
        capacity: config.capacity.as_str().into(),
        parameter_count,
        vocab_buckets: buckets,
        embedding_dim: dim,
        train_cases: train_indices.len(),
        dev_cases: dev_indices.len(),
        test_cases: test_indices.len(),
        dataset_fingerprint: dataset_fingerprint.into(),
        train_partition_fingerprint: fingerprint_of(&train_indices),
        dev_partition_fingerprint: fingerprint_of(&dev_indices),
        test_partition_fingerprint: fingerprint_of(&test_indices),
        unqualified_fallback,
        training_discipline: discipline.into(),
        train_metrics,
        dev_metrics,
        calibration,
        capacity_report,
        artifact_bytes: metadata.len(),
    };
    // Machine-readable evidence sidecar: calibration values, per-split
    // fingerprints, partition metrics, and effective-capacity data live next
    // to the artifact so reviewers and C004 can reproduce closure evidence
    // without re-running training.
    let sidecar = output.with_extension("training-report.json");
    let sidecar_temp = output.with_extension("training-report.json.tmp");
    fs::write(
        &sidecar_temp,
        serde_json::to_vec_pretty(&training_report).context("serialize training report")?,
    )
    .with_context(|| format!("write training report {}", sidecar_temp.display()))?;
    fs::rename(&sidecar_temp, &sidecar)
        .with_context(|| format!("install training report {}", sidecar.display()))?;
    Ok(training_report)
}

/// Geometry of one embedding interaction model. Production training uses
/// the manifest constants; numerical tests use tiny geometries through the
/// same code path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelGeometry {
    pub vocab_buckets: usize,
    pub embedding_dim: usize,
    pub max_context_tokens: usize,
    pub max_candidate_tokens: usize,
}

fn score_pair_with(
    weights: &[f32],
    geometry: ModelGeometry,
    context: &str,
    candidate: &str,
) -> f32 {
    let context_tokens =
        token_indices_with(context, geometry.max_context_tokens, geometry.vocab_buckets);
    let candidate_tokens = token_indices_with(
        candidate,
        geometry.max_candidate_tokens,
        geometry.vocab_buckets,
    );
    if context_tokens.is_empty() || candidate_tokens.is_empty() {
        return 0.0;
    }
    let context_mean = mean_pool(weights, geometry, &context_tokens);
    let candidate_mean = mean_pool(weights, geometry, &candidate_tokens);
    interaction_score(&context_mean, &candidate_mean, geometry.embedding_dim)
}

fn mean_pool(weights: &[f32], geometry: ModelGeometry, tokens: &[usize]) -> Vec<f32> {
    let mut output = vec![0.0; geometry.embedding_dim];
    let scale = 1.0 / tokens.len().max(1) as f32;
    for index in tokens {
        add_embedding(weights, geometry.embedding_dim, *index, &mut output, scale);
    }
    output
}

fn interaction_score(context_mean: &[f32], candidate_mean: &[f32], dim: usize) -> f32 {
    context_mean
        .iter()
        .zip(candidate_mean)
        .map(|(left, right)| left * right)
        .sum::<f32>()
        / dim as f32
}

/// Binary cross-entropy of one pair, in f64 so finite-difference checks are
/// not dominated by f32 rounding. Test-only oracle alongside
/// [`analytic_pair_gradients`].
#[cfg(test)]
fn pair_bce_loss(score: f32, target: f32) -> f64 {
    let probability = super::sigmoid(score) as f64;
    let clamped = probability.clamp(1e-12, 1.0 - 1e-12);
    let target = target as f64;
    -(target * clamped.ln() + (1.0 - target) * (1.0 - clamped).ln())
}

/// Analytic gradients of the pair BCE loss with respect to every embedding
/// weight (same layout as `weights`, zero for untouched rows) and the bias.
///
/// For one pair with mean-pooled context vector `c` and candidate vector `t`:
///
/// ```text
/// s = dot(c, t) / dim + b
/// g = dL/ds = sigmoid(s) - y
/// dL/dE(context_token_i) = g * t / (dim * n_context)
/// dL/dE(candidate_token_j) = g * c / (dim * n_candidate)
/// dL/db = g
/// ```
///
/// Means and gradients are pure functions of the pre-update parameters.
/// Repeated token occurrences accumulate one `1/n`-scaled contribution each;
/// a token on both sides receives the sum of both derivative paths.
///
/// Test-only oracle: production applies the identical math inline in
/// [`apply_pair_gradient`] to avoid allocating a full-table gradient per
/// pair-step. The finite-difference test pins the two implementations
/// together through the shared formulas.
#[cfg(test)]
fn analytic_pair_gradients(
    weights: &[f32],
    geometry: ModelGeometry,
    context: &str,
    candidate: &str,
    bias: f32,
    target: f32,
) -> (Vec<f32>, f32) {
    let context_tokens =
        token_indices_with(context, geometry.max_context_tokens, geometry.vocab_buckets);
    let candidate_tokens = token_indices_with(
        candidate,
        geometry.max_candidate_tokens,
        geometry.vocab_buckets,
    );
    let mut grads = vec![0.0; weights.len()];
    let error =
        super::sigmoid(score_pair_with(weights, geometry, context, candidate) + bias) - target;
    if context_tokens.is_empty() || candidate_tokens.is_empty() {
        return (grads, error);
    }
    let context_mean = mean_pool(weights, geometry, &context_tokens);
    let candidate_mean = mean_pool(weights, geometry, &candidate_tokens);
    let dim = geometry.embedding_dim as f32;
    let n_context = context_tokens.len() as f32;
    let n_candidate = candidate_tokens.len() as f32;
    for index in context_tokens {
        let start = index * geometry.embedding_dim;
        for offset in 0..geometry.embedding_dim {
            grads[start + offset] += error * candidate_mean[offset] / (dim * n_context);
        }
    }
    for index in candidate_tokens {
        let start = index * geometry.embedding_dim;
        for offset in 0..geometry.embedding_dim {
            grads[start + offset] += error * context_mean[offset] / (dim * n_candidate);
        }
    }
    (grads, error)
}

/// One gradient-descent step for a single pair: `w -= learning_rate * dL/dw`.
/// Means are snapshotted from the pre-update parameters before any write, so
/// repeated and shared tokens accumulate exactly the analytic contributions.
/// The sign and the mean-pooling `1/n` factors are applied here, at the call
/// site, instead of hiding a pre-negated scale inside the update.
pub fn apply_pair_gradient(
    weights: &mut [f32],
    geometry: ModelGeometry,
    context: &str,
    candidate: &str,
    learning_rate: f32,
    error: f32,
) {
    let context_tokens =
        token_indices_with(context, geometry.max_context_tokens, geometry.vocab_buckets);
    let candidate_tokens = token_indices_with(
        candidate,
        geometry.max_candidate_tokens,
        geometry.vocab_buckets,
    );
    if context_tokens.is_empty() || candidate_tokens.is_empty() {
        return;
    }
    let context_mean = mean_pool(weights, geometry, &context_tokens);
    let candidate_mean = mean_pool(weights, geometry, &candidate_tokens);
    let dim = geometry.embedding_dim as f32;
    let n_context = context_tokens.len() as f32;
    let n_candidate = candidate_tokens.len() as f32;
    for index in context_tokens {
        let start = index * geometry.embedding_dim;
        for offset in 0..geometry.embedding_dim {
            weights[start + offset] -=
                learning_rate * error * candidate_mean[offset] / (dim * n_context);
        }
    }
    for index in candidate_tokens {
        let start = index * geometry.embedding_dim;
        for offset in 0..geometry.embedding_dim {
            weights[start + offset] -=
                learning_rate * error * context_mean[offset] / (dim * n_candidate);
        }
    }
}

/// Abstention probability under explicit calibration parameters.
fn calibrated_abstain_probability(top_score: f32, abstain_bias: f32, temperature: f32) -> f64 {
    let probability = sigmoid((abstain_bias - top_score) / temperature.max(0.001));
    (probability as f64).clamp(1e-12, 1.0 - 1e-12)
}

fn abstention_nll(scores: &[(f32, bool)], abstain_bias: f32, temperature: f32) -> f64 {
    if scores.is_empty() {
        return 0.0;
    }
    scores
        .iter()
        .map(|(top, is_none)| {
            let probability = calibrated_abstain_probability(*top, abstain_bias, temperature);
            let target = if *is_none { 1.0 } else { 0.0 };
            -(target * probability.ln() + (1.0 - target) * (1.0 - probability).ln())
        })
        .sum::<f64>()
        / scores.len() as f64
}

fn abstention_brier(scores: &[(f32, bool)], abstain_bias: f32, temperature: f32) -> f64 {
    if scores.is_empty() {
        return 0.0;
    }
    scores
        .iter()
        .map(|(top, is_none)| {
            let probability = calibrated_abstain_probability(*top, abstain_bias, temperature);
            let target = if *is_none { 1.0 } else { 0.0 };
            (probability - target).powi(2)
        })
        .sum::<f64>()
        / scores.len() as f64
}

/// Expected calibration error over 10 equal-width probability bins.
fn abstention_ece(scores: &[(f32, bool)], abstain_bias: f32, temperature: f32) -> f64 {
    if scores.is_empty() {
        return 0.0;
    }
    let mut bins = vec![(0usize, 0.0, 0.0); 10];
    for (top, is_none) in scores {
        let probability = calibrated_abstain_probability(*top, abstain_bias, temperature);
        let bin = (probability * 10.0).floor() as usize;
        let bin = bin.min(9);
        bins[bin].0 += 1;
        bins[bin].1 += probability;
        if *is_none {
            bins[bin].2 += 1.0;
        }
    }
    bins.into_iter()
        .filter(|(count, _, _)| *count > 0)
        .map(|(count, sum, positives)| {
            let accuracy = positives / count as f64;
            let confidence = sum / count as f64;
            (accuracy - confidence).abs() * count as f64 / scores.len() as f64
        })
        .sum()
}

fn no_tool_prf(
    scores: &[(f32, bool)],
    abstain_bias: f32,
    temperature: f32,
) -> (Option<f64>, Option<f64>, Option<f64>) {
    let mut true_positives = 0usize;
    let mut predicted = 0usize;
    let mut actual = 0usize;
    for (top, is_none) in scores {
        let abstains = calibrated_abstain_probability(*top, abstain_bias, temperature) >= 0.5;
        if abstains {
            predicted += 1;
        }
        if *is_none {
            actual += 1;
        }
        if abstains && *is_none {
            true_positives += 1;
        }
    }
    let precision = (predicted > 0).then(|| true_positives as f64 / predicted as f64);
    let recall = (actual > 0).then(|| true_positives as f64 / actual as f64);
    let f1 = match (precision, recall) {
        (Some(left), Some(right)) if left + right > 0.0 => {
            Some(2.0 * left * right / (left + right))
        }
        _ => None,
    };
    (precision, recall, f1)
}

/// Deterministic dev-only grid search over temperature and abstention bias.
/// The grid always contains the uncalibrated point (`bias = 0`,
/// `temperature = 1`), so selection can never be worse than uncalibrated on
/// the selection metric (dev NLL); ties resolve to the first grid entry.
fn calibrate_abstention(dev_scores: &[(f32, bool)]) -> ContextualCalibration {
    const TEMPERATURES: [f32; 7] = [0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0];
    let mut sorted: Vec<f32> = dev_scores.iter().map(|(top, _)| *top).collect();
    sorted.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    sorted.dedup();
    let mut biases = vec![0.0_f32];
    if !sorted.is_empty() {
        for quantile in [0.0, 0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0] {
            let position = ((sorted.len() - 1) as f64 * quantile).round() as usize;
            biases.push(sorted[position.min(sorted.len() - 1)]);
        }
        biases.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
        biases.dedup();
    }
    let mut best = (f64::INFINITY, 0.0_f32, 1.0_f32);
    for temperature in TEMPERATURES {
        for bias in &biases {
            let nll = abstention_nll(dev_scores, *bias, temperature);
            if nll < best.0 {
                best = (nll, *bias, temperature);
            }
        }
    }
    let (dev_nll, abstain_bias, temperature) = best;
    let (precision, recall, f1) = no_tool_prf(dev_scores, abstain_bias, temperature);
    ContextualCalibration {
        method: CALIBRATION_DEV_GRID.into(),
        abstain_bias,
        temperature,
        dev_cases: dev_scores.len(),
        dev_nll,
        dev_brier: abstention_brier(dev_scores, abstain_bias, temperature),
        dev_ece: abstention_ece(dev_scores, abstain_bias, temperature),
        dev_no_tool_precision: precision,
        dev_no_tool_recall: recall,
        dev_no_tool_f1: f1,
        uncalibrated_dev_nll: abstention_nll(dev_scores, 0.0, 1.0),
        uncalibrated_dev_brier: abstention_brier(dev_scores, 0.0, 1.0),
    }
}

#[allow(clippy::too_many_arguments)]
fn capacity_report_for(
    train_cases: &[&ToolAdvisorCase],
    geometry: ModelGeometry,
    parameter_count: u64,
    artifact_bytes: u64,
    initial_weights: &[f32],
    artifact: &ContextualArtifact,
    dev_cases: &[ToolAdvisorCase],
    artifact_path: &Path,
) -> Result<ContextualCapacityReport> {
    use std::collections::{BTreeSet, HashMap};
    let mut tokens: BTreeSet<String> = BTreeSet::new();
    for case in train_cases {
        tokens.extend(
            tokenize(&case.context)
                .into_iter()
                .take(geometry.max_context_tokens),
        );
        for candidate in &case.candidates {
            tokens.extend(
                tokenize(&format!(
                    "{} {} {} {}",
                    candidate.name, candidate.description, candidate.category, candidate.disclosure
                ))
                .into_iter()
                .take(geometry.max_candidate_tokens),
            );
        }
    }
    let mut bucket_members: HashMap<usize, usize> = HashMap::new();
    for token in &tokens {
        *bucket_members
            .entry(hash_token_with(token, geometry.vocab_buckets))
            .or_insert(0) += 1;
    }
    let distinct = bucket_members.len();
    let collided = bucket_members
        .values()
        .filter(|&&count| count > 1)
        .sum::<usize>();
    let rows_changed = artifact
        .weights
        .chunks_exact(geometry.embedding_dim)
        .zip(initial_weights.chunks_exact(geometry.embedding_dim))
        .filter(|(current, initial)| {
            current
                .iter()
                .zip(initial.iter())
                .any(|(left, right)| left != right)
        })
        .count();
    let trained_parameters = rows_changed as u64 * geometry.embedding_dim as u64 + 1;

    // Cold-load timing: read the artifact back from disk exactly as an
    // operator load would.
    let started = Instant::now();
    let _ = load(artifact_path)?;
    let cold_load_millis = started.elapsed().as_millis();

    // Turn-local score latency over dev inputs.
    let advisor = ContextualAdvisor::new(ContextualArtifact {
        manifest: artifact.manifest.clone(),
        bias: artifact.bias,
        weights: artifact.weights.clone(),
    })?;
    let mut latencies: Vec<u128> = dev_cases
        .iter()
        .take(200)
        .map(|case| {
            let input = ToolAdvisorInput {
                case_id: case.case_id.clone(),
                context: case.context.clone(),
                candidates: case.candidates.clone(),
                surface_fingerprint: "capacity-probe".into(),
            };
            let started = Instant::now();
            let _ = advisor.score(&input);
            started.elapsed().as_micros()
        })
        .collect();
    latencies.sort_unstable();
    let percentile = |quantile: f64| {
        if latencies.is_empty() {
            0
        } else {
            latencies[((latencies.len() - 1) as f64 * quantile).round() as usize]
        }
    };

    Ok(ContextualCapacityReport {
        allocated_parameter_count: parameter_count,
        artifact_bytes,
        vocab_buckets: geometry.vocab_buckets,
        embedding_dim: geometry.embedding_dim,
        train_unique_normalized_tokens: tokens.len(),
        train_distinct_buckets_touched: distinct,
        train_collided_tokens: collided,
        train_collision_rate: if tokens.is_empty() {
            0.0
        } else {
            collided as f64 / tokens.len() as f64
        },
        rows_changed_by_training: rows_changed,
        trained_parameter_estimate: trained_parameters,
        trained_parameter_fraction: if parameter_count == 0 {
            0.0
        } else {
            trained_parameters as f64 / parameter_count as f64
        },
        cold_load_millis,
        peak_rss_bytes: peak_rss_bytes(),
        score_latency_p50_micros: percentile(0.5),
        score_latency_p95_micros: percentile(0.95),
    })
}

/// Resident high-water mark on Linux via procfs; `None` elsewhere. Safe
/// read-only parsing with no new dependencies.
fn peak_rss_bytes() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(value) = line.strip_prefix("VmHWM:") {
            let kilobytes: u64 = value.split_whitespace().next()?.parse().ok()?;
            return Some(kilobytes * 1024);
        }
    }
    None
}

fn add_embedding(weights: &[f32], dim: usize, index: usize, output: &mut [f32], scale: f32) {
    let start = index * dim;
    for offset in 0..dim {
        output[offset] += weights[start + offset] * scale;
    }
}

fn token_indices_with(text: &str, limit: usize, buckets: usize) -> Vec<usize> {
    tokenize(text)
        .into_iter()
        .take(limit)
        .map(|token| hash_token_with(&token, buckets))
        .collect()
}

fn hash_token_with(token: &str, buckets: usize) -> usize {
    let mut hash = 2_166_136_261_u32;
    for byte in token.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(16_777_619);
    }
    (hash as usize) % buckets.max(1)
}

fn initialize(weights: &mut [f32], seed: u64) {
    let mut state = seed | 1;
    for weight in weights {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *weight = ((state as i64 % 10_000) as f32 / 10_000.0) * 0.02;
    }
}

fn weights_bytes(weights: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(weights.len() * 4);
    for weight in weights {
        bytes.extend_from_slice(&weight.to_le_bytes());
    }
    bytes
}

pub fn capacity_report() -> Vec<(String, u64, usize)> {
    [Capacity::Small, Capacity::Medium]
        .into_iter()
        .map(|capacity| {
            (
                capacity.as_str().into(),
                capacity.parameter_count(),
                capacity.embedding_dim(),
            )
        })
        .collect()
}

pub fn artifact_path_for(output_dir: &Path, capacity: Capacity) -> PathBuf {
    output_dir.join(format!("contextual-{}.bin", capacity.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool_advisor::ToolAdvisorCandidate;

    const TINY_GEOMETRY: ModelGeometry = ModelGeometry {
        vocab_buckets: 16,
        embedding_dim: 4,
        max_context_tokens: 8,
        max_candidate_tokens: 8,
    };

    fn tiny_weights(seed: u64) -> Vec<f32> {
        let mut weights = vec![0.0; TINY_GEOMETRY.vocab_buckets * TINY_GEOMETRY.embedding_dim];
        initialize(&mut weights, seed);
        weights
    }

    /// Deterministic vocabulary with pairwise-distinct buckets under the tiny
    /// geometry, so gradient-accumulation assertions cannot be polluted by
    /// hash collisions.
    fn distinct_words(count: usize) -> Vec<String> {
        let mut words = Vec::new();
        let mut buckets = std::collections::BTreeSet::new();
        for serial in 0..1024 {
            let word = format!("w{serial}");
            let bucket = hash_token_with(&word, TINY_GEOMETRY.vocab_buckets);
            if buckets.insert(bucket) {
                words.push(word);
            }
            if words.len() == count {
                break;
            }
        }
        assert_eq!(words.len(), count);
        words
    }

    #[test]
    fn capacity_points_are_in_declared_small_model_range() {
        let report = capacity_report();
        assert_eq!(report[0].1, 5_242_881);
        assert_eq!(report[1].1, 15_728_641);
        assert!(report
            .iter()
            .all(|(_, parameters, _)| (5_000_000..=25_000_000).contains(parameters)));
    }

    #[test]
    fn context_changes_pair_score() {
        let geometry = ModelGeometry {
            vocab_buckets: VOCAB_BUCKETS,
            embedding_dim: SMALL_EMBEDDING_DIM,
            max_context_tokens: MAX_CONTEXT_TOKENS,
            max_candidate_tokens: MAX_CANDIDATE_TOKENS,
        };
        let mut weights = vec![0.0; VOCAB_BUCKETS * SMALL_EMBEDDING_DIM];
        let context_index = hash_token_with("definition", VOCAB_BUCKETS);
        let candidate_index = hash_token_with("lsp", VOCAB_BUCKETS);
        weights[context_index * SMALL_EMBEDDING_DIM] = 1.0;
        weights[candidate_index * SMALL_EMBEDDING_DIM] = 1.0;
        let score = score_pair_with(&weights, geometry, "find definition", "lsp definition");
        assert!(score > 0.0);
    }

    #[test]
    fn analytic_gradient_matches_centered_finite_differences() {
        let weights = tiny_weights(7);
        let context = "alpha beta";
        let candidate = "alpha gamma";
        let bias = 0.05;
        let target = 1.0;
        let (analytic, _) =
            analytic_pair_gradients(&weights, TINY_GEOMETRY, context, candidate, bias, target);
        let loss = |weights: &[f32], bias: f32| {
            pair_bce_loss(
                score_pair_with(weights, TINY_GEOMETRY, context, candidate) + bias,
                target,
            )
        };
        let epsilon = 1e-2;
        let mut worst = 0.0_f64;
        for index in 0..weights.len() {
            let mut plus = weights.clone();
            let mut minus = weights.clone();
            plus[index] += epsilon;
            minus[index] -= epsilon;
            let numeric = (loss(&plus, bias) - loss(&minus, bias)) / (2.0 * epsilon as f64);
            let difference = (analytic[index] as f64 - numeric).abs();
            worst = worst.max(difference);
        }
        let bias_numeric = (loss(&weights, bias + epsilon) - loss(&weights, bias - epsilon))
            / (2.0 * epsilon as f64);
        let (_, analytic_bias) =
            analytic_pair_gradients(&weights, TINY_GEOMETRY, context, candidate, bias, target);
        worst = worst.max((analytic_bias as f64 - bias_numeric).abs());
        assert!(worst < 5e-3, "analytic/finite-difference mismatch: {worst}");
    }

    #[test]
    fn apply_pair_gradient_matches_lr_scaled_analytic_gradients() {
        let weights = tiny_weights(29);
        let context = "alpha beta";
        let candidate = "alpha gamma";
        let bias = 0.05;
        let target = 1.0;
        let learning_rate = 0.3;
        let (analytic, error) =
            analytic_pair_gradients(&weights, TINY_GEOMETRY, context, candidate, bias, target);
        let mut stepped = weights.clone();
        apply_pair_gradient(
            &mut stepped,
            TINY_GEOMETRY,
            context,
            candidate,
            learning_rate,
            error,
        );
        for (index, (before, after)) in weights.iter().zip(stepped.iter()).enumerate() {
            let expected = before - learning_rate * analytic[index];
            assert!(
                (after - expected).abs() < 1e-6,
                "weight {index}: {after} != {expected}"
            );
        }
    }

    #[test]
    fn positive_target_update_increases_score_and_negative_decreases_it() {
        for (target, direction) in [(1.0, 1.0), (0.0, -1.0)] {
            let mut weights = tiny_weights(11);
            let mut bias = 0.0_f32;
            let context = "alpha beta";
            let candidate = "alpha gamma";
            let before = score_pair_with(&weights, TINY_GEOMETRY, context, candidate) + bias;
            let probability = sigmoid(before);
            let error = probability - target;
            apply_pair_gradient(&mut weights, TINY_GEOMETRY, context, candidate, 0.5, error);
            bias -= 0.5 * error;
            let after = score_pair_with(&weights, TINY_GEOMETRY, context, candidate) + bias;
            assert!(
                (after - before) * direction > 0.0,
                "target {target}: score moved the wrong way ({before} -> {after})"
            );
        }
    }

    #[test]
    fn repeated_token_occurrences_accumulate_one_scaled_contribution_each() {
        let words = distinct_words(3);
        let context = format!("{} {} {} {}", words[0], words[0], words[0], words[1]);
        let weights = tiny_weights(13);
        // words[0] occurs 3 of 4 context tokens; words[1] occurs 1 of 4.
        let (grads, _) =
            analytic_pair_gradients(&weights, TINY_GEOMETRY, &context, &words[2], 0.0, 1.0);
        let row = |token: &str| {
            let start =
                hash_token_with(token, TINY_GEOMETRY.vocab_buckets) * TINY_GEOMETRY.embedding_dim;
            grads[start..start + TINY_GEOMETRY.embedding_dim].to_vec()
        };
        let repeated = row(&words[0]);
        let solo = row(&words[1]);
        // Both rows see the same per-occurrence gradient (identical formula),
        // so the accumulated repeated gradient is exactly 3x the solo gradient.
        for (left, right) in repeated.iter().zip(solo.iter()) {
            assert!((left - 3.0 * right).abs() < 1e-6);
        }
    }

    #[test]
    fn shared_context_candidate_token_receives_the_sum_of_both_paths() {
        let weights = tiny_weights(17);
        let (grads, error) = analytic_pair_gradients(
            &weights,
            TINY_GEOMETRY,
            "shared left",
            "shared right",
            0.0,
            1.0,
        );
        let dim = TINY_GEOMETRY.embedding_dim;
        let shared = hash_token_with("shared", TINY_GEOMETRY.vocab_buckets) * dim;
        let context_mean = mean_pool(
            &weights,
            TINY_GEOMETRY,
            &token_indices_with("shared left", 8, TINY_GEOMETRY.vocab_buckets),
        );
        let candidate_mean = mean_pool(
            &weights,
            TINY_GEOMETRY,
            &token_indices_with("shared right", 8, TINY_GEOMETRY.vocab_buckets),
        );
        for offset in 0..dim {
            let expected = error * candidate_mean[offset] / (dim as f32 * 2.0)
                + error * context_mean[offset] / (dim as f32 * 2.0);
            assert!((grads[shared + offset] - expected).abs() < 1e-6);
        }
    }

    #[test]
    fn bias_derivative_equals_score_error() {
        let weights = tiny_weights(19);
        let bias = -0.3;
        let target = 0.0;
        let (_, analytic_bias) =
            analytic_pair_gradients(&weights, TINY_GEOMETRY, "alpha", "beta", bias, target);
        let expected =
            sigmoid(score_pair_with(&weights, TINY_GEOMETRY, "alpha", "beta") + bias) - target;
        assert!((analytic_bias - expected).abs() < 1e-7);
    }

    fn tiny_training_loss(weights: &[f32], bias: f32, pairs: &[(String, String, f32)]) -> f64 {
        pairs
            .iter()
            .map(|(context, candidate, target)| {
                pair_bce_loss(
                    score_pair_with(weights, TINY_GEOMETRY, context, candidate) + bias,
                    *target,
                )
            })
            .sum::<f64>()
            / pairs.len() as f64
    }

    fn run_tiny_training(
        pairs: &[(String, String, f32)],
        epochs: u32,
        learning_rate: f32,
    ) -> (Vec<f32>, f32, Vec<f64>) {
        let mut weights = tiny_weights(23);
        let mut bias = 0.0_f32;
        let mut trajectory = vec![tiny_training_loss(&weights, bias, pairs)];
        for _ in 0..epochs {
            for (context, candidate, target) in pairs {
                let error =
                    sigmoid(score_pair_with(&weights, TINY_GEOMETRY, context, candidate) + bias)
                        - *target;
                apply_pair_gradient(
                    &mut weights,
                    TINY_GEOMETRY,
                    context,
                    candidate,
                    learning_rate,
                    error,
                );
                bias -= learning_rate * error;
            }
            trajectory.push(tiny_training_loss(&weights, bias, pairs));
        }
        (weights, bias, trajectory)
    }

    /// Separable context-dependent pairs over a collision-free vocabulary:
    /// shared words (`t`, `o`, `h`) receive balanced conflicting updates and
    /// stay neutral while the discriminator rows separate.
    fn separable_pairs() -> Vec<(String, String, f32)> {
        let words = distinct_words(7);
        let context = |discriminator: &str| format!("{} {}", words[0], discriminator);
        let candidate =
            |discriminator: &str| format!("{} {} {}", words[3], discriminator, words[4]);
        vec![
            (context(&words[1]), candidate(&words[5]), 1.0),
            (context(&words[1]), candidate(&words[6]), 0.0),
            (context(&words[2]), candidate(&words[5]), 0.0),
            (context(&words[2]), candidate(&words[6]), 1.0),
        ]
    }

    #[test]
    fn tiny_training_loss_decreases_substantially() {
        let pairs = separable_pairs();
        let (_, _, trajectory) = run_tiny_training(&pairs, 250, 1.0);
        let initial = *trajectory.first().unwrap();
        let final_loss = *trajectory.last().unwrap();
        // The separable task is learned to near-zero loss (the overfit test
        // below checks the per-pair probabilities); the gate asserts the
        // optimizer actually gets there instead of oscillating at chance.
        assert!(
            final_loss < initial - 0.3,
            "loss did not substantially decrease: first={initial}, last={final_loss}"
        );
        let half = trajectory.len() / 2;
        let first_min = trajectory[..half]
            .iter()
            .fold(f64::INFINITY, |left, right| left.min(*right));
        let second_min = trajectory[half..]
            .iter()
            .fold(f64::INFINITY, |left, right| left.min(*right));
        assert!(
            second_min < first_min,
            "descent did not continue into the second half: {first_min} -> {second_min}"
        );
    }

    #[test]
    fn tiny_training_overfits_a_separable_context_dependent_task() {
        let pairs = separable_pairs();
        let (weights, bias, _) = run_tiny_training(&pairs, 250, 1.0);
        for (context, candidate, target) in &pairs {
            let probability =
                sigmoid(score_pair_with(&weights, TINY_GEOMETRY, context, candidate) + bias);
            if *target > 0.5 {
                assert!(probability > 0.7, "{context}/{candidate}: {probability}");
            } else {
                assert!(probability < 0.3, "{context}/{candidate}: {probability}");
            }
        }
    }

    fn dev_tuning_corpus() -> Vec<crate::tool_advisor::ToolAdvisorCase> {
        // Distinct singleton cases; the generator loop below keeps adding
        // uniquely-suffixed cases until both train and dev hold out data.
        // The construction order is fixed, so the result is deterministic.
        let mut cases = Vec::new();
        let mut counter = 0;
        while cases.len() < 24 {
            let relevant = if counter % 2 == 0 {
                "tool_alpha"
            } else {
                "tool_beta"
            };
            cases.push(crate::tool_advisor::ToolAdvisorCase {
                schema_version: crate::tool_advisor::CASE_SCHEMA_VERSION,
                case_id: format!("tiny-{counter}"),
                context: format!("tiny task number {counter} about alpha workflows"),
                candidates: vec![
                    ToolAdvisorCandidate {
                        name: "tool_alpha".into(),
                        description: "Alpha workflow helper".into(),
                        category: "ReadOnly".into(),
                        disclosure: "deferred".into(),
                        synthetic_identity: false,
                    },
                    ToolAdvisorCandidate {
                        name: "tool_beta".into(),
                        description: "Beta workflow helper".into(),
                        category: "ReadOnly".into(),
                        disclosure: "deferred".into(),
                        synthetic_identity: false,
                    },
                ],
                relevance: std::collections::BTreeMap::from([(relevant.to_string(), 3)]),
                preferred_order: vec![relevant.to_string()],
                none: false,
                tags: vec!["tiny".into()],
                group_id: format!("tiny-group-{counter}"),
                provenance: "tiny-fixture".into(),
                semantic_group: format!("tiny-semantic-{counter}"),
                leakage_group: String::new(),
                task_family: "tiny".into(),
                tool_family: "tiny".into(),
                generated_variant_family: format!("tiny-lineage-{counter}"),
                teacher_probabilities: std::collections::BTreeMap::new(),
            });
            counter += 1;
            if counter > 6 {
                let layout = crate::tool_advisor::partition_cases(&cases);
                if !layout.train_cases.is_empty() && !layout.dev_cases.is_empty() {
                    break;
                }
            }
        }
        cases
    }

    #[test]
    fn empty_partitions_are_hard_errors_without_the_fallback_flag() {
        // Identical inputs share one leakage component, so every split but
        // one is empty by construction.
        let duplicate = |id: &str| crate::tool_advisor::ToolAdvisorCase {
            schema_version: crate::tool_advisor::CASE_SCHEMA_VERSION,
            case_id: id.into(),
            context: "identical tiny context".into(),
            candidates: vec![ToolAdvisorCandidate {
                name: "tool_alpha".into(),
                description: "Alpha workflow helper".into(),
                category: "ReadOnly".into(),
                disclosure: "deferred".into(),
                synthetic_identity: false,
            }],
            relevance: std::collections::BTreeMap::from([("tool_alpha".to_string(), 3)]),
            preferred_order: vec!["tool_alpha".into()],
            none: false,
            tags: vec!["tiny".into()],
            group_id: format!("{id}-group"),
            provenance: "tiny-fixture".into(),
            semantic_group: format!("{id}-semantic"),
            leakage_group: String::new(),
            task_family: "tiny".into(),
            tool_family: "tiny".into(),
            generated_variant_family: String::new(),
            teacher_probabilities: std::collections::BTreeMap::new(),
        };
        let cases = vec![duplicate("dup-a"), duplicate("dup-b")];
        assert_eq!(
            crate::tool_advisor::build_leakage_groups(&cases)[0],
            crate::tool_advisor::build_leakage_groups(&cases)[1]
        );
        let strict = ContextualTrainingConfig {
            capacity: Capacity::Small,
            epochs: 1,
            learning_rate: 0.05,
            seed: 1,
            max_candidates: 8,
            model_version: None,
            vocab_buckets: Some(64),
            allow_unpartitioned_fallback: false,
        };
        let directory = tempfile::tempdir().expect("temp directory");
        let fingerprint = crate::tool_advisor::dataset_fingerprint(&cases).expect("fingerprint");
        let error = train(
            &cases,
            &fingerprint,
            &strict,
            &directory.path().join("strict.bin"),
        )
        .expect_err("single-component corpus must fail in qualification mode");
        assert!(error.to_string().contains("non-empty C001"));

        // The explicit fallback flag fits, but the artifact can never qualify.
        let fallback = ContextualTrainingConfig {
            allow_unpartitioned_fallback: true,
            ..strict
        };
        let output = directory.path().join("fallback.bin");
        let report = train(&cases, &fingerprint, &fallback, &output).expect("fallback train");
        assert!(report.unqualified_fallback);
        assert_eq!(report.training_discipline, DISCIPLINE_FALLBACK);
        let artifact = load(&output).expect("load fallback artifact");
        assert!(!artifact.is_qualified());
        assert_eq!(
            artifact.calibration_status(),
            CalibrationStatus::LegacyUnqualified
        );
    }

    #[test]
    fn dev_calibration_is_serialized_and_used_at_runtime() {
        let cases = dev_tuning_corpus();
        let layout = crate::tool_advisor::partition_cases(&cases);
        assert!(!layout.train_cases.is_empty() && !layout.dev_cases.is_empty());
        let config = ContextualTrainingConfig {
            capacity: Capacity::Small,
            epochs: 2,
            learning_rate: 0.1,
            seed: 3,
            max_candidates: 8,
            model_version: None,
            vocab_buckets: Some(128),
            allow_unpartitioned_fallback: false,
        };
        let directory = tempfile::tempdir().expect("temp directory");
        let fingerprint = crate::tool_advisor::dataset_fingerprint(&cases).expect("fingerprint");
        let output = directory.path().join("calibrated.bin");
        let report = train(&cases, &fingerprint, &config, &output).expect("train");
        assert!(!report.unqualified_fallback);
        assert_eq!(report.calibration.method, CALIBRATION_DEV_GRID);
        assert!(report.calibration.temperature > 0.0);
        assert!(report.calibration.dev_cases > 0);
        assert!(report.calibration.dev_nll <= report.calibration.uncalibrated_dev_nll + 1e-9);
        let artifact = load(&output).expect("load artifact");
        assert!(artifact.is_qualified());
        assert_eq!(
            artifact.manifest.training_discipline,
            DISCIPLINE_PARTITIONED
        );

        // Runtime abstention matches the serialized calibration formula.
        let advisor = ContextualAdvisor::new(artifact).expect("advisor");
        let input = ToolAdvisorInput {
            case_id: "calibration-probe".into(),
            context: "tiny task about alpha workflows".into(),
            candidates: vec![ToolAdvisorCandidate {
                name: "tool_alpha".into(),
                description: "Alpha workflow helper".into(),
                category: "ReadOnly".into(),
                disclosure: "deferred".into(),
                synthetic_identity: false,
            }],
            surface_fingerprint: "probe".into(),
        };
        let prediction = advisor.score(&input).expect("score");
        let top = prediction
            .ranked
            .first()
            .map(|item| item.score as f32)
            .unwrap_or(0.0);
        let calibration = advisor.artifact().manifest.calibration.clone().unwrap();
        let expected =
            sigmoid((calibration.abstain_bias - top) / calibration.temperature.max(0.001));
        assert!((prediction.abstain_probability.unwrap_or(-1.0) as f32 - expected).abs() < 1e-6);
    }

    #[test]
    fn legacy_v1_artifacts_load_but_never_qualify() {
        let dim = 2usize;
        let buckets = VOCAB_BUCKETS;
        let weights = vec![0.01; buckets * dim];
        let manifest = ContextualArtifactManifest {
            artifact_schema_version: CONTEXTUAL_SCHEMA_VERSION_V1,
            model_version: "legacy-v1".into(),
            architecture: CONTEXTUAL_ARCHITECTURE_V1.into(),
            capacity: "small".into(),
            parameter_count: buckets as u64 * dim as u64 + 1,
            embedding_dim: dim,
            vocab_buckets: buckets,
            tokenizer_version: "unicode-alnum-hash-v1".into(),
            context_schema_version: crate::tool_advisor::CASE_SCHEMA_VERSION,
            candidate_schema_version: crate::tool_advisor::CASE_SCHEMA_VERSION,
            max_context_tokens: MAX_CONTEXT_TOKENS,
            max_candidate_tokens: MAX_CANDIDATE_TOKENS,
            dataset_fingerprint: "legacy".into(),
            weights_sha256: hex::encode(sha2::Sha256::digest(weights_bytes(&weights))),
            provenance: "legacy".into(),
            license_notice: "legacy".into(),
            training_discipline: DISCIPLINE_LEGACY.into(),
            calibration: None,
        };
        let artifact = ContextualArtifact {
            manifest,
            bias: 0.0,
            weights,
        };
        let directory = tempfile::tempdir().expect("temp directory");
        let path = directory.path().join("legacy.bin");
        write_atomic(&path, &artifact).expect("write legacy");
        let loaded = load(&path).expect("legacy loads for compatibility");
        assert!(!loaded.is_qualified());
        assert_eq!(
            loaded.calibration_status(),
            CalibrationStatus::LegacyUnqualified
        );
        // Legacy runtime keeps the exact historical abstention formula.
        let advisor = ContextualAdvisor::new(loaded).expect("advisor");
        let input = ToolAdvisorInput {
            case_id: "legacy-probe".into(),
            context: "alpha".into(),
            candidates: vec![ToolAdvisorCandidate {
                name: "tool_alpha".into(),
                description: "Alpha helper".into(),
                category: "ReadOnly".into(),
                disclosure: "deferred".into(),
                synthetic_identity: false,
            }],
            surface_fingerprint: "probe".into(),
        };
        let prediction = advisor.score(&input).expect("score");
        let top = prediction
            .ranked
            .first()
            .map(|item| item.score as f32)
            .unwrap_or(0.0);
        assert!(
            (prediction.abstain_probability.unwrap_or(-1.0) as f32 - sigmoid(-top)).abs() < 1e-6
        );
    }

    #[test]
    fn v2_artifacts_require_serialized_calibration() {
        let mut manifest = ContextualArtifactManifest {
            artifact_schema_version: CONTEXTUAL_SCHEMA_VERSION,
            model_version: "bogus-v2".into(),
            architecture: CONTEXTUAL_ARCHITECTURE_V2.into(),
            capacity: "small".into(),
            parameter_count: 8 * 2 + 1,
            embedding_dim: 2,
            vocab_buckets: 8,
            tokenizer_version: "unicode-alnum-hash-v1".into(),
            context_schema_version: crate::tool_advisor::CASE_SCHEMA_VERSION,
            candidate_schema_version: crate::tool_advisor::CASE_SCHEMA_VERSION,
            max_context_tokens: MAX_CONTEXT_TOKENS,
            max_candidate_tokens: MAX_CANDIDATE_TOKENS,
            dataset_fingerprint: "bogus".into(),
            weights_sha256: String::new(),
            provenance: "bogus".into(),
            license_notice: "bogus".into(),
            training_discipline: DISCIPLINE_PARTITIONED.into(),
            calibration: None,
        };
        let weights = vec![0.0; 16];
        manifest.weights_sha256 = hex::encode(sha2::Sha256::digest(weights_bytes(&weights)));
        let artifact = ContextualArtifact {
            manifest,
            bias: 0.0,
            weights,
        };
        let directory = tempfile::tempdir().expect("temp directory");
        let path = directory.path().join("bogus.bin");
        // write_atomic validates first, so a calibration-free v2 is rejected
        // before it can ever be mistaken for a qualified artifact.
        assert!(write_atomic(&path, &artifact).is_err());
    }

    #[test]
    fn corrected_training_is_deterministic_for_a_fixed_config() {
        let cases = dev_tuning_corpus();
        let fingerprint = crate::tool_advisor::dataset_fingerprint(&cases).expect("fingerprint");
        let config = ContextualTrainingConfig {
            capacity: Capacity::Small,
            epochs: 2,
            learning_rate: 0.1,
            seed: 5,
            max_candidates: 8,
            model_version: None,
            vocab_buckets: Some(128),
            allow_unpartitioned_fallback: false,
        };
        let directory = tempfile::tempdir().expect("temp directory");
        let first = directory.path().join("first.bin");
        let second = directory.path().join("second.bin");
        train(&cases, &fingerprint, &config, &first).expect("first train");
        train(&cases, &fingerprint, &config, &second).expect("second train");
        let first_artifact = load(&first).expect("load first");
        let second_artifact = load(&second).expect("load second");
        assert_eq!(
            first_artifact.manifest.weights_sha256,
            second_artifact.manifest.weights_sha256
        );
        assert_eq!(first_artifact.bias, second_artifact.bias);
    }

    #[test]
    fn capacity_report_distinguishes_allocated_from_trained_parameters() {
        let cases = dev_tuning_corpus();
        let fingerprint = crate::tool_advisor::dataset_fingerprint(&cases).expect("fingerprint");
        let config = ContextualTrainingConfig {
            capacity: Capacity::Small,
            epochs: 2,
            learning_rate: 0.1,
            seed: 9,
            max_candidates: 8,
            model_version: None,
            vocab_buckets: Some(128),
            allow_unpartitioned_fallback: false,
        };
        let directory = tempfile::tempdir().expect("temp directory");
        let output = directory.path().join("capacity.bin");
        let report = train(&cases, &fingerprint, &config, &output).expect("train");
        let capacity = &report.capacity_report;
        assert_eq!(capacity.vocab_buckets, 128);
        assert!(capacity.train_unique_normalized_tokens > 0);
        assert!(capacity.train_distinct_buckets_touched > 0);
        assert!(capacity.train_distinct_buckets_touched <= capacity.train_unique_normalized_tokens);
        assert!(capacity.rows_changed_by_training > 0);
        assert!(capacity.rows_changed_by_training <= capacity.train_distinct_buckets_touched);
        assert!(capacity.trained_parameter_estimate < capacity.allocated_parameter_count);
        assert!((0.0..=1.0).contains(&capacity.trained_parameter_fraction));
        assert!((0.0..=1.0).contains(&capacity.train_collision_rate));
    }

    #[test]
    #[cfg(feature = "tool-advisor-training")]
    fn training_config_smoke_assets_parse() {
        for name in [
            "tiny-training.json",
            "contextual-small-training.json",
            "contextual-medium-training.json",
        ] {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("assets/tool-advisor")
                .join(name);
            let bytes = std::fs::read(&path).expect("read smoke config");
            let config: crate::tool_advisor::training::TrainingConfig =
                serde_json::from_slice(&bytes).expect("parse smoke config");
            assert!(config.epochs > 0, "{name}");
        }
    }
}
